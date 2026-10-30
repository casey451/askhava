//! The records `ops` keeps: audit entries, alerts and pause switches (JOB-005, OPS-003, OPS-010, PRIV-028).
//!
//! An audit entry holds record IDs and fixed code names only, never a contact value (PRIV-028):
//! actors and subjects are UUIDs, and every name (action, subject kind, status, field) is a
//! `&'static str` written in the code, so a runtime value such as a phone number cannot be passed
//! by accident. Names are also checked against the code-name form (`is_code_name`) here and by the
//! table's `CHECK` constraints (`migrations/0021_ops_audit.sql`).

use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use thiserror::Error;

/// Why an `ops` call failed. No variant carries a value or the database's message (PRIV-016).
#[derive(Debug, Error)]
pub enum OpsError {
    /// A name is not a lower-case code name (letters, digits, `_`, `.`, starting with a letter,
    /// at most 64 characters); the field that held it is named, never the value (PRIV-028).
    #[error("{0} is not a code name")]
    NotACodeName(&'static str),
    /// The database refused or dropped the statement.
    #[error("the ops database statement failed")]
    Database,
}

/// True when `name` is a lower-case code name, the only text an audit row may hold (PRIV-028).
#[must_use]
pub fn is_code_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
}

/// Who made a change: the system (a timer or handler) or a person by their record ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// A worker task, timer or inbound event with no person behind it.
    System,
    /// Casey, by his owner record ID.
    Owner(Uuid),
    /// A business, by its record ID.
    Business(Uuid),
    /// A homeowner, by their record ID.
    Homeowner(Uuid),
}

impl Actor {
    /// The `actor_kind` and `actor_id` columns for this actor.
    #[must_use]
    pub fn columns(self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::System => ("system", None),
            Self::Owner(id) => ("owner", Some(id)),
            Self::Business(id) => ("business", Some(id)),
            Self::Homeowner(id) => ("homeowner", Some(id)),
        }
    }
}

/// What an audit row records about its subject beyond the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing more than the action itself.
    Action,
    /// A status change from one status to another (JOB-005).
    Status {
        /// The status the record left.
        from: &'static str,
        /// The status the record entered.
        to: &'static str,
    },
    /// An edit of one field, logged by its name only, never its old or new value (PRIV-028).
    Field(&'static str),
}

impl Change {
    /// The `from_status`, `to_status` and `field` columns for this change.
    #[must_use]
    pub fn columns(
        self,
    ) -> (
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    ) {
        match self {
            Self::Action => (None, None, None),
            Self::Status { from, to } => (Some(from), Some(to), None),
            Self::Field(name) => (None, None, Some(name)),
        }
    }
}

/// One audit row to write: who did what to which record (JOB-005, OPS-010, PRIV-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEntry {
    /// Who made the change.
    pub actor: Actor,
    /// What happened, as a code name such as `jobs.status_changed`.
    pub action: &'static str,
    /// The kind of record changed, as a code name such as `jobs.job`.
    pub subject_kind: &'static str,
    /// The changed record's ID.
    pub subject_id: Uuid,
    /// The status move or field name, if any.
    pub change: Change,
}

impl AuditEntry {
    /// The entry for a status change, which every status change writes in its own transaction
    /// (JOB-005).
    #[must_use]
    pub fn status_change(
        actor: Actor,
        subject: (&'static str, Uuid),
        from: &'static str,
        to: &'static str,
    ) -> Self {
        let change = Change::Status { from, to };
        Self {
            actor,
            action: "status_changed",
            subject_kind: subject.0,
            subject_id: subject.1,
            change,
        }
    }

    /// Checks every name in the entry is a code name (PRIV-028).
    ///
    /// # Errors
    ///
    /// [`OpsError::NotACodeName`] naming the first field that is not.
    pub fn check(&self) -> Result<(), OpsError> {
        let (from, to, field) = self.change.columns();
        let names = [
            ("action", Some(self.action)),
            ("subject_kind", Some(self.subject_kind)),
        ];
        let more = [("from_status", from), ("to_status", to), ("field", field)];
        match names
            .into_iter()
            .chain(more)
            .find(|(_, v)| v.is_some_and(|v| !is_code_name(v)))
        {
            Some((column, _)) => Err(OpsError::NotACodeName(column)),
            None => Ok(()),
        }
    }
}

/// One audit row as stored, read back for export (OPS-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// The row's place in the log; the export cursor.
    pub id: i64,
    /// When the row was written (the writing transaction's start).
    pub at: Timestamp,
    /// `system`, `owner`, `business` or `homeowner`.
    pub actor_kind: String,
    /// The acting person's record ID; none for the system.
    pub actor_id: Option<Uuid>,
    /// What happened.
    pub action: String,
    /// The kind of record changed.
    pub subject_kind: String,
    /// The changed record's ID.
    pub subject_id: Uuid,
    /// The status left, for a status change.
    pub from_status: Option<String>,
    /// The status entered, for a status change.
    pub to_status: Option<String>,
    /// The edited field's name, for a field edit.
    pub field: Option<String>,
}

impl AuditRow {
    /// The row as one JSON object, the line format of the export and the off-database stream
    /// (OPS-010, OPS-011).
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "at": self.at.to_string(), "actor_kind": self.actor_kind,
            "actor_id": self.actor_id.map(|id| id.to_string()), "action": self.action,
            "subject_kind": self.subject_kind, "subject_id": self.subject_id.to_string(),
            "from_status": self.from_status, "to_status": self.to_status, "field": self.field,
        })
    }
}

/// How urgent an alert is; what pushes to Casey's phone is decided by later `ops` rules (OPS-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A critical safety alert (OPS-028).
    Critical,
    /// A High alert (OPS-001).
    High,
    /// Everything else, listed in the weekly digest.
    Normal,
}

impl Severity {
    /// The stored `severity` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Normal => "normal",
        }
    }
}

/// One alert to raise: its kind, severity and the record it is about, by ID only (PRIV-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewAlert {
    /// The alert kind as a code name, such as `ops.health_check_failed`.
    pub kind: &'static str,
    /// How urgent it is.
    pub severity: Severity,
    /// The record it is about: its kind as a code name and its ID.
    pub subject: Option<(&'static str, Uuid)>,
}

/// One owner pause switch (OPS-003). A town or trade switch carries that town's or trade's record ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// Pauses everything.
    All,
    /// Pauses one town.
    Town(Uuid),
    /// Pauses one trade.
    Trade(Uuid),
    /// Pauses charging.
    Charging,
    /// Pauses texting.
    Texting,
}

impl Switch {
    /// The stored `name` and `scope_id` (none for a switch with no scope).
    #[must_use]
    pub fn columns(self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::All => ("all", None),
            Self::Town(town) => ("town", Some(town)),
            Self::Trade(trade) => ("trade", Some(trade)),
            Self::Charging => ("charging", None),
            Self::Texting => ("texting", None),
        }
    }
}

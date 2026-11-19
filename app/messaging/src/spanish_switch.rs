//! The stored owner-portal "Spanish conversations" switch and the records it rests on (TXT-039,
//! TXT-043; D-427, D-490, D-3753, D-3844, T480).
//!
//! `messaging.spanish_switch` is append-only: each change is a new row and the newest is in force;
//! no row reads off, the launch state (TXT-039). [`spanish_switch`] gives the stored
//! [`SpanishSwitch`]: an "on" row reads on only while its recorded professional review still
//! covers every Spanish template at its current version, so a template added or changed after the
//! review turns Spanish conversations off until a new review is recorded (D-427). A template's
//! version is the lowercase hex SHA256 of its Spanish text from `Registry::template(id,
//! Language::Es)` ([`template_version`], D-3844): a changed Spanish text is a new version.
//! [`turn_spanish_on`] stores an "on" row only when [`SpanishRecords`] hold the review's date, its
//! reviewer and versions covering every Spanish template, the campaign note's date and reference
//! and a non-empty Spanish sample (TXT-043); otherwise it stores nothing and answers
//! [`SpanishChange::Refused`] naming each [`Missing`] item. [`turn_spanish_off`] always succeeds.
//! Each stored change writes one `ops` audit row in the same transaction holding the owner's ID,
//! the row's ID and the from and to states only, never the reviewer, reference or sample
//! (PRIV-028). The inbound text route and its retry (`server`) read the switch per message; the
//! owner portal (`portals`) asks for the SEC-043 fresh factor before either change.

use std::collections::BTreeMap;

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::templates::{Language, Registry};

/// The audit action of a stored switch change (from and to are `off` and `on`).
pub const SPANISH_SWITCH_ACTION: &str = "messaging.spanish_switch_changed";

/// Why reading or changing the switch failed. Carries no value (PRIV-016).
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum SpanishSwitchError {
    /// The embedded template registry did not load.
    #[error("the template registry did not load")]
    Templates,
    /// The database refused or dropped a statement.
    #[error("the Spanish switch statement failed")]
    Database,
    /// The audit row was not written; nothing was kept.
    #[error("the Spanish switch audit row was not written")]
    Ops,
}

/// What the owner records to turn Spanish conversations on (TXT-039, TXT-043, D-427).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanishRecords {
    /// The date of the paid professional translation review.
    pub reviewed_on: Option<Date>,
    /// Who did the review, 1 to 200 characters.
    pub reviewer: String,
    /// Each Spanish template ID the review covered, with the version it covered.
    pub reviewed: BTreeMap<String, String>,
    /// The date of the campaign note (the Console update saying texts are in English or Spanish).
    pub note_on: Option<Date>,
    /// The campaign note's reference, 1 to 200 characters.
    pub note_reference: String,
    /// The Spanish sample message on file, 1 to 1,600 characters.
    pub sample: String,
}

/// One thing a turn-on lacks, in the order the settings page names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// No review date (TXT-039).
    ReviewDate,
    /// No reviewer, or one over 200 characters (TXT-039).
    Reviewer,
    /// The review misses a Spanish template or names an older version of one (TXT-039, D-427).
    ReviewedTemplates,
    /// No campaign note date (TXT-043).
    NoteDate,
    /// No campaign note reference, or one over 200 characters (TXT-043).
    NoteReference,
    /// No Spanish sample, or one over 1,600 characters (TXT-043).
    Sample,
}

impl Missing {
    /// The item's code name, for logs and tests.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::ReviewDate => "review_date",
            Self::Reviewer => "reviewer",
            Self::ReviewedTemplates => "reviewed_templates",
            Self::NoteDate => "note_date",
            Self::NoteReference => "note_reference",
            Self::Sample => "sample",
        }
    }
}

/// What a switch change did (TXT-039, TXT-043).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanishChange {
    /// One new row and one audit row were stored.
    Changed,
    /// The switch was already off; nothing was stored.
    Unchanged,
    /// A record is missing; nothing was stored.
    Refused(Vec<Missing>),
}

/// The records in force while the switch is on, for the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanishInForce {
    /// The review's date.
    pub reviewed_on: Date,
    /// The reviewer.
    pub reviewer: String,
    /// The campaign note's date.
    pub note_on: Date,
    /// The campaign note's reference.
    pub note_reference: String,
}

/// The stored switch: on only with its records in force (TXT-039).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanishSwitch {
    in_force: Option<SpanishInForce>,
}

impl SpanishSwitch {
    /// Whether Spanish conversations are on, the intake setting `spanish_conversations_on`.
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.in_force.is_some()
    }

    /// The records in force while on; `None` while off.
    #[must_use]
    pub fn in_force(&self) -> Option<&SpanishInForce> {
        self.in_force.as_ref()
    }
}

/// D-3844: a Spanish template's version, the lowercase hex SHA256 of its Spanish text.
#[must_use]
pub fn template_version(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// TXT-039, D-3844: every Spanish template ID in the embedded registry with its current version.
///
/// # Errors
///
/// [`SpanishSwitchError::Templates`].
pub fn spanish_template_versions() -> Result<BTreeMap<String, String>, SpanishSwitchError> {
    let registry = Registry::embedded().map_err(|_| SpanishSwitchError::Templates)?;
    Ok(registry
        .ids()
        .into_iter()
        .filter_map(|id| {
            let (text, _) = registry.template(&id, Language::Es)?;
            let version = template_version(text);
            Some((id, version))
        })
        .collect())
}

/// D-427: whether `reviewed` names every template of `current` at its current version.
fn covers(reviewed: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> bool {
    current.iter().all(|(id, v)| reviewed.get(id) == Some(v))
}

/// Whether `text`, trimmed, is 1 to `max` characters.
fn within(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.trim().chars().count())
}

/// TXT-039, TXT-043, D-427: what `records` lack against the `current` template versions, in
/// [`Missing`]'s order; empty when a turn-on may be stored.
#[must_use]
pub fn missing(records: &SpanishRecords, current: &BTreeMap<String, String>) -> Vec<Missing> {
    [
        (Missing::ReviewDate, records.reviewed_on.is_some()),
        (Missing::Reviewer, within(&records.reviewer, 200)),
        (
            Missing::ReviewedTemplates,
            covers(&records.reviewed, current),
        ),
        (Missing::NoteDate, records.note_on.is_some()),
        (Missing::NoteReference, within(&records.note_reference, 200)),
        (Missing::Sample, within(&records.sample, 1600)),
    ]
    .into_iter()
    .filter_map(|(item, ok)| (!ok).then_some(item))
    .collect()
}

/// The "on" row in force with the versions its review covered; `None` with no row or after an
/// "off" row.
async fn latest(
    conn: &mut PgConnection,
) -> Result<Option<(SpanishInForce, BTreeMap<String, String>)>, SpanishSwitchError> {
    let row = sqlx::query!(
        r#"SELECT is_on, reviewed_on::text AS reviewed_on, reviewer, reviewed,
                  note_on::text AS note_on, note_reference
           FROM messaging.spanish_switch ORDER BY changed_at DESC, id LIMIT 1"#
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| SpanishSwitchError::Database)?;
    let Some(row) = row.filter(|r| r.is_on) else {
        return Ok(None);
    };
    let date = |d: Option<String>| d.as_deref().and_then(|d| d.parse::<Date>().ok());
    let reviewed = row
        .reviewed
        .map(serde_json::from_value::<BTreeMap<String, String>>);
    match (date(row.reviewed_on), row.reviewer, reviewed) {
        (Some(reviewed_on), Some(reviewer), Some(Ok(reviewed))) => {
            let (Some(note_on), Some(note_reference)) = (date(row.note_on), row.note_reference)
            else {
                return Err(SpanishSwitchError::Database);
            };
            let in_force = SpanishInForce {
                reviewed_on,
                reviewer,
                note_on,
                note_reference,
            };
            Ok(Some((in_force, reviewed)))
        }
        _ => Err(SpanishSwitchError::Database),
    }
}

/// TXT-039, D-427: the stored switch; off with no row, after an "off" row, or when the stored
/// review no longer covers every Spanish template at its current version.
///
/// # Errors
///
/// [`SpanishSwitchError::Templates`] or [`SpanishSwitchError::Database`].
pub async fn spanish_switch(conn: &mut PgConnection) -> Result<SpanishSwitch, SpanishSwitchError> {
    let current = spanish_template_versions()?;
    let row = latest(conn).await?;
    Ok(SpanishSwitch {
        in_force: row
            .filter(|(_, reviewed)| covers(reviewed, &current))
            .map(|(in_force, _)| in_force),
    })
}

/// TXT-039, TXT-043, D-427: turns Spanish conversations on for `owner` at `now`, storing
/// `records`, only when [`missing`] finds nothing against the current template versions;
/// otherwise stores nothing and names each missing item. Already on: a new row records the newer
/// review.
///
/// # Errors
///
/// [`SpanishSwitchError`]; nothing is kept.
pub async fn turn_spanish_on(
    conn: &mut PgConnection,
    owner: Uuid,
    records: &SpanishRecords,
    now: Timestamp,
) -> Result<SpanishChange, SpanishSwitchError> {
    let gaps = missing(records, &spanish_template_versions()?);
    if !gaps.is_empty() {
        return Ok(SpanishChange::Refused(gaps));
    }
    store(conn, owner, Some(records), now).await
}

/// TXT-039: turns Spanish conversations off for `owner` at `now`. Already off: nothing stored.
///
/// # Errors
///
/// [`SpanishSwitchError::Database`] or [`SpanishSwitchError::Ops`]; nothing is kept.
pub async fn turn_spanish_off(
    conn: &mut PgConnection,
    owner: Uuid,
    now: Timestamp,
) -> Result<SpanishChange, SpanishSwitchError> {
    store(conn, owner, None, now).await
}

/// One new row (on with `records`, off without) and its audit row in one transaction, after the
/// table's lock, so two changes never share a time; nothing for "off" while already off.
async fn store(
    conn: &mut PgConnection,
    owner: Uuid,
    records: Option<&SpanishRecords>,
    now: Timestamp,
) -> Result<SpanishChange, SpanishSwitchError> {
    let mut tx = conn
        .begin()
        .await
        .map_err(|_| SpanishSwitchError::Database)?;
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(
             hashtextextended('messaging.spanish_switch', 480))) AS lock"#
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| SpanishSwitchError::Database)?;
    let was_on = latest(&mut tx).await?.is_some();
    if !was_on && records.is_none() {
        return Ok(SpanishChange::Unchanged);
    }
    let id = insert(&mut tx, owner, records, now).await?;
    let from = if was_on { "on" } else { "off" };
    let to = if records.is_some() { "on" } else { "off" };
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: SPANISH_SWITCH_ACTION,
        subject_kind: "messaging.spanish_switch",
        subject_id: id,
        change: Change::Status { from, to },
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| SpanishSwitchError::Ops)?;
    tx.commit()
        .await
        .map_err(|_| SpanishSwitchError::Database)?;
    Ok(SpanishChange::Changed)
}

/// Inserts one row at `now`, or just after the newest row's time; its ID.
async fn insert(
    tx: &mut PgConnection,
    owner: Uuid,
    records: Option<&SpanishRecords>,
    now: Timestamp,
) -> Result<Uuid, SpanishSwitchError> {
    let reviewed = records
        .map(|r| serde_json::to_value(&r.reviewed))
        .transpose()
        .map_err(|_| SpanishSwitchError::Database)?;
    sqlx::query_scalar!(
        "INSERT INTO messaging.spanish_switch (is_on, reviewed_on, reviewer, reviewed, note_on, \
         note_reference, sample, changed_by, changed_at) \
         SELECT $1, $2::text::date, $3, $4::jsonb, $5::text::date, $6, $7, $8, \
                GREATEST($9, (SELECT max(changed_at) + interval '1 microsecond' \
                              FROM messaging.spanish_switch)) \
         RETURNING id",
        records.is_some(),
        records.and_then(|r| r.reviewed_on).map(|d| d.to_string()),
        records.map(|r| r.reviewer.trim()),
        reviewed,
        records.and_then(|r| r.note_on).map(|d| d.to_string()),
        records.map(|r| r.note_reference.trim()),
        records.map(|r| r.sample.trim()),
        owner,
        now.to_sqlx() as _,
    )
    .fetch_one(tx)
    .await
    .map_err(|_| SpanishSwitchError::Database)
}

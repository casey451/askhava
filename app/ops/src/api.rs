//! The public face of `askhava-ops`: everything other crates may call (D-481).
//!
//! The audit log, written in the caller's transaction and exported by cursor (JOB-005, OPS-010,
//! OPS-011, PRIV-028); the off-database stream events, each a `core.task` row enqueued in the
//! writing transaction for `server`'s writer (OPS-011, PRIV-029, D-734); alerts holding record
//! IDs only (PRIV-006); and the owner pause switches with their append-only history (OPS-003).

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use serde_json::{Map, Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::service;

pub use crate::models::{
    Actor, AuditEntry, AuditRow, Change, NewAlert, OpsError, Severity, Switch, is_code_name,
};
pub use crate::service::{EXPORT_PAGE_MAX, export_audit, is_paused, raise_alert};

/// The `core.task` kind of one off-database stream event; `server` registers its writer
/// (`app/server/src/offdb_stream.rs`), since `ops` calls no adapter (OPS-011, D-491).
pub const STREAM_KIND: &str = "ops.offdb_stream";

/// One event for the off-database stream (OPS-011). Every field is an ID, a code name, a route
/// template, a status code or a time, never a contact value or free text (PRIV-028, PRIV-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent<'a> {
    /// An `ops.audit_log` row, by ID; the writer reads it and chains it to the row before.
    Audit(i64),
    /// One web request: its request ID (when it was a UUID), the matched route template (never
    /// the path, which can hold a token), the status code and when it ended. Never deduplicated:
    /// a client may send its own `x-request-id`, so every request keeps its own line.
    Request {
        /// The `x-request-id` the request carried, if it parsed as a UUID.
        request_id: Option<Uuid>,
        /// The route template, such as `/q/{token}`, or `unmatched`.
        route: &'a str,
        /// The HTTP status code returned.
        status: u16,
        /// When the response was produced.
        at: Timestamp,
    },
    /// One AI structured reading: its record ID and its enumerated fields, each a code name and
    /// a code value, so no AI-written text leaves through the stream (PRIV-029).
    AiReading {
        /// The `ai.reading` row's ID.
        reading_id: Uuid,
        /// The enumerated fields as `(name, value)`, both code names.
        fields: &'a [(&'static str, &'static str)],
    },
}

impl StreamEvent<'_> {
    /// The task payload and dedupe key for this event (OPS-011).
    fn task(&self) -> Result<(Value, Option<String>), OpsError> {
        Ok(match *self {
            Self::Audit(id) => (
                json!({"stream": "audit", "id": id}),
                Some(format!("offdb.audit.{id}")),
            ),
            Self::Request {
                request_id,
                route,
                status,
                at,
            } => (
                json!({"stream": "request", "request_id": request_id.map(|id| id.to_string()),
                    "route": route, "status": status, "at": at.to_string()}),
                None,
            ),
            Self::AiReading { reading_id, fields } => (
                json!({"stream": "ai_reading", "reading_id": reading_id.to_string(),
                    "fields": code_fields(fields)?}),
                Some(format!("offdb.ai_reading.{reading_id}")),
            ),
        })
    }
}

/// The enumerated AI fields as a JSON object, each name and value checked as a code name, so no
/// AI-written text can enter the stream (PRIV-029).
fn code_fields(fields: &[(&'static str, &'static str)]) -> Result<Map<String, Value>, OpsError> {
    if fields
        .iter()
        .any(|(n, v)| !is_code_name(n) || !is_code_name(v))
    {
        return Err(OpsError::NotACodeName("fields"));
    }
    Ok(fields
        .iter()
        .map(|(n, v)| ((*n).to_owned(), Value::from(*v)))
        .collect())
}

/// Enqueues `event` for the off-database stream on the caller's connection, so it commits with
/// the row it copies (OPS-011, rust brain R11, D-734). An event already queued is not queued
/// twice (its dedupe key). It is due at once (the wall clock: a copy is not a business timer);
/// `server`'s handler writes it to storage.
///
/// # Errors
///
/// [`OpsError::NotACodeName`] for an AI field that is not a code name, before anything is
/// written, or [`OpsError::Database`].
pub async fn stream_event(db: &mut PgConnection, event: &StreamEvent<'_>) -> Result<(), OpsError> {
    let (payload, dedupe_key) = event.task()?;
    let mut task = NewTask::new(STREAM_KIND, payload, Timestamp::now());
    task.dedupe_key = dedupe_key;
    enqueue(db, &task).await.map_err(|_| OpsError::Database)?;
    Ok(())
}

/// Writes one audit row on the caller's connection and enqueues its off-database copy in the same
/// transaction, returning the row's ID (JOB-005, OPS-010, OPS-011, D-734). Call it inside the
/// transaction that makes the change. The row holds record IDs and code names only (PRIV-028).
///
/// # Errors
///
/// [`OpsError::NotACodeName`] before anything is written, or [`OpsError::Database`].
pub async fn audit(db: &mut PgConnection, entry: &AuditEntry) -> Result<i64, OpsError> {
    let id = service::audit(db, entry).await?;
    stream_event(db, &StreamEvent::Audit(id)).await?;
    Ok(id)
}

/// Turns `switch` on or off, logging the change with its actor in `ops.switch_history` and
/// `ops.audit_log` (OPS-003, SEC-006), and enqueues the audit row's off-database copy in the same
/// transaction (OPS-011, D-734). Returns false, writing nothing, when it was already in that state.
/// The row is the switch's newest audit row: the flip holds the switch row's lock until commit,
/// so no other change of this switch can have a later one.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn set_switch(
    db: &mut PgConnection,
    switch: Switch,
    paused: bool,
    actor: Actor,
) -> Result<bool, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    if !service::set_switch(&mut tx, switch, paused, actor).await? {
        return Ok(false);
    }
    let (name, scope_id) = switch.columns();
    let id = sqlx::query_scalar!(
        r#"SELECT max(a.id) AS "id!" FROM ops.audit_log a JOIN ops.pause_switch s
           ON a.subject_kind = 'ops.pause_switch' AND a.subject_id = s.id
           WHERE s.name = $1 AND s.scope_id IS NOT DISTINCT FROM $2"#,
        name,
        scope_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    stream_event(&mut tx, &StreamEvent::Audit(id)).await?;
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(true)
}

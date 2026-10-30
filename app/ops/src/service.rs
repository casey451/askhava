//! Writing and reading the audit log, alerts and pause switches (JOB-005, OPS-003, OPS-010, OPS-011, PRIV-028).
//!
//! Every function takes the caller's connection, so a module passes `&mut *tx` and the audit row,
//! the alert or the switch change commits or rolls back with its own state change (JOB-005,
//! rust brain R11). `ops` is layer 1: it records rows and never sends or reads anything outside
//! its own schema. The export is a paged read in `id` order after a cursor (OPS-010). It is not a
//! stream cursor: identity values are taken at insert and can commit out of order, so a row may
//! appear below a cursor already read. T017's off-database copy (OPS-011) must use a per-row
//! `core.task` or outbox row inserted in the audit row's transaction, or read only below a safe
//! horizon (D-729).

use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{
    Actor, AuditEntry, AuditRow, Change, NewAlert, OpsError, Switch, is_code_name,
};

/// The most rows one export call returns.
pub const EXPORT_PAGE_MAX: i64 = 1000;

/// Writes one audit row on the caller's connection and returns its ID. Call it inside the
/// transaction that makes the change, so the change and its row commit together (JOB-005). The row
/// holds record IDs and code names only (PRIV-028); the table is append-only (OPS-010).
///
/// # Errors
///
/// [`OpsError::NotACodeName`] before anything is written, or [`OpsError::Database`].
pub async fn audit(db: &mut PgConnection, entry: &AuditEntry) -> Result<i64, OpsError> {
    entry.check()?;
    let (actor_kind, actor_id) = entry.actor.columns();
    let (from, to, field) = entry.change.columns();
    sqlx::query_scalar!(
        "INSERT INTO ops.audit_log \
         (actor_kind, actor_id, action, subject_kind, subject_id, from_status, to_status, field) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        actor_kind,
        actor_id,
        entry.action,
        entry.subject_kind,
        entry.subject_id,
        from,
        to,
        field,
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// Up to `limit` audit rows (at most [`EXPORT_PAGE_MAX`]) with an ID above `after`, oldest first:
/// the export, read page by page with the last row's ID as the next cursor (OPS-010). A row whose
/// transaction commits after a later ID was read can be missed, so this is an export of a settled
/// log, never the off-database stream (OPS-011, D-729).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn export_audit(
    db: &mut PgConnection,
    after: i64,
    limit: i64,
) -> Result<Vec<AuditRow>, OpsError> {
    let rows = sqlx::query!(
        "SELECT id, at, actor_kind, actor_id, action, subject_kind, subject_id, from_status, \
         to_status, field FROM ops.audit_log WHERE id > $1 ORDER BY id LIMIT $2",
        after,
        limit.clamp(0, EXPORT_PAGE_MAX),
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| AuditRow {
            id: r.id,
            at: r.at.to_jiff(),
            actor_kind: r.actor_kind,
            actor_id: r.actor_id,
            action: r.action,
            subject_kind: r.subject_kind,
            subject_id: r.subject_id,
            from_status: r.from_status,
            to_status: r.to_status,
            field: r.field,
        })
        .collect())
}

/// Records an alert, holding the record it is about by ID only (PRIV-006, PRIV-028), and returns
/// its ID. Sending it is a `messaging` handler's work, never `ops`'s.
///
/// # Errors
///
/// [`OpsError::NotACodeName`] before anything is written, or [`OpsError::Database`].
pub async fn raise_alert(db: &mut PgConnection, alert: &NewAlert) -> Result<Uuid, OpsError> {
    if !is_code_name(alert.kind) {
        return Err(OpsError::NotACodeName("kind"));
    }
    if alert.subject.is_some_and(|(kind, _)| !is_code_name(kind)) {
        return Err(OpsError::NotACodeName("subject_kind"));
    }
    sqlx::query_scalar!(
        "INSERT INTO ops.alert (kind, severity, subject_kind, subject_id) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        alert.kind,
        alert.severity.as_str(),
        alert.subject.map(|(kind, _)| kind),
        alert.subject.map(|(_, id)| id),
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// Whether `switch` is on; a switch never set is off (OPS-003).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn is_paused(db: &mut PgConnection, switch: Switch) -> Result<bool, OpsError> {
    let (name, scope_id) = switch.columns();
    let paused = sqlx::query_scalar!(
        "SELECT paused FROM ops.pause_switch WHERE name = $1 AND scope_id IS NOT DISTINCT FROM $2",
        name,
        scope_id,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(paused.unwrap_or(false))
}

/// Turns `switch` on or off and logs the change with its actor in the append-only
/// `ops.switch_history` and in `ops.audit_log` (action `switch_on` or `switch_off`, subject the
/// `ops.pause_switch` row; OPS-003, SEC-006). Returns false, writing nothing, when it was already
/// in that state. The flip, history and audit row commit together in one transaction (a savepoint
/// inside the caller's); the row lock taken by the flip makes two owners' changes queue.
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
    let Some(switch_id) = flip(&mut tx, switch, paused).await? else {
        return Ok(false);
    };
    record_history(&mut tx, switch, paused, actor).await?;
    let entry = AuditEntry {
        actor,
        action: if paused { "switch_on" } else { "switch_off" },
        subject_kind: "ops.pause_switch",
        subject_id: switch_id,
        change: Change::Action,
    };
    audit(&mut tx, &entry).await?;
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(true)
}

/// Appends one row to `ops.switch_history`: the switch, its new state and who set it (OPS-003).
async fn record_history(
    db: &mut PgConnection,
    switch: Switch,
    paused: bool,
    actor: Actor,
) -> Result<(), OpsError> {
    let (name, scope_id) = switch.columns();
    let (actor_kind, actor_id) = actor.columns();
    sqlx::query!(
        "INSERT INTO ops.switch_history (name, scope_id, paused, actor_kind, actor_id) \
         VALUES ($1, $2, $3, $4, $5)",
        name,
        scope_id,
        paused,
        actor_kind,
        actor_id,
    )
    .execute(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(())
}

/// Sets the switch row to `paused` if it is not already, returning the row's ID when it changed. A
/// switch never set is off, so turning it on inserts its row and turning it off only updates one
/// that is on.
async fn flip(
    db: &mut PgConnection,
    switch: Switch,
    paused: bool,
) -> Result<Option<Uuid>, OpsError> {
    let (name, scope_id) = switch.columns();
    let changed = if paused {
        sqlx::query_scalar!(
            "INSERT INTO ops.pause_switch AS s (name, scope_id, paused) VALUES ($1, $2, true) \
             ON CONFLICT (name, scope_id) DO UPDATE SET paused = true, changed_at = now() \
             WHERE NOT s.paused RETURNING id",
            name,
            scope_id,
        )
        .fetch_optional(db)
        .await
    } else {
        sqlx::query_scalar!(
            "UPDATE ops.pause_switch SET paused = false, changed_at = now() \
             WHERE name = $1 AND scope_id IS NOT DISTINCT FROM $2 AND paused RETURNING id",
            name,
            scope_id,
        )
        .fetch_optional(db)
        .await
    };
    changed.map_err(|_| OpsError::Database)
}

//! Safe mode: pauses charges and releases, and nothing else, while a pushed High alert stays
//! unacknowledged (OPS-001, OPS-023, OPS-024, FEE-038; D-075, D-133, D-278, D-1427, D-3878, T487).
//!
//! A High alert here is an open `ops.alert` of severity `high` or `critical` with an
//! `ops.alert_push` row (written by T204's push, OPS-029, OPS-032), other than the text-delivery
//! alert, which never starts safe mode (OPS-023, D-278). "Unacknowledged" is `resolved_at IS
//! NULL`: every acknowledgement, by Casey or automatic (T105), resolves the alert.
//!
//! The state is the one `ops.safe_mode` row (migration 0192). [`run_safe_mode_check`], the
//! [`SAFE_MODE_KIND`] task re-queued every [`SAFE_MODE_EVERY`], turns it on once such an alert's
//! push is [`SAFE_MODE_AFTER`] old and off once no such alert is open (any push age: "no other
//! High alert open", TD-OPS-06), each change with one audit row. [`safe_mode_on`] answers the
//! flag OR an overdue alert by the database's `now()`, so safe mode never fails open between
//! checks or with the worker stopped. Only `charges_paused` and `server`'s `release_sink.rs` and
//! `release_hold.rs` read it, texts never (D-133, D-4026); nothing here starts either (FEE-016).

use askhava_core::api::{HandlerError, NewTask, enqueue};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::alerts::DELIVERY_ALERT;
use crate::api::audit;
use crate::models::{Actor, AuditEntry, Change, OpsError};

/// The `core.task` kind of the safe-mode check (OPS-001); `server` registers
/// [`run_safe_mode_check`] for it and `worker.rs` queues the first at start.
pub const SAFE_MODE_KIND: &str = "ops.safe_mode_check";
/// How often the safe-mode check runs.
pub const SAFE_MODE_EVERY: SignedDuration = SignedDuration::from_mins(1);
/// How long a pushed High alert may stay unacknowledged before safe mode starts (OPS-001).
pub const SAFE_MODE_AFTER: SignedDuration = SignedDuration::from_hours(24);
/// The audit action written when safe mode turns on (subject `ops.safe_mode`, nil ID).
pub const SAFE_MODE_ON_ACTION: &str = "ops.safe_mode_on";
/// The audit action written when safe mode turns off.
pub const SAFE_MODE_OFF_ACTION: &str = "ops.safe_mode_off";

const FAILED: HandlerError = HandlerError("the safe-mode check database call failed");

/// OPS-001, OPS-024, D-3878: true while safe mode is on: its stored flag is on, or a pushed High
/// alert is still unacknowledged [`SAFE_MODE_AFTER`] or more after its push by the database's
/// clock (so it engages at once, with or without the worker).
///
/// # Errors
/// [`OpsError::Database`].
pub async fn safe_mode_on(db: &mut PgConnection) -> Result<bool, OpsError> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE((SELECT "on" FROM ops.safe_mode), false) OR EXISTS (
             SELECT 1 FROM ops.alert a JOIN ops.alert_push p ON p.alert_id = a.id
             WHERE a.resolved_at IS NULL AND a.severity IN ('high', 'critical') AND a.kind <> $1
               AND p.pushed_at <= now() - make_interval(secs => $2)) AS "on!""#,
        DELIVERY_ALERT,
        SAFE_MODE_AFTER.as_secs_f64(),
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-001, OPS-023, TD-OPS-01, TD-OPS-06: sets the stored flag at `now`: on when a pushed High
/// alert's push is [`SAFE_MODE_AFTER`] or more before `now` (24:00:00 starts it, 23:59:59 does
/// not); while on, off only when no pushed High alert is open. A change writes its audit row in
/// the same transaction, under the row's lock; a repeat changes nothing. Returns the flag.
///
/// # Errors
/// [`OpsError::Database`].
pub async fn check_safe_mode(db: &mut PgConnection, now: Timestamp) -> Result<bool, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    let cutoff = now.saturating_sub(SAFE_MODE_AFTER).unwrap_or(now);
    let row = sqlx::query!(
        r#"SELECT s."on" AS was_on,
             EXISTS (SELECT 1 FROM ops.alert a JOIN ops.alert_push p ON p.alert_id = a.id
               WHERE a.resolved_at IS NULL AND a.severity IN ('high', 'critical') AND a.kind <> $1
                 AND p.pushed_at <= $2) AS "overdue!",
             EXISTS (SELECT 1 FROM ops.alert a JOIN ops.alert_push p ON p.alert_id = a.id
               WHERE a.resolved_at IS NULL AND a.severity IN ('high', 'critical') AND a.kind <> $1)
               AS "open!"
           FROM ops.safe_mode s FOR UPDATE OF s"#,
        DELIVERY_ALERT,
        cutoff.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let on = row.overdue || (row.was_on && row.open);
    if on != row.was_on {
        set_flag(&mut tx, on, now).await?;
    }
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(on)
}

/// Stores the flag `on` (with `since` = `now` when on) and its audit row (OPS-001, JOB-005).
async fn set_flag(tx: &mut PgConnection, on: bool, now: Timestamp) -> Result<(), OpsError> {
    let since = on.then(|| now.to_sqlx());
    sqlx::query!(
        r#"UPDATE ops.safe_mode SET "on" = $1, since = $2"#,
        on,
        since as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: if on {
            SAFE_MODE_ON_ACTION
        } else {
            SAFE_MODE_OFF_ACTION
        },
        subject_kind: "ops.safe_mode",
        subject_id: Uuid::nil(),
        change: Change::Action,
    };
    audit(tx, &entry).await.map(|_| ())
}

/// Queues the next safe-mode check one [`SAFE_MODE_EVERY`] after `now`, once per minute (its
/// dedupe key by minute number), so a start and a running check queue one task (JOB-006).
///
/// # Errors
/// [`OpsError::Database`].
pub async fn schedule_safe_mode(db: &mut PgConnection, now: Timestamp) -> Result<(), OpsError> {
    let run_at = now.saturating_add(SAFE_MODE_EVERY).unwrap_or(now);
    let minute = run_at.as_second().div_euclid(60);
    let mut task = NewTask::new(SAFE_MODE_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{SAFE_MODE_KIND}.{minute}"));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| OpsError::Database)
}

/// The [`SAFE_MODE_KIND`] handler (OPS-001, JOB-006): queues the next check, then runs
/// [`check_safe_mode`] at `now`. Safe to run twice: a repeat changes nothing.
///
/// # Errors
/// A fixed [`HandlerError`] when the database cannot be reached.
pub async fn run_safe_mode_check(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_safe_mode(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    check_safe_mode(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(|_| FAILED)
}

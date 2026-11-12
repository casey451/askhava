//! The hourly dead-letter check (T327, D-2285, T366, D-2640; OPS-004, JOB-006, IC-27).
//!
//! `core` fails a task whose kind has no registered handler with `NO_HANDLER` and dead-letters it
//! after its attempts; `core` may not call `ops`, so it raises nothing. This worker task does,
//! once an hour: in the test and live environments it reads each kind with such a dead letter
//! (`core::api::dead_without_handler`) and raises one High [`ALERT`] per kind unless that kind's
//! alert is still open (`ops::api::raise_alert_once`: the check and the raise under one advisory
//! lock, so two checks at once raise one alert, D-2852). In dev and ci it raises nothing, so a
//! local queue of stand-in tasks never alerts.
//!
//! `ops.alert` holds a kind, a severity and a subject only. The alert names the task kind as its
//! subject kind (a code name the enqueuing code wrote) with an ID derived from the kind
//! ([`kind_id`]), so one kind is one subject; the count goes on the alert's one error log line,
//! with the kind and never a payload. Each run first queues the next hour's run under the hour's
//! dedupe key, so a restart, a repeat or the worker's start-up seed ([`schedule`]) never doubles it.
//!
//! Every other dead letter alerts too (T366, D-2640; a job timer, text or check-in that ran out of
//! attempts): each run reads the dead letters of the last [`LOOK_BACK`] by kind and UTC day of
//! death (`core::api::dead_by_kind_since`, which leaves `NO_HANDLER` ones to the alert above) and
//! raises one High [`DEAD_ALERT`] per kind and day, subject the kind with [`day_id`]. The once is a
//! [`DEAD_ALERTED`] audit row on the same subject (`ops::api::audit_once`, in the alert's
//! transaction), so a resolved alert is not raised again for that day; the count goes on the error
//! log line, never a payload.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};

use askhava_core::api::{
    Clock, Environment, HandlerError, NewTask, Registry, TaskError, dead_by_kind_since,
    dead_without_handler, enqueue,
};
use askhava_ops::api::{
    Actor, AuditEntry, Change, Severity, audit_once, is_code_name, raise_alert_once,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// The `core.task` kind of the hourly check.
pub const CHECK_KIND: &str = "ops.no_handler_check";
/// The High alert raised once per kind with a `NO_HANDLER` dead letter (OPS-004).
pub const ALERT: &str = "core.task_no_handler";
/// The High alert raised once per kind and day with any other dead letter (OPS-004, T366).
pub const DEAD_ALERT: &str = "core.task_dead_letter";
/// The audit action that marks a kind and day as alerted, so it alerts once (T366).
pub const DEAD_ALERTED: &str = "ops.dead_letter_alerted";
/// How far back each run reads dead letters: two days, so one dead just before midnight is still
/// read by the first run after it.
const LOOK_BACK: SignedDuration = SignedDuration::from_hours(48);
/// How often the check runs.
const EVERY: SignedDuration = SignedDuration::from_hours(1);
/// The handler's failure when the queue or the alert table cannot be reached.
const DB_FAILED: HandlerError = HandlerError("the no-handler check could not reach the database");

/// OPS-004, D-2285: registers [`CHECK_KIND`]; `environment` decides whether it raises (test and
/// live only) and `clock` when the next run is due.
pub fn register(registry: &mut Registry, environment: Environment, clock: Arc<dyn Clock>) {
    registry.register(CHECK_KIND, move |pool, _task| {
        let now = clock.now();
        async move { run_check(&pool, environment, now).await }
    });
}

/// JOB-006: queues the run for the hour holding `now`, due at `now`, under that hour's dedupe key;
/// a second call in the same hour queues nothing.
///
/// # Errors
/// [`TaskError::Db`] when the queue cannot be reached.
pub async fn schedule(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let hour = now.as_second().div_euclid(3600);
    let mut task = NewTask::new(CHECK_KIND, json!({}), now);
    task.dedupe_key = Some(format!("{CHECK_KIND}.{hour}"));
    enqueue(db, &task).await.map(|_| ())
}

/// OPS-004, JOB-006, D-2285: queues the next hour's run, then, in test and live, raises one High
/// [`ALERT`] per kind with a `NO_HANDLER` dead letter whose alert is not open.
///
/// # Errors
/// A fixed sentence when the queue or the alert table cannot be reached.
pub async fn run_check(
    pool: &PgPool,
    environment: Environment,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let next = now.checked_add(EVERY).map_err(|_| DB_FAILED)?;
    schedule(&mut conn, next).await.map_err(|_| DB_FAILED)?;
    if matches!(environment, Environment::Dev | Environment::Ci) {
        return Ok(());
    }
    let dead = dead_without_handler(&mut conn)
        .await
        .map_err(|_| DB_FAILED)?;
    for (kind, count) in dead {
        alert_kind(&mut conn, &kind, count).await?;
    }
    let since = now.checked_sub(LOOK_BACK).map_err(|_| DB_FAILED)?;
    let dead = dead_by_kind_since(&mut conn, since)
        .await
        .map_err(|_| DB_FAILED)?;
    for (kind, day, count) in dead {
        alert_dead_day(&mut conn, &kind, day, count).await?;
    }
    Ok(())
}

/// OPS-004, D-2640: raises one High [`DEAD_ALERT`] for `kind`'s dead letters of UTC day `day`
/// unless that kind and day was alerted before ([`DEAD_ALERTED`]); logs the kind and `count`.
async fn alert_dead_day(
    db: &mut PgConnection,
    kind: &str,
    day: i64,
    count: i64,
) -> Result<(), HandlerError> {
    let Some(name) = intern(kind) else {
        tracing::error!("a dead-lettered task kind is not a code name");
        return Ok(());
    };
    let subject = (name, day_id(name, day));
    let mark = AuditEntry {
        actor: Actor::System,
        action: DEAD_ALERTED,
        subject_kind: subject.0,
        subject_id: subject.1,
        change: Change::Action,
    };
    let mut tx = db.begin().await.map_err(|_| DB_FAILED)?;
    if !audit_once(&mut tx, &mark).await.map_err(|_| DB_FAILED)? {
        return Ok(());
    }
    raise_alert_once(&mut tx, DEAD_ALERT, Severity::High, subject)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)?;
    tracing::error!(
        task_kind = name,
        day,
        count,
        "dead-lettered tasks of a kind"
    );
    Ok(())
}

/// The [`DEAD_ALERT`] subject ID of a task kind and UTC day number: [`kind_id`] of `<kind>.<day>`,
/// so one kind and day is one subject.
#[must_use]
pub fn day_id(kind: &str, day: i64) -> Uuid {
    kind_id(&format!("{kind}.{day}"))
}

/// Raises the alert for `kind` unless one is open; logs the kind and `count` when it does.
async fn alert_kind(db: &mut PgConnection, kind: &str, count: i64) -> Result<(), HandlerError> {
    let Some(name) = intern(kind) else {
        tracing::error!("a dead-lettered task kind is not a code name");
        return Ok(());
    };
    let raised = raise_alert_once(db, ALERT, Severity::High, (name, kind_id(name)))
        .await
        .map_err(|_| DB_FAILED)?;
    if raised.is_none() {
        return Ok(());
    }
    tracing::error!(
        task_kind = name,
        count,
        "dead-lettered tasks of a kind with no handler"
    );
    Ok(())
}

/// The alert's subject ID for a task kind: the first 16 bytes of its SHA256 digest, so the same kind is
/// always the same subject and the open-alert check finds it.
#[must_use]
pub fn kind_id(kind: &str) -> Uuid {
    let digest = Sha256::digest(kind.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

/// A `'static` copy of a code-name `kind`, kept once per process (an alert subject is `'static`).
/// Only code names (64 characters at most) are kept, so the set is bounded by the kinds the code
/// queues; anything else is `None`.
fn intern(kind: &str) -> Option<&'static str> {
    static KINDS: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    if !is_code_name(kind) {
        return None;
    }
    let mut kinds = KINDS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(known) = kinds.get(kind) {
        return Some(known);
    }
    let kept: &'static str = Box::leak(kind.to_owned().into_boxed_str());
    kinds.insert(kept);
    Some(kept)
}

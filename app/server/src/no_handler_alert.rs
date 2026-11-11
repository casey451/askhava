//! The hourly no-handler check (T327, D-2285; OPS-004, JOB-006, IC-27).
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

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};

use askhava_core::api::{
    Clock, Environment, HandlerError, NewTask, Registry, TaskError, dead_without_handler, enqueue,
};
use askhava_ops::api::{Severity, is_code_name, raise_alert_once};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The `core.task` kind of the hourly check.
pub const CHECK_KIND: &str = "ops.no_handler_check";
/// The High alert raised once per kind with a `NO_HANDLER` dead letter (OPS-004).
pub const ALERT: &str = "core.task_no_handler";
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
    Ok(())
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

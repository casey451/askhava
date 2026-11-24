//! The 5-minute in-process health check and its alert (T624, D-4280; OPS-004, OPS-023, D-220).
//!
//! OPS-004 alerts Casey when "the 5-minute health check fails twice". This worker task runs, once
//! per 5-minute slot (the epoch second divided by [`SLOT_SECS`]), the check `/healthz` runs
//! (`healthz::check`: the database round trip and the worker-down test; an unreachable web process
//! is UptimeRobot's alone, OPS-007). Each run checks first, writing nothing, then in one
//! transaction queues the next slot's run carrying this check's result in its payload
//! (`{"health_failed": bool, "slot": <slot>}`, D-4463) and, in test and live only, raises or
//! clears the alert: a failure after a failure raises one High [`HEALTH_ALERT`] through
//! `ops::api::raise_kind_once`, a pass acknowledges an open one through
//! `ops::api::acknowledge_cleared` (OPS-023). So two failures in a row need no stored row and no
//! in-memory state. In dev and ci each run still checks and queues the next slot but raises
//! nothing, as `no_handler_alert` does.
//!
//! Every run's due time is its slot's start and its dedupe key is `<kind>.<slot>`, shared by the
//! start-up seed ([`schedule`]) and every chained run, so a restart in a slot already queued or run
//! queues nothing and the chain never multiplies. The next slot is 1 plus the larger of the run's
//! own slot and the slot holding `now` ([`next_slot`]), so an application clock behind the
//! database clock never repeats the run's own key and ends the chain. A dead-lettered run stops the
//! chain until the next start-up seeds it again; T366's dead-letter alert reports that.

use std::sync::Arc;

use askhava_core::api::{Clock, Environment, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{HEALTH_ALERT, Severity, acknowledge_cleared, raise_kind_once};
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, PgPool};

use crate::healthz::{HealthLimits, check};

/// The `core.task` kind of the 5-minute health check.
pub const CHECK_KIND: &str = "ops.health_check";
/// The length of one slot, in seconds: the check runs once per 5 minutes (OPS-004).
pub const SLOT_SECS: i64 = 300;
/// The handler's failure when the queue or the alert table cannot be reached.
const DB_FAILED: HandlerError = HandlerError("the health check could not reach the database");
/// The handler's failure when a run's payload is not the one [`schedule`] and [`run_check`] write.
const BAD_PAYLOAD: HandlerError = HandlerError("the health check run has a malformed payload");

/// One queued run: its own slot and whether the check before it failed (D-4463).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthRun {
    /// The 5-minute slot this run is due in, the one its dedupe key names.
    pub slot: i64,
    /// Whether the previous run's check failed; the start-up seed carries `false`.
    pub health_failed: bool,
}

impl HealthRun {
    /// Parses a run's payload, `{"health_failed": bool, "slot": i64}`; anything else is `None`.
    #[must_use]
    pub fn from_payload(payload: &Value) -> Option<Self> {
        Some(Self {
            slot: payload.get("slot")?.as_i64()?,
            health_failed: payload.get("health_failed")?.as_bool()?,
        })
    }
}

/// The 5-minute slot holding `at`.
#[must_use]
pub fn slot_of(at: Timestamp) -> i64 {
    at.as_second().div_euclid(SLOT_SECS)
}

/// D-4280: the slot after a run of `own` slot that ran at `now`: 1 plus the larger of the two, so
/// a clock behind the run's due time still moves the chain on.
#[must_use]
pub fn next_slot(own: i64, now: Timestamp) -> i64 {
    own.max(slot_of(now)).saturating_add(1)
}

/// OPS-004, D-4280: registers [`CHECK_KIND`]; `environment` decides whether it raises (test and
/// live only), `limits` is `/healthz`'s worker-down limit and `clock` gives each run's `now`.
pub fn register(
    registry: &mut Registry,
    environment: Environment,
    limits: HealthLimits,
    clock: Arc<dyn Clock>,
) {
    registry.register(CHECK_KIND, move |pool, task| {
        let now = clock.now();
        async move { run_payload(&pool, environment, &limits, &task.payload, now).await }
    });
}

/// JOB-006, D-4280: queues the run for the slot holding `now`, due at the slot's start, carrying
/// no earlier failure; a second call in the same slot queues nothing.
///
/// # Errors
/// [`TaskError::Db`] when the queue cannot be reached.
pub async fn schedule(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    queue(db, slot_of(now), false).await
}

/// Queues the run of `slot`, due at its start, carrying `health_failed`, under `<kind>.<slot>`.
async fn queue(db: &mut PgConnection, slot: i64, health_failed: bool) -> Result<(), TaskError> {
    let start = slot.saturating_mul(SLOT_SECS);
    let due = Timestamp::from_second(start).map_err(|_| TaskError::Db)?;
    let payload = json!({"health_failed": health_failed, "slot": slot});
    let mut task = NewTask::new(CHECK_KIND, payload, due);
    task.dedupe_key = Some(format!("{CHECK_KIND}.{slot}"));
    enqueue(db, &task).await.map(|_| ())
}

/// The handler body: parses `payload` once, then [`run_check`].
///
/// # Errors
/// A fixed sentence for a malformed payload or an unreachable database.
pub async fn run_payload(
    pool: &PgPool,
    environment: Environment,
    limits: &HealthLimits,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let run = HealthRun::from_payload(payload).ok_or(BAD_PAYLOAD)?;
    run_check(pool, environment, limits, run, now).await
}

/// OPS-004, OPS-023, D-220, D-4280: checks the database and the worker at `now`, then in one
/// transaction queues the next slot's run with this result and, in test and live, raises one High
/// [`HEALTH_ALERT`] on a second failure in a row or acknowledges an open one on a pass.
///
/// # Errors
/// A fixed sentence when the queue or the alert table cannot be reached.
pub async fn run_check(
    pool: &PgPool,
    environment: Environment,
    limits: &HealthLimits,
    run: HealthRun,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = !check(pool, now, limits).await.healthy();
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let mut tx = conn.begin().await.map_err(|_| DB_FAILED)?;
    queue(&mut tx, next_slot(run.slot, now), failed)
        .await
        .map_err(|_| DB_FAILED)?;
    if !matches!(environment, Environment::Dev | Environment::Ci) {
        alert(&mut tx, run.health_failed && failed, failed, now).await?;
    }
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// Raises the alert on a second failure (`twice`), clears it on a pass, else does nothing.
async fn alert(
    db: &mut PgConnection,
    twice: bool,
    failed: bool,
    now: Timestamp,
) -> Result<(), HandlerError> {
    if twice {
        let raised = raise_kind_once(db, HEALTH_ALERT, Severity::High)
            .await
            .map_err(|_| DB_FAILED)?;
        if raised.is_some() {
            tracing::error!("the health check failed twice in a row");
        }
    } else if !failed {
        acknowledge_cleared(db, HEALTH_ALERT, now)
            .await
            .map_err(|_| DB_FAILED)?;
    }
    Ok(())
}

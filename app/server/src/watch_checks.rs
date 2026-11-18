//! The hourly vault-read check and the daily database-volume check (T525; SEC-047, OPS-052,
//! D-3880, D-4044, D-4280).
//!
//! Two worker tasks, each first queued by `worker.rs`'s `schedule_start` ([`schedule_read_check`],
//! [`schedule_volume_check`]) under a dedupe key for its hour or UTC day, and each queuing its
//! next run before it checks, so a restart, a repeat or the start-up seed never doubles one:
//!
//! - [`READ_CHECK_KIND`], hourly: `privacy::api::check_read_rates` for the hour just ended, which
//!   raises SEC-047's pushed High spike alert once per reader-purpose and hour (an unregistered
//!   purpose already alerts at the read itself).
//! - [`VOLUME_CHECK_KIND`], daily: `ops::api::check_volume` with `ASKHAVA_DB_VOLUME_BYTES`, Low
//!   alert from 70%, High from 85% (OPS-052). [`register`] refuses a missing volume setting in
//!   test and live, naming the key; in dev and Ci the check raises nothing.
//!
//! Neither task carries a payload or logs a value; a failure is a fixed sentence, and the task
//! retries under `core`'s backoff.

use std::io;
use std::sync::Arc;

use askhava_core::api::{
    Clock, Environment, HandlerError, NewTask, Registry, Settings, TaskError, enqueue,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::{PgConnection, PgPool};

/// The `core.task` kind of the hourly vault-read check (SEC-047).
pub const READ_CHECK_KIND: &str = "privacy.read_rate_check";
/// The `core.task` kind of the daily database-volume check (OPS-052).
pub const VOLUME_CHECK_KIND: &str = "ops.volume_check";
/// The setting holding the volume's size in bytes.
const VOLUME_KEY: &str = "ASKHAVA_DB_VOLUME_BYTES";
/// One hour: the read check's period and how far back its hour lies.
const HOUR: SignedDuration = SignedDuration::from_hours(1);
/// One day: the volume check's period.
const DAY: SignedDuration = SignedDuration::from_hours(24);
/// The handlers' failure when the queue or a check cannot reach the database.
const DB_FAILED: HandlerError = HandlerError("a watch check could not reach the database");

/// SEC-047, OPS-052, D-4280: registers both checks. The volume check gets the setting in test and
/// live, and no volume in dev and Ci, where it raises nothing.
///
/// # Errors
///
/// `ASKHAVA_DB_VOLUME_BYTES` missing in test or live, naming the key only.
pub fn register(
    registry: &mut Registry,
    settings: &Settings,
    clock: &Arc<dyn Clock>,
) -> io::Result<()> {
    let volume = match (settings.environment(), settings.db_volume_bytes()) {
        (Environment::Dev | Environment::Ci, _) => None,
        (_, Some(bytes)) => Some(bytes),
        (_, None) => return Err(io::Error::other(format!("missing {VOLUME_KEY}"))),
    };
    register_checks(registry, volume, clock);
    Ok(())
}

/// SEC-047, OPS-052: registers [`READ_CHECK_KIND`] and [`VOLUME_CHECK_KIND`]; `volume` is the
/// volume's size in bytes, `None` for a volume check that raises nothing; `clock` dates each run.
pub fn register_checks(registry: &mut Registry, volume: Option<u64>, clock: &Arc<dyn Clock>) {
    let read_clock = Arc::clone(clock);
    registry.register(READ_CHECK_KIND, move |pool, _task| {
        let now = read_clock.now();
        async move { run_read_check(&pool, now).await }
    });
    let volume_clock = Arc::clone(clock);
    registry.register(VOLUME_CHECK_KIND, move |pool, _task| {
        let now = volume_clock.now();
        async move { run_volume_check(&pool, volume, now).await }
    });
}

/// SEC-047: queues the read check for the hour holding `now`, due at `now`; a second call in the
/// same hour queues nothing.
///
/// # Errors
///
/// [`TaskError::Db`] when the queue cannot be reached.
pub async fn schedule_read_check(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let hour = now.as_second().div_euclid(3600);
    queue(db, READ_CHECK_KIND, hour, now).await
}

/// OPS-052: queues the volume check for the UTC day holding `now`, due at `now`; a second call
/// the same day queues nothing.
///
/// # Errors
///
/// [`TaskError::Db`] when the queue cannot be reached.
pub async fn schedule_volume_check(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let day = now.as_second().div_euclid(86_400);
    queue(db, VOLUME_CHECK_KIND, day, now).await
}

/// Queues `kind` due at `now` under the dedupe key `<kind>.<period>`.
async fn queue(
    db: &mut PgConnection,
    kind: &'static str,
    period: i64,
    now: Timestamp,
) -> Result<(), TaskError> {
    let mut task = NewTask::new(kind, json!({}), now);
    task.dedupe_key = Some(format!("{kind}.{period}"));
    enqueue(db, &task).await.map(|_| ())
}

/// SEC-047: queues the next hour's run, then checks the hour just ended (the hour holding
/// `now` less one hour).
///
/// # Errors
///
/// A fixed sentence when the queue or the check cannot reach the database.
pub async fn run_read_check(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let next = now.checked_add(HOUR).map_err(|_| DB_FAILED)?;
    schedule_read_check(&mut conn, next)
        .await
        .map_err(|_| DB_FAILED)?;
    let ended = now.checked_sub(HOUR).map_err(|_| DB_FAILED)?;
    askhava_privacy::api::check_read_rates(&mut conn, ended)
        .await
        .map(|_| ())
        .map_err(|_| DB_FAILED)
}

/// OPS-052: queues the next day's run, then, with a volume size, compares the database's size
/// with it and raises its level's alert once while it lasts; with none (dev and Ci) nothing.
///
/// # Errors
///
/// A fixed sentence when the queue or the check cannot reach the database.
pub async fn run_volume_check(
    pool: &PgPool,
    volume: Option<u64>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let next = now.checked_add(DAY).map_err(|_| DB_FAILED)?;
    schedule_volume_check(&mut conn, next)
        .await
        .map_err(|_| DB_FAILED)?;
    let Some(volume) = volume else {
        return Ok(());
    };
    askhava_ops::api::check_volume(&mut conn, volume)
        .await
        .map(|_| ())
        .map_err(|_| DB_FAILED)
}

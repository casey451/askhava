//! The daily deliverability re-test run (BIZ-042, D-362, D-380, D-462, D-4361): the
//! `businesses.deliverability_retests` worker task ([`RETEST_KIND`]).
//!
//! It runs once a day at [`RETEST_HOUR`] Arizona time, at the worker clock's now. Each run first
//! queues the next day's run ([`schedule_retests`], dedupe key by date), then calls
//! `deliverability_check::queue_pick_retests` with [`PICK_ALERT_YES_LIFE`] (a sample-pick Yes) and
//! `emergency_switch::queue_emergency_retests` with [`EMERGENCY_YES_LIFE`] (an emergency-alert
//! Yes). Each clears the Yes it re-tests in the same transaction as the queued email, so a person
//! stops counting until a new Yes and a rerun queues nothing twice; the emails themselves are the
//! `businesses.sample_pick` and `businesses.emergency_test` tasks the queued-email drain sends.
//! BIZ-007's after-hours suspension is not run here. No personal data is read or logged.

use askhava_core::api::{HandlerError, NewTask, enqueue};
use jiff::Timestamp;
use jiff::tz::{TimeZone, offset};
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::deliverability_check::{PICK_ALERT_YES_LIFE, queue_pick_retests};
use crate::emergency_switch::{EMERGENCY_YES_LIFE, queue_emergency_retests};
use crate::models::BusinessesError;

/// The daily re-test run's task kind (BIZ-042, D-4361).
pub const RETEST_KIND: &str = "businesses.deliverability_retests";
/// The Arizona hour the daily re-test run is due, so the emails arrive in the daytime.
pub const RETEST_HOUR: i8 = 9;

const FAILED: HandlerError = HandlerError("deliverability re-test run failed");

/// BIZ-042, JOB-006: queues the run due at the first [`RETEST_HOUR`] Arizona time after `now`,
/// once per day (dedupe key by its date).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn schedule_retests(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let arizona = TimeZone::fixed(offset(-7));
    let mut day = now.to_zoned(arizona.clone()).date();
    let due = loop {
        let at = day.at(RETEST_HOUR, 0, 0, 0).to_zoned(arizona.clone());
        let at = at.map_err(|_| BusinessesError::Database)?.timestamp();
        if at > now {
            break at;
        }
        day = day.tomorrow().map_err(|_| BusinessesError::Database)?;
    };
    let mut task = NewTask::new(RETEST_KIND, json!({}), due);
    task.dedupe_key = Some(format!("{RETEST_KIND}.{day}"));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

/// The [`RETEST_KIND`] handler (BIZ-042, D-362, D-380, JOB-006): queues tomorrow's run first,
/// then one re-test email per person and Yes older than its 90-day life, sample pick and
/// emergency alert. Safe to run twice: a re-tested Yes is cleared with its email.
///
/// # Errors
///
/// A fixed [`HandlerError`] on any failure; the task retries and queues nothing twice.
pub async fn run_retests(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_retests(&mut conn, now).await.map_err(|_| FAILED)?;
    queue_pick_retests(&mut conn, PICK_ALERT_YES_LIFE, now)
        .await
        .map_err(|_| FAILED)?;
    queue_emergency_retests(&mut conn, EMERGENCY_YES_LIFE, now)
        .await
        .map(|_| ())
        .map_err(|_| FAILED)
}

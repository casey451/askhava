//! The Payment pending wait's end, handed to `picks` (JOB-006, JOB-014, FEE-020, FEE-038; T663,
//! D-805, D-4349).
//!
//! The wait itself (1 business hour, 60 clock minutes on an emergency job, `rules::timeout`) does
//! not change, but `jobs`' timer no longer moves the job: `payments` holds the pick while a card
//! fix, a Stripe-down release, an attempt in flight or a charge pause lasts, and only `picks`
//! may ask it (`jobs` may not call `picks` or `payments`, D-491). So a live Payment pending timer
//! queues one [`PAYMENT_RETURNED_KIND`] task `{job}` on the timer's connection, and `server` runs
//! `picks::api::run_payment_returned` on it, which restarts the wait while the pick is held and
//! otherwise ends the pick and returns the job to Quotes ready. The dedupe key names the job, its
//! version and its timer version, so each live timer queues one task and a restarted wait's next
//! timer queues its own.

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;

/// The `core.task` kind a Payment pending timer queues; `picks` handles it (JOB-014, D-4349).
pub const PAYMENT_RETURNED_KIND: &str = "picks.payment_returned";

/// JOB-006, JOB-014, D-4349: queues one [`PAYMENT_RETURNED_KIND`] task for job `job`, whose live
/// Payment pending timer at `version` and timer version `timer` ran out, due at `at` (the timer's
/// due time), on `db`. Returns whether a task was new.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn queue_payment_returned(
    db: &mut PgConnection,
    job: Uuid,
    (version, timer): (i32, i32),
    at: Timestamp,
) -> Result<bool, JobsError> {
    let mut task = NewTask::new(PAYMENT_RETURNED_KIND, json!({ "job": job }), at);
    task.dedupe_key = Some(format!("{PAYMENT_RETURNED_KIND}.{job}.{version}.{timer}"));
    enqueue(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| JobsError::Database)
}

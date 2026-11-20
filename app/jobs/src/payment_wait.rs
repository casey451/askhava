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
//!
//! [`entered_payment_pending_since`] (T669, D-4349) reads `jobs.status_history`: whether a job
//! entered Payment pending at or after an instant. `picks`' lapse uses it so a pick whose
//! business pressed Confirm is never recorded `expired` (OPS-034); T670 uses it too.

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;
use crate::status::JobStatus;

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

/// OPS-034, D-4349: whether job `job` entered Payment pending at or after `since` (a history row
/// moving it there), on `db`.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn entered_payment_pending_since(
    db: &mut PgConnection,
    job: Uuid,
    since: Timestamp,
) -> Result<bool, JobsError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM jobs.status_history WHERE job_id = $1 AND to_status = $2
           AND at >= $3) AS "entered!""#,
        job,
        JobStatus::PaymentPending.code(),
        since.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| JobsError::Database)
}

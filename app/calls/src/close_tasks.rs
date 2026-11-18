//! The queued closes of masked call lines (CALL-009, CALL-010, JOB-005; D-1694, D-4338).
//!
//! [`run_close_line`] handles `jobs`' `calls.close_line` task, which a cancel, a check-in PROBLEM
//! or a check-in NO queues with the move (`jobs` and `intake` may not call `calls`): it reads the
//! payload `{"job", "number", "reason"}` and calls [`close_line`], which closes the job's open line
//! with its audit row and, for a cancel, queues the business's `line_closed_cancelled` notice with
//! no strike. A job with no open line (never had one, or the no-show path or a repeat closed it)
//! closes nothing, so the task is safe to run twice. [`run_close_expired_at`] is the daily sweep
//! ([`CLOSE_EXPIRED_KIND`]) that closes every line 30 days old ([`close_expired`]); each run first
//! queues the next one a day later, keyed by its run day, so a run that fails for good never ends
//! the chain and a restart's seed never doubles it (as `jobs`' purge sweep, D-2641).

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::lifecycle::{CloseReason, close_expired, close_line};

/// The daily sweep that closes every masked line [`LINE_DAYS`](crate::lifecycle::LINE_DAYS) old.
pub const CLOSE_EXPIRED_KIND: &str = "calls.close_expired";

const DAY: SignedDuration = SignedDuration::from_hours(24);
const BAD_PAYLOAD: HandlerError = HandlerError("calls close-line payload is malformed");
const CLOSE_FAILED: HandlerError = HandlerError("calls line close database call failed");

/// The close reason a `calls.close_line` payload names (`jobs::api::LineCloseReason::code`);
/// `None` for any other word.
fn reason_of(code: &str) -> Option<CloseReason> {
    match code {
        "cancelled" => Some(CloseReason::Cancelled),
        "complaint" => Some(CloseReason::Complaint),
        "no_show" => Some(CloseReason::NoShow),
        _ => None,
    }
}

/// CALL-009, CALL-010, JOB-005, D-4338: the `calls.close_line` handler at `now` (the worker's
/// clock): closes the payload's job's open line for its reason through [`close_line`] (a cancel
/// tells the business, never with a strike). A job with no open line closes nothing.
///
/// # Errors
///
/// [`HandlerError`] for a malformed payload or an unknown reason, or when the database fails; the
/// worker retries it.
pub async fn run_close_line(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let payload = &task.payload;
    let job = payload["job"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok());
    let number = payload["number"].as_i64();
    let reason = payload["reason"].as_str().and_then(reason_of);
    let (Some(job), Some(number), Some(reason)) = (job, number, reason) else {
        return Err(BAD_PAYLOAD);
    };
    let mut conn = pool.acquire().await.map_err(|_| CLOSE_FAILED)?;
    close_line(&mut conn, (job, number), reason, now)
        .await
        .map(|_| ())
        .map_err(|_| CLOSE_FAILED)
}

/// CALL-009: queues the [`CLOSE_EXPIRED_KIND`] sweep for a day after `now`, once per run day.
///
/// # Errors
///
/// [`TaskError::Db`].
pub async fn schedule_close_expired(
    db: &mut sqlx::PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let day = run_at.as_second() / 86_400;
    let mut task = NewTask::new(CLOSE_EXPIRED_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{CLOSE_EXPIRED_KIND}:{day}"));
    enqueue(db, &task).await?;
    Ok(())
}

/// CALL-009, D-4338: the [`CLOSE_EXPIRED_KIND`] handler at `now` (the worker's clock): queues the
/// next day's run first, then closes every line [`close_expired`] finds 30 days old, each with
/// its audit row.
///
/// # Errors
///
/// A fixed [`HandlerError`] when the database fails; the worker retries it.
pub async fn run_close_expired_at(
    pool: PgPool,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| CLOSE_FAILED)?;
    schedule_close_expired(&mut conn, now)
        .await
        .map_err(|_| CLOSE_FAILED)?;
    close_expired(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(|_| CLOSE_FAILED)
}

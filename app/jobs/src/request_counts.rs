//! Each trade's requests this calendar month, reported to `ops` for the coverage alert (OPS-006,
//! D-459, D-493, D-3844; T105).
//!
//! `ops` reads nothing above it, so once an hour [`REQUEST_COUNTS_KIND`] counts this Arizona
//! calendar month's requests per trade (a job's catalog category, from its first `jobs.job_facts`
//! row, written when its cards go out) and calls `ops::api::record_request_counts`, which alerts
//! Casey for a trade with a request and fewer than 2 active businesses. A request that never
//! reached its cards has no category yet and is not counted. The worker queues the first run at
//! start (`schedule_request_counts`, one dedupe key per hour, so a restart queues nothing more);
//! each run queues the next.

use askhava_core::api::{HandlerError, NewTask, TaskError, enqueue};
use askhava_ops::api::record_request_counts;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::models::JobsError;

/// The `core.task` kind of the hourly request count.
pub const REQUEST_COUNTS_KIND: &str = "jobs.request_counts";
/// How often the count runs.
const HOUR: SignedDuration = SignedDuration::from_hours(1);
/// The handler's failure sentence (D-491).
const FAILED: HandlerError = HandlerError("the request count database call failed");

/// OPS-006: queues the count for the hour after `now`, once per hour (dedupe key by hour number).
///
/// # Errors
/// [`TaskError::Db`].
pub async fn schedule_request_counts(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now.checked_add(HOUR).map_err(|_| TaskError::Db)?;
    let hour = run_at.as_second().div_euclid(3_600);
    let mut task = NewTask::new(REQUEST_COUNTS_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{REQUEST_COUNTS_KIND}.{hour}"));
    enqueue(db, &task).await.map(|_| ())
}

/// The [`REQUEST_COUNTS_KIND`] handler (OPS-006, JOB-006): queues the next hour's run, then
/// [`report_request_counts`] at `now`. Safe to run twice: it stores the same counts again.
///
/// # Errors
/// A fixed [`HandlerError`] when the database cannot be reached.
pub async fn run_request_counts(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_request_counts(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    report_request_counts(&mut conn, now)
        .await
        .map_err(|_| FAILED)
}

/// OPS-006, D-459: counts this Arizona calendar month's requests per trade (`now`'s month) and
/// reports them to `ops`.
///
/// # Errors
/// [`JobsError::Database`].
pub async fn report_request_counts(db: &mut PgConnection, now: Timestamp) -> Result<(), JobsError> {
    let rows = sqlx::query!(
        r#"SELECT f.category AS "trade!", count(*) AS "requests!" FROM jobs.job j
           JOIN jobs.job_facts f ON f.job_id = j.id AND f.seq = 1
           WHERE date_trunc('month', j.created_at AT TIME ZONE 'America/Phoenix')
             = date_trunc('month', $1 AT TIME ZONE 'America/Phoenix')
           GROUP BY f.category ORDER BY f.category"#,
        now.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| JobsError::Database)?;
    let counts: Vec<(String, u32)> = rows
        .into_iter()
        .map(|r| (r.trade, u32::try_from(r.requests).unwrap_or(u32::MAX)))
        .collect();
    record_request_counts(db, &counts, now)
        .await
        .map(|_| ())
        .map_err(|_| JobsError::Database)
}

//! The retention purge's start: each closed job's close date handed to `privacy` as `core.task`
//! events (PRIV-007 to PRIV-009, D-013, D-491).
//!
//! `privacy` may not read `jobs`, so this file queues one [`PURGE_DUE_KIND`] task per stage, due
//! at the close plus 30 days (photos), 180 days (contact details) and 1,095 days (scrubbed job
//! text), payload `{"job", "stage"}`; the composition runs `privacy::api::run_purge_due` on them.
//! The close date is the job's `status_at` once it reaches a final status. A daily sweep
//! ([`SWEEP_KIND`]) queues them for every job that reached one in the last week; each task's dedupe
//! key is its job and stage, so a job is queued once however often it is swept.

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::status::JobStatus;

/// A stage of a closed job's retention, due at its age after close; handled by `privacy`.
pub const PURGE_DUE_KIND: &str = "jobs.purge_due";
/// The daily sweep that queues [`PURGE_DUE_KIND`] for newly closed jobs.
pub const SWEEP_KIND: &str = "jobs.purge_sweep";
/// The stages and their days after close (PRIV-009, PRIV-007, PRIV-008; D-013).
pub const PURGE_STAGES: [(&str, i64); 3] = [("photos", 30), ("contact", 180), ("text", 3 * 365)];
const LOOK_BACK: SignedDuration = SignedDuration::from_hours(7 * 24);
const DAY: SignedDuration = SignedDuration::from_hours(24);

/// PRIV-007 to PRIV-009: queues job `job`'s three purge stages from its close at `closed_at`;
/// returns how many were new.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn enqueue_purge(
    db: &mut PgConnection,
    job: Uuid,
    closed_at: Timestamp,
) -> Result<usize, TaskError> {
    let mut queued = 0;
    for (stage, days) in PURGE_STAGES {
        let due = closed_at + SignedDuration::from_hours(days * 24);
        let mut task = NewTask::new(PURGE_DUE_KIND, json!({ "job": job, "stage": stage }), due);
        task.dedupe_key = Some(format!("{PURGE_DUE_KIND}:{job}:{stage}"));
        queued += usize::from(enqueue(db, &task).await?.is_some());
    }
    Ok(queued)
}

/// PRIV-007: queues the purge stages of every job that reached a final status in the week before
/// `now`; returns how many stage tasks were new.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn sweep_closed(db: &mut PgConnection, now: Timestamp) -> Result<usize, TaskError> {
    let finals: Vec<String> = JobStatus::ALL
        .into_iter()
        .filter(|status| status.is_final())
        .map(|status| status.code().to_owned())
        .collect();
    let rows = sqlx::query!(
        r#"SELECT id, status_at AS "status_at: jiff_sqlx::Timestamp" FROM jobs.job
           WHERE status = ANY($1) AND status_at >= $2 AND status_at <= $3"#,
        &finals,
        (now - LOOK_BACK).to_sqlx() as _,
        now.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| TaskError::Db)?;
    let mut queued = 0;
    for row in rows {
        queued += enqueue_purge(db, row.id, row.status_at.to_jiff()).await?;
    }
    Ok(queued)
}

/// Queues the sweep for the day after `now`, once per day (dedupe key by day number).
///
/// # Errors
/// [`TaskError::Db`].
pub async fn schedule_sweep(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let day = run_at.as_second() / 86_400;
    let mut task = NewTask::new(SWEEP_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{SWEEP_KIND}:{day}"));
    enqueue(db, &task).await?;
    Ok(())
}

/// The [`SWEEP_KIND`] handler: [`sweep_closed`] at the wall clock, then the next day's sweep.
///
/// # Errors
/// A fixed [`HandlerError`] when the database call fails.
pub async fn run_sweep(pool: PgPool, _task: Claimed) -> Result<(), HandlerError> {
    let failed = HandlerError("purge sweep database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    let now = Timestamp::now();
    sweep_closed(&mut conn, now).await.map_err(|_| failed)?;
    schedule_sweep(&mut conn, now).await.map_err(|_| failed)
}

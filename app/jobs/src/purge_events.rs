//! The retention purge's start: each closed job's close date handed to `privacy` as `core.task`
//! events (PRIV-007 to PRIV-009, D-013, D-491).
//!
//! `privacy` may not read `jobs`, so this file queues one [`PURGE_DUE_KIND`] task per stage, due
//! at the close plus 30 days (photos), 180 days (contact details) and 1,096 days (scrubbed job
//! text: three years, never short by a leap day), payload `{"job", "stage"}`; the composition runs
//! `privacy::api::run_purge_due` on them. The close date is the job's `status_at` once it reaches a
//! final status. A daily sweep ([`SWEEP_KIND`]) queues them for every job in a final status, with
//! no look-back limit, so a worker outage of any length is caught on the next sweep; each task's
//! dedupe key is its job and stage and `core.task` keeps the key, so a job is queued once however
//! often it is swept (D-1799). The sweep reads final jobs in pages of [`SWEEP_PAGE`] and hands on
//! only those with a stage whose key `core.task` does not hold yet (`core::api::keys_not_queued`),
//! and each run queues the next day's sweep before it reads anything, at the worker's clock, so a
//! run that fails for good never ends the chain (D-2641).

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue, keys_not_queued};
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
/// The stages and their days after close (PRIV-009, PRIV-007, PRIV-008; D-013); three years is
/// 1,096 days so a leap year never makes it short (D-1801).
pub const PURGE_STAGES: [(&str, i64); 3] =
    [("photos", 30), ("contact", 180), ("text", 3 * 365 + 1)];
/// How many final jobs one page of the sweep reads (D-2641).
pub const SWEEP_PAGE: i64 = 500;
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
        task.dedupe_key = Some(stage_key(job, stage));
        queued += usize::from(enqueue(db, &task).await?.is_some());
    }
    Ok(queued)
}

fn stage_key(job: Uuid, stage: &str) -> String {
    format!("{PURGE_DUE_KIND}:{job}:{stage}")
}

/// PRIV-007 to PRIV-009, D-2641: one page of jobs that reached a final status at or before `now`,
/// with IDs after `after` in ID order, at most [`SWEEP_PAGE`]: each job's ID and close, and whether
/// any of its stages is not queued yet. The last ID read is the next page's `after`.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn final_jobs_page(
    db: &mut PgConnection,
    now: Timestamp,
    after: Uuid,
) -> Result<Vec<(Uuid, Timestamp, bool)>, TaskError> {
    let finals: Vec<String> = JobStatus::ALL
        .into_iter()
        .filter(|status| status.is_final())
        .map(|status| status.code().to_owned())
        .collect();
    let rows = sqlx::query!(
        r#"SELECT id, status_at AS "status_at: jiff_sqlx::Timestamp" FROM jobs.job
           WHERE status = ANY($1) AND status_at <= $2 AND id > $3 ORDER BY id LIMIT $4"#,
        &finals,
        now.to_sqlx() as _,
        after,
        SWEEP_PAGE,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| TaskError::Db)?;
    let keys: Vec<String> = rows
        .iter()
        .flat_map(|row| PURGE_STAGES.map(|(stage, _)| stage_key(row.id, stage)))
        .collect();
    let missing = keys_not_queued(db, &keys).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let open = PURGE_STAGES.iter().any(|(stage, _)| {
                let key = stage_key(row.id, stage);
                missing.contains(&key)
            });
            (row.id, row.status_at.to_jiff(), open)
        })
        .collect())
}

/// PRIV-007, D-2641: the final jobs closed at or before `now` with a purge stage not yet queued,
/// read page by page ([`final_jobs_page`]); a job whose three stages are queued is left out.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn unqueued_finals(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<(Uuid, Timestamp)>, TaskError> {
    let (mut after, mut found) = (Uuid::nil(), Vec::new());
    loop {
        let page = final_jobs_page(db, now, after).await?;
        let Some(last) = page.last() else {
            return Ok(found);
        };
        after = last.0;
        let full = page.len() == usize::try_from(SWEEP_PAGE).unwrap_or(usize::MAX);
        found.extend(page.into_iter().filter(|j| j.2).map(|j| (j.0, j.1)));
        if !full {
            return Ok(found);
        }
    }
}

/// PRIV-007: queues the purge stages of every job that reached a final status at or before `now`,
/// however long ago, reading only jobs with a stage not yet queued ([`unqueued_finals`], D-2641);
/// returns how many stage tasks were new.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn sweep_closed(db: &mut PgConnection, now: Timestamp) -> Result<usize, TaskError> {
    let mut queued = 0;
    for (job, closed_at) in unqueued_finals(db, now).await? {
        queued += enqueue_purge(db, job, closed_at).await?;
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

/// The [`SWEEP_KIND`] handler at the wall clock: [`run_sweep_at`].
///
/// # Errors
/// As [`run_sweep_at`].
pub async fn run_sweep(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    run_sweep_at(pool, task, Timestamp::now()).await
}

/// PRIV-007 to PRIV-009, D-2641: the [`SWEEP_KIND`] handler at `now` (the worker's clock): queues
/// the next day's sweep first, so a run that fails for good still leaves it, then
/// [`sweep_closed`].
///
/// # Errors
/// A fixed [`HandlerError`] when the database call fails.
pub async fn run_sweep_at(
    pool: PgPool,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = HandlerError("purge sweep database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    schedule_sweep(&mut conn, now).await.map_err(|_| failed)?;
    sweep_closed(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(|_| failed)
}

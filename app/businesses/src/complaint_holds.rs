//! Complaint holds on the retention purge: `businesses` may not call `privacy`, so it queues a hold
//! event for each open complaint on a job and a release event once its box has closed, which
//! `privacy::api::run_complaint_hold` applies (PRIV-007, D-491).
//!
//! A complaint (T083) is open until its 5-day reply box closes and it is listed for Casey's digest
//! (`listed_at`). The composition runs [`run_complaint_holds`] on `jobs`' contact-stage purge task
//! and `privacy`'s re-checks, before `privacy` decides, so a complaint opened before the contact
//! purge holds it and a closed one lets it go. Each event's dedupe key is the complaint and its
//! state, so a complaint is held once and released once.

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use jiff::Timestamp;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The hold and release event `privacy` applies: payload `{"complaint", "job", "open"}`.
pub const COMPLAINT_HOLD_KIND: &str = "businesses.complaint_hold";

/// PRIV-007, D-491: queues, at `now`, a hold event for each open complaint about `job` and a
/// release event for each closed one; returns how many events were new.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn queue_complaint_holds(
    db: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<usize, TaskError> {
    let rows = sqlx::query!(
        r#"SELECT id, listed_at IS NULL AS "open!" FROM businesses.complaint WHERE job_id = $1"#,
        job,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| TaskError::Db)?;
    let mut queued = 0;
    for row in rows {
        let payload = json!({ "complaint": row.id, "job": job, "open": row.open });
        let mut task = NewTask::new(COMPLAINT_HOLD_KIND, payload, now);
        let state = if row.open { "open" } else { "closed" };
        task.dedupe_key = Some(format!("{COMPLAINT_HOLD_KIND}:{}:{state}", row.id));
        queued += usize::from(enqueue(db, &task).await?.is_some());
    }
    Ok(queued)
}

/// The composition's step on a contact-stage purge task (payload `{"job", "stage"}`): runs
/// [`queue_complaint_holds`] at the wall clock; any other stage is left alone.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database failure.
pub async fn run_complaint_holds(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    if task.payload["stage"].as_str() != Some("contact") {
        return Ok(());
    }
    let job = task.payload["job"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(HandlerError("purge task payload is not a job"))?;
    let failed = HandlerError("complaint hold database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    queue_complaint_holds(&mut conn, job, Timestamp::now())
        .await
        .map_err(|_| failed)?;
    Ok(())
}

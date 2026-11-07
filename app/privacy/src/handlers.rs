//! The retention purge's `core.task` handlers (PRIV-007 to PRIV-009, D-013, D-491).
//!
//! `jobs` queues one task per [`PurgeStage`] at the job's close date plus the stage's age, with the
//! payload `{"job", "stage"}`; the composition (`app/server/src/lib.rs`) runs `businesses`' and
//! `payments`' hold steps and then [`run_purge_due`]. Photos go at once. The contact stage first
//! waits an hour ([`RECHECK_KIND`]) so the complaint hold events just queued are applied, then,
//! while a hold is open, checks again daily; once free it queues [`PURGE_KIND`], which `intake`
//! answers by clearing the contact (`purge_contact`). A re-check on which the composition's
//! complaint step queued new hold events waits another hour ([`run_purge_due_after`]), so a
//! complaint filed after the first hour is applied before the decision. The text stage queues
//! [`PURGE_KIND`] too.
//! [`run_complaint_hold`] applies `businesses`' hold and release events.

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::{HoldSource, PurgeStage};
use crate::purge::{is_held, place_hold, purge_photos, release_hold};

/// The contact stage's own re-check: an hour after it first comes due, then daily while held.
pub const RECHECK_KIND: &str = "privacy.purge_recheck";
/// The purge `privacy` hands to the modules that keep the job's rows (`intake`): payload
/// `{"job", "stage"}` with stage `contact` or `text` (D-491).
pub const PURGE_KIND: &str = "privacy.purge";
const FIRST_WAIT: SignedDuration = SignedDuration::from_hours(1);
const HELD_WAIT: SignedDuration = SignedDuration::from_hours(24);
const BAD_PAYLOAD: HandlerError = HandlerError("purge task payload is not a job and stage");
const DB_FAILED: HandlerError = HandlerError("purge database call failed");

/// The job and stage a purge task names (D-491).
///
/// # Errors
/// A fixed [`HandlerError`] when either is missing or malformed.
pub fn purge_target(task: &Claimed) -> Result<(Uuid, PurgeStage), HandlerError> {
    let job = task.payload["job"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok());
    let stage = task.payload["stage"].as_str().and_then(PurgeStage::parse);
    job.zip(stage).ok_or(BAD_PAYLOAD)
}

/// PRIV-007 to PRIV-009: the purge-due handler, for `jobs`' stage tasks and [`RECHECK_KIND`]; safe
/// to run twice (each enqueue carries a dedupe key, each delete finds nothing the second time).
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database failure.
pub async fn run_purge_due(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let now = Timestamp::now();
    match stage {
        PurgeStage::Photos => purge_photos(&mut conn, job).await.map(|_| ()),
        PurgeStage::Text => return hand_on(&mut conn, job, stage, now).await,
        PurgeStage::Contact if task.kind != RECHECK_KIND => {
            return recheck(&mut conn, job, now + FIRST_WAIT, "first").await;
        }
        PurgeStage::Contact => {
            if is_held(&mut conn, job).await.map_err(|_| DB_FAILED)? {
                let day = now.as_second() / 86_400;
                return recheck(&mut conn, job, now + HELD_WAIT, &day.to_string()).await;
            }
            return hand_on(&mut conn, job, stage, now).await;
        }
    }
    .map_err(|_| DB_FAILED)
}

/// PRIV-007, D-1799: [`run_purge_due`] after the composition's complaint step, which reports how
/// many hold events it just queued. A contact re-check that queued any does not decide yet: those
/// events are not applied, so it comes back in an hour (a complaint state queues its event once,
/// so this ends). Any other task goes on to [`run_purge_due`].
///
/// # Errors
/// As [`run_purge_due`].
pub async fn run_purge_due_after(
    pool: PgPool,
    task: Claimed,
    events_queued: usize,
) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    if events_queued == 0 || stage != PurgeStage::Contact || task.kind != RECHECK_KIND {
        return run_purge_due(pool, task).await;
    }
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let now = Timestamp::now();
    let round = format!("events-{}", now.as_second());
    recheck(&mut conn, job, now + FIRST_WAIT, &round).await
}

async fn recheck(
    conn: &mut PgConnection,
    job: Uuid,
    at: Timestamp,
    round: &str,
) -> Result<(), HandlerError> {
    let payload = json!({ "job": job, "stage": PurgeStage::Contact.code() });
    let mut task = NewTask::new(RECHECK_KIND, payload, at);
    task.dedupe_key = Some(format!("{RECHECK_KIND}:{job}:{round}"));
    enqueue(conn, &task).await.map_err(|_| DB_FAILED)?;
    Ok(())
}

async fn hand_on(
    conn: &mut PgConnection,
    job: Uuid,
    stage: PurgeStage,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let payload = json!({ "job": job, "stage": stage.code() });
    let mut task = NewTask::new(PURGE_KIND, payload, now);
    task.dedupe_key = Some(format!("{PURGE_KIND}:{job}:{}", stage.code()));
    enqueue(conn, &task).await.map_err(|_| DB_FAILED)?;
    Ok(())
}

/// PRIV-007, D-491: applies `businesses`' complaint hold event, payload `{"complaint", "job",
/// "open"}`: an open complaint places its hold, a closed one releases it.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database failure.
pub async fn run_complaint_hold(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let id = |key: &str| {
        task.payload[key]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let (complaint, job) = id("complaint").zip(id("job")).ok_or(HandlerError(
        "hold event payload is not a complaint and job",
    ))?;
    let open = task.payload["open"]
        .as_bool()
        .ok_or(HandlerError("hold event payload has no open flag"))?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let now = Timestamp::now();
    let source = HoldSource::Complaint;
    if open {
        place_hold(&mut conn, source, complaint, job, now).await
    } else {
        release_hold(&mut conn, source, complaint, job, now).await
    }
    .map_err(|_| DB_FAILED)
}

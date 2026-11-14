//! A pick `payments` ended before it was paid: the `payments.pick_ended` handler (FEE-018,
//! FEE-020, FEE-041, PICK-046, PICK-047, QH-004; D-2286, D-3468, D-3579; T328).
//!
//! `payments` ends its own record of the pick when Stripe was unreachable for 4 hours (FEE-018),
//! the card-fix window ran out (FEE-020) or a Confirm would pass the month's fee limit (FEE-041),
//! and raises `payments::api::PICK_ENDED_KIND` (`{pick, business, job, reason}`, once per pick).
//! [`run_pick_ended`] is the `picks` handler `server` registers on it. In one transaction it ends
//! the waiting `picks.pick` row as `payment_failed` (only a pick still waiting; a repeat or a pick
//! settled, lapsed or withdrawn meanwhile changes nothing), returns a job in Payment pending to
//! Quotes ready (the `backup_choice` contract), moves the pick at once to the homeowner's backup
//! (PICK-047), and with no pick then waiting returns the job from Awaiting confirm to Quotes ready.
//! With no backup it queues one `jobs::api::TEXT_KIND` event with [`PICK_LAPSED_TEMPLATE`], the
//! job's version and the IDs `{job, pick, business}` only (never a contact; `picks` holds none),
//! at `lapse_plan`'s `send_at` (at once on an emergency job, else the next 8 a.m. at night),
//! deduped by the pick. `intake`'s job-text sender renders `lapse_text` with her other quotes and
//! applies consent, opt-out, the area-code check and quiet hours (an emergency job's text goes at
//! any hour, QH-004). The homeowner pause (CON-029) is `false` here: a pause withdraws the waiting
//! pick first, so a paused job's pick is no longer waiting when this runs.

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::backup_choice::{Backup, backup_choice};
use crate::confirm_reminders::lapse_plan;
use crate::models::{self, PicksError};
use crate::status::{PickStatus, awaits_charge, awaits_confirm};

/// The `jobs.homeowner_text` template of PICK-046's "couldn't take it this time" text; `intake`'s
/// sender builds its words from `lapse_text` (D-3579).
pub const PICK_LAPSED_TEMPLATE: &str = "pick_lapsed";

const DB_FAILED: HandlerError = HandlerError("pick-ended database statement failed");

/// The `payments.pick_ended` handler (FEE-018, FEE-020, FEE-041, PICK-046, QH-004): [`pick_ended`]
/// on the event's pick at `now`, committed in one transaction. Safe to run twice (JOB-006).
///
/// # Errors
///
/// A fixed sentence for an event with no pick or a failed step; the queue tries again.
pub async fn run_pick_ended(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = task.payload["pick"]
        .as_str()
        .and_then(|p| Uuid::parse_str(p).ok())
        .ok_or(HandlerError("pick-ended event has no pick"))?;
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    pick_ended(&mut tx, pick, now)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// FEE-018, FEE-020, FEE-041, PICK-046, PICK-047, D-3579: inside the caller's transaction, ends
/// waiting pick `pick` as `payment_failed` at `at`, frees its job and moves it to her backup;
/// [`Backup::NoBackup`] also queues her PICK-046 text event. A pick no longer waiting is
/// [`Backup::Done`] with nothing changed. The job and business are read from the pick's own row,
/// never from the event.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Job`], [`PicksError::MovedOn`] (roll back) or the
/// backup move's errors.
pub async fn pick_ended(
    tx: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<Backup, PicksError> {
    let Some((job, business)) = models::lock_waiting(tx, pick).await? else {
        return Ok(Backup::Done);
    };
    end_unpaid(tx, pick, at).await?;
    if awaits_charge(current(tx, job).await.map_err(|_| PicksError::Job)?.status) {
        guarded_move(
            tx,
            job,
            (JobStatus::PaymentPending, JobStatus::QuotesReady),
            at,
        )
        .await?;
    }
    let backup = backup_choice(tx, pick, false, (Actor::System, at)).await?;
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_confirm(status) && models::waiting_pick(tx, job).await?.is_none() {
        guarded_move(
            tx,
            job,
            (JobStatus::AwaitingConfirm, JobStatus::QuotesReady),
            at,
        )
        .await?;
    }
    if backup == Backup::NoBackup {
        queue_lapsed_text(tx, (job, pick, business), at).await?;
    }
    Ok(backup)
}

/// Ends waiting pick `id` as [`PickStatus::PaymentFailed`] at `at` (FEE-018, FEE-020, FEE-041).
async fn end_unpaid(tx: &mut PgConnection, id: Uuid, at: Timestamp) -> Result<(), PicksError> {
    sqlx::query!(
        "UPDATE picks.pick SET status = $2, ended_at = $3 WHERE id = $1 AND status = $4",
        id,
        PickStatus::PaymentFailed.code(),
        at.to_sqlx() as _,
        PickStatus::AwaitingConfirm.code(),
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)
    .map(drop)
}

/// Moves `job` along `arrow` (JOB-001, JOB-004); a job that moved on rolls the whole step back.
async fn guarded_move(
    tx: &mut PgConnection,
    job: Uuid,
    arrow: (JobStatus, JobStatus),
    at: Timestamp,
) -> Result<(), PicksError> {
    match move_to(tx, job, arrow, at, Actor::System).await {
        Ok(Guarded::Applied(())) => Ok(()),
        Ok(Guarded::MovedOn) => Err(PicksError::MovedOn),
        Err(_) => Err(PicksError::Job),
    }
}

/// PICK-046, QH-004, D-3579: one `jobs.homeowner_text` event for [`PICK_LAPSED_TEMPLATE`] at the
/// job's version now, with the job, pick and business IDs only, due at `lapse_plan`'s `send_at`
/// (the emergency choice is the job's); deduped by the pick, so a rerun queues nothing more. The
/// quote lines are `intake`'s, read at send time.
async fn queue_lapsed_text(
    tx: &mut PgConnection,
    (job, pick, business): (Uuid, Uuid, Uuid),
    at: Timestamp,
) -> Result<(), PicksError> {
    let row = current(tx, job).await.map_err(|_| PicksError::Job)?;
    let plan = lapse_plan((at, row.emergency), business, &[]);
    let payload = json!({"job": job, "version": row.version,
        "template": PICK_LAPSED_TEMPLATE,
        "values": {"job": job, "pick": pick, "business": business}});
    let mut task = NewTask::new(TEXT_KIND, payload, plan.send_at);
    task.dedupe_key = Some(format!("{TEXT_KIND}.{pick}.{PICK_LAPSED_TEMPLATE}"));
    enqueue(tx, &task)
        .await
        .map_err(|_| PicksError::Database)
        .map(drop)
}

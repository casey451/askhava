//! The pick notice and the Confirm window's timer (PICK-019, PICK-037; D-345, D-491, D-807).
//!
//! [`run_pick_notice`] is the handler on `picks.pick_made`: it resolves the owner and every
//! "Confirm and billing" member through `businesses::api::notice_addresses` (picks purpose) and
//! queues one `messaging.business_notice` task with their opaque address IDs, the pick, the job
//! number and the window's length, for `messaging` to send through T094's email pipeline by the
//! business's chosen channels. The window counts from the real send, so the sender calls
//! [`notice_sent`] with that instant and the same [`ConfirmSettings`] (read once from
//! `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES`) before it renders, and fills "Accept by" from the deadline
//! it returns; on an error nothing goes. It restarts `jobs`' Awaiting confirm backstop one business
//! minute past the deadline and queues [`CONFIRM_ENDS_KIND`] at the deadline, where
//! [`run_confirm_ends_at`] runs the lapse of a pick still waiting. A job chosen as an emergency
//! keeps `jobs`' own 60 clock minutes (PICK-044) and gets the same task at their end, so the lapse
//! runs whichever of `jobs`' timer and the task comes first (`jobs` cannot call `picks`).
//!
//! T491 (PICK-019, PICK-044, PICK-046, PICK-047, D-3750, D-3805): the lapse is the one `pick_ended`
//! runs: `confirm_reminders::lapse` (the pick expires, then `backup_choice`; never paused, since a
//! CANCEL pause withdraws the waiting pick first, CON-029), the job back to Quotes ready only when
//! no pick then waits, then PICK-047's or PICK-046's text event after the last job move. A window
//! end that finds the job already in Quotes ready (`jobs`' backstop or the emergency 60 minutes got
//! there first) lapses at the later of its due time and the worker's now, so a late worker never
//! starts the backup's window in the past; a notice sent after that lapses at its send.
//! T214 (PICK-050, D-3024): the notice's values carry `cant_take`, the pick's "Can't take it" page.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, add_business_minutes, enqueue};
use askhava_jobs::api::{
    JobRow, JobStatus, current, job_number, move_to, restart_timer, rules::timeout,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::backup_choice::Backup;
use crate::cant_take::cant_take_link;
use crate::confirm::{
    ConfirmSettings, PICK_NOTICE_EMERGENCY_TEMPLATE, PICK_NOTICE_TEMPLATE, confirm_by,
};
use crate::confirm_reminders::lapse;
use crate::models::{self, PicksError};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::pick_ended::queue_backup_text;
use crate::status::{awaits_confirm, confirm_backstop_minutes, pick_lost, returned_unconfirmed};

/// The `core.task` kind of the Confirm window's end (PICK-019).
pub const CONFIRM_ENDS_KIND: &str = "picks.confirm_ends";

const DB_FAILED: HandlerError = HandlerError("pick window database statement failed");

fn id(payload: &Value, name: &str) -> Result<Uuid, HandlerError> {
    payload[name]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("pick event is missing an ID"))
}

/// The `picks.pick_made` handler (PICK-037, BIZ-019, D-491): queues the pick notice to the owner
/// and every "Confirm and billing" member, once per pick; the emergency subject on an emergency
/// job. `window` is `settings`' Confirm window in business minutes; the "Accept by <time>" is the
/// deadline [`notice_sent`] returns at the send, from the same settings (D-1503). `cant_take` is
/// the pick's "Can't take it" page ([`cant_take_link`], PICK-050, D-3024).
///
/// # Errors
/// A fixed sentence for a malformed event or a failed statement; the queue tries again.
pub async fn run_pick_notice(
    pool: PgPool,
    settings: ConfirmSettings,
    task: Claimed,
) -> Result<(), HandlerError> {
    let (pick, business, job) = (
        id(&task.payload, "pick")?,
        id(&task.payload, "business")?,
        id(&task.payload, "job")?,
    );
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let emergency = current(&mut tx, job)
        .await
        .map_err(|_| DB_FAILED)?
        .emergency;
    let to = notice_addresses(&mut tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| DB_FAILED)?;
    let number = job_number(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    let template = match emergency {
        true => PICK_NOTICE_EMERGENCY_TEMPLATE,
        false => PICK_NOTICE_TEMPLATE,
    };
    let window = settings.window_minutes;
    let payload = json!({
        "template": template, "business": business, "pick": pick, "to": to,
        "values": {"job": number, "window": window, "cant_take": cant_take_link(pick)},
    });
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, Timestamp::now());
    notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{template}.{pick}"));
    enqueue(&mut tx, &notice).await.map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// PICK-019, PICK-044, D-345, D-381, D-3750: starts pick `pick`'s Confirm window from `sent_at`,
/// the pick notice's real send (whole seconds), and returns its end, the notice's "Accept by";
/// none for an emergency job (PICK-044, `jobs`' 60 clock minutes), which still gets one
/// [`CONFIRM_ENDS_KIND`] task at the end of those 60 minutes. `jobs`' backstop is restarted one
/// business minute past the end, so the window's own task decides. A repeat with the same instant
/// restarts nothing and queues nothing new (D-1503).
///
/// # Errors
///
/// [`PicksError::MovedOn`] when the pick no longer waits or the job left Awaiting confirm; when
/// `jobs` already returned the job to Quotes ready (its backstop or an emergency's 60 minutes fired
/// before the send) the lapse runs first at `sent_at` (PICK-046, PICK-047), and in any other lost
/// status the pick is stored as expired, so no "Accept by" goes for a dead pick (PICK-035,
/// D-1503). [`PicksError::Job`] or [`PicksError::Database`].
pub async fn notice_sent(
    db: &mut PgConnection,
    pick: Uuid,
    sent_at: Timestamp,
    settings: ConfirmSettings,
) -> Result<Option<Timestamp>, PicksError> {
    let start = Timestamp::from_second(sent_at.as_second()).map_err(|_| PicksError::Job)?;
    let mut tx = db.begin().await.map_err(|_| PicksError::Database)?;
    let (job, business) = models::lock_waiting(&mut tx, pick)
        .await?
        .ok_or(PicksError::MovedOn)?;
    let row = current(&mut tx, job).await.map_err(|_| PicksError::Job)?;
    if pick_lost(row.status) {
        if returned_unconfirmed(row.status) {
            lapse_at(&mut tx, (pick, job, business), start).await?;
        } else {
            models::expire_pick(&mut tx, pick, start).await?;
        }
        tx.commit().await.map_err(|_| PicksError::Database)?;
        return Err(PicksError::MovedOn);
    }
    if !awaits_confirm(row.status) {
        return Err(PicksError::MovedOn);
    }
    if row.emergency {
        emergency_window(&mut tx, (pick, job), row.timer_from).await?;
        tx.commit().await.map_err(|_| PicksError::Database)?;
        return Ok(None);
    }
    let due = start_window(&mut tx, (pick, job, &row), start, settings).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(Some(due))
}

/// PICK-044, D-3750: queues the emergency pick's [`CONFIRM_ENDS_KIND`] task at the end of `jobs`'
/// 60 clock minutes counted from the job's `from` (its `timer_from`, the instant `jobs` uses),
/// deduped as the normal window's, so the lapse runs even when `jobs`' timer returns the job first.
async fn emergency_window(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    from: Timestamp,
) -> Result<(), PicksError> {
    let due = timeout(JobStatus::AwaitingConfirm, from, true)
        .ok_or(PicksError::Job)?
        .at;
    queue_window_end(tx, (pick, job), (from, due)).await
}

/// One [`CONFIRM_ENDS_KIND`] task for pick `pick` on `job`, due at `due`, carrying the job's
/// timer start `from` it fences on; deduped by the pick and `due`.
async fn queue_window_end(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    (from, due): (Timestamp, Timestamp),
) -> Result<(), PicksError> {
    let payload =
        json!({"pick": pick, "job": job, "from": from.as_second(), "due": due.as_second()});
    let mut task = NewTask::new(CONFIRM_ENDS_KIND, payload, due);
    task.dedupe_key = Some(format!("{CONFIRM_ENDS_KIND}.{pick}.{}", due.as_second()));
    enqueue(tx, &task)
        .await
        .map_err(|_| PicksError::Database)
        .map(drop)
}

/// Restarts `jobs`' backstop to end one business minute after the window that starts at `start`,
/// unless it already counts from there (a repeat), and queues the window's end; returns the end.
async fn start_window(
    tx: &mut PgConnection,
    (pick, job, row): (Uuid, Uuid, &JobRow),
    start: Timestamp,
    settings: ConfirmSettings,
) -> Result<Timestamp, PicksError> {
    let due = confirm_by(start, settings);
    let backstop = (settings.window_minutes + 1).saturating_sub(confirm_backstop_minutes());
    let from = add_business_minutes(start, backstop);
    if row.timer_from.as_second() != from.as_second() {
        let restarted = restart_timer(tx, job, (JobStatus::AwaitingConfirm, from)).await;
        if !matches!(
            restarted.map_err(|_| PicksError::Job)?,
            Guarded::Applied(())
        ) {
            return Err(PicksError::MovedOn);
        }
    }
    queue_window_end(tx, (pick, job), (from, due)).await?;
    Ok(due)
}

/// The [`CONFIRM_ENDS_KIND`] handler (PICK-019, JOB-006): [`run_confirm_ends_at`] with the task's
/// own due time as the worker's now.
///
/// # Errors
/// A fixed sentence for a malformed task or a failed statement; the queue tries again.
pub async fn run_confirm_ends(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let due = second(&task.payload, "due")?;
    run_confirm_ends_at(pool, task, due).await
}

/// The [`CONFIRM_ENDS_KIND`] handler on the worker clock's `now` (PICK-019, PICK-044, PICK-046,
/// PICK-047, JOB-006, D-3750): for a pick still waiting, a job in Awaiting confirm whose wait still
/// counts from the task's `from` lapses at the task's due time ([`lapse_at`]); a job already back
/// in Quotes ready (`jobs`' backstop or the emergency 60 minutes came first) lapses at the later of
/// due and `now`, so the backup's window never starts in the past; a restarted window, any other
/// status or a pick no longer waiting does nothing. One transaction; safe to run twice.
///
/// # Errors
/// A fixed sentence for a malformed task or a failed statement; the queue tries again.
pub async fn run_confirm_ends_at(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = id(&task.payload, "pick")?;
    let (from, due) = (
        second(&task.payload, "from")?,
        second(&task.payload, "due")?,
    );
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let Some((job, business)) = models::lock_waiting(&mut tx, pick)
        .await
        .map_err(|_| DB_FAILED)?
    else {
        return Ok(());
    };
    let row = current(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    let at = if awaits_confirm(row.status) && row.timer_from.as_second() == from.as_second() {
        due
    } else if returned_unconfirmed(row.status) {
        due.max(now)
    } else {
        return Ok(());
    };
    lapse_at(&mut tx, (pick, job, business), at)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// A whole-second instant named `name` in a window task's payload.
fn second(payload: &Value, name: &str) -> Result<Timestamp, HandlerError> {
    payload[name]
        .as_i64()
        .and_then(|s| Timestamp::from_second(s).ok())
        .ok_or(HandlerError("confirm-window task is missing a time"))
}

/// PICK-019, PICK-046, PICK-047, CON-029, D-3750: the lapse of waiting pick `pick` of `business`
/// on `job` at `at`, inside the caller's transaction, as `pick_ended` runs it: the pick expires and
/// moves to her backup ([`lapse`], never paused: a pause withdraws the waiting pick first), the job
/// returns from Awaiting confirm to Quotes ready only when no pick then waits, and after that last
/// move her PICK-047 or PICK-046 text event is queued.
///
/// # Errors
///
/// [`PicksError::MovedOn`] (roll back), [`PicksError::Job`], [`PicksError::Database`] or the
/// backup move's errors.
async fn lapse_at(
    tx: &mut PgConnection,
    (pick, job, business): (Uuid, Uuid, Uuid),
    at: Timestamp,
) -> Result<(), PicksError> {
    let backup: Backup = lapse(tx, pick, false, (Actor::System, at)).await?;
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_confirm(status) && models::waiting_pick(tx, job).await?.is_none() {
        let arrow = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
        match move_to(tx, job, arrow, at, Actor::System).await {
            Ok(Guarded::Applied(())) => {}
            Ok(Guarded::MovedOn) => return Err(PicksError::MovedOn),
            Err(_) => return Err(PicksError::Job),
        }
    }
    queue_backup_text(tx, (job, pick, business), (backup, false), at).await
}

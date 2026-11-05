//! The pick notice and the Confirm window's timer (PICK-019, PICK-037; D-345, D-491, D-807).
//!
//! [`run_pick_notice`] is the handler on `picks.pick_made`: it resolves the owner and every
//! "Confirm and billing" member through `businesses::api::notice_addresses` (picks purpose) and
//! queues one `messaging.business_notice` task with their opaque address IDs, the pick, the job
//! number and the window's length, for `messaging` to send through T094's email pipeline by the
//! business's chosen channels. The window counts from the real send, so the sender calls
//! [`notice_sent`] with that instant: it restarts `jobs`' Awaiting confirm backstop one business
//! minute past the deadline and queues [`CONFIRM_ENDS_KIND`] at the deadline, where
//! [`run_confirm_ends`] expires a pick still waiting and returns the job to Quotes ready. A job
//! chosen as an emergency keeps `jobs`' own 60 clock minutes (PICK-044).

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, add_business_minutes, enqueue};
use askhava_jobs::api::{JobStatus, current, job_number, move_to, restart_timer};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::confirm::{
    ConfirmSettings, PICK_NOTICE_EMERGENCY_TEMPLATE, PICK_NOTICE_TEMPLATE, confirm_by,
};
use crate::models::{self, PicksError};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::status::awaits_confirm;

/// The `core.task` kind of the Confirm window's end (PICK-019).
pub const CONFIRM_ENDS_KIND: &str = "picks.confirm_ends";
/// `jobs`' Awaiting confirm backstop, in business minutes (JOB-013, D-805).
const BACKSTOP_MINUTES: u32 = 4 * 60;

const DB_FAILED: HandlerError = HandlerError("pick window database statement failed");

fn id(payload: &Value, name: &str) -> Result<Uuid, HandlerError> {
    payload[name]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("pick event is missing an ID"))
}

/// The `picks.pick_made` handler (PICK-037, BIZ-019, D-491): queues the pick notice to the owner
/// and every "Confirm and billing" member, once per pick; the emergency subject on an emergency
/// job. `window` is the Confirm window in business minutes, filled in as "Accept by <time>" at send.
///
/// # Errors
/// A fixed sentence for a malformed event or a failed statement; the queue tries again.
pub async fn run_pick_notice(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
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
    let window = ConfirmSettings::default().window_minutes;
    let payload = json!({
        "template": template, "business": business, "pick": pick, "to": to,
        "values": {"job": number, "window": window},
    });
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, Timestamp::now());
    notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{template}.{pick}"));
    enqueue(&mut tx, &notice).await.map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// PICK-019, D-345, D-381: starts pick `pick`'s Confirm window from `sent_at`, the pick notice's
/// real send (whole seconds), and returns its end; none for an emergency job (PICK-044, `jobs`'
/// 60 clock minutes). `jobs`' backstop is restarted one business minute past the end, so the
/// window's own task decides; a repeat with the same instant queues nothing new.
///
/// # Errors
///
/// [`PicksError::MovedOn`] when the pick no longer waits or the job left Awaiting confirm;
/// [`PicksError::Job`] or [`PicksError::Database`].
pub async fn notice_sent(
    db: &mut PgConnection,
    pick: Uuid,
    sent_at: Timestamp,
    settings: ConfirmSettings,
) -> Result<Option<Timestamp>, PicksError> {
    let start = Timestamp::from_second(sent_at.as_second()).map_err(|_| PicksError::Job)?;
    let mut tx = db.begin().await.map_err(|_| PicksError::Database)?;
    let (job, _) = models::lock_waiting(&mut tx, pick)
        .await?
        .ok_or(PicksError::MovedOn)?;
    if current(&mut tx, job)
        .await
        .map_err(|_| PicksError::Job)?
        .emergency
    {
        return Ok(None);
    }
    let due = confirm_by(start, settings);
    let backstop = (settings.window_minutes + 1).saturating_sub(BACKSTOP_MINUTES);
    let from = add_business_minutes(start, backstop);
    let restarted = restart_timer(&mut tx, job, (JobStatus::AwaitingConfirm, from)).await;
    if !matches!(
        restarted.map_err(|_| PicksError::Job)?,
        Guarded::Applied(())
    ) {
        return Err(PicksError::MovedOn);
    }
    let payload =
        json!({"pick": pick, "job": job, "from": from.as_second(), "due": due.as_second()});
    let mut task = NewTask::new(CONFIRM_ENDS_KIND, payload, due);
    task.dedupe_key = Some(format!("{CONFIRM_ENDS_KIND}.{pick}.{}", due.as_second()));
    enqueue(&mut tx, &task)
        .await
        .map_err(|_| PicksError::Database)?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(Some(due))
}

/// The [`CONFIRM_ENDS_KIND`] handler (PICK-019, JOB-006): a pick still waiting whose window this
/// task timed (the job's wait still counts from the task's `from`) is expired and the job moves
/// from Awaiting confirm to Quotes ready, in one transaction; anything else is a stale timer and
/// does nothing. Safe to run twice.
///
/// # Errors
/// A fixed sentence for a malformed task or a failed statement; the queue tries again.
pub async fn run_confirm_ends(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let pick = id(&task.payload, "pick")?;
    let second = |name: &str| {
        task.payload[name]
            .as_i64()
            .and_then(|s| Timestamp::from_second(s).ok())
            .ok_or(HandlerError("confirm-window task is missing a time"))
    };
    let (from, due) = (second("from")?, second("due")?);
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let Some((job, _)) = models::lock_waiting(&mut tx, pick)
        .await
        .map_err(|_| DB_FAILED)?
    else {
        return Ok(());
    };
    let row = current(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    if !awaits_confirm(row.status) || row.timer_from.as_second() != from.as_second() {
        return Ok(());
    }
    let arrow = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
    match move_to(&mut tx, job, arrow, due, Actor::System).await {
        Ok(Guarded::Applied(())) => {}
        Ok(Guarded::MovedOn) => return Ok(()),
        Err(_) => return Err(DB_FAILED),
    }
    models::expire_pick(&mut tx, pick, due)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

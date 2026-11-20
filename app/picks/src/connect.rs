//! A confirmed pick's job moves: Awaiting confirm to Payment pending at the Confirm press,
//! Payment pending to Connected at the settle, and the Payment pending wait's end, which defers
//! to `payments`' hold (JOB-002, JOB-004, JOB-005, JOB-006, JOB-014, PICK-022, PRIV-005, FEE-018,
//! FEE-020, FEE-021, FEE-038, OPS-024, PICK-048; T663, D-2930, D-4349).
//!
//! [`confirm_move`] is the press's move (T664 calls it). [`connect_on_settle`] is `settle`'s, in
//! the settle's transaction before the release's claim, which then sees Connected and queues the
//! check-ins (D-2930); a job still in Awaiting confirm (an auto-confirmed pick, PICK-048, or one
//! pressed before this change) gets both moves. [`end_pending`] is the one ending of a waiting pick
//! whose job is in Payment pending: it ends the pick and its PaymentIntent through `payments`' own
//! pick end (so a success before or after it is refunded once, FEE-021, D-2958), returns the job to
//! Quotes ready and offers her backup (PICK-047) or sends PICK-046's text.
//!
//! [`run_payment_returned`] handles `jobs`' `picks.payment_returned` task (`jobs::payment_wait`):
//! while `payments::api::payment_held` holds the pick it restarts the wait and leaves the job; a
//! held success (a `succeeded` attempt or a $0 decision) also queues one [`HELD_SUCCESS_KIND`] task
//! with T408's held-release payload, which `server` runs through `run_held_release`, so a success
//! whose webhook never came is still settled and released once (PICK-022); a pick held over 24
//! hours since its job entered Payment pending raises one High [`HELD_LONG_ALERT`] ever. A pick
//! nothing holds ends through [`end_pending`] as `payment_failed`. Every pick read here takes the
//! pick's row lock before the job's (D-3979). `picks` never reads safe mode (D-4726).

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{JobStatus, current, move_to, restart_timer};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit_once, raise_alert};
use askhava_payments::api::{PaymentHold, end_pick_intent, payment_held};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::backup_choice::Backup;
use crate::models::{self, PicksError};
use crate::pick_ended::{after_end, end_unpaid};
use crate::status::{awaits_charge, awaits_confirm};

/// The task a held success queues, with T408's held-release payload `{pick, run_at}`; `server`
/// runs `run_held_release` on it (PICK-022, OPS-024, D-3263, D-4349).
pub const HELD_SUCCESS_KIND: &str = "picks.held_success";

/// The High alert raised once per pick held over [`HELD_LONG`] in Payment pending (D-4349).
pub const HELD_LONG_ALERT: &str = "picks.payment_held_long";

/// How long a pick may be held in Payment pending before Casey is told (D-4349).
pub const HELD_LONG: SignedDuration = SignedDuration::from_hours(24);

const PICK_SUBJECT: &str = "picks.pick";
const RETURN_FAILED: HandlerError = HandlerError("payment-returned database statement failed");

/// What [`confirm_move`] did (JOB-004, D-4349).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmMove {
    /// The job moved Awaiting confirm to Payment pending now.
    Moved,
    /// The job was already in Payment pending for this pick; nothing moved.
    Already,
    /// The pick no longer waits, or the job is in any other status; nothing moved.
    Lost,
}

/// Why [`end_pending`] ends a waiting pick (FEE-020, PICK-033, D-4349).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingEnd {
    /// No payment came: the pick ends as `payment_failed`.
    PaymentFailed,
    /// The business pressed "Can't take it": cancelled with `cant_take`.
    CantTake,
    /// The business can no longer take it (license, leaving): cancelled with `ineligible`.
    Ineligible,
}

/// Locks pick `pick` and answers its business only while it waits for Confirm, has not ended and
/// is `job`'s (JOB-004, D-3979).
async fn lock_waiting_on(
    tx: &mut PgConnection,
    pick: Uuid,
    job: Uuid,
) -> Result<Option<Uuid>, PicksError> {
    let row = models::lock_waiting(tx, pick).await?;
    Ok(row
        .filter(|(of, _)| *of == job)
        .map(|(_, business)| business))
}

/// JOB-002, JOB-004, JOB-005, D-4349: the Confirm press's move, under pick `pick`'s row lock, in
/// the caller's transaction: job `job` moves Awaiting confirm to Payment pending as the pick's
/// business at `at`, only while the pick waits and is the job's waiting pick (the locked pick is
/// `awaiting_confirm`, unended and `job`'s, and a job has at most one such pick,
/// `migrations/0079_picks_code.sql`, so it is `models::waiting_pick`'s); a job already in
/// Payment pending is [`ConfirmMove::Already`]; any other status, or a pick no longer waiting, is
/// [`ConfirmMove::Lost`].
///
/// # Errors
/// [`PicksError::Database`] or [`PicksError::Job`].
pub async fn confirm_move(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    at: Timestamp,
) -> Result<ConfirmMove, PicksError> {
    let Some(business) = lock_waiting_on(tx, pick, job).await? else {
        return Ok(ConfirmMove::Lost);
    };
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_charge(status) {
        return Ok(ConfirmMove::Already);
    }
    if !awaits_confirm(status) {
        return Ok(ConfirmMove::Lost);
    }
    let arrow = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    match move_to(tx, job, arrow, at, Actor::Business(business)).await {
        Ok(Guarded::Applied(())) => Ok(ConfirmMove::Moved),
        Ok(Guarded::MovedOn) => Ok(ConfirmMove::Lost),
        Err(_) => Err(PicksError::Job),
    }
}

/// PICK-022, PICK-048, JOB-015, D-2930, D-4349: `settle`'s job moves for waiting pick `pick` of
/// `job` at `at`, in the settle's transaction: Payment pending to Connected, or from Awaiting
/// confirm both moves in order. False, moving nothing, for any other status (the clearing is
/// then `Left`).
///
/// # Errors
/// [`PicksError::Job`]; [`PicksError::MovedOn`] when a move lost a race (roll back).
pub(crate) async fn connect_on_settle(
    tx: &mut PgConnection,
    job: Uuid,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_confirm(status) {
        let arrow = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
        system_move(tx, job, arrow, at).await?;
    } else if !awaits_charge(status) {
        return Ok(false);
    }
    let arrow = (JobStatus::PaymentPending, JobStatus::Connected);
    system_move(tx, job, arrow, at).await?;
    Ok(true)
}

/// Moves `job` along `arrow` as the system; a job that moved on rolls the step back (JOB-004).
async fn system_move(
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

/// FEE-020, FEE-021, PICK-046, PICK-047, JOB-014, D-2290, D-2958, D-4349: ends waiting pick
/// `pick` of `job` for `reason` as `by`, in the caller's transaction, under the pick's row lock:
/// nothing at all unless the pick still waits and has not ended; then the pick ends, its
/// PaymentIntent ends through `payments`' own pick end (a success stored before it refunded
/// there, one recorded after it by `record`), and a job in Payment pending returns to Quotes
/// ready with her backup (PICK-047) or PICK-046's text. A job in a final status only has the pick
/// ended ([`Backup::Done`]).
///
/// # Errors
/// [`PicksError::Database`], [`PicksError::Job`], [`PicksError::MovedOn`] (roll back) or the
/// backup move's errors.
pub async fn end_pending(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    reason: PendingEnd,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    let Some(business) = lock_waiting_on(tx, pick, job).await? else {
        return Ok(Backup::Done);
    };
    match reason {
        PendingEnd::PaymentFailed => end_unpaid(tx, pick, at).await?,
        PendingEnd::CantTake => drop(models::end_pick(tx, pick, "cant_take", at).await?),
        PendingEnd::Ineligible => drop(models::end_pick(tx, pick, "ineligible", at).await?),
    }
    end_pick_intent(tx, pick, at)
        .await
        .map_err(|_| PicksError::Database)?;
    if current(tx, job)
        .await
        .map_err(|_| PicksError::Job)?
        .status
        .is_final()
    {
        return Ok(Backup::Done);
    }
    after_end(tx, (job, pick, business), (actor, at)).await
}

/// The `picks.payment_returned` handler (JOB-006, JOB-014, FEE-020, FEE-038, D-4349):
/// [`payment_returned`] on the task's job at `now`, in one transaction. Safe to run twice.
///
/// # Errors
/// A fixed sentence for a task with no job or a failed step; the queue tries again.
pub async fn run_payment_returned(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let job = task.payload["job"]
        .as_str()
        .and_then(|j| Uuid::parse_str(j).ok())
        .ok_or(HandlerError("payment-returned task has no job"))?;
    let mut tx = pool.begin().await.map_err(|_| RETURN_FAILED)?;
    payment_returned(&mut tx, job, now)
        .await
        .map_err(|_| RETURN_FAILED)?;
    tx.commit().await.map_err(|_| RETURN_FAILED)
}

/// JOB-014, FEE-018, FEE-020, FEE-038, PICK-022, D-4349: the Payment pending wait of `job` ran out
/// at `now`: a job no longer in Payment pending is left alone (JOB-006); with no waiting pick it
/// returns to Quotes ready; a held pick restarts the wait ([`held`]); any other pick ends through
/// [`end_pending`] as `payment_failed`.
///
/// # Errors
/// [`PicksError::Database`], [`PicksError::Job`] or [`end_pending`]'s errors.
pub async fn payment_returned(
    tx: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<(), PicksError> {
    let row = current(tx, job).await.map_err(|_| PicksError::Job)?;
    if !awaits_charge(row.status) {
        return Ok(());
    }
    let Some((pick, _, _)) = models::waiting_pick(tx, job).await? else {
        let arrow = (JobStatus::PaymentPending, JobStatus::QuotesReady);
        return match move_to(tx, job, arrow, now, Actor::System).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PicksError::Job),
        };
    };
    let hold = payment_held(tx, pick)
        .await
        .map_err(|_| PicksError::Database)?;
    if hold == PaymentHold::Free {
        let by = (Actor::System, now);
        return end_pending(tx, (pick, job), PendingEnd::PaymentFailed, by)
            .await
            .map(drop);
    }
    held(tx, (pick, job), (hold, row.status_at), now).await
}

/// D-4349: pick `pick` of `job` is held (`hold`) at `now`: the wait restarts from now (a job that
/// moved on meanwhile is left alone, JOB-006); a held
/// success queues one [`HELD_SUCCESS_KIND`] task; a pick held over [`HELD_LONG`] since `since`
/// (the job's entry into Payment pending) raises its one High [`HELD_LONG_ALERT`] ever.
async fn held(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    (hold, since): (PaymentHold, Timestamp),
    now: Timestamp,
) -> Result<(), PicksError> {
    let restarted = restart_timer(tx, job, (JobStatus::PaymentPending, now))
        .await
        .map_err(|_| PicksError::Job)?;
    if restarted.applied().is_none() {
        return Ok(());
    }
    if hold == PaymentHold::Success {
        let payload = json!({"pick": pick, "run_at": now.to_string()});
        let mut task = NewTask::new(HELD_SUCCESS_KIND, payload, now);
        task.dedupe_key = Some(format!(
            "{HELD_SUCCESS_KIND}.{pick}.{}",
            now.as_microsecond()
        ));
        enqueue(tx, &task).await.map_err(|_| PicksError::Database)?;
    }
    if now.duration_since(since) > HELD_LONG {
        held_long(tx, pick).await?;
    }
    Ok(())
}

/// D-4349: one High [`HELD_LONG_ALERT`] about pick `pick`, once per pick ever: an `audit_once`
/// mark and the alert in the caller's transaction, so an acknowledged alert is not raised again.
async fn held_long(tx: &mut PgConnection, pick: Uuid) -> Result<(), PicksError> {
    let mark = AuditEntry {
        actor: Actor::System,
        action: HELD_LONG_ALERT,
        subject_kind: PICK_SUBJECT,
        subject_id: pick,
        change: Change::Action,
    };
    if !audit_once(tx, &mark).await.map_err(|_| PicksError::Audit)? {
        return Ok(());
    }
    let alert = NewAlert {
        kind: HELD_LONG_ALERT,
        severity: Severity::High,
        subject: Some((PICK_SUBJECT, pick)),
    };
    raise_alert(tx, &alert)
        .await
        .map(drop)
        .map_err(|_| PicksError::Database)
}

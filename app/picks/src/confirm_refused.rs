//! The refused-at-Confirm ending and the public Confirm deadline (PICK-020, PICK-046, PICK-047;
//! D-056, D-4349, D-5017; T772).
//!
//! The Confirm press (`payments`, T664) re-checks the stored license and account results
//! (`confirm::check_at_confirm`, PICK-020) against the pick's stored window end, which
//! [`confirm_deadline`] reads for it. When that check refuses, [`refuse_confirm`] runs the whole
//! ending in the press's transaction: the pick ends and moves to her backup
//! (`confirm::refused_at_confirm`), the job returns from Awaiting confirm to Quotes ready when no
//! pick then waits, and only after that last move her PICK-047 or PICK-046 text event is queued
//! at the job's new version, as `handlers::lapse_at` and `cant_take::end_waiting` do (D-3750).
//! It closes no PaymentIntent: the caller does.

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::Backup;
use crate::confirm::{Refusal, refused_at_confirm};
use crate::models::{self, PicksError};
use crate::pick_ended::{pick_business, queue_backup_text};
use crate::status::{PickStatus, awaits_confirm};

/// PICK-020, D-056, D-5017: the stored end of pick `pick`'s Confirm window (`confirm_due`) while
/// it still awaits its Confirm, the deadline `check_at_confirm` takes; `None` when the window has
/// not started (its notice is not sent yet) or the pick no longer waits. It changes nothing.
///
/// # Errors
///
/// [`PicksError::Database`] when the pick cannot be read.
pub async fn confirm_deadline(
    db: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<Timestamp>, PicksError> {
    let due = sqlx::query_scalar!(
        "SELECT confirm_due FROM picks.pick WHERE id = $1 AND status = $2",
        pick,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(due.flatten().map(|t| t.to_jiff()))
}

/// PICK-020, PICK-047, PICK-046, CON-029, D-4349, D-5017: inside the caller's transaction, ends
/// waiting pick `pick` of `job` whose Confirm `check_at_confirm` refused for `refusal` (an ended
/// window expires it, an account or license refusal cancels it as `ineligible`; no fee, nothing
/// released) and moves it at once to her backup. The job returns from Awaiting confirm to Quotes
/// ready only when no pick then waits, and after that last move her PICK-047 text event
/// ([`Backup::Moved`]) or, with the job not `paused` (her pause, CON-029), her PICK-046 one
/// ([`Backup::NoBackup`]) is queued once. A pick no longer waiting is [`Backup::Done`] and queues
/// nothing. No PaymentIntent is closed here: the caller closes it.
///
/// # Errors
///
/// [`PicksError::MovedOn`] when the job moved under the pick (roll back), [`PicksError::Job`],
/// [`PicksError::Database`] or the backup move's errors.
pub async fn refuse_confirm(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    refusal: Refusal,
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    let backup = refused_at_confirm(tx, (pick, refusal), paused, (actor, at)).await?;
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_confirm(status) && models::waiting_pick(tx, job).await?.is_none() {
        let arrow = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
        match move_to(tx, job, arrow, at, actor).await {
            Ok(Guarded::Applied(())) => {}
            Ok(Guarded::MovedOn) => return Err(PicksError::MovedOn),
            Err(_) => return Err(PicksError::Job),
        }
    }
    if backup != Backup::Done {
        let business = pick_business(tx, pick).await?;
        queue_backup_text(tx, (job, pick, business), (backup, paused), at).await?;
    }
    Ok(backup)
}

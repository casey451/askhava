//! A business leaving ends its waiting picks (BIZ-051, PICK-047, PICK-046; D-446, D-491; T228).
//!
//! [`waiting_picks_of`] lists, locked, every pick still waiting for the leaving business's Confirm;
//! [`end_for_leaving`] ends each at once as `cancelled` with `end_reason` `ineligible` (the
//! account no longer takes picks; nothing was confirmed, so nothing is charged) and moves it to the
//! homeowner's backup (`backup_choice`, PICK-047); with no backup and no other pick waiting the job
//! goes back to Quotes ready. After that move it queues her PICK-047 text event (a moved pick) or,
//! with the job not paused, her PICK-046 one (no backup), as "Can't take it" does (D-3750). On a
//! job already in Payment pending it ends the pick through `connect::end_pending` as `ineligible`
//! (T669, D-4349): the intent ends through `payments`' own pick end, so a later success is
//! refunded (FEE-021) and nothing is released, and the job returns to Quotes ready.

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::{Backup, backup_choice};
use crate::connect::{PendingEnd, end_pending};
use crate::models::{self, PicksError};
use crate::pick_ended::{pick_business, queue_backup_text};
use crate::status::{PickStatus, awaits_charge, awaits_confirm};

/// BIZ-051: every pick waiting for `business`'s Confirm, as (pick, job), locked, oldest first.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn waiting_picks_of(
    tx: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<(Uuid, Uuid)>, PicksError> {
    let rows = sqlx::query!(
        "SELECT id, job_id FROM picks.pick WHERE business_id = $1 AND status = $2 \
         ORDER BY picked_at, id FOR UPDATE",
        business,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_all(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(rows.into_iter().map(|r| (r.id, r.job_id)).collect())
}

/// BIZ-051, PICK-047, PICK-046, D-446: inside the caller's transaction, ends waiting pick `pick`
/// of `job` as `ineligible` (no fee) and moves it to the backup she named ([`Backup::Moved`]);
/// after its last job move [`Backup::Moved`] queues her PICK-047 text event and
/// [`Backup::NoBackup`] her PICK-046 one (not on a paused job). With no pick waiting after it, the
/// job returns from Awaiting confirm to Quotes ready. `paused` is the job's homeowner pause
/// (CON-029). A pick no longer waiting is [`Backup::Done`]. FEE-021, D-4349: on a job in Payment
/// pending the pick ends through `end_pending` as `ineligible` (its intent ended, a later success
/// refunded, the job back to Quotes ready), so the leaving business is never charged for it.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Job`], [`PicksError::MovedOn`] (roll back) or the
/// backup move's errors.
pub async fn end_for_leaving(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_charge(status) {
        return end_pending(tx, (pick, job), PendingEnd::Ineligible, (actor, at)).await;
    }
    if !models::end_pick(tx, pick, "ineligible", at).await? {
        return Ok(Backup::Done);
    }
    let backup = backup_choice(tx, pick, paused, (actor, at)).await?;
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_confirm(status) && models::waiting_pick(tx, job).await?.is_none() {
        let arrow = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
        match move_to(tx, job, arrow, at, actor).await {
            Ok(Guarded::Applied(())) => {}
            Ok(Guarded::MovedOn) => return Err(PicksError::MovedOn),
            Err(_) => return Err(PicksError::Job),
        }
    }
    let business = pick_business(tx, pick).await?;
    queue_backup_text(tx, (job, pick, business), (backup, paused), at).await?;
    Ok(backup)
}

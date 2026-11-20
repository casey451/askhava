//! "Can't take it" (PICK-050, PICK-046, PICK-047, OPS-034, FEE-005, FEE-016, FEE-017; D-446, D-491,
//! D-493, D-3024, D-3722; T214, T467).
//!
//! The pick notice carries the link to `/b/picks/{pick}/cant-take` ([`cant_take_link`], from the
//! pick ID and the brand file's domain only); the page asks for a signed-in owner or "Confirm and
//! billing" member of the picked business. Pressing it ends the waiting pick at once as
//! `cancelled` with `end_reason` `cant_take` ([`cant_take`]): no fee is charged (nothing was
//! confirmed), the ended pick is the record, and nothing counts it against the business. The pick
//! then moves at once to the homeowner's backup (`backup_choice`, PICK-047); with no backup and
//! no other pick waiting the job goes back to Quotes ready. After that move her PICK-047 text
//! event (a moved pick) or, with the job not paused, her PICK-046 one (no backup) is queued here
//! at the job's new version, as `pick_ended` queues them (D-3722, D-3750). On a job already in
//! Payment pending (its business pressed Confirm) it ends the pick through
//! `connect::end_pending` instead (T669, D-4349): the PaymentIntent ends through `payments`' own
//! pick end, so a charge in flight that succeeds later is refunded (FEE-021) and nothing is
//! released, the job returns to Quotes ready and her backup or PICK-046 text follows.
//!
//! [`end_for_fee_terms`] (T467; D-1871 (3), D-3722) ends a waiting pick the same way with the
//! reason `fee_terms_changed` when the owner switched the fee mode after the pick: only while the
//! job is in Awaiting confirm, the stored fee's mode differs from the terms in force and
//! `payments::api::end_intent_unless_charging` finds no charge that may still succeed.

use askhava_core::api::{Guarded, brand};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{end_intent_unless_charging, fee_terms_in_force, pick_fee};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::{Backup, backup_choice};
use crate::connect::{PendingEnd, end_pending};
use crate::models::{self, PicksError};
use crate::pick_ended::{pick_business, queue_backup_text};
use crate::status::{PickStatus, awaits_charge, awaits_confirm};

/// PICK-050, D-3024: the on-domain address of pick `pick`'s "Can't take it" page: the pick ID
/// only, no token, secret or homeowner detail.
#[must_use]
pub fn cant_take_link(pick: Uuid) -> String {
    format!("https://{}/b/picks/{pick}/cant-take", brand().domain())
}

/// PICK-050: the job of pick `pick`, locked, while it waits for `business`'s Confirm; `None`
/// for another business's pick or one that no longer waits (the page then acts on nothing).
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn cant_take_job(
    tx: &mut PgConnection,
    pick: Uuid,
    business: Uuid,
) -> Result<Option<Uuid>, PicksError> {
    let row = sqlx::query_scalar!(
        "SELECT job_id FROM picks.pick WHERE id = $1 AND business_id = $2 AND status = $3 \
         FOR UPDATE",
        pick,
        business,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row)
}

/// PICK-050, PICK-047, PICK-046, D-446: inside the caller's transaction, ends waiting pick `pick`
/// of `job` at once as `cant_take` (no fee, never counted against the business) and moves it to
/// the backup she named ([`Backup::Moved`], queuing her PICK-047 text event); [`Backup::NoBackup`]
/// queues her PICK-046 text event (not on a paused job). With no pick waiting after it, the job returns from Awaiting
/// confirm to Quotes ready. `paused` is the job's homeowner pause (CON-029). A pick no longer
/// waiting is [`Backup::Done`]. FEE-021, D-4349: on a job in Payment pending the pick ends
/// through `end_pending` as `cant_take` (its intent ended, a later success refunded, the job back
/// to Quotes ready), so the business is never charged for it nor given her details.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Job`], [`PicksError::MovedOn`] (roll back) or the
/// backup move's errors.
pub async fn cant_take(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_charge(status) {
        return end_pending(tx, (pick, job), PendingEnd::CantTake, (actor, at)).await;
    }
    end_waiting(tx, (pick, job), "cant_take", paused, (actor, at)).await
}

/// PICK-050, PICK-047, PICK-046, OPS-034, FEE-005, FEE-016, FEE-017, D-1871 (3), D-3722: inside the
/// caller's transaction, ends waiting pick `pick` of `job` whose fee terms changed before its
/// Confirm. It locks the pick row first (always `picks.pick` before `payments.pick_intent`), and
/// only when the job is in Awaiting confirm and the pick's stored fee has another mode than the
/// terms in force, and `payments` ended the pick's intent because no charge of it may still
/// succeed, ends the pick as `cancelled` with `fee_terms_changed` (no fee, never `expired`, never
/// counted against the business), moves it to her backup with her PICK-047 text or queues her PICK-046 text, as
/// [`cant_take`] does. Every other case changes nothing and is [`Backup::Done`].
///
/// # Errors
///
/// [`PicksError::Database`] for a failed read or write, [`PicksError::Job`],
/// [`PicksError::MovedOn`] (roll back) or the backup move's errors.
pub async fn end_for_fee_terms(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    match models::lock_waiting(tx, pick).await? {
        Some((waiting_job, _)) if waiting_job == job => {}
        _ => return Ok(Backup::Done),
    }
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if !awaits_confirm(status) {
        return Ok(Backup::Done);
    }
    let Some(fee) = pick_fee(tx, pick).await.map_err(|_| PicksError::Database)? else {
        return Ok(Backup::Done);
    };
    let terms = fee_terms_in_force(tx)
        .await
        .map_err(|_| PicksError::Database)?;
    if fee.mode == terms.mode() {
        return Ok(Backup::Done);
    }
    let ended = end_intent_unless_charging(tx, pick, at).await;
    if !ended.map_err(|_| PicksError::Database)? {
        return Ok(Backup::Done);
    }
    end_waiting(tx, (pick, job), "fee_terms_changed", paused, (actor, at)).await
}

/// PICK-050, PICK-047, PICK-046, D-3722, D-3750: ends waiting pick `pick` as `cancelled` with
/// `reason`, moves it to her backup, returns the job to Quotes ready when no pick waits, and only
/// after that move queues her PICK-047 text (moved) or, with no pause, her PICK-046 text (no
/// backup) at the job's new version.
async fn end_waiting(
    tx: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    reason: &str,
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    if !models::end_pick(tx, pick, reason, at).await? {
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

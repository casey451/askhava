//! "Can't take it" (PICK-050; D-446, D-491, D-493, D-3024; T214).
//!
//! The pick notice carries the link to `/b/picks/{pick}/cant-take` ([`cant_take_link`], from the
//! pick ID and the brand file's domain only); the page asks for a signed-in owner or "Confirm and
//! billing" member of the picked business. Pressing it ends the waiting pick at once as
//! `cancelled` with `end_reason` `cant_take` ([`cant_take`]): no fee is charged (nothing was
//! confirmed), the ended pick is the record, and nothing counts it against the business. The pick
//! then moves at once to the homeowner's backup (`backup_choice`, PICK-047); with no backup and
//! no other pick waiting the job goes back to Quotes ready. Her PICK-047 or PICK-046 text is the
//! caller's, from the returned [`Backup`].

use askhava_core::api::{Guarded, brand};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::{Backup, backup_choice};
use crate::models::{self, PicksError};
use crate::status::{PickStatus, awaits_confirm};

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
/// the backup she named ([`Backup::Moved`]); [`Backup::NoBackup`] means the caller sends PICK-046's
/// text. With no pick waiting after it, the job returns from Awaiting confirm to Quotes ready.
/// `paused` is the job's homeowner pause (CON-029). A pick no longer waiting is [`Backup::Done`].
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
    if !models::end_pick(tx, pick, "cant_take", at).await? {
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
    Ok(backup)
}

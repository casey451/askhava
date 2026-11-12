//! A business leaving ends its waiting picks (BIZ-051, PICK-047, PICK-046; D-446, D-491; T228).
//!
//! [`waiting_picks_of`] lists, locked, every pick still waiting for the leaving business's Confirm;
//! [`end_for_leaving`] ends each at once as `cancelled` with `end_reason` `ineligible` (the
//! account no longer takes picks; nothing was confirmed, so nothing is charged) and moves it to the
//! homeowner's backup (`backup_choice`, PICK-047); with no backup and no other pick waiting the job
//! goes back to Quotes ready. Her PICK-047 or PICK-046 text is the caller's, from the returned
//! [`Backup`], as for "Can't take it" (`cant_take`).

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::{Backup, backup_choice};
use crate::models::{self, PicksError};
use crate::status::{PickStatus, awaits_confirm};

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
/// [`Backup::NoBackup`] means the caller sends PICK-046's text. With no pick waiting after it, the
/// job returns from Awaiting confirm to Quotes ready. `paused` is the job's homeowner pause
/// (CON-029). A pick no longer waiting is [`Backup::Done`].
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
    Ok(backup)
}

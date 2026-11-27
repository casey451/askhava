//! A job's final status ends its waiting picks (FEE-021, PICK-038, QH-007; D-4748 (4), D-5664).
//!
//! The `jobs.final_status` handler (`aftermath::run_final_status`) calls [`end_for_final`] in its
//! transaction, after `messaging::api::expire_job_rows` and before `no_longer_active`: every pick
//! of the job still awaiting Confirm or in Payment pending (status `awaiting_confirm`, not ended)
//! goes through `connect::end_pending` with [`PendingEnd::JobFinal`], so it ends as `job_final` and
//! its PaymentIntent ends through `payments`' own pick end: cancelled at Stripe, and a success,
//! stored before or recorded after, refunded once (D-2958). A pick already `paid`, `free` or ended
//! is untouched; the job is final, so it is not moved and no backup is offered. Its queued Confirm
//! reminders and window end then find no waiting pick and act on nothing. A second delivery finds
//! nothing waiting and ends nothing. With no stored PaymentIntent (an auto-confirm charge not yet
//! stored, case B) the cancel is a no-op; that case is T689's (D-5664).

use askhava_jobs::api::JobStatus;
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::connect::{PendingEnd, end_pending};
use crate::models::PicksError;
use crate::status::PickStatus;

/// FEE-021, PICK-038, D-4748, D-5664: when `job`'s current `status` is final, ends every waiting,
/// not-ended pick of the job at `at` in the caller's transaction, through [`end_pending`] with
/// [`PendingEnd::JobFinal`] as the system, cancelling each pick's PaymentIntent. Returns how many
/// picks this call found waiting; a replay, or a status not final, ends none.
///
/// # Errors
/// [`PicksError::Database`], or [`end_pending`]'s errors (roll back).
pub async fn end_for_final(
    tx: &mut PgConnection,
    job: Uuid,
    status: JobStatus,
    at: Timestamp,
) -> Result<usize, PicksError> {
    if !status.is_final() {
        return Ok(0);
    }
    let picks = sqlx::query_scalar!(
        "SELECT id FROM picks.pick WHERE job_id = $1 AND status = $2 AND ended_at IS NULL \
         ORDER BY id",
        job,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    for &pick in &picks {
        end_pending(tx, (pick, job), PendingEnd::JobFinal, (Actor::System, at)).await?;
    }
    Ok(picks.len())
}

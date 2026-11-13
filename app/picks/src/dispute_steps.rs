//! T314: the `picks.dispute_opened` handler that judges FEE-050's auto-submit condition for a bank
//! dispute and marks it through T191's `dispute_auto_submit` (FEE-027, FEE-050; D-229, D-2204,
//! D-3118).
//!
//! `payments::api::record_dispute` queues two runs in the dispute's transaction: one at once and
//! one at the due date less `SUBMIT_BEFORE`, so a call attempt or release recorded after the
//! dispute opened still marks it before the submit task runs. A run gathers the pick's release row
//! (`picks.release`), the call log of the line opened for that release (`calls::api::job_line`,
//! `call_log`, each call read through T270's `call_record`; a business call is an attempt, D-3510) and the stored 3-D Secure result: with
//! a Stripe client it first records it through T405's `payments::api::record_three_ds` (outside
//! any transaction, rust brain R11), then reads `three_ds_authenticated` (no row reads `false`).
//! The mark and its submit task commit in one transaction; a dispute already marked or submitted
//! is unchanged, so a retried or repeated run marks nothing twice (JOB-006). Payloads hold IDs only.

use askhava_calls::api::{call_log, job_line};
use askhava_core::api::{Claimed, HandlerError};
use askhava_payments::api::{Caller, ThreeDsStripe, record_three_ds, three_ds_authenticated};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::unreachable_refund::{call_record, dispute_auto_submit};
use crate::unreachable_steps::StepParts;

const FAILED: HandlerError = HandlerError("the dispute check failed");
const MALFORMED: HandlerError = HandlerError("the dispute check payload is malformed");

/// FEE-050, D-3510: whether `pick` was released, and how many business call attempts the line
/// opened for that release logged (each read through [`call_record`], as FEE-022 counts them);
/// `(false, 0)` before a release.
async fn release_and_calls(
    db: &mut PgConnection,
    pick: Uuid,
) -> Result<(bool, usize), HandlerError> {
    let row = sqlx::query!(
        "SELECT job_id, business_id, contact_id FROM picks.release WHERE pick_id = $1",
        pick
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| FAILED)?;
    let Some(row) = row else {
        return Ok((false, 0));
    };
    let whose = (row.job_id, row.business_id, row.contact_id);
    let Some((line, _)) = job_line(db, whose).await.map_err(|_| FAILED)? else {
        return Ok((true, 0));
    };
    let log = call_log(db, line).await.map_err(|_| FAILED)?;
    let business = log.iter().map(call_record);
    Ok((
        true,
        business.filter(|c| c.caller == Caller::Business).count(),
    ))
}

/// The `picks.dispute_opened` handler (FEE-027, FEE-050, D-2204, D-3118): for the payload's
/// dispute (`dp_…`) and pick, records the payment's 3-D Secure result when `parts` holds a Stripe
/// client (none in dev and CI: only what is stored is read), reads the release, the call attempts
/// and the stored result, and marks the dispute for auto-submit when they meet FEE-050; every
/// other dispute waits for Casey. Safe to run twice.
///
/// # Errors
/// A fixed sentence for a malformed payload, or a failed Stripe read or database step (retried).
pub async fn run_dispute_opened<P: ThreeDsStripe>(
    pool: PgPool,
    parts: std::sync::Arc<StepParts<P>>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let dispute = task.payload["dispute"].as_str().ok_or(MALFORMED)?;
    let pick: Uuid = task.payload["pick"]
        .as_str()
        .and_then(|p| p.parse().ok())
        .ok_or(MALFORMED)?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    if let Some(stripe) = parts.stripe.as_ref() {
        record_three_ds(&mut db, stripe, pick)
            .await
            .map_err(|_| FAILED)?;
    }
    let (released, calls) = release_and_calls(&mut db, pick).await?;
    let three_ds = three_ds_authenticated(&mut db, pick)
        .await
        .map_err(|_| FAILED)?;
    let mut tx = db.begin().await.map_err(|_| FAILED)?;
    dispute_auto_submit(&mut tx, dispute, (released, calls, three_ds), now)
        .await
        .map_err(|_| FAILED)?;
    tx.commit().await.map_err(|_| FAILED)
}

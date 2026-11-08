//! The settle and the masked-line retry (PICK-022, PRIV-005, BIZ-014, CALL-001; D-1564, D-1688,
//! D-1739).
//!
//! [`settle`] is the only code that moves a pick to `paid` or `free`. It takes `payments`'
//! cleared [`Releasable`] by reference, never a bare basis, so no crate can mark a pick paid or
//! free without `payments`' clearance (a verified succeeded charge or a recorded $0 decision);
//! the release then consumes the same `Releasable` through the minter. It moves only an
//! `awaiting_confirm` pick that has not ended, under the pick's row lock, with one audit row in
//! the caller's transaction, and leaves every other status alone ([`Settled`]); a pick already
//! `paid` or `free` for this basis answers [`Settled::Was`], so a restart between the settle's
//! commit and the release's claim releases on the retry (D-1739).
//!
//! [`retry_line`] is the [`LINE_RETRY_KIND`] handler `release` queues when a shown release's
//! masked line fails to open (D-1688). It opens the line from the release row's bound contact
//! (T253) only when the job never had one (`calls::api::job_had_line`), so a line closed after
//! 30 days is never reopened and a job whose line is open is left alone; a failure is the task's
//! error, so the worker's retries and dead letter apply, and the release's High alert stays.

use askhava_adapters::api::MaskedPool;
use askhava_calls::api::{Connected, assign_number, job_had_line};
use askhava_core::api::{Claimed, HandlerError};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_payments::api::Releasable;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::PicksError;
use crate::status::{PickStatus, release_status};

/// The masked-line retry task a failed line queues, one per pick (CALL-001, D-1688).
pub const LINE_RETRY_KIND: &str = "picks.line_retry";

const RETRY_FAILED: HandlerError = HandlerError("masked line retry failed");

/// What [`settle`] did with the pick (PICK-022, D-1564).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// Moved from awaiting confirm to `paid` or `free` now; the pick's job.
    Moved(Uuid),
    /// Already `paid` or `free` for the clearing's basis (an earlier call moved it); the job.
    Was(Uuid),
    /// Unknown, ended, or in any other status: left alone, and nothing is to be released.
    Left,
}

/// Moves the pick `cleared` names from `awaiting_confirm` to `paid` (a paid charge) or `free` (a
/// $0 decision), in the caller's transaction (PICK-022, PRIV-005, BIZ-014, D-1564, D-1739).
///
/// # Errors
/// [`PicksError::Database`] or [`PicksError::Audit`]; nothing is moved then.
pub async fn settle(tx: &mut PgConnection, cleared: &Releasable) -> Result<Settled, PicksError> {
    let (pick, want) = (cleared.pick(), release_status(cleared.basis()));
    let row = sqlx::query!(
        r#"SELECT job_id, status, ended_at IS NOT NULL AS "ended!"
           FROM picks.pick WHERE id = $1 FOR UPDATE"#,
        pick
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    let Some(row) = row else {
        return Ok(Settled::Left);
    };
    if row.ended {
        return Ok(Settled::Left);
    }
    if row.status == want.code() {
        return Ok(Settled::Was(row.job_id));
    }
    if row.status != PickStatus::AwaitingConfirm.code() {
        return Ok(Settled::Left);
    }
    sqlx::query!(
        "UPDATE picks.pick SET status = $2 WHERE id = $1",
        pick,
        want.code()
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    let action = if want == PickStatus::Paid {
        "picks.settled_paid"
    } else {
        "picks.settled_free"
    };
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "picks.pick",
        subject_id: pick,
        change: Change::Action,
    };
    audit(tx, &entry).await.map_err(|_| PicksError::Audit)?;
    Ok(Settled::Moved(row.job_id))
}

/// The [`LINE_RETRY_KIND`] handler: opens the released job's masked line from `masked` for the
/// release row's bound contact, only when the job never had one (CALL-001, CALL-010, D-1688).
/// Safe to run twice: once a line exists it does nothing.
///
/// # Errors
/// A [`HandlerError`] when the payload has no pick or the line still fails to open; the worker
/// retries it and dead-letters it after its attempts.
pub async fn retry_line(
    pool: PgPool,
    masked: MaskedPool,
    task: Claimed,
    at: Timestamp,
) -> Result<(), HandlerError> {
    let pick = task.payload["pick"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("line retry is missing a pick"))?;
    let mut conn = pool.acquire().await.map_err(|_| RETRY_FAILED)?;
    let bound = sqlx::query!(
        "SELECT job_id, business_id, contact_id FROM picks.release WHERE pick_id = $1",
        pick
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| RETRY_FAILED)?;
    let Some(bound) = bound else {
        return Ok(());
    };
    if job_had_line(&mut conn, bound.job_id)
        .await
        .map_err(|_| RETRY_FAILED)?
    {
        return Ok(());
    }
    let job = Connected {
        job: bound.job_id,
        homeowner: bound.contact_id,
        business: bound.business_id,
    };
    assign_number(&mut conn, &masked, job, at)
        .await
        .map(|_| ())
        .map_err(|_| RETRY_FAILED)
}

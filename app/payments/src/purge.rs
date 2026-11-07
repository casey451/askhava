//! The retention purge's dispute step: at the contact purge, each bank dispute on the job's
//! charged picks (T066's `payments.dispute`) becomes a `privacy.retention_hold` through
//! `privacy::api`, so contact details stay while a dispute is open (PRIV-007, D-491).
//!
//! T066 records no dispute outcome, so a dispute counts as open until [`DISPUTE_OPEN_AFTER_DUE`]
//! after its evidence due date, the time the card network takes to decide (D-1800); after that its
//! hold is released. Ledger rows hold IDs only and are kept 7 years, so this step deletes nothing
//! (PRIV-010).

use askhava_core::api::{Claimed, HandlerError};
use askhava_privacy::api::{HoldSource, PurgeStage, place_hold, purge_target, release_hold};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// How long after its evidence due date a dispute with no recorded outcome still holds (D-1800).
pub const DISPUTE_OPEN_AFTER_DUE: SignedDuration = SignedDuration::from_hours(90 * 24);

/// PRIV-007, D-491: places a hold for each open dispute on `job`'s charges and releases the hold
/// of each one past [`DISPUTE_OPEN_AFTER_DUE`]; returns how many disputes are open.
///
/// # Errors
/// A fixed [`HandlerError`] on a database failure.
pub async fn place_dispute_holds(
    db: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<usize, HandlerError> {
    let failed = HandlerError("dispute hold database call failed");
    let rows = sqlx::query!(
        r#"SELECT d.id, d.due_at AS "due_at: jiff_sqlx::Timestamp" FROM payments.dispute d
           JOIN payments.charge_evidence e ON e.pick_id = d.pick_id WHERE e.job_id = $1"#,
        job,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| failed)?;
    let mut open = 0;
    for row in rows {
        let source = HoldSource::Dispute;
        if row.due_at.to_jiff() + DISPUTE_OPEN_AFTER_DUE > now {
            open += 1;
            place_hold(db, source, row.id, job, now).await
        } else {
            release_hold(db, source, row.id, job, now).await
        }
        .map_err(|_| failed)?;
    }
    Ok(open)
}

/// The composition's step on a contact-stage purge task, before `privacy` decides: runs
/// [`place_dispute_holds`] at the wall clock; any other stage is left alone.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database failure.
pub async fn run_dispute_holds(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    if stage != PurgeStage::Contact {
        return Ok(());
    }
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| HandlerError("dispute hold database call failed"))?;
    place_dispute_holds(&mut conn, job, Timestamp::now()).await?;
    Ok(())
}

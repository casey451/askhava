//! What `payments` still holds for a pick whose job waits in Payment pending (JOB-014, FEE-018,
//! FEE-020, FEE-021, FEE-038, OPS-024, PICK-022; T663, D-4349).
//!
//! `payments` runs the real payment waits on its own timers (FEE-020's card fix, FEE-018's
//! 4-hour Stripe-down release, both stopped while charges are paused, FEE-038) and ends the pick
//! itself through `payments.pick_ended`. `jobs`' Payment pending timer therefore never returns a
//! job on its own: `picks::api::run_payment_returned` asks [`payment_held`] first, and only a pick
//! nothing holds is ended (through [`end_pick_intent`], `payments`' own pick end). `picks` never
//! reads safe mode or the charging switch itself (OPS-001, D-4726); [`press_may_charge`] is the
//! one question a Confirm press asks before it moves a job (T664).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::handlers::{EndReason, charges_paused, end_pick};
use crate::models::ChargeError;
use crate::pick_fee::pick_fee;
use crate::release_proof::no_fee_release;
use jiff::Timestamp;

/// What `payments` holds for a pick (JOB-014, FEE-020, FEE-038, PICK-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentHold {
    /// Nothing holds the pick: no running or stopped pay timer, no attempt in flight or
    /// succeeded, no $0 decision and charges not paused; the wait may end it.
    Free,
    /// A pay timer runs or is stopped, an attempt is `sent`, `processing` or `needs_action`, or
    /// charges are paused (safe mode or the owner's charging switch, FEE-038).
    Held,
    /// A `succeeded` attempt or a recorded $0 decision: the pick is to be settled and released
    /// (PICK-022), never ended as failed.
    Success,
}

/// JOB-014, FEE-018, FEE-020, FEE-038, OPS-024, PICK-022, D-4349: what `payments` holds for pick
/// `pick`. A recorded $0 decision or a `succeeded` attempt on an unended PaymentIntent is
/// [`PaymentHold::Success`]; then paused charges, an unended pay timer (running or stopped) or an
/// attempt still `sent`, `processing` or `needs_action` is [`PaymentHold::Held`]; anything else,
/// an intent `payments` already ended included, is [`PaymentHold::Free`].
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn payment_held(conn: &mut PgConnection, pick: Uuid) -> Result<PaymentHold, ChargeError> {
    if free_decided(conn, pick).await? {
        return Ok(PaymentHold::Success);
    }
    let row = sqlx::query!(
        r#"SELECT
             EXISTS (SELECT 1 FROM payments.charge_attempt a
                     WHERE a.pick_id = i.pick_id AND a.status = 'succeeded') AS "paid!",
             EXISTS (SELECT 1 FROM payments.charge_attempt a WHERE a.pick_id = i.pick_id
                     AND a.status IN ('sent', 'processing', 'needs_action')) AS "in_flight!",
             EXISTS (SELECT 1 FROM payments.pay_timer t
                     WHERE t.pick_id = i.pick_id AND t.ended_at IS NULL) AS "timer!"
           FROM payments.pick_intent i WHERE i.pick_id = $1 AND i.ended_at IS NULL"#,
        pick
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| ChargeError::Database)?;
    if row.as_ref().is_some_and(|r| r.paid) {
        return Ok(PaymentHold::Success);
    }
    if charges_paused(conn).await? || row.is_some_and(|r| r.in_flight || r.timer) {
        return Ok(PaymentHold::Held);
    }
    Ok(PaymentHold::Free)
}

/// Whether the ledger holds the $0 decision for pick `pick`'s own business and job, through
/// `no_fee_release`'s check (FEE-010, FEE-013, FEE-045, D-1871).
async fn free_decided(conn: &mut PgConnection, pick: Uuid) -> Result<bool, ChargeError> {
    let Some(fee) = pick_fee(conn, pick)
        .await
        .map_err(|_| ChargeError::Database)?
    else {
        return Ok(false);
    };
    Ok(no_fee_release(conn, pick, (fee.business.0, fee.job))
        .await
        .is_ok())
}

/// OPS-024, FEE-038, D-4349: whether a Confirm press may charge now: false while charges are
/// paused by safe mode or the owner's charging switch (T664's press asks before it moves a job).
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn press_may_charge(conn: &mut PgConnection) -> Result<bool, ChargeError> {
    charges_paused(conn).await.map(|paused| !paused)
}

/// FEE-021, FEE-020, D-2290, D-2958, D-4349: ends pick `pick`'s PaymentIntent through `payments`'
/// own pick end at `at`, in the caller's transaction, whatever attempt is in flight: a success
/// stored before it gets its one late-success refund here, and one recorded after it gets it in
/// `record`. Returns false, changing nothing, for a pick already ended or with no PaymentIntent.
/// The reason is the card-fix end: the pick had its Confirm window and no payment came.
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn end_pick_intent(
    tx: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<bool, ChargeError> {
    end_pick(tx, pick, EndReason::CardFix, at).await
}

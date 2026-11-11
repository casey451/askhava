//! A payment that succeeds after its job moved on is refunded automatically, exactly once, and
//! Casey is told (FEE-021, D-1547).
//!
//! `payments` never reads job or pick tables (D-491), so `picks`, which knows the pick moved on,
//! passes that down as [`PickNow`] when Stripe's success reaches it; `payments` also counts its own
//! record of the pick (`pick_intent.ended_at`, set when it ended the pick itself). In the caller's
//! transaction, [`late_success`] queues one [`LATE_KIND`] task per pick, deduplicated by the pick,
//! and raises the [`LATE_SUCCESS_ALERT`] only when that task is new, so Casey is told once however
//! often the success is reported. The task's handler, [`run_late_refund`], calls Stripe outside any
//! transaction (R11) through `request_refund`, which refunds a pick once (FEE-026).
//!
//! T332 (FEE-018, FEE-020, D-2290): the refund row's trigger is its own `late-success`
//! ([`LATE_TRIGGER`]), and every success `payments` stores calls [`late_success`] in its own
//! transaction, so a success on a pick `payments` ended (the 4-hour end or the card-fix window)
//! is queued whether the webhook, the charge check, or both record it, and whether the end or the
//! success commits first (D-2958).

use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::models::{MoneyEventError, PickNow, RefundTrigger};
use crate::refund_stripe::RefundStripe;
use crate::refunds::request_refund;

/// The `core.task` kind that refunds a late success (FEE-021); one per pick.
pub const LATE_KIND: &str = "payments.late_success_refund";
/// The alert raised once per pick whose payment succeeded after it moved on (FEE-021).
pub const LATE_SUCCESS_ALERT: &str = "payments.late_success";
/// The trigger stored on a late success's refund row, its own code `late-success` (FEE-021,
/// D-1547, D-2290; migration 0177).
pub const LATE_TRIGGER: RefundTrigger = RefundTrigger::LateSuccess;
const PICK_SUBJECT: &str = "payments.pick_intent";

/// What [`late_success`] did with a succeeded payment (FEE-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LateSuccess {
    /// The pick is still current: the payment is the normal Confirm-and-pay success.
    Current,
    /// The pick moved on: its refund is queued and Casey alerted, both for the first time.
    Queued,
    /// The pick moved on and its refund was queued already, or the pick already has a refund or a
    /// bank dispute (`request_refund` or `record_dispute` alerted it): nothing more is written.
    AlreadyQueued,
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, MoneyEventError> {
    result.map_err(|_| MoneyEventError::Database)
}

/// FEE-021: a payment of `pick` that succeeded while `pick_now` says the job or pick moved on
/// (or `payments` ended the pick itself) is queued for its refund and alerted, once per pick.
/// Runs in the caller's transaction; calls no vendor.
///
/// The success must be recorded first. `payments` calls this itself, with `PickNow::Payable`, in
/// the transaction that stores any success (`handlers::record`, so `paid_release` and the
/// `CHECK_KIND` round alike), and `handlers::end_pick` calls it when it ends a pick whose success
/// is already stored, so a success on a pick `payments` ended is queued once whichever of the two
/// commits last (D-2290, D-2958). The select takes the pick's `pick_intent` share lock, so it waits
/// for a concurrent end and reads the committed `ended_at`.
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] when the pick has no succeeded charge (no FEE-042 evidence
/// record) yet, else `Database`.
pub async fn late_success(
    conn: &mut PgConnection,
    pick: Uuid,
    pick_now: PickNow,
    now: Timestamp,
) -> Result<LateSuccess, MoneyEventError> {
    let row = db(sqlx::query!(
        r#"SELECT p.ended_at IS NOT NULL AS "ended!",
                  (EXISTS (SELECT 1 FROM payments.refund r WHERE r.pick_id = e.pick_id)
                   OR EXISTS (SELECT 1 FROM payments.dispute d WHERE d.pick_id = e.pick_id))
                   AS "in_hand!"
             FROM payments.charge_evidence e JOIN payments.pick_intent p ON p.pick_id = e.pick_id
            WHERE e.pick_id = $1 FOR SHARE OF p"#,
        pick,
    )
    .fetch_optional(&mut *conn)
    .await)?
    .ok_or(MoneyEventError::NothingCharged)?;
    if pick_now == PickNow::Payable && !row.ended {
        return Ok(LateSuccess::Current);
    }
    if row.in_hand {
        return Ok(LateSuccess::AlreadyQueued);
    }
    let mut task = NewTask::new(LATE_KIND, json!({ "pick": pick }), now);
    task.dedupe_key = Some(format!("{LATE_KIND}.{pick}"));
    let queued = enqueue(conn, &task)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    if queued.is_none() {
        return Ok(LateSuccess::AlreadyQueued);
    }
    let alert = NewAlert {
        kind: LATE_SUCCESS_ALERT,
        severity: Severity::High,
        subject: Some((PICK_SUBJECT, pick)),
    };
    raise_alert(conn, &alert)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(LateSuccess::Queued)
}

/// The [`LATE_KIND`] handler (FEE-021, FEE-026, JOB-006): refunds the pick in full through
/// `request_refund`, so running it twice refunds once; a failed try is retried hourly by
/// `request_refund`'s own task. A pick with nothing charged has nothing to refund, so the task is
/// done, not dead-lettered (JOB-006, D-1547); [`late_success`] queues it only after a success.
///
/// # Errors
/// A fixed sentence when the payload is malformed or the refund could not be recorded.
pub async fn run_late_refund(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    secret: &SecretString,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = payload["pick"]
        .as_str()
        .and_then(|p| Uuid::parse_str(p).ok())
        .ok_or(HandlerError("late success refund task is malformed"))?;
    match request_refund(pool, stripe, secret, (pick, LATE_TRIGGER), now).await {
        Ok(_) | Err(MoneyEventError::NothingCharged) => Ok(()),
        Err(_) => Err(HandlerError("late success refund could not be recorded")),
    }
}

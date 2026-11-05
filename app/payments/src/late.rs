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

use askhava_adapters::api::PaymentsAdapter;
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::models::{MoneyEventError, PickNow, RefundTrigger};
use crate::refund_rules::RefundReason;
use crate::refunds::request_refund;

/// The `core.task` kind that refunds a late success (FEE-021); one per pick.
pub const LATE_KIND: &str = "payments.late_success_refund";
/// The alert raised once per pick whose payment succeeded after it moved on (FEE-021).
pub const LATE_SUCCESS_ALERT: &str = "payments.late_success";
/// The trigger stored on a late success's refund row: a charge taken after the job moved on
/// (D-1547; `payments.refund.trigger` has no code of its own for it yet).
pub const LATE_TRIGGER: RefundTrigger = RefundTrigger::Rule(RefundReason::ChargedAfterCancel);
const PICK_SUBJECT: &str = "payments.pick_intent";

/// What [`late_success`] did with a succeeded payment (FEE-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LateSuccess {
    /// The pick is still current: the payment is the normal Confirm-and-pay success.
    Current,
    /// The pick moved on: its refund is queued and Casey alerted, both for the first time.
    Queued,
    /// The pick moved on and its refund was queued already: nothing more is written.
    AlreadyQueued,
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, MoneyEventError> {
    result.map_err(|_| MoneyEventError::Database)
}

/// FEE-021: a payment of `pick` that succeeded while `pick_now` says the job or pick moved on
/// (or `payments` ended the pick itself) is queued for its refund and alerted, once per pick.
/// Runs in the caller's transaction; calls no vendor.
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] when the pick has no succeeded charge (no FEE-042 evidence
/// record), else `Database`.
pub async fn late_success(
    conn: &mut PgConnection,
    pick: Uuid,
    pick_now: PickNow,
    now: Timestamp,
) -> Result<LateSuccess, MoneyEventError> {
    let ended = db(sqlx::query_scalar!(
        r#"SELECT p.ended_at IS NOT NULL AS "ended!"
             FROM payments.charge_evidence e JOIN payments.pick_intent p ON p.pick_id = e.pick_id
            WHERE e.pick_id = $1"#,
        pick,
    )
    .fetch_optional(&mut *conn)
    .await)?
    .ok_or(MoneyEventError::NothingCharged)?;
    if pick_now == PickNow::Payable && !ended {
        return Ok(LateSuccess::Current);
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
/// `request_refund`'s own task.
///
/// # Errors
/// A fixed sentence when the payload is malformed or the refund could not be recorded.
pub async fn run_late_refund(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    secret: &SecretString,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = payload["pick"]
        .as_str()
        .and_then(|p| Uuid::parse_str(p).ok())
        .ok_or(HandlerError("late success refund task is malformed"))?;
    request_refund(pool, stripe, secret, (pick, LATE_TRIGGER), now)
        .await
        .map(drop)
        .map_err(|_| HandlerError("late success refund could not be recorded"))
}

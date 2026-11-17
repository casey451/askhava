//! The safety call's key press, kept when the database fails (CON-035, CALL-022, PRIV-016;
//! D-4051, D-4088).
//!
//! The safety call's `Gather` action (`server`'s `voice_status`) takes "press 9" through
//! [`safety_key_once`]. When that step fails, the route queues [`SAFETY_KEY_RETRY_KIND`] with
//! [`queue_safety_key_retry`]: the whole [`SafetyCall`] (IDs and the arrival time only) and the
//! key, never her phone or any body (PRIV-016), one task per call (its dedupe key is the alert).
//! [`run_safety_key_retry`] re-runs [`safety_key_once`], which takes a key once per call, so a
//! repeat changes nothing; the core queue retries it up to [`SAFETY_KEY_RETRY_ATTEMPTS`] claims,
//! and a failure on the last one raises one High [`SAFETY_KEY_LOST_ALERT`] about the call's alert
//! ([`safety_key_lost`], no phone).
//!
//! Twilio and the key (checked for D-4051): Twilio retries no voice action (`Gather`) on a non-2xx
//! answer; it speaks an application error or fetches the number's fallback URL, and the stand-in
//! (`stand_ins/twilio.py`) models no `Gather` or `Redirect` (D-2763). So when the step and the
//! queue both fail (a pool that cannot hand out a connection fails both), the route answers 200
//! with a bounded `<Redirect>` back to itself, and after the last one a spoken STOP line and the
//! same alert when one can be raised.

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Severity, raise_alert_once};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::models::CallsError;
use crate::voice::{SafetyCall, safety_key_once};

/// The core task that takes a safety call's key again after the route's step failed.
pub const SAFETY_KEY_RETRY_KIND: &str = "calls.safety_key_retry";
/// The High alert raised once when a safety call's key could not be taken (no phone).
pub const SAFETY_KEY_LOST_ALERT: &str = "calls.safety_key_lost";
/// Claims of [`SAFETY_KEY_RETRY_KIND`] before it gives up and alerts.
pub const SAFETY_KEY_RETRY_ATTEMPTS: i32 = 8;

const RETRY_FAILED: HandlerError = HandlerError("the safety key retry failed");

/// CON-035, PRIV-016, D-4051: queues one [`SAFETY_KEY_RETRY_KIND`] task for `call` and the key
/// `digits`, due at `now`; a second queue for the same call is refused by its dedupe key.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn queue_safety_key_retry(
    db: &mut PgConnection,
    call: &SafetyCall,
    digits: &str,
    now: Timestamp,
) -> Result<(), CallsError> {
    let payload = json!({
        "contact": call.contact, "message": call.message,
        "arrived_at": call.arrived_at.to_string(), "alert": call.alert, "digits": digits,
    });
    let mut task = NewTask::new(SAFETY_KEY_RETRY_KIND, payload, now);
    task.max_attempts = SAFETY_KEY_RETRY_ATTEMPTS;
    task.dedupe_key = Some(format!("{SAFETY_KEY_RETRY_KIND}:{}", call.alert));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| CallsError::Database)
}

/// CON-035, CALL-022, D-4051: the [`SAFETY_KEY_RETRY_KIND`] handler: takes the key once through
/// [`safety_key_once`] with `vault` at `now`; a failure (or no vault) fails the task for the queue
/// to retry, and on its last attempt raises [`SAFETY_KEY_LOST_ALERT`] first.
///
/// # Errors
///
/// [`HandlerError`] for a bad payload or a failed step.
pub async fn run_safety_key_retry(
    pool: PgPool,
    vault: Option<&Vault>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let call = SafetyCall::from_payload(&task.payload)
        .ok_or(HandlerError("safety key retry payload is malformed"))?;
    let digits = task.payload.get("digits").and_then(|d| d.as_str());
    let digits = digits.ok_or(HandlerError("safety key retry payload is malformed"))?;
    let taken = match (vault, pool.acquire().await) {
        (Some(vault), Ok(mut db)) => safety_key_once(&mut db, vault, &call, digits, now)
            .await
            .map(|_| ()),
        _ => Err(CallsError::Database),
    };
    if taken.is_ok() {
        return Ok(());
    }
    if task.attempts >= SAFETY_KEY_RETRY_ATTEMPTS
        && let Ok(mut db) = pool.acquire().await
    {
        safety_key_lost(&mut db, &call)
            .await
            .map_err(|_| RETRY_FAILED)?;
    }
    Err(RETRY_FAILED)
}

/// CON-035, OPS-004: raises one High [`SAFETY_KEY_LOST_ALERT`] about `call`'s alert (no phone);
/// none while one is open.
///
/// # Errors
///
/// [`CallsError::Ops`].
pub async fn safety_key_lost(db: &mut PgConnection, call: &SafetyCall) -> Result<(), CallsError> {
    let subject = ("ops.alert", call.alert);
    raise_alert_once(db, SAFETY_KEY_LOST_ALERT, Severity::High, subject)
        .await
        .map(|_| ())
        .map_err(|_| CallsError::Ops)
}

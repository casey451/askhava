//! The Confirm-and-pay charge: one PaymentIntent per pick, one attempt per press (FEE-016,
//! FEE-017, FEE-037, FEE-041, FEE-047, D-153, D-204, D-365, D-490, D-491).
//!
//! [`open_intent`] creates the pick's one PaymentIntent on the first Confirm-page load under the
//! key derived from `pick-<id>-intent`, and stores it (FEE-047). [`confirm_and_pay`] is the only
//! code that charges: it runs for the business's press alone, never on its own (FEE-016), and in
//! this order: a repeated press answers from its own attempt; the FEE-017 check (the caller's
//! [`PickNow`] and `payments`' own record of the pick); the charging pause (OPS-024, FEE-038);
//! the monthly fee limit (FEE-041), which ends the pick at once; an undecided earlier attempt is
//! settled by asking Stripe first, and blocks a new try until Stripe says it failed (FEE-037);
//! then a new attempt `pick-<id>-try<n>` confirms that same PaymentIntent with its own key. No
//! transaction or pooled connection is held across a Stripe call (R11). The timers and events
//! each answer starts are `handlers.rs`'s.

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::arizona;
use askhava_ops::api::{Actor, AuditEntry, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::handlers::{EndReason, charges_paused, end_pick, record};
use crate::models::{
    BusinessId, ChargeError, ChargeOutcome, PickNow, Press, attempt_label, intent_label,
};
use crate::service::customer_of;
use crate::status::AttemptStatus;

/// The idempotency action of a pick's PaymentIntent create (FEE-047, SEC-018).
pub const INTENT_ACTION: &str = "payments.pick_intent";
/// The idempotency action of one charge attempt's confirm (FEE-016, SEC-018).
pub const ATTEMPT_ACTION: &str = "payments.charge_attempt";
/// The audit subject kind of a pick's charge attempts (JOB-005).
pub const ATTEMPT_SUBJECT: &str = "payments.charge_attempt";

/// The two Stripe calls a pick's one PaymentIntent needs beyond [`PaymentsAdapter`]: create it
/// unconfirmed, and confirm it with one attempt's key (FEE-047). The adapter methods that post
/// them are T132's (`app/adapters/src/payments.rs`, D-1424).
pub trait IntentStripe: PaymentsAdapter {
    /// Creates the PaymentIntent for `amount_cents` on `customer` with `metadata[pick_id]`,
    /// unconfirmed; returns `pi_…`.
    fn create_intent(
        &self,
        idempotency: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// Confirms `intent` on-session with `payment_method` under one attempt's key.
    fn confirm_intent(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send;
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, ChargeError> {
    result.map_err(|_| ChargeError::Database)
}

async fn intent_of(conn: &mut PgConnection, pick: Uuid) -> Result<Option<String>, ChargeError> {
    db(sqlx::query_scalar!(
        "SELECT stripe_intent_id FROM payments.pick_intent WHERE pick_id = $1",
        pick
    )
    .fetch_optional(conn)
    .await)
}

/// The pick's one PaymentIntent, created on the first call under the key derived from
/// `pick-<id>-intent`, so two tabs, a reload or a retry reach the same one (FEE-047, D-490).
///
/// # Errors
/// [`ChargeError::BadField`] for a $0 fee or an answer that is not `pi_…`;
/// [`ChargeError::NoCustomer`]; [`ChargeError::Stripe`] (nothing stored); `Database`.
pub async fn open_intent(
    pool: &PgPool,
    stripe: &impl IntentStripe,
    secret: &SecretString,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<String, ChargeError> {
    if press.amount_cents == 0 {
        return Err(ChargeError::BadField);
    }
    let mut conn = db(pool.acquire().await)?;
    if let Some(found) = intent_of(&mut conn, press.pick).await? {
        return Ok(found);
    }
    let customer = customer_of(&mut conn, press.business)
        .await
        .map_err(|_| ChargeError::Database)?
        .ok_or(ChargeError::NoCustomer)?;
    drop(conn);
    let key = IdempotencyKey::derive(secret, INTENT_ACTION, &[&intent_label(press.pick)]);
    let pick = press.pick.to_string();
    let created = stripe
        .create_intent(&key, customer.as_str(), press.amount_cents, &pick)
        .await
        .map_err(|_| ChargeError::Stripe)?;
    if !created.starts_with("pi_") {
        return Err(ChargeError::BadField);
    }
    let mut conn = db(pool.acquire().await)?;
    db(sqlx::query!(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (pick_id) DO NOTHING",
        press.pick,
        press.business.0,
        press.job,
        i64::from(press.amount_cents),
        created,
        press.emergency,
        at.to_sqlx() as _,
    )
    .execute(&mut *conn)
    .await)?;
    intent_of(&mut conn, press.pick)
        .await?
        .ok_or(ChargeError::Database)
}

/// One "Confirm and pay" press (FEE-016, FEE-017, FEE-037, FEE-041, FEE-047): see the module
/// docs for the order of checks. Every attempt confirms the pick's one PaymentIntent with the key
/// derived from `pick-<id>-try<n>`; nothing here retries a charge.
///
/// # Errors
/// [`ChargeError::BadField`] for a $0 fee or a card ID that is not `pm_…`; the errors of
/// [`open_intent`]; [`ChargeError::Database`].
pub async fn confirm_and_pay(
    pool: &PgPool,
    stripe: &impl IntentStripe,
    secret: &SecretString,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<ChargeOutcome, ChargeError> {
    if !press.payment_method.starts_with("pm_") {
        return Err(ChargeError::BadField);
    }
    let intent = open_intent(pool, stripe, secret, press, at).await?;
    let mut conn = db(pool.acquire().await)?;
    if let Some(status) = press_status(&mut conn, press).await? {
        drop(conn);
        let status = if status.undecided() {
            ask_stripe(pool, stripe, press.pick, at)
                .await?
                .unwrap_or(status)
        } else {
            status
        };
        return report(pool, press.pick, status, &intent).await;
    }
    if let Some(stop) = before_attempt(&mut conn, press, at).await? {
        return Ok(stop);
    }
    if latest(&mut conn, press.pick)
        .await?
        .is_some_and(|(_, s)| s.undecided())
    {
        drop(conn);
        let asked = ask_stripe(pool, stripe, press.pick, at).await?;
        let Some(asked) = asked else {
            return Ok(ChargeOutcome::Pending);
        };
        if let Some(stop) = asked.blocks_new_try() {
            return Ok(stop);
        }
        conn = db(pool.acquire().await)?;
    }
    let Some(try_no) = begin_attempt(&mut conn, press, at).await? else {
        return Ok(ChargeOutcome::Pending);
    };
    drop(conn);
    let key = IdempotencyKey::derive(
        secret,
        ATTEMPT_ACTION,
        &[&attempt_label(press.pick, try_no)],
    );
    let answer = stripe
        .confirm_intent(&key, &intent, press.payment_method)
        .await;
    let status = record(pool, press.pick, try_no, answer_status(&answer), at).await?;
    report(pool, press.pick, status, &intent).await
}

/// The attempt status a confirm call's answer means: a 402 is a decline; a request that never
/// left is unreachable; a timeout or any other refusal stays `Sent` until Stripe is asked
/// (FEE-018, FEE-037).
fn answer_status(answer: &Result<PaymentIntent, AdapterError>) -> AttemptStatus {
    match answer {
        Ok(intent) => AttemptStatus::from_intent(&intent.status),
        Err(AdapterError::Vendor { status: 402, .. }) => AttemptStatus::Declined,
        Err(AdapterError::NotSent | AdapterError::BadConfig | AdapterError::InvalidId) => {
            AttemptStatus::Unreachable
        }
        Err(
            AdapterError::OutcomeUnknown
            | AdapterError::Vendor { .. }
            | AdapterError::Decode
            | AdapterError::ObjectExists,
        ) => AttemptStatus::Sent,
    }
}

/// The page's outcome for `status`, with the card-fix deadline when one runs (FEE-020).
async fn report(
    pool: &PgPool,
    pick: Uuid,
    status: AttemptStatus,
    intent: &str,
) -> Result<ChargeOutcome, ChargeError> {
    let mut conn = db(pool.acquire().await)?;
    let fix_by = db(sqlx::query_scalar!(
        "SELECT due_at FROM payments.pay_timer
         WHERE pick_id = $1 AND kind = 'card-fix' AND ended_at IS NULL",
        pick
    )
    .fetch_optional(&mut *conn)
    .await)?
    .flatten()
    .map(|due| due.to_jiff());
    Ok(status.outcome(fix_by, intent))
}

/// The status of this press's own attempt, if it made one (a repeated press, FEE-016).
async fn press_status(
    conn: &mut PgConnection,
    press: &Press<'_>,
) -> Result<Option<AttemptStatus>, ChargeError> {
    let code = db(sqlx::query_scalar!(
        "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 AND press_id = $2",
        press.pick,
        press.press
    )
    .fetch_optional(conn)
    .await)?;
    Ok(code.as_deref().and_then(AttemptStatus::from_code))
}

/// The pick's latest attempt and its status.
pub(crate) async fn latest(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<(i32, AttemptStatus)>, ChargeError> {
    let row = db(sqlx::query!(
        "SELECT try_no, status FROM payments.charge_attempt WHERE pick_id = $1
         ORDER BY try_no DESC LIMIT 1",
        pick
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.and_then(|r| AttemptStatus::from_code(&r.status).map(|s| (r.try_no, s))))
}

/// The checks before a new attempt (FEE-017, OPS-024, FEE-038, FEE-041); `Some` stops the press.
async fn before_attempt(
    conn: &mut PgConnection,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<Option<ChargeOutcome>, ChargeError> {
    match press.now {
        PickNow::MovedOn => return Ok(Some(ChargeOutcome::MovedOn)),
        PickNow::Payable => {}
    }
    let row = db(sqlx::query!(
        r#"SELECT ended_at IS NOT NULL AS "ended!", EXISTS (SELECT 1 FROM payments.charge_attempt a
             WHERE a.pick_id = i.pick_id AND a.status = 'succeeded') AS "paid!"
           FROM payments.pick_intent i WHERE pick_id = $1"#,
        press.pick
    )
    .fetch_one(&mut *conn)
    .await)?;
    if row.paid {
        return Ok(Some(ChargeOutcome::AlreadyPaid));
    }
    if row.ended {
        return Ok(Some(ChargeOutcome::MovedOn));
    }
    if charges_paused(conn).await? {
        return Ok(Some(ChargeOutcome::Paused));
    }
    if over_limit(conn, press.business, press.amount_cents, at).await? {
        let mut tx = db(sqlx::Connection::begin(&mut *conn).await)?;
        end_pick(&mut tx, press.pick, EndReason::FeeLimit, at).await?;
        db(tx.commit().await)?;
        return Ok(Some(ChargeOutcome::OverLimit));
    }
    Ok(None)
}

/// Sets, or with `None` removes, the business's monthly fee limit (FEE-041, D-204); the portal
/// and card-authorization field that calls it is T224's.
///
/// # Errors
/// [`ChargeError::BadField`] for a limit of 0; [`ChargeError::Database`].
pub async fn set_fee_limit(
    conn: &mut PgConnection,
    business: BusinessId,
    limit_cents: Option<u32>,
) -> Result<(), ChargeError> {
    if limit_cents == Some(0) {
        return Err(ChargeError::BadField);
    }
    db(sqlx::query!(
        "DELETE FROM payments.fee_limit WHERE business_id = $1",
        business.0
    )
    .execute(&mut *conn)
    .await)?;
    if let Some(cents) = limit_cents {
        db(sqlx::query!(
            "INSERT INTO payments.fee_limit (business_id, limit_cents) VALUES ($1, $2)",
            business.0,
            i64::from(cents)
        )
        .execute(&mut *conn)
        .await)?;
    }
    Ok(())
}

/// True when `amount_cents` would take the business's succeeded charges this Arizona calendar
/// month over its limit (FEE-041); exactly at the limit is still charged.
async fn over_limit(
    conn: &mut PgConnection,
    business: BusinessId,
    amount_cents: u32,
    at: Timestamp,
) -> Result<bool, ChargeError> {
    let limit = db(sqlx::query_scalar!(
        "SELECT limit_cents FROM payments.fee_limit WHERE business_id = $1",
        business.0
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(limit) = limit else {
        return Ok(false);
    };
    let month = at
        .to_zoned(arizona())
        .first_of_month()
        .and_then(|day| day.start_of_day())
        .map_err(|_| ChargeError::BadField)?
        .timestamp();
    let charged = db(sqlx::query_scalar!(
        r#"SELECT coalesce(sum(i.amount_cents), 0)::bigint AS "sum!" FROM payments.charge_attempt a
           JOIN payments.pick_intent i USING (pick_id)
           WHERE i.business_id = $1 AND a.status = 'succeeded' AND a.decided_at >= $2"#,
        business.0,
        month.to_sqlx() as _,
    )
    .fetch_one(&mut *conn)
    .await)?;
    Ok(charged + i64::from(amount_cents) > limit)
}

/// Starts the next attempt, try `n`, for this press; `None` when another press started one at
/// the same moment (the page waits for it, FEE-016).
async fn begin_attempt(
    conn: &mut PgConnection,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<Option<i32>, ChargeError> {
    let mut tx = db(sqlx::Connection::begin(&mut *conn).await)?;
    let try_no = db(sqlx::query_scalar!(
        r#"INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           SELECT $1, coalesce(max(try_no), 0) + 1, $2, 'sent', $3
           FROM payments.charge_attempt WHERE pick_id = $1
           ON CONFLICT DO NOTHING RETURNING try_no"#,
        press.pick,
        press.press,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    if try_no.is_some() {
        let actor = Actor::Business(press.business.0);
        let entry = AuditEntry::status_change(actor, (ATTEMPT_SUBJECT, press.pick), "new", "sent");
        audit(&mut tx, &entry)
            .await
            .map_err(|_| ChargeError::Database)?;
    }
    db(tx.commit().await)?;
    Ok(try_no)
}

/// Asks Stripe for the pick's PaymentIntent and records what it says on the latest attempt
/// (FEE-019, FEE-037); `None` when Stripe could not be reached. Asking never charges.
///
/// # Errors
/// [`ChargeError::Database`].
pub(crate) async fn ask_stripe(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    pick: Uuid,
    at: Timestamp,
) -> Result<Option<AttemptStatus>, ChargeError> {
    let mut conn = db(pool.acquire().await)?;
    let (Some(intent), Some((try_no, stored))) = (
        intent_of(&mut conn, pick).await?,
        latest(&mut conn, pick).await?,
    ) else {
        return Ok(None);
    };
    drop(conn);
    let Ok(found) = stripe.fetch_payment_intent(&intent).await else {
        return Ok(None);
    };
    let status = AttemptStatus::from_intent(&found.status);
    if status.settles(stored) {
        return record(pool, pick, try_no, status, at).await.map(Some);
    }
    Ok(Some(stored))
}

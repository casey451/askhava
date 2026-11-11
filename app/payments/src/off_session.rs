//! The one off-session charge: the PICK-048 auto-confirm charge (T209; PICK-048, PICK-056,
//! FEE-014, FEE-016, FEE-041, D-401, D-480, D-1758).
//!
//! [`charge_off_session`] is the only code that charges a card without its business's press, and
//! `picks::auto_confirm` its only caller. In order: Stripe's written answer must be filed and
//! allow it ([`stripe_answer_filed`], `catalog`'s record), else nothing is charged; the card the
//! box was ticked with must still be the business's saved card ([`saved_card_of`]), else nothing
//! is charged; then one attempt goes through [`confirm_and_pay`] itself, so it passes every check
//! a press passes (the fee fixed at the pick, the FEE-017 state, the charging pause, an earlier
//! undecided try, and the hard monthly limit reserved in `begin_attempt`'s transaction, D-1758),
//! with the pick's ID as its one press ID: the attempt key is `pick-<id>-try<n>` like every
//! attempt, and a second call answers from the first attempt and never sends again (FEE-016).
//! Only the confirm call differs: [`OffSessionStripe::confirm_off_session`] instead of the
//! on-session confirm. A decline or a card that needs the bank's approval (Stripe's
//! `authentication_required`) stops the card-fix window that answer opened, so the pick falls back
//! to the normal Confirm step and window (PICK-019) with nothing charged.

use std::future::Future;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter, StripeRefund,
};
use askhava_catalog::api as catalog;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::charge::{IntentStripe, confirm_and_pay};
use crate::models::{BusinessId, ChargeError, ChargeOutcome, PickNow, Press};

/// The off-session confirm a PICK-048 charge needs beyond [`IntentStripe`]: Stripe's confirm with
/// `off_session=true` (the merchant-initiated charge of a card saved with `usage=off_session`).
/// The HTTP body is T132's adapter call; tests use a fake.
pub trait OffSessionStripe: IntentStripe {
    /// Confirms `intent` off-session with the saved `payment_method` under one attempt's key.
    fn confirm_off_session(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send;
}

/// `stripe` with its confirm made off-session, so [`confirm_and_pay`]'s checks and reservation
/// run unchanged for the one PICK-048 charge. Crate-private: no other caller can build it.
struct OffSession<'a, S>(&'a S);

impl<S: OffSessionStripe + Sync> PaymentsAdapter for OffSession<'_, S> {
    fn create_customer(
        &self,
        idempotency: &IdempotencyKey,
        business_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send {
        self.0.create_customer(idempotency, business_id)
    }

    fn create_payment_intent(
        &self,
        idempotency: &IdempotencyKey,
        charge: Charge<'_>,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send {
        self.0.create_payment_intent(idempotency, charge)
    }

    fn fetch_payment_intent(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send {
        self.0.fetch_payment_intent(id)
    }

    fn create_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send {
        self.0.create_refund(idempotency, payment_intent)
    }

    fn send_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<StripeRefund, AdapterError>> + Send {
        self.0.send_refund(idempotency, payment_intent)
    }

    fn read_refund(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<StripeRefund, AdapterError>> + Send {
        self.0.read_refund(id)
    }
}

impl<S: OffSessionStripe + Sync> IntentStripe for OffSession<'_, S> {
    fn create_intent(
        &self,
        idempotency: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send {
        self.0
            .create_intent(idempotency, customer, amount_cents, pick_id)
    }

    fn confirm_intent(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send {
        self.0
            .confirm_off_session(idempotency, intent, payment_method)
    }
}

/// PICK-048, FEE-014, D-401: whether Stripe's written answer on charges without the business
/// present is filed and allows them (`catalog::api::stripe_answer_filed`).
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn stripe_answer_filed(conn: &mut PgConnection) -> Result<bool, ChargeError> {
    catalog::stripe_answer_filed(conn)
        .await
        .map_err(|_| ChargeError::Database)
}

/// FEE-014, FEE-015, PICK-048: the business's saved card, `pm_…`: its latest card not removed;
/// `None` when it has none. Only the ID, never card data.
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn saved_card_of(
    conn: &mut PgConnection,
    business: BusinessId,
) -> Result<Option<String>, ChargeError> {
    sqlx::query_scalar!(
        "SELECT stripe_payment_method_id FROM payments.payment_method
         WHERE business_id = $1 AND removed_at IS NULL
         ORDER BY saved_at DESC, stripe_payment_method_id LIMIT 1",
        business.0
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| ChargeError::Database)
}

/// One PICK-048 charge: the pick, its business and job, the fee fixed at the pick and the saved
/// card the box was ticked with.
#[derive(Debug, Clone, Copy)]
pub struct OffSessionCharge<'a> {
    /// The pick of the ticked quote; also the attempt's one press ID.
    pub pick: Uuid,
    /// The picked business.
    pub business: BusinessId,
    /// The pick's job.
    pub job: Uuid,
    /// The fee fixed at the pick, in cents (FEE-005), equal to the fee shown at the tick.
    pub amount_cents: u32,
    /// The saved card the box was ticked with, `pm_…`.
    pub payment_method: &'a str,
    /// The job was chosen as an emergency (PICK-044).
    pub emergency: bool,
}

/// What a PICK-048 charge came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffSessionOutcome {
    /// Stripe's written answer is not filed, or says no: nothing charged, no Stripe call.
    NotFiled,
    /// The card the box was ticked with is not the saved card now: nothing charged (PICK-056).
    CardChanged,
    /// Stripe says `pi_…` succeeded; the release waits on the verified event (PICK-022).
    Charged(String),
    /// Stripe has not decided or could not be reached; the 30-minute check asks it (FEE-019).
    Undecided,
    /// Over the monthly fee limit: nothing charged and the pick ended, as for a press (FEE-041).
    OverLimit,
    /// Declined, needs the bank's approval, paused or moved on: nothing charged, and the pick
    /// waits for the normal Confirm step and window (PICK-048, PICK-019).
    FellBack,
}

/// PICK-048, PICK-056, FEE-014, FEE-016, FEE-041, D-401, D-1758: the one off-session charge of
/// `charge`'s pick (see the module docs for the order of checks). Never retried here.
///
/// # Errors
/// The errors of [`confirm_and_pay`]; [`ChargeError::Database`].
pub async fn charge_off_session(
    pool: &PgPool,
    stripe: &(impl OffSessionStripe + Sync),
    secret: &SecretString,
    charge: &OffSessionCharge<'_>,
    at: Timestamp,
) -> Result<OffSessionOutcome, ChargeError> {
    let mut conn = pool.acquire().await.map_err(|_| ChargeError::Database)?;
    if !stripe_answer_filed(&mut conn).await? {
        return Ok(OffSessionOutcome::NotFiled);
    }
    let saved = saved_card_of(&mut conn, charge.business).await?;
    if saved.as_deref() != Some(charge.payment_method) {
        return Ok(OffSessionOutcome::CardChanged);
    }
    drop(conn);
    let press = Press {
        pick: charge.pick,
        business: charge.business,
        job: charge.job,
        amount_cents: charge.amount_cents,
        payment_method: charge.payment_method,
        press: charge.pick,
        emergency: charge.emergency,
        now: PickNow::Payable,
    };
    let outcome = confirm_and_pay(pool, &OffSession(stripe), secret, &press, at).await?;
    Ok(match outcome {
        ChargeOutcome::Succeeded(intent) => OffSessionOutcome::Charged(intent),
        ChargeOutcome::Pending | ChargeOutcome::StripeDown => OffSessionOutcome::Undecided,
        ChargeOutcome::OverLimit => OffSessionOutcome::OverLimit,
        ChargeOutcome::Declined(_) | ChargeOutcome::NeedsApproval(_) => {
            stop_card_fix(pool, charge.pick, at).await?;
            OffSessionOutcome::FellBack
        }
        ChargeOutcome::Paused | ChargeOutcome::MovedOn | ChargeOutcome::AlreadyPaid => {
            OffSessionOutcome::FellBack
        }
    })
}

/// PICK-048: ends the card-fix window a declined off-session attempt opened, so its notice and
/// its end never run; the pick keeps the normal Confirm window (PICK-019).
async fn stop_card_fix(pool: &PgPool, pick: Uuid, at: Timestamp) -> Result<(), ChargeError> {
    sqlx::query!(
        "UPDATE payments.pay_timer SET ended_at = $2
         WHERE pick_id = $1 AND kind = 'card-fix' AND ended_at IS NULL",
        pick,
        at.to_sqlx() as _,
    )
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChargeError::Database)
}

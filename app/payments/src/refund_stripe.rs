//! A Stripe refund as Stripe reports it: pending, requires action, succeeded, failed or canceled,
//! never "done" on a 200 (FEE-026, FEE-028, D-1753, D-2066).
//!
//! [`RefundStripe`] is the payments-owned refund call. It is provided for every
//! [`PaymentsAdapter`] over the adapter's `send_refund` and `read_refund`, which `StripePayments`
//! answers from Stripe, so `picks`' callers that hold an `impl PaymentsAdapter` reach the typed
//! refund too (D-2066). Each answer is parsed once here: an unknown status string is
//! [`AdapterError::Decode`], never a success, an ID that is not `re_…` is
//! [`AdapterError::InvalidId`], and a refund of another PaymentIntent than the one asked for is
//! `Decode`.
//!
//! T333 (D-2303): [`Refund::amount_against`] reads a refund's reported amount and currency against
//! the charge as a [`RefundAmount`]; every path that books or announces a refund uses it.

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentsAdapter, StripeRefund};

/// Stripe's status of a refund (FEE-026, D-1753).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundState {
    /// Stripe has not finished the refund yet.
    Pending,
    /// The refund waits for an action (some payment methods); not refunded yet.
    RequiresAction,
    /// The money went back: the only state that is refunded.
    Succeeded,
    /// Stripe could not refund (it may fail after first answering `succeeded`).
    Failed,
    /// The refund was canceled.
    Canceled,
}

impl RefundState {
    /// The state of Stripe's status string; `None` for any other text (never a success).
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "pending" => Some(Self::Pending),
            "requires_action" => Some(Self::RequiresAction),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "canceled" => Some(Self::Canceled),
            _ => None,
        }
    }
}

/// One checked Stripe refund (FEE-026, D-1753).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refund {
    /// `re_…`.
    pub id: String,
    /// Stripe's status.
    pub state: RefundState,
    /// The refunded amount in cents, when the adapter reported it.
    pub amount_cents: Option<i64>,
    /// The currency, when the adapter reported it.
    pub currency: Option<String>,
    /// The refunded PaymentIntent (`pi_…`).
    pub payment_intent: String,
}

impl Refund {
    /// D-1753: parses Stripe's object; `asked` is the PaymentIntent the caller refunded, when it
    /// knows it, and the refund must name it.
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for an ID that is not `re_…`; [`AdapterError::Decode`] for an
    /// unknown status, a missing PaymentIntent or one other than `asked`.
    pub fn checked(raw: StripeRefund, asked: Option<&str>) -> Result<Self, AdapterError> {
        if !raw.id.starts_with("re_") || raw.id.len() <= 3 {
            return Err(AdapterError::InvalidId);
        }
        let state = RefundState::parse(&raw.status).ok_or(AdapterError::Decode)?;
        let payment_intent = raw.payment_intent.ok_or(AdapterError::Decode)?;
        if asked.is_some_and(|a| a != payment_intent) {
            return Err(AdapterError::Decode);
        }
        Ok(Self {
            id: raw.id,
            state,
            amount_cents: raw.amount,
            currency: raw.currency,
            payment_intent,
        })
    }
}

/// The one currency Ask Hava charges and refunds in (D-2303).
pub const REFUND_CURRENCY: &str = "usd";

/// What a refund's reported amount and currency say against the charge it refunds (FEE-026,
/// FEE-028, D-2303). The app only ever asks for full refunds, so anything but `Full` is Casey's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundAmount {
    /// `usd` and exactly the charged amount.
    Full,
    /// `usd` but another amount: the ledger records this one, the money Stripe moved.
    Differs(i64),
    /// Another currency: never booked at the foreign figure.
    Foreign,
    /// No amount (or not above zero), or an amount with no currency: unconfirmed.
    Missing,
}

impl Refund {
    /// FEE-026, FEE-028, D-2303: reads this refund's amount against `charged` cents. A currency
    /// other than `usd` is `Foreign` whatever the amount; a missing currency or amount is `Missing`.
    /// The request path, the webhook, the check, `refund_in_full` and `reconcile` (T334) share it.
    #[must_use]
    pub fn amount_against(&self, charged: i64) -> RefundAmount {
        match (self.currency.as_deref(), self.amount_cents) {
            (Some(c), _) if !c.eq_ignore_ascii_case(REFUND_CURRENCY) => RefundAmount::Foreign,
            (Some(_), Some(a)) if a == charged => RefundAmount::Full,
            (Some(_), Some(a)) if a > 0 => RefundAmount::Differs(a),
            _ => RefundAmount::Missing,
        }
    }
}

/// The typed refund calls `refunds`, `late` and `reconcile` take (FEE-026, FEE-029, D-1753).
pub trait RefundStripe: PaymentsAdapter {
    /// Refunds `payment_intent` in full under `idempotency` and returns Stripe's checked answer.
    fn refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<Refund, AdapterError>> + Send;

    /// Re-reads the refund `id` as Stripe holds it now.
    fn fetch_refund(&self, id: &str) -> impl Future<Output = Result<Refund, AdapterError>> + Send;
}

impl<T: PaymentsAdapter> RefundStripe for T {
    fn refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<Refund, AdapterError>> + Send {
        let sent = self.send_refund(idempotency, payment_intent);
        let asked = payment_intent.to_owned();
        async move { Refund::checked(sent.await?, Some(&asked)) }
    }

    fn fetch_refund(&self, id: &str) -> impl Future<Output = Result<Refund, AdapterError>> + Send {
        let read = self.read_refund(id);
        let asked = id.to_owned();
        async move {
            let refund = Refund::checked(read.await?, None)?;
            if refund.id == asked {
                Ok(refund)
            } else {
                Err(AdapterError::Decode)
            }
        }
    }
}

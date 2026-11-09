//! The payments adapter: Stripe behind one trait (FEE-032, SEC-018, SEC-032).
//!
//! Only `payments` (and `server` wiring it) may hold this adapter (FEE-032,
//! D-595). Every create carries a derived idempotency key with no personal
//! data, and nothing here retries a charge (FEE-016); Stripe's form encoding
//! and `Authorization: Bearer` key, so the stand-in, test mode and live differ
//! only by the base URL and key (D-453).
//!
//! A refund is read as Stripe reports it (D-1753, D-2066): [`PaymentsAdapter::send_refund`] and
//! [`PaymentsAdapter::read_refund`] return the Refund object with its status, which `payments`
//! parses (`payments::api::RefundStripe`); `create_refund` stays until T132 retires it.
//!
//! The Confirm page's two calls (FEE-014, FEE-016, FEE-036, FEE-047, D-1424, D-1754):
//! [`StripePayments::create_unconfirmed_intent`] makes a pick's one PaymentIntent with
//! `confirm=false`, and [`StripePayments::confirm_payment_intent`] confirms it on-session with
//! one attempt's key. Both hold the client secret only as a [`SecretString`], so it never reaches
//! `Debug` or a log line; it goes to the page only for a bank approval (`requires_action`).

use std::future::Future;
use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp, is_vendor_id};

/// The fields of a Stripe PaymentIntent the release check compares after the
/// re-fetch: livemode, customer, amount, currency and pick (PICK-022, SEC-030,
/// SEC-039). Every field but `livemode` is required, so a body missing one
/// fails with [`AdapterError::Decode`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PaymentIntent {
    /// `pi_…`.
    pub id: String,
    /// Stripe's status, `succeeded` once paid.
    pub status: String,
    /// The amount in cents.
    pub amount: i64,
    /// The three-letter currency, `usd`.
    pub currency: String,
    /// The Stripe customer charged (`cus_…`).
    pub customer: String,
    /// The metadata Ask Hava set on the create.
    pub metadata: IntentMetadata,
    /// Stripe's `livemode`; `None` when the body did not say (the stand-in
    /// omits it, D-727). The release check (SEC-030) treats `None` as a
    /// mismatch in every mode, never as test mode.
    pub livemode: Option<bool>,
}

/// The metadata on a PaymentIntent: the pick it pays for (PICK-022).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct IntentMetadata {
    /// The pick's record ID, compared with the pick being released.
    pub pick_id: String,
}

/// A Stripe Refund object as Stripe sent it (FEE-026, D-1753): `payments` checks the status and
/// the PaymentIntent before anything is recorded, so an unknown status is never a success.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct StripeRefund {
    /// `re_…`.
    pub id: String,
    /// Stripe's status: `pending`, `requires_action`, `succeeded`, `failed` or `canceled`.
    pub status: String,
    /// The refunded amount in cents; `None` when the adapter did not report it.
    pub amount: Option<i64>,
    /// The three-letter currency; `None` when the adapter did not report it.
    pub currency: Option<String>,
    /// The refunded PaymentIntent (`pi_…`).
    pub payment_intent: Option<String>,
}

/// A pick's PaymentIntent made unconfirmed (FEE-047, D-1754): nothing is charged yet.
#[derive(Debug, Clone)]
pub struct UnconfirmedIntent {
    /// `pi_…`.
    pub id: String,
    /// Stripe's status, `requires_payment_method` for a new intent.
    pub status: String,
    /// The secret Stripe Elements needs on the page; never logged (`Debug` redacts it).
    pub client_secret: SecretString,
}

/// One confirm's answer (FEE-014, FEE-016): the intent as Stripe left it, and its client secret
/// only while the bank must approve (`requires_action`), for the page's 3-D Secure step.
#[derive(Debug, Clone)]
pub struct ConfirmedIntent {
    /// The intent after the confirm; a decline is a 402 [`AdapterError::Vendor`] instead.
    pub intent: PaymentIntent,
    /// `Some` only when `intent.status` is `requires_action`.
    pub client_secret: Option<SecretString>,
}

/// An unconfirmed create's answer, read once; the secret is wrapped at once.
#[derive(serde::Deserialize)]
struct NewIntent {
    id: String,
    status: String,
    client_secret: String,
}

/// A confirm's answer: the [`PaymentIntent`] and, beside it, its client secret.
#[derive(serde::Deserialize)]
struct Confirmed {
    #[serde(flatten)]
    intent: PaymentIntent,
    client_secret: Option<String>,
}

#[derive(serde::Deserialize)]
struct Created {
    id: String,
}

/// One introduction fee charge: record IDs and the amount only, no card data (FEE-014).
#[derive(Debug, Clone, Copy)]
pub struct Charge<'a> {
    /// The Stripe customer (`cus_…`).
    pub customer: &'a str,
    /// The saved PaymentMethod ID (`pm_…`).
    pub payment_method: &'a str,
    /// The fee in cents, computed on the server (FEE-032).
    pub amount_cents: u32,
    /// The pick's record ID, kept in Stripe metadata.
    pub pick_id: &'a str,
}

/// Moving money through Stripe (FEE-032; SEC-018 on every create).
pub trait PaymentsAdapter {
    /// Creates a customer for the business record `business_id`; returns `cus_…`.
    fn create_customer(
        &self,
        idempotency: &IdempotencyKey,
        business_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// Creates the one PaymentIntent of a pick (FEE-047), confirmed at once.
    fn create_payment_intent(
        &self,
        idempotency: &IdempotencyKey,
        charge: Charge<'_>,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send;

    /// Re-fetches a PaymentIntent before release (PICK-022); `id` must be
    /// `pi_…` or the call fails with [`AdapterError::InvalidId`].
    fn fetch_payment_intent(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send;

    /// Refunds the PaymentIntent `pi_…` in full (FEE-026); returns `re_…`.
    fn create_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// FEE-026, D-1753, D-2066: refunds `pi_…` in full and returns the Refund with Stripe's
    /// status. [`StripePayments`] posts it and reads the answer; the provided body, kept only for
    /// the test fakes written before the typed refund (T132 removes it with `create_refund`),
    /// reports `create_refund`'s `re_…` as `succeeded`.
    fn send_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> impl Future<Output = Result<StripeRefund, AdapterError>> + Send {
        let sent = self.create_refund(idempotency, payment_intent);
        let asked = payment_intent.to_owned();
        async move {
            Ok(StripeRefund {
                id: sent.await?,
                status: "succeeded".to_owned(),
                amount: None,
                currency: None,
                payment_intent: Some(asked),
            })
        }
    }

    /// FEE-026, D-1753: re-reads the Refund `re_…` with Stripe's status now. The provided body
    /// asks nothing and answers [`AdapterError::NotSent`], so a refund it cannot read stays where
    /// it stands; [`StripePayments`] reads it from Stripe.
    fn read_refund(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<StripeRefund, AdapterError>> + Send {
        let _ = id;
        std::future::ready(Err(AdapterError::NotSent))
    }
}

/// The one HTTP Stripe client (D-453).
#[derive(Debug, Clone)]
pub struct StripePayments {
    http: VendorHttp,
}

impl StripePayments {
    /// A client for `base_url` (already checked by `check_base_url`) and `key`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn new(base_url: &str, key: SecretString) -> Result<Self, AdapterError> {
        let http = VendorHttp::new(base_url, Auth::Bearer(Arc::new(key)))?;
        Ok(Self { http })
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        form: &[(&str, &str)],
        key: &IdempotencyKey,
    ) -> Result<T, AdapterError> {
        let request = self.http.request(Method::POST, &["v1", kind], Some(key));
        self.http.json(request.form(form)).await
    }

    /// FEE-047, FEE-036, D-1754: creates the pick's one PaymentIntent for `amount_cents` on
    /// `customer` with `metadata[pick_id]`, `confirm=false`, so nothing is charged until a press
    /// confirms it. The caller derives `idempotency` from the pick (`pick-<id>-intent`).
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for a customer that is not `cus_…` (nothing sent); the HTTP
    /// errors; [`AdapterError::Decode`] for an answer with no `client_secret`.
    pub async fn create_unconfirmed_intent(
        &self,
        idempotency: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> Result<UnconfirmedIntent, AdapterError> {
        if !is_vendor_id(customer, "cus_") {
            return Err(AdapterError::InvalidId);
        }
        let amount = amount_cents.to_string();
        let form = [
            ("amount", amount.as_str()),
            ("currency", "usd"),
            ("customer", customer),
            ("confirm", "false"),
            ("metadata[pick_id]", pick_id),
        ];
        let made: NewIntent = self.post("payment_intents", &form, idempotency).await?;
        Ok(UnconfirmedIntent {
            id: made.id,
            status: made.status,
            client_secret: SecretString::from(made.client_secret),
        })
    }

    /// FEE-014, FEE-016, D-1754: confirms `intent` on-session with `payment_method` under one
    /// attempt's key (`pick-<id>-try<n>`); a repeat of the key gets Stripe's first answer, and
    /// nothing here retries. A decline is Stripe's 402, returned as [`AdapterError::Vendor`].
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for an intent not `pi_…` or a card not `pm_…` (nothing sent);
    /// the HTTP errors; [`AdapterError::Decode`] for an answer missing a [`PaymentIntent`] field.
    pub async fn confirm_payment_intent(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> Result<ConfirmedIntent, AdapterError> {
        if !is_vendor_id(intent, "pi_") || !is_vendor_id(payment_method, "pm_") {
            return Err(AdapterError::InvalidId);
        }
        let path = ["v1", "payment_intents", intent, "confirm"];
        let request = self.http.request(Method::POST, &path, Some(idempotency));
        let form = [("payment_method", payment_method)];
        let answer: Confirmed = self.http.json(request.form(&form)).await?;
        let approval = answer.intent.status == "requires_action";
        Ok(ConfirmedIntent {
            client_secret: answer
                .client_secret
                .filter(|_| approval)
                .map(SecretString::from),
            intent: answer.intent,
        })
    }
}

impl PaymentsAdapter for StripePayments {
    async fn create_customer(
        &self,
        idempotency: &IdempotencyKey,
        business_id: &str,
    ) -> Result<String, AdapterError> {
        let form = [("metadata[business_id]", business_id)];
        let created: Created = self.post("customers", &form, idempotency).await?;
        Ok(created.id)
    }

    async fn create_payment_intent(
        &self,
        idempotency: &IdempotencyKey,
        charge: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        if !is_vendor_id(charge.customer, "cus_") || !is_vendor_id(charge.payment_method, "pm_") {
            return Err(AdapterError::InvalidId);
        }
        let amount = charge.amount_cents.to_string();
        let form = [
            ("amount", amount.as_str()),
            ("currency", "usd"),
            ("customer", charge.customer),
            ("payment_method", charge.payment_method),
            ("confirm", "true"),
            ("metadata[pick_id]", charge.pick_id),
        ];
        self.post("payment_intents", &form, idempotency).await
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        if !is_vendor_id(id, "pi_") {
            return Err(AdapterError::InvalidId);
        }
        let request = self
            .http
            .request(Method::GET, &["v1", "payment_intents", id], None);
        self.http.json(request).await
    }

    async fn create_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> Result<String, AdapterError> {
        if !is_vendor_id(payment_intent, "pi_") {
            return Err(AdapterError::InvalidId);
        }
        let form = [("payment_intent", payment_intent)];
        let created: Created = self.post("refunds", &form, idempotency).await?;
        Ok(created.id)
    }

    async fn send_refund(
        &self,
        idempotency: &IdempotencyKey,
        payment_intent: &str,
    ) -> Result<StripeRefund, AdapterError> {
        if !is_vendor_id(payment_intent, "pi_") {
            return Err(AdapterError::InvalidId);
        }
        let form = [("payment_intent", payment_intent)];
        self.post("refunds", &form, idempotency).await
    }

    async fn read_refund(&self, id: &str) -> Result<StripeRefund, AdapterError> {
        if !is_vendor_id(id, "re_") {
            return Err(AdapterError::InvalidId);
        }
        let request = self.http.request(Method::GET, &["v1", "refunds", id], None);
        self.http.json(request).await
    }
}

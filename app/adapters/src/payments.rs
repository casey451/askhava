//! The payments adapter: Stripe behind one trait (FEE-032, SEC-018, SEC-032).
//!
//! Only `payments` (and `server` wiring it) may hold this adapter (FEE-032,
//! D-595). Every create carries a derived idempotency key with no personal
//! data, and nothing here retries a charge (FEE-016); Stripe's form encoding
//! and `Authorization: Bearer` key, so the stand-in, test mode and live differ
//! only by the base URL and key (D-453).

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
}

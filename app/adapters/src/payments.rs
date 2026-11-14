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
//!
//! T405 (FEE-050, D-3118): [`StripePayments::fetch_three_ds`] reads whether a payment was 3-D
//! Secure authenticated as a separate [`ThreeDs`] value, so neither [`PaymentIntent`] nor
//! [`PaymentsAdapter`] changes.
//!
//! T335 (FEE-029, FEE-051, FEE-027, FEE-050, D-2305): [`StripePayments::list_charge_page`] reads
//! one page of succeeded charges with their processing fee, refunded amount and dispute for the
//! nightly reconcile, and [`StripePayments::submit_dispute_evidence`] submits a dispute's
//! evidence; `payments` implements its `ReconcileStripe` and `DisputeStripe` over them.

use std::future::Future;
use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp, is_vendor_id};

/// The fields of a Stripe PaymentIntent the release check compares after the
/// re-fetch: livemode, customer, amount, currency and pick (PICK-022, SEC-030,
/// SEC-039). Every field but `livemode` and `latest_charge` is required, so a body missing one
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
    /// Stripe's `latest_charge`, unexpanded: the `ch_…` ID of the intent's most recent charge,
    /// for the charge ledger entry and evidence record (FEE-042, SEC-030, D-2623). It is no
    /// proof of payment: after a decline real Stripe names the failed charge here while the
    /// status is `requires_payment_method`, so only `status == "succeeded"` decides payment.
    /// `None` when Stripe sent `null` (no charge attempted yet) or a body lacks the field;
    /// requiring it on a succeeded intent is the release check's job (T359), not the decode's.
    #[serde(default)]
    pub latest_charge: Option<String>,
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

/// Whether a payment was 3-D Secure authenticated (FEE-050, D-3118), read apart from
/// [`PaymentIntent`] by [`StripePayments::fetch_three_ds`]; anything Stripe did not report as
/// `authenticated` is [`ThreeDs::NotAuthenticated`], never unknown-as-yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeDs {
    /// The card's bank authenticated the payment (`three_d_secure.result` `authenticated`).
    Authenticated,
    /// Any other result, no 3-D Secure, or nothing readable.
    NotAuthenticated,
}

impl ThreeDs {
    /// FEE-050, D-3118: Stripe's `three_d_secure.result`, `None` when the charge had none; only
    /// `authenticated` is [`ThreeDs::Authenticated`].
    #[must_use]
    pub fn from_result(result: Option<&str>) -> Self {
        if result == Some("authenticated") {
            Self::Authenticated
        } else {
            Self::NotAuthenticated
        }
    }

    /// FEE-050, D-3118: reads an intent fetched with `latest_charge` expanded.
    fn of_intent(intent: &serde_json::Value) -> Self {
        Self::from_result(
            intent
                .pointer("/latest_charge/payment_method_details/card/three_d_secure/result")
                .and_then(serde_json::Value::as_str),
        )
    }
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

/// Stripe's largest list page (`limit` 1 to 100, D-2305).
pub const CHARGE_PAGE_MAX: usize = 100;

/// One succeeded charge as Stripe lists it: IDs and amounts only (FEE-029, FEE-051, D-2305).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedCharge {
    /// `ch_…`.
    pub id: String,
    /// Its PaymentIntent, `pi_…`, if any.
    pub payment_intent: Option<String>,
    /// Its `metadata[pick_id]`, if any (copied from the PaymentIntent).
    pub pick_id: Option<String>,
    /// The gross amount in cents.
    pub amount: i64,
    /// What Stripe has refunded of it so far, in cents.
    pub amount_refunded: i64,
    /// Its balance transaction, `txn_…`, and that transaction's processing fee in cents.
    pub balance_transaction: Option<(String, i64)>,
    /// Its dispute, `dp_…`, and the disputed amount in cents.
    pub dispute: Option<(String, i64)>,
}

/// A charge ID as Stripe lists it: `ch_…`, or `py_…` for a non-card payment (D-3538).
fn is_charge_id(id: &str) -> bool {
    is_vendor_id(id, "ch_") || is_vendor_id(id, "py_")
}

/// One page of [`ListedCharge`]s and the cursor of the next, `None` after the last (D-2305).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeList {
    /// The page's succeeded charges.
    pub charges: Vec<ListedCharge>,
    /// The `starting_after` of the next page: the last charge Stripe listed, of any status.
    pub next: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawList<T> {
    data: Vec<T>,
    has_more: bool,
}

#[derive(serde::Deserialize)]
struct RawCharge {
    id: String,
    status: String,
    payment_intent: Option<String>,
    amount: i64,
    amount_refunded: i64,
    #[serde(default)]
    disputed: bool,
    #[serde(default)]
    metadata: RawMetadata,
    balance_transaction: Option<RawTransaction>,
}

#[derive(Default, serde::Deserialize)]
struct RawMetadata {
    pick_id: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawTransaction {
    id: String,
    fee: i64,
}

#[derive(serde::Deserialize)]
struct RawDispute {
    id: String,
    amount: i64,
}

impl RawCharge {
    fn listed(self, dispute: Option<(String, i64)>) -> ListedCharge {
        ListedCharge {
            id: self.id,
            payment_intent: self.payment_intent,
            pick_id: self.metadata.pick_id,
            amount: self.amount,
            amount_refunded: self.amount_refunded,
            balance_transaction: self.balance_transaction.map(|t| (t.id, t.fee)),
            dispute,
        }
    }
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
    /// confirms it, and `card` as the only method type (FEE-014, D-073). The caller derives `idempotency` from the pick (`pick-<id>-intent`).
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
            ("payment_method_types[]", "card"),
        ];
        let made: NewIntent = self.post("payment_intents", &form, idempotency).await?;
        Ok(UnconfirmedIntent {
            id: made.id,
            status: made.status,
            client_secret: SecretString::from(made.client_secret),
        })
    }

    /// FEE-050, FEE-027, D-3118: whether the PaymentIntent `pi_…` was paid with 3-D Secure, read
    /// from `GET /v1/payment_intents/{id}?expand[]=latest_charge`. Only a
    /// `latest_charge.payment_method_details.card.three_d_secure.result` of `authenticated` is
    /// [`ThreeDs::Authenticated`]; every other result, no `three_d_secure`, a missing or
    /// unexpanded `latest_charge` and a card-less method are [`ThreeDs::NotAuthenticated`].
    /// Nothing else of the answer (card data included) is kept.
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for an ID not `pi_…` (nothing sent); the HTTP errors.
    pub async fn fetch_three_ds(&self, id: &str) -> Result<ThreeDs, AdapterError> {
        if !is_vendor_id(id, "pi_") {
            return Err(AdapterError::InvalidId);
        }
        let path = ["v1", "payment_intents", id];
        let answer: serde_json::Value = self
            .get_query(&path, &[("expand[]", "latest_charge")])
            .await?;
        Ok(ThreeDs::of_intent(&answer))
    }

    /// A GET of `path` with `query` appended (the client has no query feature).
    async fn get_query<T: serde::de::DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T, AdapterError> {
        let (client, request) = self.http.request(Method::GET, path, None).build_split();
        let mut request = request.map_err(|_| AdapterError::NotSent)?;
        request.url_mut().query_pairs_mut().extend_pairs(query);
        self.http
            .json(reqwest::RequestBuilder::from_parts(client, request))
            .await
    }

    /// FEE-029, FEE-051, D-2305: one page of the charges created at or after `created_gte`
    /// (Unix seconds; an earlier time reads from 0), newest first as Stripe lists them, at most
    /// `limit` (1 to [`CHARGE_PAGE_MAX`]) after the cursor `starting_after` (`ch_…` or `py_…`), from
    /// `GET /v1/charges` with `expand[]=data.balance_transaction`. Only `succeeded` charges are
    /// kept; each `disputed` one has its dispute read from `GET /v1/disputes?charge=`, because a
    /// Charge names no dispute. Nothing but IDs and amounts is kept (no card data, FEE-014).
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for a cursor not `ch_…` or `py_…` (nothing sent); the HTTP errors;
    /// [`AdapterError::Decode`] for an answer that is not a charge list.
    pub async fn list_charge_page(
        &self,
        created_gte: i64,
        starting_after: Option<&str>,
        limit: usize,
    ) -> Result<ChargeList, AdapterError> {
        if starting_after.is_some_and(|c| !is_charge_id(c)) {
            return Err(AdapterError::InvalidId);
        }
        let (since, limit) = (
            created_gte.max(0).to_string(),
            limit.clamp(1, CHARGE_PAGE_MAX).to_string(),
        );
        let mut query = vec![
            ("created[gte]", since.as_str()),
            ("limit", limit.as_str()),
            ("expand[]", "data.balance_transaction"),
        ];
        query.extend(starting_after.map(|c| ("starting_after", c)));
        let page: RawList<RawCharge> = self.get_query(&["v1", "charges"], &query).await?;
        let next = page
            .has_more
            .then(|| page.data.last().map(|c| c.id.clone()))
            .flatten();
        let mut charges = Vec::with_capacity(page.data.len());
        for raw in page.data.into_iter().filter(|c| c.status == "succeeded") {
            charges.push(self.listed(raw).await?);
        }
        Ok(ChargeList { charges, next })
    }

    /// FEE-029, FEE-051, D-1872, D-3538: one charge as Stripe holds it now, by
    /// `GET /v1/charges/{id}` with `expand[]=balance_transaction`, read as
    /// [`list_charge_page`](Self::list_charge_page) reads a listed one; `None` when Stripe
    /// answers 404 or the charge has not succeeded (the list keeps only succeeded charges).
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for an ID not `ch_…` or `py_…` (nothing sent); the other
    /// HTTP errors; [`AdapterError::Decode`] for an answer that is not a charge.
    pub async fn fetch_charge(&self, id: &str) -> Result<Option<ListedCharge>, AdapterError> {
        if !is_charge_id(id) {
            return Err(AdapterError::InvalidId);
        }
        let query = [("expand[]", "balance_transaction")];
        let raw: RawCharge = match self.get_query(&["v1", "charges", id], &query).await {
            Err(AdapterError::Vendor { status: 404, .. }) => return Ok(None),
            other => other?,
        };
        if raw.status != "succeeded" {
            return Ok(None);
        }
        self.listed(raw).await.map(Some)
    }

    /// A succeeded raw charge as a [`ListedCharge`], its dispute read when it is `disputed`.
    async fn listed(&self, raw: RawCharge) -> Result<ListedCharge, AdapterError> {
        let dispute = if raw.disputed {
            self.dispute_of(&raw.id).await?
        } else {
            None
        };
        Ok(raw.listed(dispute))
    }

    /// The first dispute Stripe lists for charge `id`, as its ID and amount (FEE-027).
    async fn dispute_of(&self, id: &str) -> Result<Option<(String, i64)>, AdapterError> {
        let query = [("charge", id), ("limit", "1")];
        let list: RawList<RawDispute> = self.get_query(&["v1", "disputes"], &query).await?;
        Ok(list.data.into_iter().next().map(|d| (d.id, d.amount)))
    }

    /// FEE-027, FEE-050, D-2305: sends `evidence` (its `evidence[…]` fields; any other field is
    /// dropped) as the evidence of `dispute` (`dp_…`) with `submit=true` by
    /// `POST /v1/disputes/{id}` under `idempotency`, so a repeat of the key submits once.
    ///
    /// # Errors
    /// [`AdapterError::InvalidId`] for a dispute not `dp_…` (nothing sent); the HTTP errors.
    pub async fn submit_dispute_evidence(
        &self,
        idempotency: &IdempotencyKey,
        dispute: &str,
        evidence: &[(&str, String)],
    ) -> Result<(), AdapterError> {
        if !is_vendor_id(dispute, "dp_") {
            return Err(AdapterError::InvalidId);
        }
        let mut form: Vec<(&str, &str)> = evidence
            .iter()
            .filter(|(k, _)| k.starts_with("evidence["))
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        form.push(("submit", "true"));
        let path = ["v1", "disputes", dispute];
        let request = self.http.request(Method::POST, &path, Some(idempotency));
        let _: Created = self.http.json(request.form(&form)).await?;
        Ok(())
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

#[cfg(test)]
mod tests {
    //! The two Confirm-page calls against a one-request scripted vendor that records the
    //! request (FEE-014, D-073, D-2076); fake data only.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::Receiver;

    use secrecy::{ExposeSecret, SecretString};

    use super::{ChargeList, PaymentIntent, StripePayments, ThreeDs};
    use crate::base::{AdapterError, IdempotencyKey};

    /// A client for a vendor that answers one request with `answer` and hands back the request
    /// line, headers and body it received.
    fn scripted(answer: &'static str) -> (StripePayments, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        let (send, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream);
            let request = read_request(&mut reader);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
            let _ = reader.into_inner().write_all(reply.as_bytes());
            let _ = send.send(request);
        });
        let base = format!("http://127.0.0.1:{port}/stripe");
        let stripe =
            StripePayments::new(&base, SecretString::from("sk_test_fake")).expect("client");
        (stripe, got)
    }

    /// One request's line, headers and body (read up to its `Content-Length`).
    fn read_request(reader: &mut BufReader<TcpStream>) -> String {
        let (mut head, mut line) = (String::new(), String::new());
        while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
            head.push_str(&line);
            line.clear();
        }
        let length = head
            .lines()
            .find_map(|h| {
                h.to_ascii_lowercase()
                    .strip_prefix("content-length:")?
                    .trim()
                    .parse()
                    .ok()
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).expect("body");
        head + &String::from_utf8_lossy(&body)
    }

    fn key(label: &str) -> IdempotencyKey {
        IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "t260", &[label])
    }

    const MADE: &str =
        r#"{"id":"pi_fake1","status":"requires_payment_method","client_secret":"fake-cs"}"#;

    /// FEE-014, D-073: the unconfirmed create posts `confirm=false` and `card` as the only
    /// method type, and wraps the secret.
    #[tokio::test]
    async fn the_unconfirmed_intent_asks_for_cards_only() {
        let (stripe, got) = scripted(MADE);
        let made = stripe
            .create_unconfirmed_intent(&key("pick-fake-1-intent"), "cus_fake1", 7500, "pick-fake-1")
            .await
            .expect("made");
        assert_eq!(made.client_secret.expose_secret(), "fake-cs");
        let request = got.recv().expect("request");
        assert!(
            request.starts_with("POST /stripe/v1/payment_intents "),
            "{request}"
        );
        assert!(request.contains("confirm=false"), "{request}");
        assert!(
            request.contains("payment_method_types%5B%5D=card"),
            "{request}"
        );
    }

    /// An unconfirmed create's answer without a `client_secret` is [`AdapterError::Decode`].
    #[tokio::test]
    async fn an_intent_without_a_client_secret_is_a_decode_error() {
        let (stripe, _got) = scripted(r#"{"id":"pi_fake1","status":"requires_payment_method"}"#);
        let made = stripe
            .create_unconfirmed_intent(&key("pick-fake-1-intent"), "cus_fake1", 7500, "pick-fake-1")
            .await;
        assert!(matches!(made, Err(AdapterError::Decode)));
    }

    /// SEC-032: a card that is not `pm_…` is refused before any request.
    #[tokio::test]
    async fn a_card_not_pm_is_refused_unsent() {
        let stripe = StripePayments::new(
            "http://127.0.0.1:9/stripe",
            SecretString::from("sk_test_fake"),
        )
        .expect("client");
        for card in ["", "pm_", "card_fake1", "pm_a.b"] {
            let got = stripe
                .confirm_payment_intent(&key("pick-fake-1-try1"), "pi_fake1", card)
                .await;
            assert!(matches!(got, Err(AdapterError::InvalidId)), "{card:?}");
        }
    }

    /// FEE-014, D-2076: the confirm hands back the secret only for `requires_action`.
    #[tokio::test]
    async fn the_secret_is_dropped_unless_the_bank_must_approve() {
        let answer = r#"{"id":"pi_fake1","status":"succeeded","amount":7500,"currency":"usd","customer":"cus_fake1","metadata":{"pick_id":"pick-fake-1"},"client_secret":"fake-cs"}"#;
        let (stripe, got) = scripted(answer);
        let paid = stripe
            .confirm_payment_intent(&key("pick-fake-1-try1"), "pi_fake1", "pm_card_visa")
            .await
            .expect("paid");
        assert_eq!(paid.intent.status, "succeeded");
        assert!(paid.client_secret.is_none());
        let request = got.recv().expect("request");
        assert!(request.starts_with("POST /stripe/v1/payment_intents/pi_fake1/confirm "));
        assert!(request.contains("payment_method=pm_card_visa"), "{request}");
    }

    /// FEE-050, D-3118: only `authenticated` is authenticated; every other result, no 3-D Secure,
    /// a missing or unexpanded `latest_charge` and a card-less method are not.
    #[test]
    fn only_an_authenticated_result_reads_as_authenticated() {
        let charge = |card: &str| {
            format!(r#"{{"latest_charge":{{"payment_method_details":{{"card":{card}}}}}}}"#)
        };
        let result = |r: &str| charge(&format!(r#"{{"three_d_secure":{{"result":"{r}"}}}}"#));
        assert_eq!(read(&result("authenticated")), ThreeDs::Authenticated);
        let others = [
            "attempt_acknowledged",
            "exempted",
            "failed",
            "not_supported",
            "processing_error",
        ];
        let mut not = others.map(result).to_vec();
        not.extend([
            charge(r#"{"three_d_secure":null}"#),
            charge("{}"),
            r#"{"latest_charge":"ch_fake1"}"#.to_owned(),
            r#"{"latest_charge":null}"#.to_owned(),
            "{}".to_owned(),
            r#"{"latest_charge":{"payment_method_details":{"type":"us_bank_account"}}}"#.to_owned(),
        ]);
        for body in not {
            assert_eq!(read(&body), ThreeDs::NotAuthenticated, "{body}");
        }
    }

    fn read(body: &str) -> ThreeDs {
        ThreeDs::of_intent(&serde_json::from_str(body).expect("json"))
    }

    /// FEE-050, D-3118: the read asks `expand[]=latest_charge` on a GET with no key, and refuses
    /// an ID not `pi_…` unsent.
    #[tokio::test]
    async fn the_three_ds_read_expands_the_latest_charge() {
        let answer = r#"{"id":"pi_fake1","latest_charge":{"payment_method_details":{"card":{"three_d_secure":{"result":"authenticated"}}}}}"#;
        let (stripe, got) = scripted(answer);
        let read = stripe.fetch_three_ds("pi_fake1").await.expect("read");
        assert_eq!(read, ThreeDs::Authenticated);
        let request = got.recv().expect("request");
        assert!(
            request
                .starts_with("GET /stripe/v1/payment_intents/pi_fake1?expand%5B%5D=latest_charge "),
            "{request}"
        );
        assert!(!request.to_ascii_lowercase().contains("idempotency-key"));
        let refused = stripe.fetch_three_ds("ch_fake1").await;
        assert!(matches!(refused, Err(AdapterError::InvalidId)));
    }

    /// TD-FEE-61 (FEE-029, FEE-051): the page read sends the filter, limit, cursor and expand,
    /// keeps only succeeded charges with their fee and refund, and takes the next cursor from
    /// the last charge listed.
    #[tokio::test]
    async fn td_fee_61_the_charge_page_keeps_succeeded_charges_and_the_cursor() {
        let answer = r#"{"object":"list","has_more":true,"data":[
            {"id":"ch_fake1","status":"succeeded","payment_intent":"pi_fake1","amount":7500,
             "amount_refunded":0,"disputed":false,"metadata":{"pick_id":"pick-fake-1"},
             "balance_transaction":{"id":"txn_fake1","fee":248}},
            {"id":"ch_fake2","status":"failed","payment_intent":"pi_fake2","amount":2500,
             "amount_refunded":0,"balance_transaction":null}]}"#;
        let (stripe, got) = scripted(answer);
        let page = stripe
            .list_charge_page(-5, Some("ch_fake0"), 500)
            .await
            .expect("page");
        let request = got.recv().expect("request");
        assert!(
            request.starts_with(
                "GET /stripe/v1/charges?created%5Bgte%5D=0&limit=100\
                 &expand%5B%5D=data.balance_transaction&starting_after=ch_fake0 "
            ),
            "{request}"
        );
        assert_eq!(page.next.as_deref(), Some("ch_fake2"));
        assert_eq!(page.charges.len(), 1);
        let charge = &page.charges[0];
        assert_eq!(
            (
                charge.pick_id.as_deref(),
                charge.amount,
                charge.amount_refunded
            ),
            (Some("pick-fake-1"), 7500, 0)
        );
        assert_eq!(charge.balance_transaction, Some(("txn_fake1".into(), 248)));
        assert_eq!(charge.dispute, None);
        let refused = stripe.list_charge_page(0, Some("pi_fake1"), 10).await;
        assert!(matches!(refused, Err(AdapterError::InvalidId)));
    }

    /// D-3538: a `py_…` last item is a cursor like a `ch_…` one, so the next page is read.
    #[tokio::test]
    async fn a_py_cursor_is_sent_as_the_next_page() {
        let (stripe, got) = scripted(r#"{"object":"list","has_more":false,"data":[]}"#);
        let page = stripe
            .list_charge_page(0, Some("py_fake1"), 10)
            .await
            .expect("page");
        let request = got.recv().expect("request");
        assert!(request.contains("&starting_after=py_fake1 "), "{request}");
        assert_eq!(
            page,
            ChargeList {
                charges: vec![],
                next: None
            }
        );
    }

    /// FEE-029, D-3538: the one-charge read retrieves by ID with the transaction expanded;
    /// a charge that has not succeeded is none, and a non-charge ID is refused unsent.
    #[tokio::test]
    async fn the_charge_read_expands_the_transaction_and_keeps_only_succeeded() {
        let (stripe, got) = scripted(
            r#"{"id":"ch_fake3","status":"failed","payment_intent":"pi_fake3","amount":2500,
                "amount_refunded":0,"balance_transaction":null}"#,
        );
        assert_eq!(stripe.fetch_charge("ch_fake3").await.expect("read"), None);
        let request = got.recv().expect("request");
        assert!(
            request
                .starts_with("GET /stripe/v1/charges/ch_fake3?expand%5B%5D=balance_transaction "),
            "{request}"
        );
        let refused = stripe.fetch_charge("pi_fake3").await;
        assert!(matches!(refused, Err(AdapterError::InvalidId)));
    }

    /// TD-FEE-61 (FEE-027): the evidence submit posts only the evidence fields and
    /// `submit=true` under the key.
    #[tokio::test]
    async fn td_fee_61_the_evidence_submit_posts_submit_true_with_the_key() {
        let (stripe, got) = scripted(r#"{"id":"dp_fake1","status":"under_review"}"#);
        let form = [
            ("evidence[uncategorized_text]", "fake record".to_owned()),
            ("submit", "false".to_owned()),
            ("metadata[x]", "dropped".to_owned()),
        ];
        stripe
            .submit_dispute_evidence(&key("dp-fake1"), "dp_fake1", &form)
            .await
            .expect("submitted");
        let request = got.recv().expect("request");
        assert!(
            request.starts_with("POST /stripe/v1/disputes/dp_fake1 "),
            "{request}"
        );
        assert!(request.to_ascii_lowercase().contains("idempotency-key:"));
        assert!(
            request.ends_with("evidence%5Buncategorized_text%5D=fake+record&submit=true"),
            "{request}"
        );
        let refused = stripe
            .submit_dispute_evidence(&key("x"), "ch_fake1", &form)
            .await;
        assert!(matches!(refused, Err(AdapterError::InvalidId)));
    }

    /// A stand-in shaped PaymentIntent body, `latest_charge` given as raw JSON or left out.
    fn intent_body(status: &str, latest_charge: Option<&str>) -> String {
        let charge = latest_charge.map_or(String::new(), |c| format!(r#","latest_charge":{c}"#));
        format!(
            r#"{{"id":"pi_fake66","object":"payment_intent","status":"{status}","amount":7500,"currency":"usd","customer":"cus_fake66","metadata":{{"pick_id":"pick-fake-66"}}{charge}}}"#
        )
    }

    /// TD-FEE-66 (FEE-042, SEC-030, D-2623): a succeeded intent decodes with its `ch_` charge
    /// ID; Stripe's `null` on an unpaid intent and a body without the field decode to `None`.
    #[test]
    fn td_fee_66_the_intent_decodes_its_latest_charge() {
        let decode = |body: String| serde_json::from_str::<PaymentIntent>(&body).expect("decodes");
        let paid = decode(intent_body("succeeded", Some(r#""ch_fake66""#)));
        assert_eq!(paid.latest_charge.as_deref(), Some("ch_fake66"));
        let unpaid = decode(intent_body("requires_payment_method", Some("null")));
        assert_eq!(unpaid.latest_charge, None);
        let without = decode(intent_body("requires_payment_method", None));
        assert_eq!(without.latest_charge, None);
        assert_eq!(without.id, "pi_fake66");
    }
}

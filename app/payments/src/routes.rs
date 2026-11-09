//! The Stripe webhook route: the only way a paid pick is released, and the one door through which
//! Stripe's refund, dispute and early-fraud-warning events reach `payments` (PICK-022, SEC-016,
//! SEC-017, SEC-039, SEC-049, FEE-026, FEE-027, FEE-052, TD-FEE-25, TD-FEE-43, TD-SEC-31; T224,
//! D-1479; T259, D-1754, D-2071).
//!
//! `POST` [`STRIPE_WEBHOOK_PATH`] takes a Stripe event in this order and nothing else sees it
//! first: the source address, read as SEC-040 takes it (`ClientIpPolicy::client_ip` over the
//! socket peer), must be on the committed copy of Stripe's webhook list in sandbox and live, or
//! the stand-in's host in stand-in mode (`check_webhook_source`, SEC-049); then the
//! `Stripe-Signature` is checked on the raw body with the 5-minute tolerance (`verify_webhook`,
//! SEC-016, SEC-039). Either refusal is a 403 with one log line (`refuse`).
//!
//! A checked event is parsed once into a [`Dispatch`]; an event of the other mode or another
//! account, with a malformed `evt_…` ID or of a type nothing handles is acknowledged and changes
//! nothing. An event ID already in `payments.webhook_event` answers 200 and changes nothing
//! (SEC-017). Then: `payment_intent.succeeded` goes to T189's [`paid_release`] (livemode,
//! account, the pick's own PaymentIntent, a re-fetch from Stripe) and only a [`Releasable`]
//! reaches the [`ReleaseSink`], whose holder mints the `ReleaseProof` and releases once;
//! `refund.updated` and `charge.refund.updated` go by the refund's status, and `refund.failed`
//! always, to T258's `refund_succeeded`, `refund_failed` or `refund_canceled` (a `succeeded`
//! status is the one Stripe can still leave, P13, so it is first re-read through the adapter and
//! the state Stripe reports now is applied); `charge.dispute.created` goes to `record_dispute`;
//! `charge.dispute.closed` writes one audit row and one High alert per dispute, no money moves
//! (D-1545); `radar.early_fraud_warning.created` enqueues one [`FRAUD_WARNING_KIND`] task for the
//! charge's pick, which `picks` (T281) runs through `picks::api::fraud_warning` (D-1845). The two
//! last write the event row in the transaction of their effect; the release and the refund and
//! dispute handlers commit their own transaction (each is idempotent by its own row or key) and
//! the event row is written right after, so a crash between the two is a resend they absorb
//! (D-2071). A database error, or a Stripe that could not be asked, answers 500 so Stripe sends
//! the event again; an event for an unknown object answers 200 and writes nothing.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Delivery, PaymentsAdapter, VerifiedWebhook, WebhookVendor,
    check_webhook_source, refuse, verify_webhook,
};
use askhava_core::api::{ClientIpPolicy, Clock, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit_once, raise_alert};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::disputes::record_dispute;
use crate::models::{DisputeOpened, MoneyEventError};
use crate::refund_stripe::{RefundState, RefundStripe};
use crate::refunds::{refund_canceled, refund_failed, refund_succeeded};
use crate::release_proof::{
    Releasable, ReleaseError, SUCCEEDED_EVENT, StripeAccount, VerifiedStripeEvent, paid_release,
};

/// The Stripe webhook's path (SEC-039).
pub const STRIPE_WEBHOOK_PATH: &str = "/webhooks/stripe";

/// The `picks` task an early fraud warning enqueues, payload `{"pick": …}`, one per pick (FEE-052,
/// D-1845); `picks` (T281) registers its handler.
pub const FRAUD_WARNING_KIND: &str = "picks.fraud_warning";

/// The audit action and High alert kind of a closed dispute (FEE-027, D-1545).
pub const DISPUTE_CLOSED_ACTION: &str = "payments.dispute_closed";

const DISPUTE_SUBJECT: &str = "payments.dispute";

/// Where a cleared pick goes: the holder of `server`'s release minter, which mints the proof for
/// the pick's homeowner contact and releases once however often it is called (PICK-022).
pub trait ReleaseSink: Send + Sync + 'static {
    /// Mints and releases for `cleared`.
    ///
    /// # Errors
    /// A [`ReleaseError`] the route answers 500 to, so Stripe sends the event again.
    fn release(&self, cleared: Releasable)
    -> impl Future<Output = Result<(), ReleaseError>> + Send;
}

/// What the route needs: the pool, the Stripe client used for the re-fetch, the webhook signing
/// secret, the configured [`StripeAccount`], the adapter mode and stand-in host for the source
/// rule, the SEC-040 client-address policy, the clock and the release sink (SEC-039, SEC-049).
pub struct StripeWebhook<S, R> {
    /// The app's pool.
    pub pool: PgPool,
    /// The Stripe client the success and a `succeeded` refund are re-fetched with (SEC-049).
    pub stripe: S,
    /// The endpoint's `whsec_…` signing secret.
    pub secret: SecretString,
    /// This process's Stripe mode and account (SEC-036, SEC-039).
    pub account: StripeAccount,
    /// Stand-in, sandbox or live: which source rule applies (SEC-049).
    pub mode: AdapterMode,
    /// The stand-in's host, the only accepted source in stand-in mode.
    pub stand_in_host: IpAddr,
    /// How the client address is read (SEC-040).
    pub client_ip: ClientIpPolicy,
    /// The clock the signature tolerance is measured on.
    pub clock: Arc<dyn Clock>,
    /// Where a cleared pick is minted and released.
    pub sink: R,
}

/// The webhook router; `server` merges it and serves with connect info, so the socket peer is
/// known (SEC-040). No cross-site refusal is layered over it: Stripe sends no `Origin`.
pub fn stripe_webhook_routes<S, R>(hooks: Arc<StripeWebhook<S, R>>) -> Router
where
    S: PaymentsAdapter + Send + Sync + 'static,
    R: ReleaseSink,
{
    Router::new()
        .route(STRIPE_WEBHOOK_PATH, post(receive::<S, R>))
        .with_state(hooks)
}

/// One delivery: source, signature, then the event once by its ID (see the module docs).
async fn receive<S, R>(
    State(h): State<Arc<StripeWebhook<S, R>>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode
where
    S: PaymentsAdapter + Send + Sync + 'static,
    R: ReleaseSink,
{
    let client = h.client_ip.client_ip(&headers, peer.ip());
    if let Err(error) = check_webhook_source(h.mode, client, h.stand_in_host) {
        return rejected(error);
    }
    let pairs: Vec<(&str, &str)> = headers
        .iter()
        .filter_map(|(name, value)| value.to_str().ok().map(|v| (name.as_str(), v)))
        .collect();
    let delivery = Delivery {
        url: STRIPE_WEBHOOK_PATH,
        body: &body,
        headers: &pairs,
    };
    let now = h.clock.now();
    let hook = match verify_webhook(WebhookVendor::Stripe, &h.secret, delivery, now.as_second()) {
        Ok(hook) => hook,
        Err(error) => return rejected(error),
    };
    let Some(event) = checked(&h.account, hook.body()) else {
        return StatusCode::OK;
    };
    match seen(&h.pool, &event.id).await {
        Ok(false) => status(&event, dispatch(&h, &hook, &event, now).await),
        Ok(true) => StatusCode::OK,
        Err(_) => status(&event, Acted::Retry),
    }
}

/// The 403 of a refused source or signature, with its one log line (SEC-016).
fn rejected(error: askhava_adapters::api::SignatureError) -> StatusCode {
    StatusCode::from_u16(refuse(WebhookVendor::Stripe, error)).unwrap_or(StatusCode::FORBIDDEN)
}

/// A checked event's ID, type and what it asks for, parsed once (SEC-039, rust brain R1).
struct Event {
    id: String,
    kind: String,
    dispatch: Dispatch,
}

/// What an event asks of `payments` (FEE-026, FEE-027, FEE-052, PICK-022).
enum Dispatch {
    Release(Uuid),
    Refund(String, RefundState),
    Dispute(String, String, i64, Timestamp),
    DisputeClosed(String),
    FraudWarning(String),
}

/// What became of an event: done (its row still to write), done with its row written, about an
/// object `payments` does not know (nothing written), or to be resent.
enum Acted {
    Done,
    Recorded,
    Unknown,
    Retry,
}

#[derive(serde::Deserialize)]
struct Envelope {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    livemode: bool,
    #[serde(default)]
    account: Option<String>,
    data: EnvelopeData,
}

#[derive(serde::Deserialize)]
struct EnvelopeData {
    object: Value,
}

/// The event, when it is of this process's mode and account, has a well-formed `evt_…` ID and is
/// of a type `payments` acts on (SEC-017, SEC-039); `None` is acknowledged with nothing changed.
fn checked(account: &StripeAccount, body: &[u8]) -> Option<Event> {
    let e: Envelope = serde_json::from_slice(body).ok()?;
    let ours = e.livemode == account.live;
    let same_account = e.account.as_ref().is_none_or(|a| *a == account.account);
    let well_formed = e.id.strip_prefix("evt_").is_some_and(|rest| {
        (1..=200).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    if !(ours && same_account && well_formed) {
        return None;
    }
    let dispatch = dispatch_of(&e.kind, &e.data.object)?;
    Some(Event {
        id: e.id,
        kind: e.kind,
        dispatch,
    })
}

/// The string at `pointer` in the event's object.
fn text(object: &Value, pointer: &str) -> Option<String> {
    object.pointer(pointer)?.as_str().map(str::to_owned)
}

/// What the event type and object ask for (FEE-026, FEE-027, FEE-052, D-1754).
fn dispatch_of(kind: &str, object: &Value) -> Option<Dispatch> {
    let id = text(object, "/id");
    match kind {
        SUCCEEDED_EVENT => pick_of(object).map(Dispatch::Release),
        "refund.failed" => Some(Dispatch::Refund(id?, RefundState::Failed)),
        "refund.updated" | "charge.refund.updated" => {
            let state = RefundState::parse(object.get("status")?.as_str()?)?;
            Some(Dispatch::Refund(id?, state))
        }
        "charge.dispute.created" => {
            let amount = object.get("amount")?.as_i64()?;
            let due = object.pointer("/evidence_details/due_by")?.as_i64()?;
            let due = Timestamp::from_second(due).ok()?;
            let intent = text(object, "/payment_intent")?;
            Some(Dispatch::Dispute(id?, intent, amount, due))
        }
        "charge.dispute.closed" => Some(Dispatch::DisputeClosed(id?)),
        "radar.early_fraud_warning.created" => {
            text(object, "/payment_intent").map(Dispatch::FraudWarning)
        }
        _ => None,
    }
}

/// The pick a checked success's PaymentIntent names in `metadata[pick_id]`; [`paid_release`]
/// then holds it to that pick's stored PaymentIntent and the re-fetched metadata (SEC-039).
fn pick_of(object: &Value) -> Option<Uuid> {
    Uuid::parse_str(&text(object, "/metadata/pick_id")?).ok()
}

/// Runs the event's effect once (SEC-017; see the module docs).
async fn dispatch<S, R>(
    h: &StripeWebhook<S, R>,
    hook: &VerifiedWebhook,
    event: &Event,
    now: Timestamp,
) -> Acted
where
    S: PaymentsAdapter + Send + Sync + 'static,
    R: ReleaseSink,
{
    let acted = match &event.dispatch {
        Dispatch::Release(pick) => release(h, hook, *pick, now).await,
        Dispatch::Refund(refund, state) => refund_event(h, refund, *state, now).await,
        Dispatch::Dispute(dispute, intent, amount_cents, due_at) => {
            let opened = DisputeOpened {
                dispute,
                payment_intent: intent,
                amount_cents: *amount_cents,
                due_at: *due_at,
            };
            money(record_dispute(&h.pool, &opened, now).await)
        }
        Dispatch::DisputeClosed(_) | Dispatch::FraudWarning(_) => {
            return in_event_tx(&h.pool, event, now)
                .await
                .unwrap_or(Acted::Retry);
        }
    };
    if !matches!(acted, Acted::Done) {
        return acted;
    }
    let written = async {
        let mut conn = h.pool.acquire().await?;
        insert_event(&mut conn, event, now).await
    };
    written.await.map_or(Acted::Retry, |_| Acted::Recorded)
}

/// The 200 or 500 for what became of the event, with one log line when it is resent (R5).
fn status(event: &Event, acted: Acted) -> StatusCode {
    if matches!(acted, Acted::Retry) {
        tracing::warn!(event_type = %event.kind, "stripe event not finished; Stripe resends");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

/// A success through [`paid_release`] to the [`ReleaseSink`] as T224 built it: only `Stripe` and
/// `Database` failures are resent, every other refusal is final (PICK-022, SEC-049).
async fn release<S, R>(
    h: &StripeWebhook<S, R>,
    hook: &VerifiedWebhook,
    pick: Uuid,
    now: Timestamp,
) -> Acted
where
    S: PaymentsAdapter + Send + Sync + 'static,
    R: ReleaseSink,
{
    let Ok(event) = VerifiedStripeEvent::from_webhook(hook) else {
        return Acted::Unknown;
    };
    let done = match paid_release(&h.pool, &h.stripe, &h.account, &event, pick, now).await {
        Ok(cleared) => h.sink.release(cleared).await,
        Err(error) => Err(error),
    };
    match done {
        Ok(()) => Acted::Done,
        Err(ReleaseError::Stripe | ReleaseError::Database) => Acted::Retry,
        Err(other) => {
            tracing::info!(reason = %other, "stripe event cleared nothing");
            Acted::Done
        }
    }
}

/// FEE-026, D-1753: a refund event by its status; a `succeeded` one is re-read through the adapter
/// first and Stripe's present state applied, so a late `succeeded` never undoes a `failed`. A
/// refund Stripe does not know, or answers unreadably, is unknown; an unreachable Stripe resends.
async fn refund_event<S, R>(
    h: &StripeWebhook<S, R>,
    refund: &str,
    state: RefundState,
    now: Timestamp,
) -> Acted
where
    S: PaymentsAdapter + Send + Sync + 'static,
{
    let state = if state == RefundState::Succeeded {
        match h.stripe.fetch_refund(refund).await {
            Ok(read) => read.state,
            Err(
                AdapterError::Vendor { status: 404, .. }
                | AdapterError::InvalidId
                | AdapterError::Decode,
            ) => return Acted::Unknown,
            Err(_) => return Acted::Retry,
        }
    } else {
        state
    };
    let pool = &h.pool;
    money(match state {
        RefundState::Succeeded => refund_succeeded(pool, refund, now).await,
        RefundState::Failed => refund_failed(pool, refund, now).await,
        RefundState::Canceled => refund_canceled(pool, refund, now).await,
        RefundState::Pending | RefundState::RequiresAction => Ok(false),
    })
}

/// A money handler's answer: an unknown charge changes nothing, any other error is resent.
fn money(result: Result<bool, MoneyEventError>) -> Acted {
    match result {
        Ok(_) => Acted::Done,
        Err(MoneyEventError::UnknownCharge) => Acted::Unknown,
        Err(_) => Acted::Retry,
    }
}

/// A closed dispute or an early fraud warning: the event row and its effect in one transaction;
/// a repeat, or an unknown dispute or charge, rolls back and changes nothing (SEC-017, D-1754).
async fn in_event_tx(pool: &PgPool, event: &Event, now: Timestamp) -> Result<Acted, OpsOrDb> {
    let mut tx = pool.begin().await?;
    if !insert_event(&mut tx, event, now).await? {
        return Ok(Acted::Recorded);
    }
    let known = match &event.dispatch {
        Dispatch::DisputeClosed(dispute) => dispute_closed(&mut tx, dispute).await?,
        Dispatch::FraudWarning(intent) => fraud_warning(&mut tx, intent, now).await?,
        _ => false,
    };
    if !known {
        return Ok(Acted::Unknown);
    }
    tx.commit().await?;
    Ok(Acted::Recorded)
}

/// Any failure of [`in_event_tx`]: the transaction rolls back and Stripe resends.
struct OpsOrDb;

impl From<sqlx::Error> for OpsOrDb {
    fn from(_: sqlx::Error) -> Self {
        Self
    }
}

impl From<askhava_ops::api::OpsError> for OpsOrDb {
    fn from(_: askhava_ops::api::OpsError) -> Self {
        Self
    }
}

impl From<askhava_core::api::TaskError> for OpsOrDb {
    fn from(_: askhava_core::api::TaskError) -> Self {
        Self
    }
}

/// Writes the event's row; `false` when its ID is already there (SEC-017).
async fn insert_event(
    conn: &mut PgConnection,
    event: &Event,
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let new = sqlx::query_scalar!(
        "INSERT INTO payments.webhook_event (stripe_event_id, event_type, received_at)
         VALUES ($1, $2, $3) ON CONFLICT (stripe_event_id) DO NOTHING RETURNING true AS \"new!\"",
        event.id,
        event.kind,
        now.to_sqlx() as _,
    )
    .fetch_optional(conn)
    .await?;
    Ok(new.is_some())
}

/// Whether the event ID was already handled (SEC-017).
async fn seen(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM payments.webhook_event WHERE stripe_event_id = $1) AS \"seen!\"",
        id,
    )
    .fetch_one(pool)
    .await
}

/// FEE-027, D-1545: one audit row and one High alert per closed dispute `dp_…`, whatever its
/// outcome and however many events report it; no money moves. `false` for an unknown dispute.
async fn dispute_closed(conn: &mut PgConnection, dispute: &str) -> Result<bool, OpsOrDb> {
    let id = sqlx::query_scalar!(
        "SELECT id FROM payments.dispute WHERE stripe_dispute_id = $1",
        dispute,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(id) = id else {
        return Ok(false);
    };
    let entry = AuditEntry {
        actor: Actor::System,
        action: DISPUTE_CLOSED_ACTION,
        subject_kind: DISPUTE_SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    if audit_once(conn, &entry).await? {
        let alert = NewAlert {
            kind: DISPUTE_CLOSED_ACTION,
            severity: Severity::High,
            subject: Some((DISPUTE_SUBJECT, id)),
        };
        raise_alert(conn, &alert).await?;
    }
    Ok(true)
}

/// FEE-052, D-1845: enqueues one [`FRAUD_WARNING_KIND`] task for the pick charged through
/// `intent`; `false` when no pick was.
async fn fraud_warning(
    conn: &mut PgConnection,
    intent: &str,
    now: Timestamp,
) -> Result<bool, OpsOrDb> {
    let pick = sqlx::query_scalar!(
        "SELECT pick_id FROM payments.charge_evidence WHERE stripe_intent_id = $1",
        intent,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(pick) = pick else {
        return Ok(false);
    };
    let mut task = NewTask::new(FRAUD_WARNING_KIND, json!({"pick": pick}), now);
    task.dedupe_key = Some(format!("{FRAUD_WARNING_KIND}.{pick}"));
    enqueue(conn, &task).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    //! TD-SEC-31 at the route (SEC-039, SEC-049); the database cases of the release are T189's
    //! `tests/integration/tests/confirm_and_pay.rs`, those of the dispatch T259's
    //! `tests/integration/tests/stripe_webhook_dispatch.rs` (TD-FEE-43). Fake data only.
    use std::sync::Mutex;

    use askhava_adapters::api::StripePayments;
    use askhava_core::api::ManualClock;
    use axum::body::Body;
    use axum::http::Request;
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    use tower::ServiceExt;

    use super::*;

    const SECRET: &str = "whsec_fake_signing_secret_for_tests";
    const STAND_IN: [u8; 4] = [127, 0, 0, 1];

    #[derive(Default)]
    struct Recorded(Mutex<Vec<Uuid>>);

    impl ReleaseSink for Arc<Recorded> {
        async fn release(&self, cleared: Releasable) -> Result<(), ReleaseError> {
            self.0
                .lock()
                .map_err(|_| ReleaseError::Database)?
                .push(cleared.pick());
            Ok(())
        }
    }

    fn hooks(
        mode: AdapterMode,
        live: bool,
        sink: Arc<Recorded>,
    ) -> Arc<StripeWebhook<StripePayments, Arc<Recorded>>> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(1))
            .connect_lazy("postgres://fake@127.0.0.1:1/none")
            .expect("a lazy pool");
        let stripe = StripePayments::new("http://127.0.0.1:1", SecretString::from("sk_test_fake"))
            .expect("a client");
        let none: [(String, String); 0] = [];
        Arc::new(StripeWebhook {
            pool,
            stripe,
            secret: SecretString::from(SECRET),
            account: StripeAccount {
                live,
                account: "acct_fake".to_owned(),
            },
            mode,
            stand_in_host: IpAddr::from(STAND_IN),
            client_ip: ClientIpPolicy::from_pairs(none).expect("default policy"),
            clock: Arc::new(ManualClock::new(
                jiff::Timestamp::from_second(1_800_000_000).expect("time"),
            )),
            sink,
        })
    }

    fn event(kind: &str, livemode: bool) -> String {
        format!(
            r#"{{"id":"evt_fake1","type":"{kind}","livemode":{livemode},"data":{{"object":{{"id":"pi_fake1","metadata":{{"pick_id":"{}"}}}}}}}}"#,
            Uuid::nil()
        )
    }

    fn signed(body: &str, secret: &str) -> String {
        let stamp = "1800000000";
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).expect("key");
        mac.update(format!("{stamp}.{body}").as_bytes());
        let sig: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("t={stamp},v1={sig}")
    }

    async fn post(
        hooks: Arc<StripeWebhook<StripePayments, Arc<Recorded>>>,
        from: [u8; 4],
        body: String,
        signature: String,
    ) -> StatusCode {
        post_with(hooks, from, &[], body, signature).await
    }

    async fn post_with(
        hooks: Arc<StripeWebhook<StripePayments, Arc<Recorded>>>,
        from: [u8; 4],
        extra: &[(&str, String)],
        body: String,
        signature: String,
    ) -> StatusCode {
        let mut builder = Request::post(STRIPE_WEBHOOK_PATH).header("stripe-signature", signature);
        for (name, value) in extra {
            builder = builder.header(*name, value);
        }
        let mut request = builder.body(Body::from(body)).expect("a request");
        let peer = SocketAddr::from((from, 443));
        request.extensions_mut().insert(ConnectInfo(peer));
        let reply = stripe_webhook_routes(hooks)
            .oneshot(request)
            .await
            .expect("an answer");
        reply.status()
    }

    #[tokio::test]
    async fn td_sec_31_a_forged_signature_gets_403() {
        let sink = Arc::new(Recorded::default());
        let body = event(SUCCEEDED_EVENT, false);
        let forged = signed(&body, "whsec_someone_else");
        let status = post(
            hooks(AdapterMode::StandIn, false, sink.clone()),
            STAND_IN,
            body,
            forged,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn td_sec_31_stand_in_mode_takes_only_the_stand_ins_host() {
        let sink = Arc::new(Recorded::default());
        let body = event("charge.updated", false);
        let sig = signed(&body, SECRET);
        let hooks = hooks(AdapterMode::StandIn, false, sink.clone());
        let other = post(hooks.clone(), [10, 0, 0, 9], body.clone(), sig.clone()).await;
        assert_eq!(other, StatusCode::FORBIDDEN);
        let stand_in = post(hooks, STAND_IN, body, sig).await;
        assert_eq!(stand_in, StatusCode::OK);
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn td_sec_31_sandbox_refuses_an_address_off_stripes_list() {
        let sink = Arc::new(Recorded::default());
        let body = event(SUCCEEDED_EVENT, false);
        let sig = signed(&body, SECRET);
        let status = post(
            hooks(AdapterMode::Sandbox, false, sink.clone()),
            [203, 0, 113, 7],
            body,
            sig,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn td_sec_31_sandbox_reads_only_the_edge_header_never_a_forwarded_or_cloudflare_one() {
        let sink = Arc::new(Recorded::default());
        let hooks = hooks(AdapterMode::Sandbox, false, sink.clone());
        let stripe_ip = askhava_adapters::api::webhook_source_list()
            .first()
            .expect("the committed Stripe list")
            .to_string();
        let body = event("charge.updated", false);
        let sig = signed(&body, SECRET);
        let off_list = [203, 0, 113, 7];
        for name in ["x-forwarded-for", "cf-connecting-ip", "forwarded"] {
            let forged = [(name, stripe_ip.clone())];
            let status = post_with(hooks.clone(), off_list, &forged, body.clone(), sig.clone());
            assert_eq!(status.await, StatusCode::FORBIDDEN, "{name} is never read");
        }
        let edge = [("x-real-ip", "203.0.113.8".to_owned())];
        let status = post_with(hooks.clone(), off_list, &edge, body.clone(), sig.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "an off-list edge address");
        let edge = [("x-real-ip", stripe_ip)];
        let status = post_with(hooks, off_list, &edge, body, sig).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the edge header, which Railway overwrites"
        );
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn td_sec_31_a_test_mode_event_at_the_live_endpoint_builds_no_proof() {
        let sink = Arc::new(Recorded::default());
        let body = event(SUCCEEDED_EVENT, false);
        let sig = signed(&body, SECRET);
        let status = post(
            hooks(AdapterMode::StandIn, true, sink.clone()),
            STAND_IN,
            body,
            sig,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "acknowledged, so Stripe stops resending"
        );
        assert!(
            sink.0.lock().expect("lock").is_empty(),
            "no release from a test-mode event"
        );
    }

    #[tokio::test]
    async fn td_fee_25_a_success_whose_pick_store_is_down_is_resent_not_released() {
        let sink = Arc::new(Recorded::default());
        let body = event(SUCCEEDED_EVENT, false);
        let sig = signed(&body, SECRET);
        let status = post(
            hooks(AdapterMode::StandIn, false, sink.clone()),
            STAND_IN,
            body,
            sig,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn an_event_without_a_pick_changes_nothing() {
        let sink = Arc::new(Recorded::default());
        let body = r#"{"id":"evt_fake2","type":"payment_intent.succeeded","livemode":false,"data":{"object":{"id":"pi_fake2","metadata":{"pick_id":"not-a-pick"}}}}"#.to_owned();
        let sig = signed(&body, SECRET);
        let status = post(
            hooks(AdapterMode::StandIn, false, sink.clone()),
            STAND_IN,
            body,
            sig,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(sink.0.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn td_fee_43_an_event_nothing_acts_on_never_reaches_the_database() {
        let sink = Arc::new(Recorded::default());
        let hooks = hooks(AdapterMode::StandIn, false, sink.clone());
        let object = r#"{"id":"re_Fake1","status":"succeeded"}"#;
        let bodies = [
            format!(r#"{{"id":"evt_fake3","type":"refund.updated","livemode":false,"account":"acct_other","data":{{"object":{object}}}}}"#),
            format!(r#"{{"id":"evt_bad-id","type":"refund.updated","livemode":false,"data":{{"object":{object}}}}}"#),
            r#"{"id":"evt_fake4","type":"refund.updated","livemode":false,"data":{"object":{"id":"re_Fake1","status":"done"}}}"#.to_owned(),
            r#"{"id":"evt_fake5","type":"charge.dispute.created","livemode":false,"data":{"object":{"id":"dp_Fake1","amount":7500}}}"#.to_owned(),
            r#"{"id":"evt_fake6","type":"radar.early_fraud_warning.created","livemode":false,"data":{"object":{"id":"issfr_Fake1"}}}"#.to_owned(),
        ];
        for body in bodies {
            let sig = signed(&body, SECRET);
            let status = post(hooks.clone(), STAND_IN, body, sig).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "acknowledged before any database read"
            );
        }
        assert!(sink.0.lock().expect("lock").is_empty());
    }
}

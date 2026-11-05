//! The Stripe webhook route: the only way a paid pick is released (PICK-022, SEC-039, SEC-049,
//! TD-FEE-25, TD-SEC-31; T224, D-1479).
//!
//! `POST` [`STRIPE_WEBHOOK_PATH`] takes a Stripe event in this order and nothing else sees it
//! first: the source address, read as SEC-040 takes it (`ClientIpPolicy::client_ip` over the
//! socket peer), must be on the committed copy of Stripe's webhook list in sandbox and live, or
//! the stand-in's host in stand-in mode (`check_webhook_source`, SEC-049); then the
//! `Stripe-Signature` is checked on the raw body with the 5-minute tolerance (`verify_webhook`,
//! SEC-016, SEC-039). Either refusal is a 403 with one log line (`refuse`). A checked event of
//! any type but `payment_intent.succeeded` is acknowledged and changes nothing. A success goes
//! to T189's [`paid_release`], which checks livemode, account, the pick's own PaymentIntent and
//! re-fetches it from Stripe (amount, currency, pick metadata) before anything is cleared; only
//! a [`Releasable`] from it reaches the [`ReleaseSink`], whose holder mints the `ReleaseProof`
//! with `server::release_minter()` and releases once. A redirect back from Stripe's page carries
//! no event, so it clears nothing (SEC-030). A test-mode event at the live endpoint is
//! `WrongMode` and builds no proof. When Stripe could not be asked or the database failed the
//! route answers 500, so Stripe sends the event again; every other refusal is acknowledged with
//! 200 so Stripe stops resending something that will never pass.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterMode, Delivery, PaymentsAdapter, WebhookVendor, check_webhook_source, refuse,
    verify_webhook,
};
use askhava_core::api::{ClientIpPolicy, Clock};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::release_proof::{
    Releasable, ReleaseError, SUCCEEDED_EVENT, StripeAccount, VerifiedStripeEvent, paid_release,
};

/// The Stripe webhook's path (SEC-039).
pub const STRIPE_WEBHOOK_PATH: &str = "/webhooks/stripe";

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
    /// The Stripe client the success is re-fetched with (SEC-049).
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

/// One delivery: source, signature, then the release (see the module docs).
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
    let Ok(event) = VerifiedStripeEvent::from_webhook(&hook) else {
        return StatusCode::OK;
    };
    if event.kind() != SUCCEEDED_EVENT {
        return StatusCode::OK;
    }
    let Some(pick) = pick_of(hook.body()) else {
        return StatusCode::OK;
    };
    let cleared = paid_release(&h.pool, &h.stripe, &h.account, &event, pick, now).await;
    answer(cleared.map(|c| h.sink.release(c))).await
}

/// The 403 of a refused source or signature, with its one log line (SEC-016).
fn rejected(error: askhava_adapters::api::SignatureError) -> StatusCode {
    StatusCode::from_u16(refuse(WebhookVendor::Stripe, error)).unwrap_or(StatusCode::FORBIDDEN)
}

/// The status for a release attempt: 500 only when trying again could help (SEC-049).
async fn answer<F>(cleared: Result<F, ReleaseError>) -> StatusCode
where
    F: Future<Output = Result<(), ReleaseError>>,
{
    let done = match cleared {
        Ok(release) => release.await,
        Err(error) => Err(error),
    };
    match done {
        Ok(()) => StatusCode::OK,
        Err(ReleaseError::Stripe | ReleaseError::Database) => {
            tracing::warn!(
                step = "stripe_release",
                "release not finished; Stripe resends"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(other) => {
            tracing::info!(reason = %other, "stripe event cleared nothing");
            StatusCode::OK
        }
    }
}

#[derive(serde::Deserialize)]
struct PickEvent {
    data: PickData,
}

#[derive(serde::Deserialize)]
struct PickData {
    object: PickObject,
}

#[derive(serde::Deserialize)]
struct PickObject {
    metadata: PickMetadata,
}

#[derive(serde::Deserialize)]
struct PickMetadata {
    pick_id: String,
}

/// The pick a checked event's PaymentIntent names in `metadata[pick_id]`; [`paid_release`] then
/// holds it to that pick's stored PaymentIntent and the re-fetched metadata (SEC-039).
fn pick_of(body: &[u8]) -> Option<Uuid> {
    let event: PickEvent = serde_json::from_slice(body).ok()?;
    Uuid::parse_str(&event.data.object.metadata.pick_id).ok()
}

#[cfg(test)]
mod tests {
    //! TD-SEC-31 at the route (SEC-039, SEC-049); the database cases of the release are T189's
    //! `tests/integration/tests/confirm_and_pay.rs`. Fake data only.
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
}

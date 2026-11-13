//! Security-event rows are capped per minute, and webhook refusals are security events
//! (TD-SEC-53; SEC-048, SEC-016, SEC-040; D-2207, D-3445, D-3469 to D-3473).
//!
//! A flood of cross-site posts stores at most the cap of rows per (kind, client address, route)
//! and per (kind, route) in a minute, each still a 403, and the burst alert still fires once; the
//! cap defaults to the alert threshold and a lower setting is raised to it. The webhook layer
//! (`app/server/src/webhook_refusals.rs`, compiled here by `#[path]` as `offdb_stream.rs` is in
//! `sentinel_db_logs.rs`, since no test crate depends on `askhava-server`) records each Stripe
//! and Twilio refusal as one `webhook_signature_failed` event with route and request ID and no
//! actor, under the same cap, and a valid delivery records none. The routes under the layer
//! refuse exactly as `payments`' Stripe route and `server`'s `inbound_text`, `inbound_voice`
//! and `voice_status` do (the adapter's `check_webhook_source` for Stripe, `verify_webhook`,
//! then `refuse`; only `payments` may build its route's Stripe client, FEE-032), and a source
//! check pins the layer on all four routers in `build_router`. Every value is fake.

#[path = "../../../app/server/src/webhook_refusals.rs"]
mod webhook_refusals;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterMode, Delivery, WebhookVendor, check_webhook_source, refuse, verify_webhook,
};
use askhava_core::api::{ClientIpPolicy, ManualClock, Settings};
use askhava_portals::api::{
    EVENTS_PER_HOUR_ALERT, SECURITY_EVENT_CAP_FLOOR, SecurityEventKind, WebSecurity,
    admit_security_event, configure_security_events, refuse_cross_site, security_event_cap,
};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::routing::post;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use webhook_refusals::{RefusalLog, layer};

const HOST: &str = "laftly.test";
const STRIPE_SECRET: &str = "whsec_fake_cap_0001";
const TWILIO_TOKEN: &str = "fake_twilio_cap_token";
const SIGNED_AT: i64 = 1_800_000_000;
const FAKE_REQUEST_ID: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";
const STRIPE_PATH: &str = "/webhooks/stripe";
const SMS_PATH: &str = "/webhooks/twilio/sms";
const VOICE_PATH: &str = "/webhooks/twilio/voice";
const CALL_STATUS_PATH: &str = "/webhooks/twilio/voice/status";

/// One fixed minute for every capped case in this file: the cap's window is per process, and a
/// later minute would drop the earlier minute's counts of a case running beside it.
fn now() -> Timestamp {
    "2030-01-01T00:00:10Z".parse().expect("time")
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(now()))
}

/// A portals route behind the cross-site refusal, at its own template so cases do not share caps.
fn portal(pool: PgPool, route: &str) -> Router {
    let security = WebSecurity::new(pool, clock());
    Router::new()
        .route(route, post(|| async { "posted" }))
        .layer(from_fn_with_state(security.clone(), refuse_cross_site))
        .with_state(security)
}

/// A cross-site post to `path` from `client` (the SEC-040 edge header the default policy reads).
fn cross_site(path: &str, client: &str) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://elsewhere.example")
        .header("x-real-ip", client)
        .header("x-request-id", FAKE_REQUEST_ID)
        .body(Body::empty())
        .expect("request")
}

async fn status(app: &Router, request: Request<Body>) -> StatusCode {
    app.clone().oneshot(request).await.expect("answer").status()
}

async fn rows(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM portals.security_event WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("alerts")
}

/// TD-SEC-53 (SEC-048): 500 cross-site posts from one address to one route in a minute store
/// the cap of rows, each answered 403, and the burst alert fires once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_53_a_flood_from_one_address_stores_the_cap(pool: PgPool) {
    let app = portal(pool.clone(), "/cap/one/{token}");
    for _ in 0..500 {
        let answer = status(&app, cross_site("/cap/one/fake", "203.0.113.10")).await;
        assert_eq!(answer, StatusCode::FORBIDDEN, "every refusal stays a 403");
    }
    assert_eq!(rows(&pool, "csrf_rejected").await, EVENTS_PER_HOUR_ALERT);
    assert_eq!(alerts(&pool, "portals.csrf_rejected_burst").await, 1);
}

/// TD-SEC-53 (SEC-048, SEC-040): 500 posts, each with a new forged client address, store at most
/// the per-(kind, route) ceiling, so a forged header buys no fresh cap.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_53_forged_addresses_stop_at_the_route_ceiling(pool: PgPool) {
    let app = portal(pool.clone(), "/cap/forged/{token}");
    for n in 0..500_u32 {
        let forged = Ipv4Addr::from(0xC633_6400 + n).to_string();
        let answer = status(&app, cross_site("/cap/forged/fake", &forged)).await;
        assert_eq!(answer, StatusCode::FORBIDDEN);
    }
    assert_eq!(rows(&pool, "csrf_rejected").await, EVENTS_PER_HOUR_ALERT);
}

/// TD-SEC-53 (SEC-048): a second address gets its own count: after 20 from one address, a flood
/// from another stores the rest of the ceiling (D-3470: the ceiling equals the cap, D-3445).
#[test]
fn td_sec_53_a_second_address_gets_its_own_count() {
    let (kind, route) = (SecurityEventKind::AuthFailed, "/cap/second");
    let first = IpAddr::from([198, 51, 100, 1]);
    let second = IpAddr::from([198, 51, 100, 2]);
    let stored = (0..20)
        .filter(|_| admit_security_event(kind, first, route, now()))
        .count();
    assert_eq!(stored, 20, "the first address is under its cap");
    let rest = (0..500)
        .filter(|_| admit_security_event(kind, second, route, now()))
        .count();
    assert_eq!(
        rest, 30,
        "the second address stores up to the shared ceiling"
    );
    assert!(!admit_security_event(kind, first, route, now()));
    let other_kind = SecurityEventKind::AuthzDenied;
    assert!(admit_security_event(other_kind, first, route, now()));
}

/// TD-SEC-53 (SEC-048, D-3469): the cap defaults to the alert threshold, a lower setting is raised
/// to it, a higher one holds; the setting parses as a whole number above 0.
#[test]
fn td_sec_53_the_cap_never_falls_below_the_alert_threshold() {
    assert_eq!(i64::from(SECURITY_EVENT_CAP_FLOOR), EVENTS_PER_HOUR_ALERT);
    assert_eq!(security_event_cap(None), SECURITY_EVENT_CAP_FLOOR);
    assert_eq!(security_event_cap(Some(10)), SECURITY_EVENT_CAP_FLOOR);
    assert_eq!(security_event_cap(Some(80)), 80);
    let none: [(String, String); 0] = [];
    let policy = ClientIpPolicy::from_pairs(none).expect("policy");
    assert_eq!(
        configure_security_events(Some(10), policy),
        SECURITY_EVENT_CAP_FLOOR
    );
    let base = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
    ];
    let parse = |value: Option<&str>| {
        let extra = value.map(|v| ("ASKHAVA_SECURITY_EVENTS_PER_MINUTE", v));
        let pairs = base.iter().copied().chain(extra);
        Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned())))
    };
    let setting = |value| parse(value).expect("settings").security_events_per_minute();
    assert_eq!(setting(None), None);
    assert_eq!(setting(Some("10")), Some(10));
    assert!(parse(Some("0")).is_err() && parse(Some("ten")).is_err());
}

/// A Stripe route that refuses as `payments`' `receive` does: the SEC-040 source check in
/// stand-in mode (127.0.0.1 only), then the signature, each refusal `refuse`'s 403.
async fn stripe_route(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    head: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let none: [(String, String); 0] = [];
    let policy = ClientIpPolicy::from_pairs(none).expect("policy");
    let client = policy.client_ip(&head, peer.ip());
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let refused = |e| StatusCode::from_u16(refuse(WebhookVendor::Stripe, e));
    if let Err(e) = check_webhook_source(AdapterMode::StandIn, client, localhost) {
        return refused(e).unwrap_or(StatusCode::FORBIDDEN);
    }
    let pairs: Vec<(&str, &str)> = head
        .iter()
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.as_str(), v)))
        .collect();
    let delivery = Delivery {
        url: STRIPE_PATH,
        body: &body,
        headers: &pairs,
    };
    let secret = SecretString::from(STRIPE_SECRET);
    match verify_webhook(WebhookVendor::Stripe, &secret, delivery, SIGNED_AT) {
        Ok(_) => StatusCode::OK,
        Err(e) => refused(e).unwrap_or(StatusCode::FORBIDDEN),
    }
}

/// The Stripe route under the refusal layer, whose clock reads `at`.
fn stripe(pool: PgPool, at: Timestamp) -> Router {
    let log = RefusalLog::new(pool, Arc::new(ManualClock::new(at)));
    layer(Router::new().route(STRIPE_PATH, post(stripe_route)), log)
}

fn stripe_request(body: &str, signature: &str, edge: Option<&str>) -> Request<Body> {
    let mut builder = Request::post(STRIPE_PATH)
        .header("stripe-signature", signature)
        .header("x-request-id", FAKE_REQUEST_ID);
    if let Some(edge) = edge {
        builder = builder.header("x-real-ip", edge);
    }
    let mut request = builder.body(Body::from(body.to_owned())).expect("request");
    let peer = SocketAddr::from(([127, 0, 0, 1], 443));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

fn stripe_signature(body: &str) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(STRIPE_SECRET.as_bytes()).expect("key");
    mac.update(format!("{SIGNED_AT}.{body}").as_bytes());
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("t={SIGNED_AT},v1={hex}")
}

/// A Twilio route that refuses as `server`'s `inbound_text`, `inbound_voice` and `voice_status` do.
async fn twilio(uri: axum::http::Uri, head: HeaderMap, body: Bytes) -> StatusCode {
    let pairs: Vec<(&str, &str)> = head
        .iter()
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.as_str(), v)))
        .collect();
    let url = format!("https://{HOST}{}", uri.path());
    let delivery = Delivery {
        url: &url,
        body: &body,
        headers: &pairs,
    };
    let token = SecretString::from(TWILIO_TOKEN);
    match verify_webhook(WebhookVendor::Twilio, &token, delivery, SIGNED_AT) {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            StatusCode::from_u16(refuse(WebhookVendor::Twilio, e)).unwrap_or(StatusCode::FORBIDDEN)
        }
    }
}

fn twilio_routes(pool: PgPool) -> Router {
    let text = layer(
        Router::new().route(SMS_PATH, post(twilio)),
        RefusalLog::new(pool.clone(), clock()),
    );
    let voice = layer(
        Router::new().route(VOICE_PATH, post(twilio)),
        RefusalLog::new(pool.clone(), clock()),
    );
    let calls = layer(
        Router::new().route(CALL_STATUS_PATH, post(twilio)),
        RefusalLog::new(pool, clock()),
    );
    text.merge(voice).merge(calls)
}

fn twilio_request(path: &str) -> Request<Body> {
    Request::post(path)
        .header("x-twilio-signature", "bm90LWEtcmVhbC1zaWduYXR1cmU=")
        .header("x-request-id", FAKE_REQUEST_ID)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("From=%2B15005550006&Body=fake"))
        .expect("request")
}

async fn webhook_events(pool: &PgPool) -> Vec<(String, Option<Uuid>, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT route, request_id, actor_ref FROM portals.security_event \
         WHERE kind = 'webhook_signature_failed' ORDER BY at, id",
    )
    .fetch_all(pool)
    .await
    .expect("events")
}

/// TD-SEC-53 (SEC-016, SEC-048): a bad Stripe signature, a Stripe request from an address off
/// the list and a bad Twilio signature on the text, voice and call-status routes each record one
/// `webhook_signature_failed` event with route and request ID and no actor; a valid delivery
/// records none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_53_each_webhook_refusal_is_one_event(pool: PgPool) {
    let body =
        r#"{"id":"evt_fakecap1","type":"customer.created","livemode":false,"data":{"object":{}}}"#;
    let app = stripe(pool.clone(), now());
    let good = stripe_signature(body);
    assert_eq!(
        status(&app, stripe_request(body, &good, None)).await,
        StatusCode::OK
    );
    assert!(
        webhook_events(&pool).await.is_empty(),
        "a valid delivery records none"
    );
    let forged = stripe_request(body, "t=1800000000,v1=00", None);
    assert_eq!(status(&app, forged).await, StatusCode::FORBIDDEN);
    let off_list = stripe_request(body, &good, Some("203.0.113.77"));
    assert_eq!(status(&app, off_list).await, StatusCode::FORBIDDEN);
    let twilio = twilio_routes(pool.clone());
    for path in [SMS_PATH, VOICE_PATH, CALL_STATUS_PATH] {
        assert_eq!(
            status(&twilio, twilio_request(path)).await,
            StatusCode::FORBIDDEN
        );
    }
    let id = Uuid::parse_str(FAKE_REQUEST_ID).ok();
    let mut events = webhook_events(&pool).await;
    events.sort();
    let mut expected: Vec<_> = [
        STRIPE_PATH,
        SMS_PATH,
        VOICE_PATH,
        CALL_STATUS_PATH,
        STRIPE_PATH,
    ]
    .into_iter()
    .map(|route| (route.to_owned(), id, None))
    .collect();
    expected.sort();
    assert_eq!(events, expected);
}

/// TD-SEC-53 (SEC-016, SEC-048): webhook refusals share the cap: 60 forged Stripe deliveries
/// from one address in a minute store the cap, each a 403. Timed a minute before the other
/// cases, so the route's ceiling is this case's own.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_53_webhook_refusals_are_capped(pool: PgPool) {
    let app = stripe(pool.clone(), now() - jiff::SignedDuration::from_mins(1));
    for _ in 0..60 {
        let forged = stripe_request("{}", "t=1800000000,v1=00", Some("127.0.0.1"));
        assert_eq!(status(&app, forged).await, StatusCode::FORBIDDEN);
    }
    let stored = rows(&pool, "webhook_signature_failed").await;
    assert_eq!(stored, EVENTS_PER_HOUR_ALERT);
}

/// TD-SEC-53 (SEC-016, D-3473): `build_router` puts the layer on the Stripe, Twilio text, Twilio
/// voice and Twilio call-callback routers, and on no other.
#[test]
fn td_sec_53_the_layer_is_on_the_four_webhook_routers() {
    let lib = include_str!("../../../app/server/src/lib.rs");
    for wired in [
        "refused(inbound_text::routes(h))",
        "refused(inbound_voice::routes(v))",
        "refused(stripe_webhook_routes(s))",
        "refused(voice_status::routes(h))",
    ] {
        assert!(lib.contains(wired), "{wired} in build_router");
    }
    assert_eq!(lib.matches("refused(").count(), 4, "four routers only");
}

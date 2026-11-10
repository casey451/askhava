//! TD-SEC-42 (T286, D-1870; FEE-037): with the clock paused, `build_router`'s whole-request time
//! limit answers a never-answering handler and a stalled body 503 with `Retry-After`, the SEC-012
//! headers, `no-store` and the request ID; a handler just inside the limit is untouched; a bad
//! `ASKHAVA_REQUEST_TIMEOUT_SECS` stops start-up naming the key only; a Confirm press cut while its
//! Stripe call is in flight leaves its attempt `sent`, and a later press charges nothing until
//! Stripe says the first failed. Fake data only.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use askhava_adapters::api::PaymentsAdapter;
use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent};
use askhava_core::api::Settings;
use askhava_payments::api::{BusinessId, ChargeOutcome, IntentStripe, PickNow, Press};
use askhava_payments::api::{
    FeeJob, FeeMode, FeeQuote, PeriodJob, RecurringCap, Service, VisitTier, confirm_and_pay,
    ensure_customer, fee_at_pick, fix_fee_at_pick,
};
use askhava_server::request_timeout::RequestTimeout;
use askhava_server::{Hooks, build_router};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::time::Instant;
use tower::ServiceExt;

const KEY: &str = "ASKHAVA_REQUEST_TIMEOUT_SECS";

fn settings(timeout: Option<&str>) -> Result<Settings, String> {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
    ];
    pairs.extend(timeout.map(|t| (KEY, t)));
    Settings::from_pairs(pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())))
        .map_err(|e| e.to_string())
}

/// The app over `routes` (merged where the test-control routes go) and `pool` at `limit`.
fn app(pool: PgPool, routes: Router, limit: RequestTimeout) -> Router {
    let hooks: Hooks = (None, None, None, None);
    build_router(pool, routes, (hooks, limit))
}

fn lazy_app(limit: RequestTimeout) -> Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("a lazy pool");
    let routes = Router::new()
        .route("/never", get(std::future::pending::<&'static str>))
        .route("/inside", get(inside))
        .route(
            "/body",
            post(|body: Bytes| async move { body.len().to_string() }),
        );
    app(pool, routes, limit)
}

async fn inside() -> &'static str {
    tokio::time::sleep(Duration::from_secs(59)).await;
    "answered"
}

/// A cut answer: 503, `Retry-After: 5`, `no-store`, the request ID and the SEC-012 headers.
fn assert_cut(status: StatusCode, headers: &HeaderMap) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let value = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    assert_eq!(value("retry-after"), "5");
    assert_eq!(value("cache-control"), "no-store");
    assert!(!value("x-request-id").is_empty(), "the request ID");
    assert_eq!(value("x-content-type-options"), "nosniff");
    assert!(value("content-security-policy").contains("frame-ancestors 'none'"));
    assert!(value("strict-transport-security").contains("max-age="));
    assert!(value("permissions-policy").contains("camera=()"));
}

async fn get_at(app: Router, path: &str) -> (StatusCode, HeaderMap, Duration) {
    let start = Instant::now();
    let request = Request::get(path).body(Body::empty()).expect("request");
    let response = app.oneshot(request).await.expect("infallible");
    (
        response.status(),
        response.headers().clone(),
        start.elapsed(),
    )
}

/// TD-SEC-42: a handler that never answers is cut at the configured limit, never before.
#[tokio::test(start_paused = true)]
async fn td_sec_42_a_handler_that_never_answers_is_cut_at_the_limit() {
    for (raw, secs) in [(None, 60), (Some("10"), 10), (Some("300"), 300)] {
        let limit = RequestTimeout::from_settings(&settings(raw).expect("settings"));
        assert_eq!(limit.duration(), Duration::from_secs(secs));
        let (status, headers, took) = get_at(lazy_app(limit), "/never").await;
        assert_cut(status, &headers);
        assert!(took >= limit.duration() && took < limit.duration() + Duration::from_secs(1));
    }
}

/// TD-SEC-42: a handler that answers one second inside the limit is untouched.
#[tokio::test(start_paused = true)]
async fn td_sec_42_a_handler_just_inside_the_limit_is_untouched() {
    let (status, headers, took) = get_at(lazy_app(RequestTimeout::DEFAULT), "/inside").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("retry-after").is_none());
    assert_eq!(took, Duration::from_secs(59));
}

/// TD-SEC-42: a body that stops arriving over a real connection is cut the same way.
#[tokio::test(start_paused = true)]
async fn td_sec_42_a_body_that_stops_arriving_is_cut() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let served = axum::serve(listener, lazy_app(RequestTimeout::DEFAULT)).into_future();
    tokio::spawn(served);
    let mut client = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect");
    let head = "POST /body HTTP/1.1\r\nHost: fake.example\r\nContent-Length: 100\r\n\r\nabc";
    client.write_all(head.as_bytes()).await.expect("write");
    let start = Instant::now();
    let mut answer = Vec::new();
    while !answer.windows(4).any(|w| w == b"\r\n\r\n") {
        let mut chunk = [0_u8; 1024];
        let read = client.read(&mut chunk).await.expect("read");
        assert!(read > 0, "the connection closed without an answer");
        answer.extend_from_slice(&chunk[..read]);
    }
    assert!(start.elapsed() >= Duration::from_secs(60));
    let text = String::from_utf8_lossy(&answer).to_ascii_lowercase();
    let mut lines = text.split("\r\n");
    let status = lines.next().expect("status line");
    let code: u16 = status
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .expect("code");
    let mut headers = HeaderMap::new();
    for (name, value) in lines.filter_map(|l| l.split_once(": ")) {
        let name = axum::http::HeaderName::from_bytes(name.as_bytes()).expect("name");
        headers.insert(name, value.parse().expect("value"));
    }
    assert_cut(StatusCode::from_u16(code).expect("status"), &headers);
}

/// TD-SEC-42: a limit outside 10 to 300 or not a whole number stops start-up naming the key and
/// never the value.
#[test]
fn td_sec_42_a_bad_limit_stops_start_up_naming_the_key() {
    for bad in ["9", "301", "0", "-60", "60.5", "sixty", "1e2"] {
        let error = settings(Some(bad)).expect_err(bad);
        assert!(error.contains(KEY), "{error}");
        assert!(!error.contains(bad), "{error}");
    }
}

/// A Stripe whose first confirm never answers (and says when it is in flight); a later confirm
/// succeeds; each fetch takes the next scripted status.
struct StalledStripe {
    in_flight: Notify,
    confirms: AtomicUsize,
    fetches: Mutex<VecDeque<&'static str>>,
}

fn intent(status: &str) -> Result<PaymentIntent, AdapterError> {
    Ok(PaymentIntent {
        id: "pi_FakeT286".to_owned(),
        status: status.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT286".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
    })
}

impl PaymentsAdapter for StalledStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_FakeT286".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("a charge never creates a confirmed intent")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        let next = self.fetches.lock().expect("lock").pop_front();
        next.map_or(Err(AdapterError::NotSent), intent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for StalledStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        Ok("pi_FakeT286".to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        if self.confirms.fetch_add(1, Ordering::SeqCst) == 0 {
            self.in_flight.notify_one();
            std::future::pending::<()>().await;
        }
        intent("succeeded")
    }
}

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb286_0001));
const PICK: Uuid = Uuid::from_u128(0x2860);

fn press(n: u128) -> Press<'static> {
    Press {
        pick: PICK,
        business: BUSINESS,
        job: Uuid::from_u128(0x2861),
        amount_cents: 2500,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(0x2870 + n),
        emergency: false,
        now: PickNow::Payable,
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t286".to_owned())
}

/// The business's Stripe customer and the $25 fee fixed at the pick (D-1871), through the
/// payments API only (TD-SEC-10).
async fn seed(pool: &PgPool, stripe: &StalledStripe) {
    ensure_customer(pool, stripe, &secret(), BUSINESS)
        .await
        .expect("customer");
    let job = FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    };
    let job = PeriodJob {
        job,
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(10_000), None, job, FeeMode::Tiers, None).expect("fee");
    let mut conn = pool.acquire().await.expect("conn");
    let at = jiff::Timestamp::from_second(1_793_000_000).expect("time");
    fix_fee_at_pick(&mut conn, PICK, BUSINESS, press(0).job, &fee, at)
        .await
        .expect("fee fixed");
}

async fn pressed(pool: &PgPool, stripe: &StalledStripe, n: u128) -> ChargeOutcome {
    confirm_and_pay(pool, stripe, &secret(), &press(n), jiff::Timestamp::now())
        .await
        .expect("press")
}

/// TD-SEC-42 (FEE-037): a Confirm press cut at the limit during its Stripe call leaves its
/// attempt `sent`; a second press asks Stripe and charges nothing while the first may still go
/// through, and only Stripe saying it failed lets the next try confirm.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_42_a_press_cut_in_flight_never_charges_twice(pool: PgPool) {
    let stripe = Arc::new(StalledStripe {
        in_flight: Notify::new(),
        confirms: AtomicUsize::new(0),
        fetches: Mutex::new(VecDeque::from(["processing", "requires_payment_method"])),
    });
    seed(&pool, &stripe).await;
    let (db, fake) = (pool.clone(), Arc::clone(&stripe));
    let route = post(move || async move { format!("{:?}", pressed(&db, &fake, 1).await) });
    let app = app(
        pool.clone(),
        Router::new().route("/press", route),
        RequestTimeout::DEFAULT,
    );
    let request = Request::post("/press")
        .body(Body::empty())
        .expect("request");
    let call = tokio::spawn(app.oneshot(request));
    stripe.in_flight.notified().await;
    tokio::time::pause();
    tokio::time::advance(RequestTimeout::DEFAULT.duration()).await;
    let response = call.await.expect("joined").expect("infallible");
    tokio::time::resume();
    assert_cut(response.status(), response.headers());
    assert_eq!(pressed(&pool, &stripe, 2).await, ChargeOutcome::Pending);
    assert_eq!(
        stripe.confirms.load(Ordering::SeqCst),
        1,
        "no second confirm"
    );
    let paid = pressed(&pool, &stripe, 3).await;
    assert_eq!(paid, ChargeOutcome::Succeeded("pi_FakeT286".to_owned()));
    assert_eq!(stripe.confirms.load(Ordering::SeqCst), 2);
}

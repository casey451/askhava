//! TD-SEC-47 (SEC-006, SEC-034, SEC-042, SEC-043, SEC-048, FEE-026, FEE-050, OPS-008, OPS-010;
//! T304, D-2128, D-2204, D-3890): the owner refund review step, its presses, the audit log viewer
//! and the health page served by the assembled router against PostgreSQL. Only the owner session
//! reaches them: no session or a business session gets the 401 linking to
//! `/owner/sign-in`. After a fresh second factor a refund press issues one Stripe refund (a fake
//! Stripe on a local port, reached by the hooks' own client) and one ledger row, a replay
//! nothing more; a disputed charge is blocked, and "I submitted the evidence myself" books the
//! countering fee once. Every answer is `no-store`; a cross-site or wrong-token `POST` is a 403
//! `csrf_rejected` event that changes nothing. Seeds go through module APIs only; the pass
//! phrase, token, keys, IDs and number are fake.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, MaskedPool, PaymentIntent, PaymentsAdapter,
    StripePayments, TwilioConfig,
};
use askhava_core::api::{ClientIpPolicy, MIGRATOR, Settings, SystemClock};
use askhava_payments::api::{
    BusinessId, ChargeOutcome, DisputeOpened, EntryKind, FeeJob, FeeMode, FeeQuote, IntentStripe,
    PeriodJob, PickNow, Press, RecurringCap, SavedCard, Service, StripeAccount, StripeWebhook,
    VisitTier, business_statement, confirm_and_pay, ensure_customer, fee_at_pick, fix_fee_at_pick,
    record_dispute, save_card,
};
use askhava_portals::api::{
    CookieJar, OwnerAuth, Realm, SecondFactor, bootstrap_owner, csrf_token, ensure_pre_session,
    owner_sign_in, start_session,
};
use askhava_privacy::api::Vault;
use askhava_server::release_sink::PickReleaser;
use askhava_server::{RouterParts, build_router};
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t304-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t304 owner pass phrase";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t304";
const AUDIT: &str = "/owner/audit-log";
const HEALTH: &str = "/owner/health";

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
    refunds: Arc<AtomicUsize>,
    events: Events,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Reply {
    status: u16,
    text: String,
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x34), &key(0x62), &url).expect("vault")
}

fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    config.pool().clone()
}

/// The fake Stripe's `POST /v1/refunds`: counts the call and answers a full `succeeded` refund.
async fn fake_refund(
    State(refunds): State<Arc<AtomicUsize>>,
    body: String,
) -> Json<serde_json::Value> {
    let intent = body
        .split('&')
        .find_map(|pair| pair.strip_prefix("payment_intent="));
    let n = refunds.fetch_add(1, Ordering::SeqCst);
    Json(serde_json::json!({
        "id": format!("re_FakeT304n{n}"), "status": "succeeded", "amount": 7500,
        "currency": "usd", "payment_intent": intent,
    }))
}

/// A fake Stripe on a local port and the production hooks whose client calls it (D-3844).
async fn stripe_hooks(
    pool: &PgPool,
    refunds: &Arc<AtomicUsize>,
) -> askhava_server::release_sink::StripeHooks {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("port");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let fake = Router::new()
        .route("/v1/refunds", post(fake_refund))
        .with_state(Arc::clone(refunds));
    tokio::spawn(async move { axum::serve(listener, fake).await });
    let none: [(String, String); 0] = [];
    let clock = Arc::new(SystemClock);
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: StripePayments::new(&base, SecretString::from("sk_test_fake304")).expect("client"),
        secret: SecretString::from("whsec_fake_t304"),
        account: StripeAccount {
            live: false,
            account: "acct_fake304".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    })
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let refunds = Arc::new(AtomicUsize::new(0));
    let hooks = stripe_hooks(pool, &refunds).await;
    let parts = RouterParts::from((None, None, None, Some(hooks))).with_confirm_settings(
        "pk_test_fake304".to_owned(),
        Timestamp::now(),
        SecretString::from(ID_SECRET),
    );
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        secret: secret.as_bytes().to_vec(),
        refunds,
        events,
        _guard: guard,
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `at`.
fn totp_code(secret: &[u8], at: Timestamp) -> String {
    let step = u64::try_from(at.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

fn own_jar(jar: &CookieJar) -> Jar {
    jar.iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

fn csrf(jar: &Jar) -> String {
    let mut headers = HeaderMap::new();
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let value = HeaderValue::from_str(&cookies.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, value);
    csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default()
}

/// The owner signed in at `at` with a TOTP code: the factor is fresh for 5 minutes from `at`.
async fn owner_at(w: &World, at: Timestamp) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let code = totp_code(&w.secret, at);
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    own_jar(&jar.expect("owner signed in"))
}

async fn realm_session(w: &World, realm: Realm) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(1);
    let who = (realm, Uuid::new_v4());
    let jar = start_session(&mut db, CookieJar::new(), who, life, Timestamp::now()).await;
    own_jar(&jar.expect("session"))
}

/// A same-origin request with `jar`'s cookies and `body`.
fn request(method: &str, path: &str, jar: &Jar, body: String) -> Request<Body> {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The answer to `request`, which must be `no-store` (SEC-034).
async fn answer(w: &World, request: Request<Body>) -> Reply {
    let what = format!("{} {}", request.method(), request.uri());
    let response = w.app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let cache = parts
        .headers
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    assert!(
        cache.is_some_and(|v| v.contains("no-store")),
        "{what} is no-store (SEC-034)"
    );
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn get(w: &World, path: &str, jar: &Jar) -> Reply {
    answer(w, request("GET", path, jar, String::new())).await
}

/// A same-origin `POST` of `fields` with `jar`'s CSRF token.
async fn post_form(w: &World, path: &str, jar: &Jar, fields: &str) -> Reply {
    answer(
        w,
        request("POST", path, jar, format!("csrf={}{fields}", csrf(jar))),
    )
    .await
}

/// A Stripe that makes one customer and confirms `pick`'s intent `succeeded` at $75.
struct FakeStripe(Uuid);

impl FakeStripe {
    fn customer(&self) -> String {
        format!("cus_fake{}", self.0.simple())
    }

    fn succeeded(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let body = serde_json::json!({
            "id": id, "status": "succeeded", "amount": 7500, "currency": "usd",
            "customer": self.customer(), "metadata": {"pick_id": self.0.to_string()},
            "livemode": false, "latest_charge": format!("ch_fake{}", self.0.simple()),
        });
        Ok(serde_json::from_value(body).expect("a fake intent"))
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok(self.customer())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(id)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        Ok(format!("pi_fake{}", self.0.simple()))
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(intent)
    }
}

/// The $75 introduction fee by job size, fixed as at a pick of a $1,000 quote.
fn medium_fee() -> askhava_payments::api::PickedFee {
    let job_facts = FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    };
    let period = PeriodJob {
        job: job_facts,
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee =
        fee_at_pick(FeeQuote::Fixed(100_000), None, period, FeeMode::Tiers, None).expect("fee");
    assert_eq!(fee.cents(), 7500, "the $75 introduction fee by job size");
    fee
}

/// A fake business's paid $75 pick, charged through `payments`' own API: the business, the pick
/// and its intent.
async fn charged(pool: &PgPool) -> (Uuid, Uuid, String) {
    let (business, pick, job, now) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Timestamp::now(),
    );
    let fee = medium_fee();
    let (id, secret) = (BusinessId(business), SecretString::from(ID_SECRET));
    let mut db = pool.acquire().await.expect("db");
    fix_fee_at_pick(&mut db, pick, id, job, &fee, now)
        .await
        .expect("fee fixed");
    let stripe = FakeStripe(pick);
    ensure_customer(pool, &stripe, &secret, id)
        .await
        .expect("customer");
    let method = format!("pm_fake{}", business.simple());
    let card = SavedCard::new(&method, "card", "visa", "4242").expect("card");
    save_card(&mut db, id, &card, now - SignedDuration::from_mins(1))
        .await
        .expect("saved");
    drop(db);
    let press = Press {
        pick,
        business: id,
        job,
        amount_cents: 7500,
        payment_method: &method,
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let paid = confirm_and_pay(pool, &stripe, &secret, &press, now)
        .await
        .expect("charge");
    let intent = format!("pi_fake{}", pick.simple());
    assert_eq!(paid, ChargeOutcome::Succeeded(intent.clone()));
    (business, pick, intent)
}

/// How many ledger rows of `kind` the business has.
async fn ledger(pool: &PgPool, business: Uuid, kind: EntryKind) -> usize {
    let all = (
        Timestamp::UNIX_EPOCH,
        Timestamp::now() + SignedDuration::from_hours(1),
    );
    let statement = business_statement(pool, business, all)
        .await
        .expect("statement");
    statement.lines.iter().filter(|l| l.kind == kind).count()
}

/// The `security event` log lines captured, `[kind, route]` (SEC-048), as
/// `owner_portal_route.rs` reads them.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<[String; 2]>>>);

#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl tracing::Subscriber for Events {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let get = |name: &str| {
            let found = fields.0.iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap_or_default()
        };
        if get("message") == "security event" {
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push([get("kind"), get("route")]);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn events(w: &World, kind: &str, route: &str) -> usize {
    let all = w.events.0.lock().unwrap_or_else(PoisonError::into_inner);
    all.iter().filter(|e| e[0] == kind && e[1] == route).count()
}

/// TD-SEC-47 (SEC-006, SEC-034, OPS-008, OPS-010): no session and a business session each get
/// the 401 linking to `/owner/sign-in` on every page and press and change nothing; the owner
/// reads the review step, the audit log and the health page. The third realm's session is
/// probed on the same rows by `door_matrix.rs`: this file calls `payments`, so TD-FEE-07 lets it
/// name no such session.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_47_only_the_owner_reaches_the_owner_tools(pool: PgPool) {
    let w = world(&pool).await;
    let (business, pick, _) = charged(&pool).await;
    let step = format!("/owner/refunds/{pick}");
    let visitor = own_jar(&ensure_pre_session(CookieJar::new()).expect("pre-session"));
    let others = [visitor, realm_session(&w, Realm::Business).await];
    for jar in &others {
        let presses = [format!("{step}/refund"), format!("{step}/evidence")];
        let mut replies = Vec::new();
        for path in [step.as_str(), AUDIT, HEALTH] {
            replies.push(get(&w, path, jar).await);
        }
        for path in &presses {
            replies.push(post_form(&w, path, jar, "").await);
        }
        for reply in replies {
            assert_eq!(reply.status, 401, "{}", reply.text);
            assert!(
                reply.text.contains(r#"href="/owner/sign-in""#),
                "{}",
                reply.text
            );
            for words in [
                "Refund review",
                "Audit log rows",
                "System health",
                &pick.to_string(),
            ] {
                assert!(!reply.text.contains(words), "no owner detail: {words}");
            }
        }
    }
    assert_eq!(w.refunds.load(Ordering::SeqCst), 0);
    assert_eq!(ledger(&pool, business, EntryKind::Refund).await, 0);
    let owner = owner_at(&w, Timestamp::now()).await;
    let review = get(&w, &step, &owner).await;
    assert_eq!(review.status, 200, "{}", review.text);
    assert!(
        review.text.contains("Refund this charge"),
        "{}",
        review.text
    );
    let log = get(&w, AUDIT, &owner).await;
    assert_eq!(log.status, 200, "{}", log.text);
    assert!(
        log.text
            .contains(r#"<div role="region" tabindex="0" aria-label="Audit log rows">"#)
    );
    let health = get(&w, HEALTH, &owner).await;
    assert_eq!(health.status, 200, "{}", health.text);
    assert!(
        health.text.contains("Web app running for"),
        "{}",
        health.text
    );
    assert!(
        health.text.contains(r#"aria-label="Required secrets""#),
        "{}",
        health.text
    );
    assert_eq!(
        get(&w, "/owner/refunds/not-a-pick", &owner).await.status,
        404
    );
}

/// TD-SEC-47 (FEE-026, FEE-050, SEC-006, SEC-043, SEC-048, D-2204): a stale factor refunds
/// nothing; a fresh one issues one Stripe refund and one ledger row, a replay nothing more, and
/// the audit log names the press; a disputed charge's refund is blocked, and its evidence press
/// books the countering fee once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_47_a_refund_is_issued_once_and_blocked_on_a_dispute(pool: PgPool) {
    let w = world(&pool).await;
    let (business, pick, _) = charged(&pool).await;
    let press = format!("/owner/refunds/{pick}/refund");
    let stale = owner_at(&w, Timestamp::now() - SignedDuration::from_mins(6)).await;
    let refused = post_form(&w, &press, &stale, "").await;
    assert_eq!(refused.status, 401, "{}", refused.text);
    assert!(
        refused.text.contains(r#"href="/owner/step-up""#),
        "{}",
        refused.text
    );
    assert_eq!(
        events(&w, "authz_denied", &press),
        1,
        "the stale press is a security event"
    );
    assert_eq!(w.refunds.load(Ordering::SeqCst), 0);
    let owner = owner_at(&w, Timestamp::now()).await;
    let done = post_form(&w, &press, &owner, "").await;
    assert_eq!(done.status, 200, "{}", done.text);
    assert!(done.text.contains("Refunded in full."), "{}", done.text);
    let again = post_form(&w, &press, &owner, "").await;
    assert!(
        again.text.contains("Nothing new was sent."),
        "{}",
        again.text
    );
    assert_eq!(w.refunds.load(Ordering::SeqCst), 1, "one Stripe refund");
    assert_eq!(
        ledger(&pool, business, EntryKind::Refund).await,
        1,
        "one ledger row"
    );
    let log = get(&w, AUDIT, &owner).await.text;
    assert_eq!(log.matches("portals.owner_refund").count(), 1, "{log}");

    let (business, pick, intent) = charged(&pool).await;
    let dispute = DisputeOpened {
        dispute: "dp_FakeT304",
        payment_intent: &intent,
        amount_cents: 7500,
        due_at: Timestamp::now() + SignedDuration::from_hours(240),
    };
    assert_eq!(
        record_dispute(&pool, &dispute, Timestamp::now()).await,
        Ok(true)
    );
    let blocked = post_form(&w, &format!("/owner/refunds/{pick}/refund"), &owner, "").await;
    assert_eq!(blocked.status, 400, "{}", blocked.text);
    assert!(
        blocked.text.contains("This charge is disputed"),
        "{}",
        blocked.text
    );
    assert_eq!(
        w.refunds.load(Ordering::SeqCst),
        1,
        "no refund on a disputed charge"
    );
    assert_eq!(ledger(&pool, business, EntryKind::Refund).await, 0);
    let evidence = format!("/owner/refunds/{pick}/evidence");
    let other = post_form(&w, &evidence, &owner, "&dispute=dp_FakeOther").await;
    assert_eq!(other.status, 400, "{}", other.text);
    for line in ["Marked as sent by you.", "That evidence is already in."] {
        let pressed = post_form(&w, &evidence, &owner, "&dispute=dp_FakeT304").await;
        assert_eq!(pressed.status, 200, "{}", pressed.text);
        assert!(pressed.text.contains(line), "{}", pressed.text);
    }
    assert_eq!(
        ledger(&pool, business, EntryKind::CounteringFee).await,
        1,
        "the fee once"
    );
}

/// TD-SEC-47 (SEC-042, SEC-048): on both presses a cross-site request (by `Sec-Fetch-Site` or by
/// `Origin`) or a wrong CSRF token is a 403 `csrf_rejected` event that changes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_47_cross_site_and_wrong_token_posts_change_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let (business, pick, _) = charged(&pool).await;
    let owner = owner_at(&w, Timestamp::now()).await;
    for (route, path) in [
        (
            "/owner/refunds/{pick}/refund",
            format!("/owner/refunds/{pick}/refund"),
        ),
        (
            "/owner/refunds/{pick}/evidence",
            format!("/owner/refunds/{pick}/evidence"),
        ),
    ] {
        let good = format!("csrf={}", csrf(&owner));
        let mut by_site = request("POST", &path, &owner, good.clone());
        by_site
            .headers_mut()
            .insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        let mut by_origin = request("POST", &path, &owner, good);
        by_origin.headers_mut().remove("sec-fetch-site");
        let evil = HeaderValue::from_static("https://evil.example");
        by_origin.headers_mut().insert(header::ORIGIN, evil);
        let by_token = request("POST", &path, &owner, format!("csrf={}", "0".repeat(64)));
        for refused in [by_site, by_origin, by_token] {
            let reply = answer(&w, refused).await;
            assert_eq!(reply.status, 403, "{}", reply.text);
        }
        assert_eq!(
            events(&w, "csrf_rejected", route),
            3,
            "{route}: three security events"
        );
    }
    assert_eq!(w.refunds.load(Ordering::SeqCst), 0);
    assert_eq!(ledger(&pool, business, EntryKind::Refund).await, 0);
}

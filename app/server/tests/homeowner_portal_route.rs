//! TD-PRIV-21 (T307, D-2128, D-3967, D-4025; SEC-008, SEC-014, SEC-034, SEC-042, SEC-046,
//! SEC-048, PRIV-021, CON-035): the homeowner portal's sign-in, home and texts-off routes through
//! the assembled router over PostgreSQL, with the Twilio webhooks' vault and a fake Verify part.
//!
//! Sign-in by phone and texted code, a number not on file getting the same answer; the home shows
//! only her own record; the texts-off switch lists her number at once; another homeowner's proof
//! reads only her own facts; none, a business or owner session and an ended one get a 401 linking
//! to `/h/sign-in` with no detail; every answer is `no-store`; a cross-site `POST` (by
//! `Sec-Fetch-Site` or `Origin`) or a wrong CSRF token is a 403, changes nothing and is logged as a
//! `csrf_rejected` security event; a send past `admit_code_send`'s limit sends nothing and a check
//! past `admit_code_try`'s limit calls no Verify; the code page never waits on Verify (TD-PICK-36,
//! D-4013), so the cases wait for the send before checking a code. The `/h/my-data` cases are T561's. Fake data only.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{Settings, TestControl};
use askhava_messaging::api::texts_on;
use askhava_portals::api::{
    CookieJar, HomeFacts, Realm, TEXTS_OFF_LINE, csrf_token, ensure_pre_session, start_session,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_server::homeowner_portal::HomeownerVerify;
use askhava_server::{RouterParts, build_router, session_minter, webhooks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// Her fake number, address, and the right fake code.
const HER: (&str, &str) = ("+19285550111", "12 Example Way");
/// Another homeowner's fake number and address.
const OTHER: (&str, &str) = ("+19285550122", "34 Sample Road");
const CODE: &str = "123456";

/// A Verify that counts its sends and checks; only [`CODE`] is right. A `stalled` one never
/// finishes a send, as a hung Verify would.
#[derive(Clone, Default)]
struct FakeVerify {
    sends: Arc<AtomicUsize>,
    checks: Arc<AtomicUsize>,
    stalled: bool,
}

impl FakeVerify {
    /// Waits (5 s at most) until `n` sends were made: the route sends after it answers (D-4013).
    async fn sent(&self, n: usize) {
        for _ in 0..500 {
            if self.sends.load(Ordering::SeqCst) >= n {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(self.sends.load(Ordering::SeqCst), n);
    }

    /// Lets a spawned send finish (500 ms, far past the time `sent` sees an admitted send
    /// take), then checks exactly `n` sends were made: proves a send did not happen (D-4013).
    async fn quiet(&self, n: usize) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(self.sends.load(Ordering::SeqCst), n);
    }
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.stalled {
            std::future::pending::<()>().await;
        }
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(code == CODE)
    }
}

fn vault_key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// Dev settings with the Twilio webhooks configured from fake values.
fn pairs() -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-0001",
        ),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
    ];
    let mut pairs: Vec<(String, String)> = fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    pairs.push(("VAULT_KEY".to_owned(), vault_key(0x51)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), vault_key(0x52)));
    pairs
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x52), &url).expect("vault")
}

/// The assembled router with the webhooks and `verify` as its homeowner Verify part.
fn app(pool: &PgPool, verify: &FakeVerify) -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    assert!(hooks.is_some(), "the webhooks are configured");
    let parts = RouterParts::from((hooks, None, None, None))
        .with_homeowner_verify(Some(HomeownerVerify::new(verify.clone())));
    build_router(pool.clone(), control.router(), parts)
}

/// A homeowner contact with `(phone, address)`.
async fn contact(pool: &PgPool, (phone, address): (&str, &str)) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        address: Some(SecretString::from(address.to_owned())),
        ..NewContact::default()
    };
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

/// A browser's jar with a `realm` session on `actor`, ending `life` from now.
async fn session(pool: &PgPool, (realm, actor): (Realm, Uuid), life: SignedDuration) -> CookieJar {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now() - SignedDuration::from_hours(1);
    start_session(&mut db, CookieJar::new(), (realm, actor), life, now)
        .await
        .expect("session")
}

/// Where a request comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    CrossFetch,
    CrossOrigin,
    WrongToken,
}

/// One answer: status, body plus `Location`, and the jar after its cookies.
struct Reply {
    status: u16,
    text: String,
    jar: CookieJar,
}

/// The `name=value` cookies of `jar` with `set` (a `Set-Cookie` list) applied.
fn cookies(jar: &CookieJar, set: &HeaderMap) -> CookieJar {
    let mut pairs: Vec<(String, String)> = jar
        .iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect();
    for value in set.get_all(header::SET_COOKIE) {
        let first = value
            .to_str()
            .expect("cookie")
            .split(';')
            .next()
            .expect("pair");
        let (name, value) = first.split_once('=').expect("name=value");
        pairs.retain(|(n, _)| n != name);
        if !value.is_empty() {
            pairs.push((name.to_owned(), value.to_owned()));
        }
    }
    let line: Vec<String> = pairs.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let mut headers = HeaderMap::new();
    if !line.is_empty() {
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&line.join("; ")).expect("h"),
        );
    }
    CookieJar::from_headers(&headers)
}

/// `GET path` (no form) or `POST path` with `form` plus the jar's CSRF token, from `site`.
fn request(path: &str, jar: &CookieJar, (form, site): (Option<&str>, Site)) -> Request<Body> {
    let cookie: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut builder = Request::builder()
        .uri(path)
        .header(header::HOST, "askhava.example")
        .header(header::COOKIE, cookie.join("; "));
    builder = match site {
        Site::Own | Site::WrongToken => builder.header("sec-fetch-site", "same-origin"),
        Site::CrossFetch => builder.header("sec-fetch-site", "cross-site"),
        Site::CrossOrigin => builder.header(header::ORIGIN, "https://elsewhere.example"),
    };
    let request = match form {
        None => builder.method("GET").body(Body::empty()),
        Some(form) => {
            let token = match site {
                Site::WrongToken => "0".repeat(64),
                _ => csrf_token(jar).unwrap_or_default(),
            };
            builder
                .method("POST")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf={token}{form}")))
        }
    };
    request.expect("request")
}

/// [`request`]'s answer from `app`; every answer must be `no-store` (SEC-034).
async fn call(app: &Router, path: &str, jar: &CookieJar, how: (Option<&str>, Site)) -> Reply {
    let sent = request(path, jar, how);
    let response = app.clone().oneshot(sent).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cache = parts
        .headers
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    assert_eq!(cache, Some("no-store"), "{path} is no-store (SEC-034)");
    let location = parts
        .headers
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok());
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&bytes),
            location.unwrap_or_default()
        ),
        jar: cookies(jar, &parts.headers),
    }
}

/// A fresh browser on the sign-in page: its jar holds the pre-session cookie.
async fn browser(app: &Router) -> CookieJar {
    let page = call(app, "/h/sign-in", &CookieJar::new(), (None, Site::Own)).await;
    assert_eq!(page.status, 200);
    assert!(page.text.contains("action=\"/h/sign-in\""));
    page.jar
}

/// `phone` typed into the sign-in form, form-encoded.
fn phone_field(phone: &str) -> String {
    format!("&phone={}", phone.replace('+', "%2B"))
}

/// Signs in as `phone` through both steps with the right code; the jar after.
async fn sign_in(app: &Router, verify: &FakeVerify, phone: &str) -> CookieJar {
    let jar = browser(app).await;
    let before = verify.sends.load(Ordering::SeqCst);
    let sent = call(
        app,
        "/h/sign-in",
        &jar,
        (Some(&phone_field(phone)), Site::Own),
    )
    .await;
    assert_eq!(sent.status, 200);
    verify.sent(before + 1).await;
    let form = format!("{}&code={CODE}", phone_field(phone));
    let done = call(app, "/h/sign-in/code", &sent.jar, (Some(&form), Site::Own)).await;
    assert_eq!((done.status, done.text.as_str()), (303, "/h"));
    done.jar
}

/// The `kind` of every "security event" log line while it is the default subscriber (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    fn count(&self, kind: &str) -> usize {
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().filter(|k| *k == kind).count()
    }
}

/// One log event's `message` and `kind`.
#[derive(Default)]
struct Fields(Option<String>, Option<String>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record_debug(field, &value);
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}").trim_matches('"').to_owned();
        match field.name() {
            "message" => self.0 = Some(value),
            "kind" => self.1 = Some(value),
            _ => {}
        }
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
        if let (Some("security event"), Some(kind)) = (fields.0.as_deref(), fields.1) {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(kind);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// TD-PRIV-21 (SEC-008): a number on file gets one code and the same page as a number not on
/// file, which gets none; the right code signs her in to her own home, a wrong one does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_sign_in_by_texted_code_and_the_same_answer_for_any_number(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    contact(&pool, HER).await;
    let jar = browser(&app).await;
    let known = call(
        &app,
        "/h/sign-in",
        &jar,
        (Some(&phone_field(HER.0)), Site::Own),
    )
    .await;
    verify.sent(1).await;
    let unknown = call(
        &app,
        "/h/sign-in",
        &jar,
        (Some(&phone_field(OTHER.0)), Site::Own),
    )
    .await;
    verify.quiet(1).await;
    assert_eq!((known.status, unknown.status), (200, 200));
    assert_eq!(
        known.text.replace(HER.0, ""),
        unknown.text.replace(OTHER.0, "")
    );
    let wrong = format!("{}&code=654321", phone_field(HER.0));
    let refused = call(
        &app,
        "/h/sign-in/code",
        &known.jar,
        (Some(&wrong), Site::Own),
    )
    .await;
    assert_eq!(refused.status, 400);
    assert!(refused.text.contains("That code didn't work"));
    let jar = sign_in(&app, &verify, HER.0).await;
    let home = call(&app, "/h", &jar, (None, Site::Own)).await;
    assert_eq!(home.status, 200);
    assert!(home.text.contains("ending in 0111") && home.text.contains(HER.1));
}

/// TD-PRIV-21 (PRIV-021, SEC-046, PRIV-016): each home shows only her own record, and another
/// homeowner's proof reads only that homeowner's facts, whose `Debug` prints none of them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_the_home_shows_only_her_own_record(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let (her, other) = (contact(&pool, HER).await, contact(&pool, OTHER).await);
    let life = SignedDuration::from_hours(24);
    let jar = session(&pool, (Realm::Homeowner, other), life).await;
    let page = call(&app, "/h", &jar, (None, Site::Own)).await;
    assert_eq!(page.status, 200);
    assert!(page.text.contains("ending in 0122") && page.text.contains(OTHER.1));
    assert!(!page.text.contains("0111") && !page.text.contains(HER.1));
    let mut db = pool.acquire().await.expect("db");
    let proof = session_minter().homeowner(other);
    let facts = HomeFacts::read(&mut db, &vault(), &proof, false).await;
    let facts = facts.expect("read").expect("a phone on file");
    assert_eq!(facts.phone_last4, "0122");
    assert_eq!(facts.address.as_deref(), Some(OTHER.1));
    let debug = format!("{facts:?}");
    assert!(!debug.contains("0122") && !debug.contains("Sample"));
    let mine = HomeFacts::read(&mut db, &vault(), &session_minter().homeowner(her), true).await;
    assert_eq!(
        mine.expect("read").map(|f| (f.phone_last4, f.spanish)),
        Some(("0111".to_owned(), true))
    );
}

/// TD-PRIV-21 (CON-035): the switch puts her number on the suppression list at once and confirms
/// on screen; the home then shows no switch.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_texts_off_lists_her_number_at_once(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let her = contact(&pool, HER).await;
    let jar = session(
        &pool,
        (Realm::Homeowner, her),
        SignedDuration::from_hours(24),
    )
    .await;
    let on = call(&app, "/h", &jar, (None, Site::Own)).await;
    assert!(on.text.contains("action=\"/h/texts-off\""));
    let off = call(&app, "/h/texts-off", &jar, (Some(""), Site::Own)).await;
    let confirmed = TEXTS_OFF_LINE
        .split_once('.')
        .map_or(TEXTS_OFF_LINE, |(first, _)| first);
    assert_eq!(off.status, 200);
    assert!(off.text.contains(confirmed) && !off.text.contains("action=\"/h/texts-off\""));
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(texts_on(&mut db, &vault(), HER.0).await, Ok(false));
    assert_eq!(
        verify.sends.load(Ordering::SeqCst),
        0,
        "no text confirms it"
    );
}

/// TD-PRIV-21 (SEC-008, SEC-034): none, a business or owner session and an ended homeowner
/// session each get a 401 linking to `/h/sign-in` with no detail, on the home and the switch,
/// and the switch changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_no_homeowner_session_gets_the_sign_in_link_and_no_detail(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let her = contact(&pool, HER).await;
    let day = SignedDuration::from_hours(24);
    let jars = [
        ensure_pre_session(CookieJar::new()).expect("pre-session"),
        session(&pool, (Realm::Business, Uuid::new_v4()), day).await,
        session(&pool, (Realm::Owner, Uuid::new_v4()), day).await,
        session(
            &pool,
            (Realm::Homeowner, her),
            SignedDuration::from_mins(30),
        )
        .await,
    ];
    for jar in &jars {
        for (path, form) in [("/h", None), ("/h/texts-off", Some(""))] {
            let reply = call(&app, path, jar, (form, Site::Own)).await;
            assert_eq!(reply.status, 401, "{path}");
            assert!(reply.text.contains("href=\"/h/sign-in\""), "{path}");
            assert!(
                !reply.text.contains("0111") && !reply.text.contains(HER.1),
                "{path}"
            );
        }
    }
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(texts_on(&mut db, &vault(), HER.0).await, Ok(true));
}

/// TD-PRIV-21 (SEC-042, SEC-048): a cross-site `POST` by `Sec-Fetch-Site` or by `Origin`, or one
/// with a wrong CSRF token, is a 403 that changes nothing, sends nothing and is logged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_cross_site_and_wrong_token_posts_are_refused_and_logged(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let her = contact(&pool, HER).await;
    let jar = session(
        &pool,
        (Realm::Homeowner, her),
        SignedDuration::from_hours(24),
    )
    .await;
    let anonymous = browser(&app).await;
    let code = format!("{}&code={CODE}", phone_field(HER.0));
    let posts = [
        ("/h/texts-off", &jar, String::new()),
        ("/h/sign-in", &anonymous, phone_field(HER.0)),
        ("/h/sign-in/code", &anonymous, code),
    ];
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let mut logged = 0;
    for (path, jar, form) in &posts {
        for site in [Site::CrossFetch, Site::CrossOrigin, Site::WrongToken] {
            let reply = call(&app, path, jar, (Some(form), site)).await;
            assert_eq!(reply.status, 403, "{path}");
            let now = events.count("csrf_rejected");
            assert_eq!(now, logged + 1, "{path} logged once");
            logged = now;
        }
    }
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(texts_on(&mut db, &vault(), HER.0).await, Ok(true));
    assert_eq!(verify.sends.load(Ordering::SeqCst), 0);
    assert_eq!(verify.checks.load(Ordering::SeqCst), 0);
}

/// TD-PRIV-21 (SEC-014, D-2206): unknown numbers spend the IP's send limit and get nothing; past
/// it a number on file gets the same page and no code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_a_send_past_the_limit_sends_nothing(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    contact(&pool, HER).await;
    let jar = browser(&app).await;
    for n in 0..10 {
        let phone = format!("+1928555{:04}", 200 + n);
        let reply = call(
            &app,
            "/h/sign-in",
            &jar,
            (Some(&phone_field(&phone)), Site::Own),
        )
        .await;
        assert_eq!(reply.status, 200);
    }
    let past = call(
        &app,
        "/h/sign-in",
        &jar,
        (Some(&phone_field(HER.0)), Site::Own),
    )
    .await;
    assert_eq!(past.status, 200);
    assert!(past.text.contains("If we have that number"));
    verify.quiet(0).await;
}

/// TD-PRIV-21 (SEC-014, SEC-020, D-2206): past the IP's try limit even the right code after a
/// real send calls no Verify and signs nobody in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_a_check_past_the_limit_calls_no_verify(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    contact(&pool, HER).await;
    let jar = browser(&app).await;
    let unknown = format!("{}&code={CODE}", phone_field(OTHER.0));
    for _ in 0..20 {
        let reply = call(&app, "/h/sign-in/code", &jar, (Some(&unknown), Site::Own)).await;
        assert_eq!(reply.status, 400);
    }
    let sent = call(
        &app,
        "/h/sign-in",
        &jar,
        (Some(&phone_field(HER.0)), Site::Own),
    )
    .await;
    verify.sent(1).await;
    let right = format!("{}&code={CODE}", phone_field(HER.0));
    let past = call(
        &app,
        "/h/sign-in/code",
        &sent.jar,
        (Some(&right), Site::Own),
    )
    .await;
    assert_eq!(past.status, 400);
    assert_eq!(verify.checks.load(Ordering::SeqCst), 0);
    let home = call(&app, "/h", &past.jar, (None, Site::Own)).await;
    assert_eq!(home.status, 401);
}

/// TD-PICK-36 (D-1954, D-4013): the code page never waits on Verify, so a number on file answers
/// as fast as one that is not; a Verify that never finishes still gets the page at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_36_the_code_page_does_not_wait_on_verify(pool: PgPool) {
    let verify = FakeVerify {
        stalled: true,
        ..FakeVerify::default()
    };
    let app = app(&pool, &verify);
    contact(&pool, HER).await;
    let jar = browser(&app).await;
    let form = phone_field(HER.0);
    let sent = call(&app, "/h/sign-in", &jar, (Some(&form), Site::Own));
    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), sent).await;
    let reply = reply.expect("the code page waited on Verify");
    assert_eq!(reply.status, 200);
    assert!(reply.text.contains("If we have that number"));
    verify.sent(1).await;
}

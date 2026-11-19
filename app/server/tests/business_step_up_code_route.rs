//! T364 (D-3744, D-4045; SEC-004, SEC-014, SEC-020, SEC-038): the business step-up code page
//! served by the assembled router over PostgreSQL with a fake Verify. A second send with no token
//! reuses the member's live record and its tries go on from the first send's count (a sixth try
//! across both is refused before Verify); two simultaneous sends mint one record, neither answers
//! 500 and Verify sends once; a right code passes `require_step_up` and works once; a send past
//! the per-member or per-IP limit is refused and sends nothing. People sign in through `portals`'
//! emailed-link sign-in, as `business_portal_route.rs` does. Every ID, address and code is fake.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{add_owner, create};
use askhava_core::api::{MIGRATOR, SystemClock};
use askhava_portals::api::{
    BUSINESS_STEP_UP_CHECK_ROUTE, BUSINESS_STEP_UP_ROUTE, BUSINESS_STEP_UP_SEND_ROUTE, CodeAddress,
    CodeFuture, CodeTo, CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SensitiveChange,
    SignIn, SignInAddress, SignInMail, StepUpCodes, business_principal, csrf_token,
    ensure_pre_session, require_step_up, signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const RIGHT: &str = "246810";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

/// A fake Verify counting its sends and checks; only [`RIGHT`] is right.
#[derive(Clone, Default)]
struct FakeVerify {
    sends: Arc<AtomicUsize>,
    checks: Arc<AtomicUsize>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok("VE0000".to_owned())
    }

    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(code == RIGHT)
    }
}

/// Every member's code goes to one fake phone.
struct FakeAddress;

impl CodeAddress for FakeAddress {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        _: sqlx::types::Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        Box::pin(async { Ok(CodeTo::Phone(SecretString::from("+15555550123".to_owned()))) })
    }
}

fn app(pool: &PgPool, verify: &FakeVerify) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let secret = SecretString::from("fake-id-secret-0123456789abcdef".to_owned());
    let codes = StepUpCodes::new(verify.clone(), FakeAddress, secret);
    let parts = RouterParts::from(hooks).with_step_up(Some(codes));
    build_router(pool.clone(), Router::new(), parts)
}

fn request(path: &str, jar: &CookieJar, body: &str, ip: u8) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, ip], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status, the cookies set and the body text.
async fn answer(app: &Router, request: Request<Body>) -> (u16, Vec<String>, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    (
        parts.status.as_u16(),
        cookies,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// The one sign-in address of one person; the link it "emails" is kept.
struct OneMail {
    to: SignInAddress,
    sent: Mutex<Option<String>>,
}

impl SignInMail for OneMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(vec![self.to])
    }

    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let token = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = token;
        Ok(())
    }
}

/// A new active business's owner at `address`, signed in through the emailed link.
async fn owner(pool: &PgPool, address: &str) -> CookieJar {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let business = create(&mut db, false, now).await.expect("business");
    add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let mail = Arc::new(OneMail {
        to: SignInAddress { business, mailbox },
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let signin =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    answer(
        &signin,
        request(SIGNIN_ROUTE, &jar, "&email=person%40x.example", 1),
    )
    .await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let (_, cookies, _) = answer(&signin, request(&path, &jar, "", 1)).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

async fn send(app: &Router, jar: &CookieJar, ip: u8) -> (u16, String) {
    let (status, _, text) = answer(
        app,
        request(BUSINESS_STEP_UP_SEND_ROUTE, jar, "&return=%2Fb%2Fteam", ip),
    )
    .await;
    (status, text)
}

async fn check(app: &Router, jar: &CookieJar, code: &str) -> (u16, String) {
    let body = format!("&return=%2Fb%2Fteam&code={code}");
    let (status, _, text) = answer(app, request(BUSINESS_STEP_UP_CHECK_ROUTE, jar, &body, 9)).await;
    (status, text)
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_second_send_reuses_the_record_and_its_tries(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let jar = owner(&pool, "owner@x.example").await;
    let mut get = request(
        &format!("{BUSINESS_STEP_UP_ROUTE}?return=%2F%2Fevil.example"),
        &jar,
        "",
        2,
    );
    *get.method_mut() = Method::GET;
    let (status, _, page) = answer(&app, get).await;
    assert!(
        status == 200 && page.contains("Send me a code") && !page.contains("evil"),
        "{page}"
    );
    assert_eq!(send(&app, &jar, 2).await.0, 200);
    for _ in 0..3 {
        assert!(
            check(&app, &jar, "111111")
                .await
                .1
                .contains("error-summary")
        );
    }
    let (status, again) = send(&app, &jar, 2).await;
    assert!(status == 200 && again.contains("one-time-code"), "{again}");
    assert_eq!(
        verify.sends.load(Ordering::SeqCst),
        1,
        "the reused record sends nothing again"
    );
    assert_eq!(check(&app, &jar, "111111").await.0, 200);
    assert_eq!(
        check(&app, &jar, "111111").await.0,
        429,
        "the fifth wrong try uses it up"
    );
    assert_eq!(
        check(&app, &jar, RIGHT).await.0,
        429,
        "a sixth try is refused"
    );
    assert_eq!(
        verify.checks.load(Ordering::SeqCst),
        5,
        "the sixth try never reached Verify"
    );
    assert_eq!(
        send(&app, &jar, 2).await.0,
        429,
        "a used-up record refuses a send until it ends"
    );
    assert_eq!(verify.sends.load(Ordering::SeqCst), 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn two_simultaneous_sends_mint_one_record_and_send_once(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let jar = owner(&pool, "owner@x.example").await;
    let (a, b) = tokio::join!(send(&app, &jar, 3), send(&app, &jar, 4));
    assert!(a.0 == 200 && b.0 == 200, "{} {}", a.0, b.0);
    assert_eq!(verify.sends.load(Ordering::SeqCst), 1);
    for _ in 0..5 {
        check(&app, &jar, "111111").await;
    }
    send(&app, &jar, 3).await;
    assert_eq!(
        check(&app, &jar, RIGHT).await.0,
        429,
        "one record: five tries in all"
    );
    assert_eq!(verify.checks.load(Ordering::SeqCst), 5);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_right_code_records_the_step_up_once(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let jar = owner(&pool, "owner@x.example").await;
    let mut db = pool.acquire().await.expect("db");
    let who = business_principal(&mut db, &jar, Timestamp::now())
        .await
        .expect("read")
        .expect("member");
    let change = (SensitiveChange::TeamRole, "/b/team");
    assert!(
        require_step_up(&mut db, &who, change, Timestamp::now())
            .await
            .is_err()
    );
    send(&app, &jar, 5).await;
    let (status, done) = check(&app, &jar, RIGHT).await;
    assert!(
        status == 200 && done.contains("step-up-done") && done.contains("href=\"/b/team\""),
        "{done}"
    );
    assert!(
        require_step_up(&mut db, &who, change, Timestamp::now())
            .await
            .is_ok()
    );
    let (_, again) = check(&app, &jar, RIGHT).await;
    assert!(
        again.contains("Send me a code"),
        "the used record checks nothing: {again}"
    );
    assert_eq!(verify.checks.load(Ordering::SeqCst), 1);
    let (_, fresh) = send(&app, &jar, 5).await;
    assert!(fresh.contains("step-up-fresh"), "{fresh}");
    assert_eq!(verify.sends.load(Ordering::SeqCst), 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn sends_past_the_member_or_ip_limit_are_refused(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let one = owner(&pool, "one@x.example").await;
    for ip in 10..15 {
        assert_eq!(send(&app, &one, ip).await.0, 200);
    }
    assert_eq!(
        send(&app, &one, 15).await.0,
        429,
        "a sixth send for one member today"
    );
    let two = owner(&pool, "two@x.example").await;
    let three = owner(&pool, "three@x.example").await;
    for jar in [
        &two, &two, &two, &two, &two, &three, &three, &three, &three, &three,
    ] {
        assert_eq!(send(&app, jar, 20).await.0, 200);
    }
    let four = owner(&pool, "four@x.example").await;
    assert_eq!(
        send(&app, &four, 20).await.0,
        429,
        "an eleventh send from one IP this hour"
    );
    assert_eq!(send(&app, &four, 21).await.0, 200);
    assert_eq!(verify.sends.load(Ordering::SeqCst), 4);
}

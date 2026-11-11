//! Business sign-in's caps are atomic admissions (TD-SEC-38; SEC-004, SEC-014, SEC-020,
//! SEC-048; T272, D-1837, D-2349, D-2635, AVAIL-01).
//!
//! Drives the sign-in routes with `SignIn::with_device_codes` over a fake Verify whose send and
//! check can answer, fail, time out or hang, against PostgreSQL over several pooled connections:
//! simultaneous link requests, code presses and code checks each pass their cap once, a try a
//! failure, timeout or crash cut short is still used, no transaction is open and no pooled
//! connection is held while Verify hangs. Every address, number, ID and code is fake.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create};
use askhava_core::api::ManualClock;
use askhava_portals::api::{
    CODE_TRIES, CodeAddress, CodeFuture, CodeTo, DeviceCodes, LINKS_PER_LIFE, PortalsError,
    SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail, signin_form, signin_open,
    signin_post, signin_request,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use axum::routing::get;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::task::JoinSet;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const CODE: &str = "246810";
const WRONG: &str = "&code=111111";
const RIGHT: &str = "&code=246810";

/// What the fake Verify does with the next send or check.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Answer,
    Fail,
    Timeout,
    Hang,
}

/// The fake Verify's switches and how often each call was reached.
struct Fake {
    send: Mutex<Mode>,
    check: Mutex<Mode>,
    sends: AtomicUsize,
    checks: AtomicUsize,
}

impl Fake {
    fn new() -> Self {
        Self {
            send: Mutex::new(Mode::Answer),
            check: Mutex::new(Mode::Answer),
            sends: AtomicUsize::new(0),
            checks: AtomicUsize::new(0),
        }
    }

    fn set(&self, send: Mode, check: Mode) {
        *self.send.lock().unwrap_or_else(PoisonError::into_inner) = send;
        *self.check.lock().unwrap_or_else(PoisonError::into_inner) = check;
    }
}

async fn act<T>(mode: Mode, value: T) -> Result<T, AdapterError> {
    match mode {
        Mode::Answer => Ok(value),
        Mode::Fail => Err(AdapterError::Vendor {
            status: 500,
            code: None,
        }),
        Mode::Timeout => Err(AdapterError::OutcomeUnknown),
        Mode::Hang => std::future::pending().await,
    }
}

struct FakeVerify(Arc<Fake>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.sends.fetch_add(1, Ordering::SeqCst);
        let mode = *self.0.send.lock().unwrap_or_else(PoisonError::into_inner);
        act(mode, "VEfake".to_owned()).await
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.checks.fetch_add(1, Ordering::SeqCst);
        let mode = *self.0.check.lock().unwrap_or_else(PoisonError::into_inner);
        act(mode, code == CODE).await
    }
}

struct FakeCodeTo;

impl CodeAddress for FakeCodeTo {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        _: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = CodeTo::Phone(SecretString::from("+15005550101".to_owned()));
        Box::pin(async move { Ok(to) })
    }
}

/// Two team members' addresses; the links "emailed", in order.
struct FakeMail {
    book: Vec<(&'static str, SignInAddress)>,
    sent: Mutex<Vec<String>>,
}

impl SignInMail for FakeMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        address: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        let found = self.book.iter().filter(|(a, _)| *a == address);
        Ok(found.map(|(_, to)| *to).collect())
    }
    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let path = link.expose_secret().replace("https://askhava.com", "");
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push(path);
        Ok(())
    }
}

struct World {
    app: Router,
    mail: Arc<FakeMail>,
    fake: Arc<Fake>,
    clock: Arc<ManualClock>,
    owner: Uuid,
}

fn start() -> Timestamp {
    "2026-11-07T17:00:00Z".parse().expect("time")
}

/// A pool of `size` connections on the test's database.
async fn pooled(pool: &PgPool, size: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(size)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("pool")
}

/// The sign-in routes over a pool of `size`, for an owner and a crew member of one business.
async fn world(pool: &PgPool, size: u32) -> World {
    let mut db = pool.acquire().await.expect("db");
    let business = create(&mut db, false, start()).await.expect("business");
    let (box_a, box_b) = (Uuid::from_u128(0xa1), Uuid::from_u128(0xa2));
    let owner = add_owner(&mut db, business, box_a, start())
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let roles = Roles::new(true, false).expect("roles");
    add_member(&mut db, by, box_b, roles, start())
        .await
        .expect("crew");
    let at = |mailbox| SignInAddress { business, mailbox };
    let book = vec![
        ("owner@x.example", at(box_a)),
        ("crew@x.example", at(box_b)),
    ];
    let mail = Arc::new(FakeMail {
        book,
        sent: Mutex::default(),
    });
    let fake = Arc::new(Fake::new());
    let key = SecretString::from("fake-key-for-tests-only-0123456789".to_owned());
    let codes = DeviceCodes::new(FakeVerify(Arc::clone(&fake)), FakeCodeTo, key);
    let clock = Arc::new(ManualClock::new(start()));
    let small = pooled(pool, size).await;
    let state =
        SignIn::new(small, Arc::clone(&clock) as _, Arc::clone(&mail)).with_device_codes(codes);
    let routes = Router::new()
        .route(
            SIGNIN_ROUTE,
            get(signin_form).post(signin_request::<FakeMail>),
        )
        .route(
            SIGNIN_LINK_ROUTE,
            get(signin_open::<FakeMail>).post(signin_post::<FakeMail>),
        );
    World {
        app: state.into_router(routes),
        mail,
        fake,
        clock,
        owner,
    }
}

/// One request; the response's cookies and body.
async fn call(
    app: Router,
    method: &str,
    uri: &str,
    cookie: String,
    form: String,
) -> (Vec<String>, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let cookies = response.headers().get_all(header::SET_COOKIE).iter();
    let cookies = cookies
        .map(|c| c.to_str().expect("ascii").to_owned())
        .collect();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (cookies, String::from_utf8_lossy(&body).into_owned())
}

/// One browser: its cookies and its form's CSRF token, ready for many posts at once.
#[derive(Default, Clone)]
struct Browser {
    cookies: BTreeMap<String, String>,
    csrf: String,
}

impl Browser {
    fn line(&self) -> String {
        let pairs: Vec<String> = self
            .cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect();
        pairs.join("; ")
    }

    async fn send(&mut self, app: &Router, method: &str, uri: &str, form: String) -> String {
        let (set, body) = call(app.clone(), method, uri, self.line(), form).await;
        for cookie in set {
            let pair = cookie.split(';').next().unwrap_or_default();
            let (name, value) = pair.split_once('=').expect("pair");
            self.cookies.insert(name.to_owned(), value.to_owned());
        }
        if let Some(csrf) = body.split("name=\"csrf\" value=\"").nth(1) {
            self.csrf = csrf.split('"').next().unwrap_or_default().to_owned();
        }
        body
    }

    /// Opens `uri` (for the CSRF token) and posts `fields` to `action`.
    async fn press(&mut self, app: &Router, uri: &str, action: &str, fields: &str) -> String {
        self.send(app, "GET", uri, String::new()).await;
        let form = format!("csrf={}{fields}", self.csrf);
        self.send(app, "POST", action, form).await
    }

    /// `n` simultaneous posts of `fields` to `action`; their bodies.
    async fn many(&self, app: &Router, n: usize, action: &str, fields: &str) -> Vec<String> {
        let mut set = JoinSet::new();
        for _ in 0..n {
            let (app, cookie) = (app.clone(), self.line());
            let (action, form) = (action.to_owned(), format!("csrf={}{fields}", self.csrf));
            set.spawn(async move { call(app, "POST", &action, cookie, form).await.1 });
        }
        set.join_all().await
    }
}

/// Asks for a sign-in link for `address`; the last link emailed.
async fn link_for(w: &World, address: &str) -> String {
    let field = format!("&email={address}");
    Browser::default()
        .press(&w.app, SIGNIN_ROUTE, SIGNIN_ROUTE, &field)
        .await;
    let sent = w.mail.sent.lock().unwrap_or_else(PoisonError::into_inner);
    sent.last().expect("a link").clone()
}

fn emailed(w: &World) -> usize {
    w.mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Transactions open but idle on this test's database: one waiting on Verify would show here.
async fn idle_in_transaction(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
         AND state LIKE 'idle in transaction%'",
    )
    .await
}

/// Waits until `sql` counts `n`.
async fn until(pool: &PgPool, sql: &'static str, n: i64) {
    for _ in 0..500 {
        if count(pool, sql).await == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never reached {n}: {sql}");
}

/// TD-SEC-38 (D-1725, D-1837): 20 simultaneous link requests for a member with one link left in
/// the 15 minutes store exactly one link and send one email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_twenty_link_requests_with_one_left_send_one(pool: PgPool) {
    let w = world(&pool, 8).await;
    for _ in 1..LINKS_PER_LIFE {
        link_for(&w, "owner@x.example").await;
    }
    let mut browser = Browser::default();
    browser
        .send(&w.app, "GET", SIGNIN_ROUTE, String::new())
        .await;
    let replies = browser
        .many(&w.app, 20, SIGNIN_ROUTE, "&email=owner@x.example")
        .await;
    assert!(replies.iter().all(|r| r.contains("Check your email")));
    assert_eq!(emailed(&w), 5, "one email for the one link left");
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM portals.login_link WHERE member_id = $1")
            .bind(w.owner)
            .fetch_one(&pool)
            .await
            .expect("links");
    assert_eq!(links, LINKS_PER_LIFE, "exactly one link stored");
}

/// TD-SEC-38 (D-2635): a new-device sign-in's remembered-device row is not a link sent, so the
/// member then gets all 5 links in the next 15 minutes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_new_device_sign_in_then_five_links_all_send(pool: PgPool) {
    let w = world(&pool, 4).await;
    let link = link_for(&w, "owner@x.example").await;
    w.clock.advance(SignedDuration::from_mins(14));
    let mut phone = Browser::default();
    phone.press(&w.app, &link, &link, "").await;
    let done = phone.press(&w.app, &link, &link, RIGHT).await;
    assert!(done.contains("You're signed in"));
    w.clock.advance(SignedDuration::from_mins(2));
    for _ in 0..=LINKS_PER_LIFE {
        link_for(&w, "owner@x.example").await;
    }
    assert_eq!(emailed(&w), 6, "the first link, then 5 of 6 requests");
}

/// TD-SEC-38 (D-1777, D-1837, SEC-048): 20 simultaneous checks on a link with one try left reach
/// Verify once; each wrong check is one `auth_failed` event; the link has then stopped working.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_twenty_checks_with_one_try_left_reach_verify_once(pool: PgPool) {
    let w = world(&pool, 8).await;
    let link = link_for(&w, "owner@x.example").await;
    let mut phone = Browser::default();
    phone.press(&w.app, &link, &link, "").await;
    for _ in 1..CODE_TRIES {
        assert!(
            phone
                .press(&w.app, &link, &link, WRONG)
                .await
                .contains("didn't work")
        );
    }
    phone.many(&w.app, 20, &link, WRONG).await;
    assert_eq!(
        w.fake.checks.load(Ordering::SeqCst),
        5,
        "the last try reached Verify once"
    );
    let tries = count(&pool, "SELECT count(*) FROM portals.code_try").await;
    assert_eq!(tries, CODE_TRIES);
    let events = "SELECT count(*) FROM portals.security_event \
                  WHERE kind = 'auth_failed' AND actor_ref IS NOT NULL";
    assert_eq!(
        count(&pool, events).await,
        CODE_TRIES,
        "one event per wrong check"
    );
    let late = phone.press(&w.app, &link, &link, RIGHT).await;
    assert!(late.contains("This sign-in link has stopped working"));
    assert_eq!(w.fake.checks.load(Ordering::SeqCst), 5);
}

/// TD-SEC-38 (D-1837): a Verify failure, a timeout and a crash between reservation and check each
/// use a try, and no transaction is open while the check hangs; the fifth used try ends the link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_failure_timeout_and_crash_each_use_a_try(pool: PgPool) {
    let w = world(&pool, 4).await;
    let link = link_for(&w, "owner@x.example").await;
    let mut phone = Browser::default();
    phone.press(&w.app, &link, &link, "").await;
    for mode in [Mode::Fail, Mode::Timeout] {
        w.fake.set(Mode::Answer, mode);
        phone.press(&w.app, &link, &link, RIGHT).await;
    }
    w.fake.set(Mode::Answer, Mode::Hang);
    let (app, cookie) = (w.app.clone(), phone.line());
    let form = format!("csrf={}{RIGHT}", phone.csrf);
    let target = link.clone();
    let crash = tokio::spawn(async move { call(app, "POST", &target, cookie, form).await });
    until(&pool, "SELECT count(*) FROM portals.code_try", 3).await;
    assert_eq!(
        idle_in_transaction(&pool).await,
        0,
        "no transaction waits on Verify"
    );
    crash.abort();
    let left = "SELECT count(*) FROM portals.code_try WHERE outcome = 'pending'";
    assert_eq!(count(&pool, left).await, 1, "the crashed try stays used");
    w.fake.set(Mode::Answer, Mode::Answer);
    for _ in 0..2 {
        assert!(
            phone
                .press(&w.app, &link, &link, WRONG)
                .await
                .contains("didn't work")
        );
    }
    let late = phone.press(&w.app, &link, &link, RIGHT).await;
    assert!(late.contains("This sign-in link has stopped working"));
    assert_eq!(
        w.fake.checks.load(Ordering::SeqCst),
        5,
        "the right code never reached Verify"
    );
}

/// TD-SEC-38 (SEC-004): a right code and a wrong code at once never both sign in, and the link
/// is used once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_right_and_wrong_at_once_never_both_sign_in(pool: PgPool) {
    let w = world(&pool, 8).await;
    for round in 1..=3_i64 {
        let link = link_for(&w, "crew@x.example").await;
        let mut right = Browser::default();
        right.press(&w.app, &link, &link, "").await;
        let mut wrong = Browser::default();
        wrong.send(&w.app, "GET", &link, String::new()).await;
        let (a, b) = tokio::join!(
            call(
                w.app.clone(),
                "POST",
                &link,
                right.line(),
                format!("csrf={}{RIGHT}", right.csrf)
            ),
            call(
                w.app.clone(),
                "POST",
                &link,
                wrong.line(),
                format!("csrf={}{WRONG}", wrong.csrf)
            ),
        );
        assert!(a.1.contains("You're signed in"), "the right code signs in");
        assert!(
            !b.1.contains("You're signed in"),
            "the wrong code never does"
        );
        let sessions = count(&pool, "SELECT count(*) FROM portals.business_session").await;
        assert_eq!(sessions, round, "one sign-in per link");
    }
}

/// TD-SEC-38 (PT-03, D-2349): a failed code send can be pressed again; 20 simultaneous presses
/// send one code; a check on a link with no sent code reaches Verify zero times.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_sends_retry_once_and_checks_need_a_sent_code(pool: PgPool) {
    let w = world(&pool, 8).await;
    let link = link_for(&w, "owner@x.example").await;
    let mut phone = Browser::default();
    let early = phone.press(&w.app, &link, &link, RIGHT).await;
    assert!(early.contains("didn't work") && !early.contains("You're signed in"));
    assert_eq!(
        w.fake.checks.load(Ordering::SeqCst),
        0,
        "no sent code, no Verify check"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM portals.code_try").await,
        0
    );
    w.fake.set(Mode::Fail, Mode::Answer);
    let failed = phone.press(&w.app, &link, &link, "").await;
    assert!(
        !failed.contains("one-time-code"),
        "a failed send shows no code field"
    );
    w.fake.set(Mode::Answer, Mode::Answer);
    let presses = phone.many(&w.app, 20, &link, "").await;
    assert!(presses.iter().any(|p| p.contains("one-time-code")));
    assert_eq!(
        w.fake.sends.load(Ordering::SeqCst),
        2,
        "the failed send, then one more"
    );
    let sent = "SELECT count(*) FROM portals.code_send WHERE outcome = 'sent'";
    assert_eq!(count(&pool, sent).await, 1);
    assert!(
        phone
            .press(&w.app, &link, &link, RIGHT)
            .await
            .contains("You're signed in")
    );
}

/// TD-SEC-38 (PT-04, D-2349): while a fake Verify send hangs no transaction is open and another
/// press sends nothing; a reservation the crash left is retaken after the adapter's timeout plus
/// a minute.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_hanging_send_holds_no_transaction(pool: PgPool) {
    let w = world(&pool, 4).await;
    let link = link_for(&w, "owner@x.example").await;
    let mut phone = Browser::default();
    phone.send(&w.app, "GET", &link, String::new()).await;
    w.fake.set(Mode::Hang, Mode::Answer);
    let (app, cookie, form) = (w.app.clone(), phone.line(), format!("csrf={}", phone.csrf));
    let target = link.clone();
    let crash = tokio::spawn(async move { call(app, "POST", &target, cookie, form).await });
    until(
        &pool,
        "SELECT count(*) FROM portals.code_send WHERE outcome = 'pending'",
        1,
    )
    .await;
    assert_eq!(
        idle_in_transaction(&pool).await,
        0,
        "no transaction waits on Verify"
    );
    crash.abort();
    w.fake.set(Mode::Answer, Mode::Answer);
    phone.press(&w.app, &link, &link, "").await;
    assert_eq!(
        w.fake.sends.load(Ordering::SeqCst),
        1,
        "a fresh reservation is not retaken"
    );
    w.clock.advance(SignedDuration::from_secs(76));
    assert!(
        phone
            .press(&w.app, &link, &link, "")
            .await
            .contains("one-time-code")
    );
    assert_eq!(
        w.fake.sends.load(Ordering::SeqCst),
        2,
        "the crashed send is retaken"
    );
}

/// TD-SEC-38, AVAIL-01 (D-2349): with a fake Verify that hangs, 30 concurrent presses and checks
/// leave a pool of 5 connections able to serve another request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_38_hanging_verify_leaves_the_pool_free(pool: PgPool) {
    let w = world(&pool, 5).await;
    let mut checks = Vec::new();
    for _ in 0..LINKS_PER_LIFE {
        let link = link_for(&w, "owner@x.example").await;
        let mut phone = Browser::default();
        phone.press(&w.app, &link, &link, "").await;
        checks.push((link, phone));
    }
    let presses: Vec<String> = {
        let mut links = Vec::new();
        for _ in 0..LINKS_PER_LIFE {
            links.push(link_for(&w, "crew@x.example").await);
        }
        links
    };
    w.fake.set(Mode::Hang, Mode::Hang);
    let mut hung = JoinSet::new();
    for (link, phone) in &checks {
        for _ in 0..CODE_TRIES {
            let (app, cookie) = (w.app.clone(), phone.line());
            let (link, form) = (link.clone(), format!("csrf={}{RIGHT}", phone.csrf));
            hung.spawn(async move { call(app, "POST", &link, cookie, form).await });
        }
    }
    let mut phone = Browser::default();
    phone.send(&w.app, "GET", &presses[0], String::new()).await;
    for link in &presses {
        let (app, cookie) = (w.app.clone(), phone.line());
        let (link, form) = (link.clone(), format!("csrf={}", phone.csrf));
        hung.spawn(async move { call(app, "POST", &link, cookie, form).await });
    }
    assert_eq!(hung.len(), 30);
    until(&pool, "SELECT count(*) FROM portals.code_try", 25).await;
    until(
        &pool,
        "SELECT count(*) FROM portals.code_send WHERE outcome = 'pending'",
        5,
    )
    .await;
    let other = call(
        w.app.clone(),
        "GET",
        &presses[0],
        String::new(),
        String::new(),
    );
    let (_, page) = tokio::time::timeout(Duration::from_secs(10), other)
        .await
        .expect("served");
    assert!(page.contains("Sign in"), "another request is served");
    assert_eq!(idle_in_transaction(&pool).await, 0);
    hung.abort_all();
}

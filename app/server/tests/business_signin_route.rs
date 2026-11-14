//! TD-SEC-40 (SEC-004, SEC-014, SEC-025, SEC-026, SEC-048; T280, D-1844, D-2206, D-3562): the
//! business sign-in pages served by the assembled router against PostgreSQL. No code
//! configuration refuses to start; a link alone never signs in, a right code does; T272's link
//! and try caps hold; the link email is queued once and its stored row never holds the link;
//! T089's limits refuse before any email or Verify call; the code goes to the member's own
//! texting line, else the member's own address, never to another member's line or the main
//! phone, and to no one for a removed member. Fake addresses (`example`) and 555-01xx numbers.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, CodeChannel, Email, EmailAdapter, IdempotencyKey, VerifyAdapter,
};
use askhava_businesses::api::{
    Acting, ConsentChannel, Roles, add_member, add_owner, create, link_phone, record_consent,
    remove_member,
};
use askhava_core::api::{IdSecret, MIGRATOR, ManualClock, Settings};
use askhava_intake::api::Admission;
use askhava_messaging::api::{DailyLimit, MailboxKind, SignInLinkSender};
use askhava_portals::api::{DeviceCodes, admit_code_try, admit_sign_in_link};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::business_contacts::ContactDirectory;
use askhava_server::business_signin::{MemberCodeAddress, ServedSignIn, SignInMailer};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const RIGHT: &str = "123456";
/// The test project's fake vendor allow list.
const ALLOWED: &str = "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:b";

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = (key(0x53).into(), key(0x63).into());
    Vault::new(&seal, &index, &url).expect("vault")
}

fn ip(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, n))
}

/// Verify: records each send and counts checks; only [`RIGHT`] is right.
#[derive(Clone, Default)]
struct FakeVerify(Arc<Mutex<Seen>>);

/// The codes sent (to, channel) and the number of checks.
type Seen = (Vec<(String, CodeChannel)>, usize);

impl FakeVerify {
    fn sends(&self) -> Vec<(String, CodeChannel)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .clone()
    }
    fn checks(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).1
    }
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.0.push((to.to_owned(), channel));
        Ok("VE_fake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).1 += 1;
        Ok(code == RIGHT)
    }
}

/// The email provider: keeps each email's recipient and plain text.
#[derive(Clone, Default)]
struct FakeMail(Arc<Mutex<Vec<(String, String)>>>);

impl FakeMail {
    fn sent(&self) -> Vec<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((e.to.to_owned(), e.text.to_owned()));
        Ok(format!("re_fake_{}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// One business with its owner; the assembled router with the sign-in part over fakes.
struct World {
    app: Router,
    pool: PgPool,
    verify: FakeVerify,
    mail: FakeMail,
    owner: Acting,
    now: Timestamp,
}

fn served(pool: &PgPool, verify: &FakeVerify, mail: &FakeMail, now: Timestamp) -> Router {
    let secret = || SecretString::from(SECRET.to_owned());
    let clock: Arc<dyn askhava_core::api::Clock> = Arc::new(ManualClock::new(now));
    let vault = Some(Arc::new(vault()));
    let postal = ("PO Box 1234, Faketown, AZ 00000", MailboxKind::PoBox);
    let limit = DailyLimit::new(100, 90).expect("limit");
    let sender = SignInLinkSender::new(secret(), mail.clone(), postal, limit).expect("sender");
    let mailer = SignInMailer::new((pool.clone(), clock.clone()), vault.clone(), Some(sender));
    let to = MemberCodeAddress::new(pool.clone(), clock, vault);
    let codes = DeviceCodes::new(verify.clone(), to, secret());
    let notice = IdSecret::new(1, secret()).expect("id secret");
    let part = ServedSignIn::new(pool.clone(), mailer, codes, notice);
    let parts = RouterParts::from((None, None, None, None)).with_sign_in(part);
    build_router(pool.clone(), Router::new(), parts)
}

async fn world(pool: &PgPool) -> World {
    let now = Timestamp::now();
    let mut db = pool.acquire().await.expect("db");
    let mailbox = mailbox_for(&mut db, &vault(), "owner@x.example").await;
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, mailbox.expect("mailbox"), now).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let (verify, mail) = (FakeVerify::default(), FakeMail::default());
    let app = served(pool, &verify, &mail, now);
    World {
        app,
        pool: pool.clone(),
        verify,
        mail,
        owner,
        now,
    }
}

/// A member of the world's business at `address`, with both roles.
async fn member(w: &World, address: &str) -> Acting {
    let mut db = w.pool.acquire().await.expect("db");
    let mailbox = mailbox_for(&mut db, &vault(), address).await.expect("box");
    let added = add_member(&mut db, w.owner, mailbox, Roles::BOTH, w.now).await;
    Acting {
        business: w.owner.business,
        member: added.expect("member"),
    }
}

/// Links `number` as `by`'s phone (`main` for the business's main phone), its text box ticked
/// when `text`.
async fn line(w: &World, by: Acting, (number, main, text): (&str, bool, bool)) {
    let mut db = w.pool.acquire().await.expect("db");
    let dir = ContactDirectory::new(vault());
    let n = SecretString::from(number.to_owned());
    let line = link_phone(&mut db, &dir, (by, main), &n, w.now).await;
    let line = Some(line.expect("linked"));
    if text {
        let at = (by.business, by.member, line);
        let ticked = record_consent(&mut db, at, &[ConsentChannel::Text], true, w.now).await;
        ticked.expect("consent");
    }
}

/// One browser: its cookies, kept from each `Set-Cookie`, and its client address.
struct Browser(BTreeMap<String, String>, IpAddr);

impl Browser {
    fn at(n: u8) -> Self {
        Self(BTreeMap::new(), ip(n))
    }

    /// Sends one request; returns the status, the body and whether a session cookie was set.
    async fn send(&mut self, app: &Router, method: &str, uri: &str, form: &str) -> (u16, String) {
        let line: Vec<String> = self.0.iter().map(|(n, v)| format!("{n}={v}")).collect();
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"))
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, line.join("; "))
            .body(Body::from(form.to_owned()))
            .expect("request");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(self.1, 40_000)));
        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status().as_u16();
        for cookie in response.headers().get_all(header::SET_COOKIE) {
            let pair = cookie.to_str().expect("ascii").split(';').next();
            let (name, value) = pair.and_then(|p| p.split_once('=')).expect("pair");
            if value.is_empty() {
                self.0.remove(name);
            } else {
                self.0.insert(name.to_owned(), value.to_owned());
            }
        }
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// Opens `uri` and posts its form to the same path with the page's CSRF token and `fields`.
    async fn press(&mut self, app: &Router, uri: &str, fields: &str) -> (u16, String) {
        let (_, page) = self.send(app, "GET", uri, "").await;
        let csrf = page.split("name=\"csrf\" value=\"").nth(1);
        let csrf = csrf
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default();
        let form = format!("csrf={csrf}{fields}");
        self.send(app, "POST", uri, &form).await
    }

    fn signed_in(&self) -> bool {
        self.0.contains_key("__Host-askhava-session")
    }
}

/// Asks for a link for `address` from client `n`; returns the link path emailed, if one was.
async fn ask(w: &World, address: &str, n: u8) -> Option<String> {
    let before = w.mail.sent().len();
    let form = format!("&email={address}");
    let (_, page) = Browser::at(n).press(&w.app, "/b/sign-in", &form).await;
    assert!(page.contains("Check your email"), "the same reply always");
    let sent = w.mail.sent();
    let (to, text) = sent.get(before)?;
    assert_eq!(to, address, "the link goes to the address asked for");
    let token = text.split("/b/sign-in/").nth(1).expect("a link");
    Some(format!("/b/sign-in/{}", &token[..64]))
}

/// Presses the link's button from a new browser at client `n`: the code is sent; returns it.
async fn code_step(w: &World, link: &str, n: u8) -> Browser {
    let mut browser = Browser::at(n);
    let (_, page) = browser.press(&w.app, link, "").await;
    assert!(page.contains("If the code doesn't come"), "the code step");
    assert!(
        !browser.signed_in(),
        "a valid link without the code never signs in"
    );
    browser
}

#[tokio::test]
async fn td_sec_40_no_code_configuration_refuses_to_start() {
    let base = [
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@127.0.0.1:1/none".to_owned(),
        ),
        ("VAULT_KEY", key(1)),
        ("VAULT_INDEX_KEY", key(2)),
    ];
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("lazy pool");
    let cases = [
        vec![
            ("ASKHAVA_APP_ENV", "test".to_owned()),
            ("ASKHAVA_ID_SECRET", SECRET.to_owned()),
            ("ASKHAVA_ALLOW_LIST_TEST", ALLOWED.to_owned()),
        ],
        vec![("ASKHAVA_APP_ENV", "dev".to_owned())],
    ];
    for extra in cases {
        let pairs: Vec<(String, String)> = base
            .iter()
            .cloned()
            .chain(extra)
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
        let built = ServedSignIn::from_settings(&settings, pool.clone(), pairs);
        assert!(
            built.is_err(),
            "no Verify settings or no ID secret: no start"
        );
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_40_a_link_signs_in_only_with_the_right_code(pool: PgPool) {
    let w = world(&pool).await;
    let link = ask(&w, "owner@x.example", 1).await.expect("emailed");
    let mut opened = Browser::at(2);
    let (_, page) = opened.send(&w.app, "GET", &link, "").await;
    assert!(
        page.contains("Press the button"),
        "a GET only shows the button"
    );
    assert!(!opened.signed_in(), "opening the link signs no one in");
    let token = link.trim_start_matches("/b/sign-in/");
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messaging.email_outbox WHERE template_id = 'business_signin_link'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(stored, 1, "the sign-in email queues once");
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messaging.email_outbox WHERE strpos(concat(subject, html, plain), $1) > 0",
    )
    .bind(token)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(leaked, 0, "the outbox never holds the link");
    let mut browser = code_step(&w, &link, 3).await;
    let (_, page) = browser.press(&w.app, &link, "&code=000000").await;
    assert!(page.contains("If the code doesn't come") && !browser.signed_in());
    let (_, page) = browser
        .press(&w.app, &link, &format!("&code={RIGHT}"))
        .await;
    assert!(
        page.contains("You're signed in") && browser.signed_in(),
        "a right code does"
    );
    let to = ("owner@x.example".to_owned(), CodeChannel::Email);
    assert_eq!(
        w.verify.sends(),
        vec![to],
        "one code, to the member's own address"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_40_the_link_and_try_caps_hold_through_the_route(pool: PgPool) {
    let w = world(&pool).await;
    let link = ask(&w, "owner@x.example", 1).await.expect("emailed");
    let mut browser = code_step(&w, &link, 2).await;
    for _ in 0..5 {
        browser.press(&w.app, &link, "&code=000000").await;
    }
    let (_, page) = browser
        .press(&w.app, &link, &format!("&code={RIGHT}"))
        .await;
    assert!(
        page.contains("stopped working") && !browser.signed_in(),
        "five tries a link"
    );
    assert_eq!(w.verify.checks(), 5, "the sixth try reaches no Verify");
    for n in 0..5 {
        ask(&w, "owner@x.example", 10 + n).await;
    }
    assert_eq!(w.mail.sent().len(), 5, "five links a member in 15 minutes");
}

/// Spends `admit` from new clients until it refuses.
async fn exhaust<F, Fut>(admit: F)
where
    F: Fn(u8) -> Fut,
    Fut: Future<Output = Result<Admission, askhava_intake::api::LimitError>>,
{
    for n in 100..=250 {
        if matches!(admit(n).await, Ok(Admission::Refused(_))) {
            return;
        }
    }
    panic!("the limit never refused");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_40_the_limits_refuse_before_any_email_or_verify(pool: PgPool) {
    let w = world(&pool).await;
    let (business, now) = (w.owner.business, w.now);
    let link = ask(&w, "owner@x.example", 1).await.expect("emailed");
    let mut browser = code_step(&w, &link, 2).await;
    exhaust(|_| admit_sign_in_link(&pool, (ip(3), None), now)).await;
    assert_eq!(
        ask(&w, "owner@x.example", 3).await,
        None,
        "past the per-IP limit"
    );
    exhaust(|n| admit_sign_in_link(&pool, (ip(n), Some(business)), now)).await;
    assert_eq!(
        ask(&w, "owner@x.example", 4).await,
        None,
        "past the per-business limit"
    );
    assert_eq!(w.mail.sent().len(), 1, "no email past a limit");
    exhaust(|n| admit_code_try(&pool, (ip(n), Some(business)), now)).await;
    let (status, _) = browser
        .press(&w.app, &link, &format!("&code={RIGHT}"))
        .await;
    assert_eq!(status, 500, "a refused try answers as any failure");
    assert!(!browser.signed_in());
    assert_eq!(w.verify.checks(), 0, "a refused try calls no Verify");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn d_3562_the_code_goes_to_the_members_own_line_else_own_address(pool: PgPool) {
    let w = world(&pool).await;
    let (texts, untexted) = (
        member(&w, "a@x.example").await,
        member(&w, "b@x.example").await,
    );
    line(&w, w.owner, ("928-555-0100", true, true)).await;
    line(&w, texts, ("928-555-0101", false, true)).await;
    line(&w, untexted, ("928-555-0102", false, false)).await;
    let bare = member(&w, "c@x.example").await;
    let mut db = pool.acquire().await.expect("db");
    remove_member(&mut db, w.owner, bare.member, w.now)
        .await
        .expect("removed");
    for (n, address) in ["a@x.example", "b@x.example", "owner@x.example"]
        .into_iter()
        .enumerate()
    {
        let link = ask(&w, address, 1).await.expect("emailed");
        code_step(&w, &link, 10 + u8::try_from(n).expect("small")).await;
    }
    let want = vec![
        ("+19285550101".to_owned(), CodeChannel::Sms),
        ("b@x.example".to_owned(), CodeChannel::Email),
        ("owner@x.example".to_owned(), CodeChannel::Email),
    ];
    assert_eq!(
        w.verify.sends(),
        want,
        "own texting line, else own address; never main"
    );
    assert_eq!(
        ask(&w, "c@x.example", 1).await,
        None,
        "a removed member gets no link"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn d_3562_no_code_for_a_member_removed_after_the_link(pool: PgPool) {
    let w = world(&pool).await;
    let gone = member(&w, "d@x.example").await;
    let link = ask(&w, "d@x.example", 1).await.expect("emailed");
    let mut db = pool.acquire().await.expect("db");
    remove_member(&mut db, w.owner, gone.member, w.now)
        .await
        .expect("removed");
    let mut browser = Browser::at(2);
    browser.press(&w.app, &link, "").await;
    assert!(!browser.signed_in(), "a removed member is never signed in");
    assert!(w.verify.sends().is_empty(), "and gets no code");
}

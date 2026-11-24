//! TD-PRIV-32 (T804, D-5281, D-5332, D-5339, D-5340; PRIV-031, SEC-014, SEC-026, SEC-037,
//! TXT-018): a delete or copy request made in the signed-in portal for a number on the
//! suppression list is confirmed by a single-use 15-minute link emailed to her confirmed update
//! address, never by a Verify code, through the assembled router over PostgreSQL.
//!
//! A listed number with a confirmed update address gets the link email and no Verify send, and a
//! second request while that link is live stores nothing new; an unconfirmed or missing address
//! gets no link and the help-email answer; an unlisted number keeps the code path. Opening the link
//! (`GET`) shows a `no_store` confirm page and changes nothing; its `POST` with the CSRF token acts
//! once and uses the link up, a copy answering only the sign-in link and no data; a repeat `POST`
//! answers the same and acts nothing again; a cross-site or wrong-token `POST` is refused; a used,
//! expired, unknown or malformed link's `GET` is the generic page and acts nothing; the help
//! form's answer is the same for a listed and an unlisted number; no number or address reaches a
//! log line. The homeowner session proof comes from the composition root, so
//! `app/server/src/lib.rs` is compiled in place as `problem_complaint.rs` does (D-2853's
//! exception, logged for this file in D-5747). Fake data only.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, LinkKind, LinkSalt, Settings, TestControl, link_token};
use askhava_messaging::api::{MailboxKind, PostalAddress, RevokeChannel, suppress};
use askhava_portals::api::{
    CookieJar, Realm, csrf_token, help_form_page, send_help_form, start_session,
};
use askhava_privacy::api::{
    DELETE_CONFIRMATION, NewContact, UpdateAddress, Vault, confirm_update_address, store_contact,
    store_update_address,
};
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
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate
/// for its router and session minter (D-5747).
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the router and the session minter are used here"
)]
mod server;
use server::homeowner_portal::HomeownerVerify;
use server::*;

/// Her fake number and address.
const HER: (&str, &str) = ("+19285550184", "80 Sample Lane");
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0804";
const UPDATE_ADDRESS: &str = "pat.update@mail.test";

/// A Verify that counts its sends; every check is wrong.
#[derive(Clone, Default)]
struct FakeVerify(Arc<AtomicUsize>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _: &str, _: &str) -> Result<bool, AdapterError> {
        Ok(false)
    }
}

impl FakeVerify {
    /// Sends made, after giving a spawned send time to finish.
    async fn sends(&self) -> usize {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        self.0.load(Ordering::SeqCst)
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
        ("ASKHAVA_ID_SECRET", ID_SECRET),
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
    pairs.push(("VAULT_KEY".to_owned(), vault_key(0x84)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), vault_key(0x85)));
    pairs
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x84), &key(0x85), &url).expect("vault")
}

/// The assembled router with the webhooks, `verify` and the link email's postal line.
fn app(pool: &PgPool, verify: &FakeVerify) -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    let postal = PostalAddress::new("PO Box 0804, Sample City, AZ 86400", MailboxKind::PoBox);
    let parts = RouterParts::from((hooks, None, None, None))
        .with_homeowner_verify(Some(HomeownerVerify::new(verify.clone())))
        .with_data_link_postal(Some(postal.expect("postal")));
    build_router(pool.clone(), control.router(), parts)
}

/// What her record holds: her fake number, texts off (`listed`) and an update address
/// (confirmed or not).
#[derive(Clone, Copy)]
struct Her {
    phone: &'static str,
    listed: bool,
    address: Option<bool>,
}

/// Her fake contact as `her` says, and a browser jar signed in as her.
async fn homeowner(pool: &PgPool, her: Her) -> (Uuid, CookieJar) {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now() - SignedDuration::from_mins(30);
    let new = NewContact {
        phone: Some(SecretString::from(her.phone.to_owned())),
        address: Some(SecretString::from(HER.1.to_owned())),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    if her.listed {
        let how = (RevokeChannel::Portal, "portal:switch");
        suppress(&mut db, &vault(), her.phone, how, now)
            .await
            .expect("listed");
    }
    if let Some(confirmed) = her.address {
        let address = UpdateAddress::parse(UPDATE_ADDRESS).expect("address");
        let row = store_update_address(&mut db, &vault(), contact, &address, now).await;
        if confirmed {
            let done = confirm_update_address(&mut db, row.expect("stored"), now).await;
            assert!(done.expect("confirm"));
        }
    }
    let life = SignedDuration::from_hours(2);
    let login = (Realm::Homeowner, contact);
    let jar = start_session(&mut db, CookieJar::new(), login, life, now).await;
    (contact, jar.expect("session"))
}

/// Where a `POST` comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    WrongToken,
}

/// One answer: status, body, and the jar after its cookies.
struct Reply {
    status: u16,
    text: String,
    jar: CookieJar,
    no_store: bool,
    attachment: bool,
}

/// `jar` with `set` (a `Set-Cookie` list) applied.
fn cookies(jar: &CookieJar, set: &HeaderMap) -> CookieJar {
    let mut pairs: Vec<(String, String)> = jar
        .iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect();
    for value in set.get_all(header::SET_COOKIE) {
        let first = value.to_str().expect("cookie").split(';').next();
        let (name, value) = first.expect("pair").split_once('=').expect("name=value");
        pairs.retain(|(n, _)| n != name);
        pairs.push((name.to_owned(), value.to_owned()));
    }
    let line: Vec<String> = pairs.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let mut headers = HeaderMap::new();
    if !line.is_empty() {
        let value = HeaderValue::from_str(&line.join("; ")).expect("cookie header");
        headers.insert(header::COOKIE, value);
    }
    CookieJar::from_headers(&headers)
}

/// `GET path` (no form) or `POST path` with `form` after the CSRF token, from `site`.
async fn call(
    app: &Router,
    path: &str,
    jar: &CookieJar,
    (form, site): (Option<&str>, Site),
) -> Reply {
    let cookie: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let fetch = if site == Site::Cross {
        "cross-site"
    } else {
        "same-origin"
    };
    let builder = Request::builder()
        .uri(path)
        .header(header::HOST, "askhava.example")
        .header(header::COOKIE, cookie.join("; "))
        .header("sec-fetch-site", fetch);
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
    let response = app.clone().oneshot(request.expect("request")).await;
    let (parts, body) = response.expect("answer").into_parts();
    let header_is =
        |name, want: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()) == Some(want);
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        jar: cookies(jar, &parts.headers),
        no_store: header_is(header::CACHE_CONTROL, "no-store"),
        attachment: parts.headers.contains_key(header::CONTENT_DISPOSITION),
    }
}

/// Her portal request of `kind` (`copy` or `delete`).
async fn ask(app: &Router, jar: &CookieJar, kind: &str) -> Reply {
    let form = format!("&kind={kind}");
    call(app, "/h/my-data/request", jar, (Some(&form), Site::Own)).await
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn links(pool: &PgPool) -> i64 {
    count(pool, "SELECT count(*) FROM portals.data_request_link").await
}

async fn link_emails(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM messaging.email_outbox WHERE template_id = 'data_request_link'";
    count(pool, sql).await
}

async fn confirmed(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM ops.audit_log WHERE action LIKE 'portals.data_%_confirmed'";
    count(pool, sql).await
}

async fn received(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM ops.audit_log WHERE action LIKE 'portals.data_%_requested'";
    count(pool, sql).await
}

/// The path of the newest stored link, its token computed from the stored salt (SEC-037).
async fn link_path(pool: &PgPool) -> String {
    let (id, salt): (i64, Vec<u8>) = sqlx::query_as(
        "SELECT request_id, salt FROM portals.data_request_link ORDER BY request_id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("link row");
    let object = Uuid::from_u128(u128::from(id.unsigned_abs()));
    let secret = IdSecret::new(1, SecretString::from(ID_SECRET.to_owned())).expect("secret");
    let salt = LinkSalt::from_bytes(&salt).expect("salt");
    let token = link_token(&secret, LinkKind::Action, object, &salt);
    format!("/help/my-data/{object}/{}", token.as_str())
}

/// Every log line's message and fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Lines {
    fn push(&self, line: String) {
        self.0.lock().expect("lines").push(line);
    }

    fn all(&self) -> String {
        self.0.lock().expect("lines").join("\n")
    }
}

impl Subscriber for Lines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

const LISTED: Her = Her {
    phone: HER.0,
    listed: true,
    address: Some(true),
};

/// TD-PRIV-32: a listed number with a confirmed update address gets one link email and no Verify
/// send; a second request while the link is live answers the same and stores nothing new; no
/// number or address reaches a log line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_listed_number_gets_the_email_link_not_a_code(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let (_, jar) = homeowner(&pool, LISTED).await;
    let sent = ask(&app, &jar, "delete").await;
    assert_eq!(sent.status, 200, "{}", sent.text);
    assert!(sent.no_store);
    assert!(sent.text.contains("Check your email"));
    assert_eq!(
        verify.sends().await,
        0,
        "no Verify code for a listed number"
    );
    assert_eq!((links(&pool).await, link_emails(&pool).await), (1, 1));
    let again = ask(&app, &jar, "delete").await;
    assert_eq!((again.status, again.text), (sent.status, sent.text));
    assert_eq!((links(&pool).await, link_emails(&pool).await), (1, 1));
    assert_eq!(received(&pool).await, 1);
    let copy = ask(&app, &jar, "copy").await;
    assert_eq!(copy.status, 200);
    assert_eq!((links(&pool).await, link_emails(&pool).await), (2, 2));
    let logged = lines.all();
    for secret in [HER.0, "9285550184", HER.1, UPDATE_ADDRESS] {
        assert!(!logged.contains(secret), "a log line holds {secret}");
    }
}

/// TD-PRIV-32: an unconfirmed update address, or none, gets no link and the help-email answer;
/// nothing is stored and no code is sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_no_confirmed_address_gets_the_help_email_answer(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let help = askhava_messaging::api::reply_to();
    for (phone, address) in [("+19285550185", Some(false)), ("+19285550186", None)] {
        let her = Her {
            phone,
            listed: true,
            address,
        };
        let (_, jar) = homeowner(&pool, her).await;
        let answer = ask(&app, &jar, "copy").await;
        assert_eq!(answer.status, 200, "{}", answer.text);
        assert!(answer.text.contains(&format!("mailto:{help}")));
        assert!(answer.text.contains("from your own email address"));
        assert_eq!((links(&pool).await, link_emails(&pool).await), (0, 0));
        assert_eq!(received(&pool).await, 0);
    }
    assert_eq!(verify.sends().await, 0);
}

/// TD-PRIV-32: an unlisted number keeps the code path: one Verify send, the code step, no link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_unlisted_number_keeps_the_code(pool: PgPool) {
    let verify = FakeVerify::default();
    let app = app(&pool, &verify);
    let her = Her {
        phone: HER.0,
        listed: false,
        address: Some(true),
    };
    let (_, jar) = homeowner(&pool, her).await;
    let step = ask(&app, &jar, "delete").await;
    assert_eq!(step.status, 200, "{}", step.text);
    assert!(step.text.contains("action=\"/h/my-data/code\""));
    assert_eq!(verify.sends().await, 1);
    assert_eq!((links(&pool).await, link_emails(&pool).await), (0, 0));
}

/// TD-PRIV-32, SEC-026: opening the delete link shows a `no_store` confirm page and changes
/// nothing; a cross-site or wrong-token `POST` is refused; the right `POST` deletes once and uses
/// the link up; a repeat answers the same and acts nothing again; the used link's `GET` is the
/// generic page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_the_link_acts_once_on_its_post(pool: PgPool) {
    let app = app(&pool, &FakeVerify::default());
    let (_, jar) = homeowner(&pool, LISTED).await;
    assert_eq!(ask(&app, &jar, "delete").await.status, 200);
    let path = link_path(&pool).await;
    let used = "SELECT count(*) FROM portals.data_request_link WHERE used_at IS NOT NULL";
    let browser = CookieJar::new();
    for _ in 0..2 {
        let page = call(&app, &path, &browser, (None, Site::Own)).await;
        assert_eq!(page.status, 200);
        assert!(page.no_store, "the confirm page is no_store");
        assert!(page.text.contains("method=\"post\""));
        assert!(page.text.contains("You asked us to delete your data."));
    }
    let page = call(&app, &path, &browser, (None, Site::Own)).await;
    assert_eq!((count(&pool, used).await, confirmed(&pool).await), (0, 0));
    for site in [Site::Cross, Site::WrongToken] {
        let refused = call(&app, &path, &page.jar, (Some(""), site)).await;
        assert_eq!(refused.status, 403);
    }
    assert_eq!((count(&pool, used).await, confirmed(&pool).await), (0, 0));
    let done = call(&app, &path, &page.jar, (Some(""), Site::Own)).await;
    assert_eq!(done.status, 200, "{}", done.text);
    assert!(done.text.contains(DELETE_CONFIRMATION));
    assert_eq!((count(&pool, used).await, confirmed(&pool).await), (1, 1));
    let repeat = call(&app, &path, &page.jar, (Some(""), Site::Own)).await;
    assert_eq!((repeat.status, &repeat.text), (done.status, &done.text));
    assert_eq!(confirmed(&pool).await, 1);
    let gone = call(&app, &path, &browser, (None, Site::Own)).await;
    assert_eq!(gone.status, 200);
    assert!(
        gone.text
            .contains("This link has expired or was already used.")
    );
    assert!(gone.text.contains("href=\"/h/sign-in\""));
}

/// TD-PRIV-32, D-5340: the copy link's `POST` records the confirmation and answers only the
/// sign-in link: no file, no data.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_the_copy_link_returns_no_data(pool: PgPool) {
    let app = app(&pool, &FakeVerify::default());
    let (_, jar) = homeowner(&pool, LISTED).await;
    assert_eq!(ask(&app, &jar, "copy").await.status, 200);
    let path = link_path(&pool).await;
    let page = call(&app, &path, &CookieJar::new(), (None, Site::Own)).await;
    assert!(page.text.contains("You asked for a copy of your data."));
    let done = call(&app, &path, &page.jar, (Some(""), Site::Own)).await;
    assert_eq!(done.status, 200, "{}", done.text);
    assert!(
        done.text
            .contains("Your copy is ready. Sign in to download it.")
    );
    assert!(done.text.contains("href=\"/h/sign-in\""));
    assert!(!done.attachment, "no file");
    for held in [HER.1, "9285550184", "0184", "{\""] {
        assert!(!done.text.contains(held), "the answer holds {held}");
    }
    assert_eq!(confirmed(&pool).await, 1);
}

/// TD-PRIV-32, D-5332: an expired, unknown or malformed link's `GET` answers the generic page
/// (200) and its `POST` acts nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_dead_links_show_the_generic_page(pool: PgPool) {
    let app = app(&pool, &FakeVerify::default());
    let (_, jar) = homeowner(&pool, LISTED).await;
    assert_eq!(ask(&app, &jar, "delete").await.status, 200);
    let live = link_path(&pool).await;
    sqlx::query("UPDATE portals.data_request_link SET sent_at = sent_at - interval '16 minutes'")
        .execute(&pool)
        .await
        .expect("expire");
    let unknown = format!(
        "/help/my-data/{}/{}",
        Uuid::from_u128(0x804),
        "a".repeat(64)
    );
    let malformed = "/help/my-data/not-a-request/NOT-A-TOKEN".to_owned();
    let last = if live.ends_with('0') { "1" } else { "0" };
    let forged = format!("{}{last}", &live[..live.len() - 1]);
    for path in [live, unknown, malformed, forged] {
        let page = call(&app, &path, &CookieJar::new(), (None, Site::Own)).await;
        assert_eq!(page.status, 200, "{path}");
        assert!(page.no_store);
        assert!(
            page.text
                .contains("This link has expired or was already used.")
        );
        assert!(!page.text.contains("method=\"post\""));
        let pressed = call(&app, &path, &page.jar, (Some(""), Site::Own)).await;
        assert!(pressed.status < 500 && pressed.status != 403, "{path}");
    }
    let used = "SELECT count(*) FROM portals.data_request_link WHERE used_at IS NOT NULL";
    assert_eq!((count(&pool, used).await, confirmed(&pool).await), (0, 0));
}

/// TD-PRIV-32, TD-PRIV-16, TD-SEC-51, D-5339: the help form's delete answer is the same for a
/// listed and an unlisted number, and it issues no link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_32_the_help_form_answer_is_unchanged(pool: PgPool) {
    let (_, _) = homeowner(&pool, LISTED).await;
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let message = "please delete my data";
    let listed = send_help_form(&mut db, &vault(), (message, Some(HER.0)), now).await;
    let unlisted = send_help_form(&mut db, &vault(), (message, Some("+19285550199")), now).await;
    let (listed, unlisted) = (listed.expect("listed"), unlisted.expect("unlisted"));
    assert_eq!(listed, unlisted);
    let page = help_form_page(listed).expect("page");
    assert_eq!(page, help_form_page(unlisted).expect("page"));
    assert_eq!((links(&pool).await, link_emails(&pool).await), (0, 0));
}

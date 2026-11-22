//! TD-TXT-38 (TXT-018, CON-035, SEC-028, PICK-029, PRIV-001, A11Y-030; T729, D-4727, D-5340,
//! D-5442): the quote page's update choice through `portals`' quote page routes, PostgreSQL, fake
//! data only.
//!
//! The live page offers "Get updates by email instead" and "Get updates by text and email" and
//! asks nothing until one is pressed; either asks the address; "pat@" gets the A11Y-030 error
//! with the value kept and a second malformed one keeps text and says so; a good one is sealed
//! in `privacy.update_email` with source `quote_page` (never the contact's email), the channel
//! set, and the page says a confirm link went with no confirm button. Only the first email's
//! single-use token for her current address shows "Confirm this email", whose press confirms
//! once; no token, a used one, an earlier address's or another homeowner's shows nothing and
//! sets nothing. "Text only" sets text; a bounce on text shows its note; the contact purge
//! deletes the address; a keyless router asks and checks but stores nothing. SEC-028 and CON-035
//! (the update email's footer and unsubscribe) are T730's cases.

use std::sync::Arc;

use askhava_core::api::{IdSecret, LinkKind, LinkToken, ManualClock, link_token};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, job_number, move_to};
use askhava_messaging::api::{UpdateChannel, update_channel};
use askhava_ops::api::Actor;
use askhava_portals::api::{
    UPDATES_CONFIRM_ROUTE, UPDATES_ROUTE, quote_page_routes, quote_page_routes_with,
};
use askhava_privacy::api::{
    Field, Purpose, Reader, UpdateAddress, UpdateStatus, Vault, VaultError, confirm_update_address,
    mark_update_bounced, purge_contact, read_contact, read_update_address, store_update_address,
    update_status,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const ID_KEY: &str = "fake-id-secret-for-tests-00000001";
const ASK: &str = "What email should we use? We never share it with businesses.";
const SENT: &str = "We sent a confirm link to that email.";
const CONFIRM: &str = "Confirm this email</button>";

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn secret() -> IdSecret {
    IdSecret::new(1, SecretString::from(ID_KEY.to_owned())).expect("secret")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x29), &key(0x72), &url).expect("vault")
}

fn app(pool: &PgPool, keyed: bool) -> Router {
    let clock = Arc::new(ManualClock::new(at(30)));
    if keyed {
        quote_page_routes_with(pool.clone(), clock, Some((vault(), secret())))
    } else {
        quote_page_routes(pool.clone(), clock)
    }
}

/// A fake homeowner `n` (a vault contact with no fields) with one open job; the job's token and
/// her contact.
async fn open_job(pool: &PgPool, n: u128) -> (LinkToken, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let contact = Uuid::from_u128(n);
    sqlx::query("INSERT INTO privacy.contact (id) VALUES ($1)")
        .bind(contact)
        .execute(&mut *db)
        .await
        .expect("contact");
    let id_secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x8_0000 + n),
        contact,
        inbound: Inbound {
            words: "my kitchen sink is leaking under the cabinet",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(0),
    };
    let handled = receive_text(&mut db, &id_secret, &msg, Settings::default()).await;
    let start = JobStart {
        conversation: handled.expect("open").conversation.expect("request"),
        words: "my kitchen sink is leaking",
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, at(2), Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    job_number(&mut db, job).await.expect("number");
    let token = issue(&mut db, &secret(), job, at(2)).await.expect("token");
    (token, contact)
}

/// One browser on one page: its pre-session cookie and CSRF token, and whether the router has keys.
struct Browser {
    cookie: String,
    csrf: String,
    keyed: bool,
}

async fn call(pool: &PgPool, keyed: bool, request: Request<Body>) -> (u16, String, String) {
    let response = app(pool, keyed).oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| {
            v.to_str()
                .expect("ascii")
                .split(';')
                .next()
                .expect("pair")
                .to_owned()
        })
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("utf8"),
        cookie,
    )
}

async fn browser(pool: &PgPool, token: &LinkToken, keyed: bool) -> Browser {
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .expect("request");
    let (status, body, cookie) = call(pool, keyed, request).await;
    assert_eq!(status, 200);
    let (_, rest) = body.split_once("name=\"csrf\" value=\"").expect("csrf");
    let (csrf, _) = rest.split_once('"').expect("csrf value");
    Browser {
        cookie,
        csrf: csrf.to_owned(),
        keyed,
    }
}

async fn get(pool: &PgPool, b: &Browser, path: &str) -> String {
    let request = Request::get(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, &b.cookie)
        .body(Body::empty())
        .expect("request");
    let (status, body, _) = call(pool, b.keyed, request).await;
    assert_eq!(status, 200, "{path}");
    body
}

/// A same-origin `POST` of `form` (plus the CSRF token) to `route` of `token`'s page.
async fn post(
    pool: &PgPool,
    b: &Browser,
    (route, token): (&str, &LinkToken),
    form: &str,
) -> String {
    let request = Request::post(route.replace("{token}", token.as_str()))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, &b.cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={}&{form}", b.csrf)))
        .expect("request");
    let (status, body, _) = call(pool, b.keyed, request).await;
    assert_eq!(status, 200, "{form}");
    body
}

async fn choose(pool: &PgPool, b: &Browser, token: &LinkToken, form: &str) -> String {
    post(pool, b, (UPDATES_ROUTE, token), form).await
}

async fn channel(pool: &PgPool, contact: Uuid) -> UpdateChannel {
    let mut db = pool.acquire().await.expect("db");
    update_channel(&mut db, contact).await.expect("channel")
}

async fn state(pool: &PgPool, contact: Uuid) -> Option<UpdateStatus> {
    let mut db = pool.acquire().await.expect("db");
    update_status(&mut db, contact).await.expect("status")
}

fn confirm_of(s: &UpdateStatus) -> LinkToken {
    link_token(&secret(), LinkKind::Action, s.row, &s.salt)
}

fn page_with(token: &LinkToken, confirm: &LinkToken) -> String {
    format!("/q/{}?confirm={}", token.as_str(), confirm.as_str())
}

/// TD-TXT-38: the two choices and no question; either asks; "pat@" gets the A11Y-030 error with
/// the value kept; a second malformed address keeps text, says so and stores nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_the_page_asks_once_more_then_keeps_text(pool: PgPool) {
    let (token, contact) = open_job(&pool, 0x7291).await;
    let b = browser(&pool, &token, true).await;
    let page = get(&pool, &b, &format!("/q/{}", token.as_str())).await;
    assert!(
        page.contains("Get updates by email instead</button>"),
        "{page}"
    );
    assert!(page.contains("Get updates by text and email</button>"));
    assert!(!page.contains(ASK), "no question until she chooses");
    for choice in ["email", "both"] {
        let asked = choose(&pool, &b, &token, &format!("choice={choice}")).await;
        assert!(asked.contains(ASK), "{asked}");
        assert!(!asked.contains("<title>Error: "));
    }
    let wrong = choose(&pool, &b, &token, "choice=email&email=pat%40&tries=0").await;
    assert!(wrong.contains("<title>Error: "), "{wrong}");
    assert!(wrong.contains("id=\"update-summary\" role=\"alert\" tabindex=\"-1\" autofocus"));
    assert!(wrong.contains("<a href=\"#update-email\">"));
    assert!(wrong.contains("value=\"pat@\""), "the value is kept");
    assert!(wrong.contains("aria-invalid=\"true\" aria-describedby=\"update-email-error\""));
    assert!(wrong.contains("name=\"tries\" value=\"1\""));
    let kept = choose(&pool, &b, &token, "choice=email&email=pat%40home&tries=1").await;
    assert!(kept.contains("so your updates stay by text."), "{kept}");
    assert!(!kept.contains("<title>Error: "));
    assert_eq!(channel(&pool, contact).await, UpdateChannel::Text);
    assert!(state(&pool, contact).await.is_none(), "nothing stored");
}

/// TD-TXT-38, PICK-029, PRIV-001: a good address is sealed apart from the contact's email with
/// source `quote_page`, the channel set; the page says a link went and shows no confirm button;
/// the pick-code and conversation-email reads never return it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_a_good_address_is_sealed_apart_and_waits(pool: PgPool) {
    let (token, contact) = open_job(&pool, 0x7292).await;
    let b = browser(&pool, &token, true).await;
    let typed = "choice=both&email=Pat.Example%40mail.test&tries=0";
    let page = choose(&pool, &b, &token, typed).await;
    assert!(page.contains(SENT), "{page}");
    assert!(
        !page.contains(CONFIRM),
        "no button without the email's link"
    );
    assert!(page.contains("Get updates by text only</button>"));
    assert_eq!(channel(&pool, contact).await, UpdateChannel::Both);
    let (source, sealed): (String, Vec<u8>) =
        sqlx::query_as("SELECT source, email FROM privacy.update_email WHERE contact_id = $1")
            .bind(contact)
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(source, "quote_page");
    let plain = b"pat.example@mail.test";
    assert!(!sealed.windows(plain.len()).any(|w| w == plain), "sealed");
    let mut db = pool.acquire().await.expect("db");
    let read = read_update_address(&mut db, &vault(), contact)
        .await
        .expect("read");
    let read = read.expect("an address");
    assert_eq!(read.address.expose_secret(), "pat.example@mail.test");
    let access = (Reader::Messaging, Purpose::Send);
    let own = read_contact(&mut db, &vault(), access, contact, &[Field::Email]).await;
    assert!(
        own.expect("contact").email.is_none(),
        "never the contact's email"
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_kind = 'privacy.update_email' \
         AND action = 'vault_read.messaging.send'",
    )
    .fetch_one(&pool)
    .await
    .expect("audit");
    assert_eq!(audited, 1);
}

/// TD-TXT-38, D-5340, SEC-026: only her current address's token shows "Confirm this email";
/// opening it changes nothing; the press confirms once; no token, a wrong, used or earlier one
/// shows no button and sets nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_only_the_emails_token_confirms_once(pool: PgPool) {
    let (token, contact) = open_job(&pool, 0x7293).await;
    let b = browser(&pool, &token, true).await;
    choose(&pool, &b, &token, "choice=email&email=pat%40mail.test").await;
    assert_eq!(channel(&pool, contact).await, UpdateChannel::Email);
    let first = state(&pool, contact).await.expect("stored");
    let good = confirm_of(&first);
    let bare = get(&pool, &b, &format!("/q/{}", token.as_str())).await;
    assert!(bare.contains(SENT) && !bare.contains(CONFIRM), "{bare}");
    let opened = get(&pool, &b, &page_with(&token, &good)).await;
    assert!(opened.contains(CONFIRM), "{opened}");
    assert!(opened.contains(&format!("name=\"confirm\" value=\"{}\"", good.as_str())));
    assert!(
        !state(&pool, contact).await.expect("row").confirmed,
        "a GET sets nothing"
    );
    let wrong = format!("confirm={}", "ab".repeat(32));
    post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token), &wrong).await;
    post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token), "").await;
    assert!(!state(&pool, contact).await.expect("row").confirmed);
    let form = format!("confirm={}", good.as_str());
    let done = post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token), &form).await;
    assert!(done.contains("That email is confirmed"), "{done}");
    assert!(state(&pool, contact).await.expect("row").confirmed);
    let used = get(&pool, &b, &page_with(&token, &good)).await;
    assert!(!used.contains(CONFIRM) && !used.contains(SENT), "used once");
    let again = post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token), &form).await;
    assert!(!again.contains("That email is confirmed"));
    let confirms: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action = 'update_email_confirmed'",
    )
    .fetch_one(&pool)
    .await
    .expect("audit");
    assert_eq!(confirms, 1, "confirmed once");
    choose(&pool, &b, &token, "choice=email&email=pat2%40mail.test").await;
    let second = state(&pool, contact).await.expect("replaced");
    assert_ne!(second.row, first.row);
    assert!(!second.confirmed, "a new address waits again");
    let earlier = get(&pool, &b, &page_with(&token, &good)).await;
    assert!(!earlier.contains(CONFIRM), "an earlier address's token");
    post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token), &form).await;
    assert!(!state(&pool, contact).await.expect("row").confirmed);
}

/// TD-TXT-38, D-5442: "Text only" sets text; a bounce on text shows its note; a late bounce for
/// a replaced address marks nothing; a new address clears the bounce.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_text_only_and_the_bounce_note(pool: PgPool) {
    let (token, contact) = open_job(&pool, 0x7294).await;
    let b = browser(&pool, &token, true).await;
    choose(&pool, &b, &token, "choice=both&email=pat%40mail.test").await;
    let page = choose(&pool, &b, &token, "choice=text").await;
    assert_eq!(channel(&pool, contact).await, UpdateChannel::Text);
    assert!(
        page.contains("Get updates by email instead</button>"),
        "{page}"
    );
    assert!(!page.contains("bounced"));
    let row = state(&pool, contact).await.expect("row").row;
    let mut db = pool.acquire().await.expect("db");
    assert!(
        mark_update_bounced(&mut db, row, at(40))
            .await
            .expect("bounce")
    );
    let page = get(&pool, &b, &format!("/q/{}", token.as_str())).await;
    assert!(
        page.contains("Your email bounced, so updates go by text again."),
        "{page}"
    );
    choose(&pool, &b, &token, "choice=email&email=pat3%40mail.test").await;
    let fresh = state(&pool, contact).await.expect("row");
    assert!(!fresh.bounced, "a new address clears the bounce");
    assert!(
        !mark_update_bounced(&mut db, row, at(41))
            .await
            .expect("late")
    );
    assert!(!state(&pool, contact).await.expect("row").bounced);
}

/// TD-TXT-38: another homeowner's confirm token shows nothing and confirms nothing on this page;
/// the contact purge deletes the address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_another_homeowners_token_changes_nothing(pool: PgPool) {
    let (token_a, contact_a) = open_job(&pool, 0x7295).await;
    let (token_b, contact_b) = open_job(&pool, 0x7296).await;
    let a = browser(&pool, &token_a, true).await;
    let b = browser(&pool, &token_b, true).await;
    choose(&pool, &a, &token_a, "choice=email&email=pat%40mail.test").await;
    choose(&pool, &b, &token_b, "choice=email&email=sam%40mail.test").await;
    let theirs = confirm_of(&state(&pool, contact_a).await.expect("a"));
    let page = get(&pool, &b, &page_with(&token_b, &theirs)).await;
    assert!(!page.contains(CONFIRM), "{page}");
    let form = format!("confirm={}", theirs.as_str());
    post(&pool, &b, (UPDATES_CONFIRM_ROUTE, &token_b), &form).await;
    assert!(!state(&pool, contact_a).await.expect("a").confirmed);
    assert!(!state(&pool, contact_b).await.expect("b").confirmed);
    let mut db = pool.acquire().await.expect("db");
    assert!(purge_contact(&mut db, contact_a).await.expect("purge"));
    assert!(
        state(&pool, contact_a).await.is_none(),
        "purged with the contact"
    );
    assert!(state(&pool, contact_b).await.is_some());
}

/// TD-TXT-38 (D-5442): with no keys the page still asks and shows the A11Y-030 error, and a good
/// address stores nothing and leaves text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_a_keyless_router_asks_and_stores_nothing(pool: PgPool) {
    let (token, contact) = open_job(&pool, 0x7297).await;
    let b = browser(&pool, &token, false).await;
    let asked = choose(&pool, &b, &token, "choice=email").await;
    assert!(asked.contains(ASK), "{asked}");
    let wrong = choose(&pool, &b, &token, "choice=email&email=pat%40&tries=0").await;
    assert!(wrong.contains("<title>Error: "), "{wrong}");
    choose(&pool, &b, &token, "choice=email&email=pat%40mail.test").await;
    assert!(state(&pool, contact).await.is_none());
    assert_eq!(channel(&pool, contact).await, UpdateChannel::Text);
}

/// TD-TXT-38 (TXT-018, PRIV-001): an unknown contact gets no address row; a contact with no
/// address reads `None`; an unknown row confirms and bounces nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_38_nothing_is_stored_or_changed_for_what_is_not_there(pool: PgPool) {
    let (_, contact) = open_job(&pool, 0x7298).await;
    let address = UpdateAddress::parse("pat@mail.test").expect("well formed");
    let stranger = Uuid::from_u128(0x7299);
    let mut db = pool.acquire().await.expect("db");
    let stored = store_update_address(&mut db, &vault(), stranger, &address, at(1)).await;
    assert_eq!(stored.err(), Some(VaultError::NotFound));
    assert!(state(&pool, stranger).await.is_none());
    let read = read_update_address(&mut db, &vault(), contact)
        .await
        .expect("read");
    assert!(read.is_none(), "she gave no address");
    assert!(
        !confirm_update_address(&mut db, stranger, at(2))
            .await
            .expect("confirm")
    );
    assert!(
        !mark_update_bounced(&mut db, stranger, at(3))
            .await
            .expect("bounce")
    );
}

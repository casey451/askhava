//! The owner help page and its reply box (TD-OPS-23; OPS-036, SEC-005, WORD-021; T530, D-3883,
//! D-4726): signed out it shows nothing; a 300-character reply goes as "<brand>: Reply from the
//! <brand> owner: ...", once per press, held for quiet hours; 301 characters, a phone number or
//! an outside link is refused; a business, a help email, a homeowner without consent and a
//! server without texting store and queue nothing. The router has no session mint (a test crate
//! may not name it), so no kept words are read here. PostgreSQL through `#[sqlx::test]`; fake
//! data only.

use std::sync::Arc;

use askhava_core::api::{ManualClock, brand};
use askhava_messaging::api::{AreaCodes, Language, SendWindow, window};
use askhava_ops::api::{Asker, HelpLimits, open_help_request};
use askhava_portals::api::{
    CookieJar, OWNER_HELP_CLOSE_ROUTE, OWNER_HELP_REPLY_ROUTE, OWNER_HELP_ROUTE, OwnerAuth,
    SecondFactor, bootstrap_owner, csrf_token, owner_help_routes, owner_sign_in, reply_body,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
const ASKED: &str = "my sign-in code never came";
const OUTBOX: &str = "SELECT count(*) FROM messaging.outbox";
const REPLIES: &str = "SELECT count(*) FROM ops.help_message WHERE from_owner";
/// Names from the WORD-021 scan's kind: none may be in a reply (TD-WORD-04, D-493).
const NAMES: [&str; 3] = ["Casey", "Pat", "Hava "];

/// 21:00 in Arizona on a fake day.
fn night() -> Timestamp {
    "2026-11-21T04:00:00Z".parse().expect("time")
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The bootstrapped owner signed in at [`night`] with a TOTP code; the session's jar.
async fn signed_in(db: &mut PgConnection) -> CookieJar {
    let now = night();
    bootstrap_owner(db, Some(TOKEN), PASSWORD, now)
        .await
        .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let code = totp_code(&secret, now);
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    owner_sign_in(db, CookieJar::new(), factor, &auth, now)
        .await
        .expect("signed in")
}

/// A help request from `asker` with [`ASKED`] kept; with `consent`, a homeowner record and one
/// SMS consent record for `contact`.
async fn asked(db: &mut PgConnection, asker: Asker, consent: Option<Uuid>) -> Uuid {
    let inbox = Uuid::new_v4();
    let at = night() - SignedDuration::from_mins(5);
    let key = (inbox, at, at + SignedDuration::from_hours(24));
    let opened = open_help_request(db, asker, key, HelpLimits::default()).await;
    let request = opened.expect("opened").request();
    sqlx::query(
        "INSERT INTO privacy.message_body (inbox_id, job_id, words, safety, created_at) \
         VALUES ($1, $2, $3, false, now())",
    )
    .bind(inbox)
    .bind(request)
    .bind(ASKED)
    .execute(&mut *db)
    .await
    .expect("words");
    if let Some(contact) = consent {
        sqlx::query(
            "WITH h AS (INSERT INTO intake.homeowner (contact_id, created_at) \
               VALUES ($1, now()) RETURNING id), \
             c AS (INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
               consent_template, consent_sent_at, opened_at) SELECT h.id, 'en', \
               gen_random_uuid(), 'consent_en', now(), now() FROM h RETURNING id, homeowner_id) \
             INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, inbox_id, \
               consent_version, consented_at) SELECT c.homeowner_id, c.id, 'sms', \
               gen_random_uuid(), 'con_031.v1', now() FROM c",
        )
        .bind(contact)
        .execute(db)
        .await
        .expect("consent");
    }
    request
}

fn served(pool: &PgPool, texting: bool) -> Router {
    let secret = SecretString::from("fake-id-secret-0123456789abcdef-0123456789abcdef");
    let clock = Arc::new(ManualClock::new(night()));
    owner_help_routes(pool.clone(), clock, None, texting.then_some(secret))
}

/// The status and body of `GET` `uri`, or with `form` its same-origin `POST`, under `jar`.
async fn send(app: &Router, jar: &CookieJar, uri: &str, form: Option<String>) -> (u16, String) {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let request = Request::builder()
        .uri(uri)
        .header(header::COOKIE, cookies.join("; "))
        .header("sec-fetch-site", "same-origin");
    let request = match form {
        Some(body) => request
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body)),
        None => request.body(Body::empty()),
    };
    let response = app.clone().oneshot(request.expect("request")).await;
    let response = response.expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// Posts reply `text` on `request` under a new press ID, or `press`.
async fn reply(
    app: &Router,
    jar: &CookieJar,
    (request, press): (Uuid, Option<Uuid>),
    text: &str,
) -> (u16, String) {
    let csrf = csrf_token(jar).expect("csrf");
    let press = press.unwrap_or_else(Uuid::new_v4);
    let text = text
        .replace(' ', "+")
        .replace(':', "%3A")
        .replace('/', "%2F");
    let form = format!("csrf={csrf}&request={request}&press={press}&text={text}");
    send(app, jar, OWNER_HELP_REPLY_ROUTE, Some(form)).await
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_23_the_help_page_shows_nothing_without_the_owners_sign_in(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let contact = Uuid::new_v4();
    let request = asked(&mut db, Asker::Homeowner(contact), Some(contact)).await;
    let jar = signed_in(&mut db).await;
    let app = served(&pool, true);
    let uri = format!("{OWNER_HELP_ROUTE}?request={request}");
    let (status, page) = send(&app, &CookieJar::new(), &uri, None).await;
    assert_eq!(status, 401);
    assert!(!page.contains(ASKED) && !page.contains(&request.to_string()));
    let (status, page) = send(&app, &jar, &uri, None).await;
    assert_eq!(status, 200);
    assert!(
        page.contains(&request.to_string()) && page.contains("Reply by"),
        "{page}"
    );
    let form = format!("csrf={}&request={request}", csrf_token(&jar).expect("csrf"));
    let (status, page) = send(&app, &jar, OWNER_HELP_CLOSE_ROUTE, Some(form.clone())).await;
    assert!(status == 200 && page.contains("Request closed."), "{page}");
    let form = format!("{form}&reopen=1");
    let (status, page) = send(&app, &jar, OWNER_HELP_CLOSE_ROUTE, Some(form)).await;
    assert!(
        status == 200 && page.contains("Request reopened."),
        "{page}"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_23_a_300_character_reply_goes_once_and_waits_for_8_am(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let contact = Uuid::new_v4();
    let request = asked(&mut db, Asker::Homeowner(contact), Some(contact)).await;
    let jar = signed_in(&mut db).await;
    let app = served(&pool, true);
    let text = format!("{}.", "help ".repeat(60).trim_end());
    assert_eq!(text.chars().count(), 300);
    let press = Uuid::new_v4();
    for _ in 0..2 {
        let (status, page) = reply(&app, &jar, (request, Some(press)), &text).await;
        assert!(
            status == 200 && page.contains("Reply saved."),
            "{status} {page}"
        );
    }
    let (body, template, to, exempt): (String, String, Uuid, bool) = sqlx::query_as(
        "SELECT body, template_id, recipient_id, quiet_exempt_until IS NOT NULL \
         FROM messaging.outbox",
    )
    .fetch_one(&pool)
    .await
    .expect("one text");
    let short = brand().short_name();
    assert_eq!(
        body,
        format!("{short}: Reply from the {short} owner: {text}")
    );
    assert_eq!(
        (template.as_str(), to, exempt),
        ("owner_reply", contact, false)
    );
    assert!(NAMES.iter().all(|name| !body.contains(name)));
    assert!(
        NAMES
            .iter()
            .all(|n| !reply_body(Language::Es, "hola").contains(n))
    );
    let table = AreaCodes::embedded().expect("area codes");
    let held = window(&table, "+19285550142", night()).expect("window");
    let eight = "2026-11-21T15:00:00Z".parse().expect("08:00 in Arizona");
    assert_eq!(held, SendWindow::Hold { until: eight });
    assert_eq!(count(&pool, REPLIES).await, 1);
    let own = format!("See https://{}/help for more.", brand().domain());
    let (status, _) = reply(&app, &jar, (request, None), &own).await;
    assert_eq!((status, count(&pool, OUTBOX).await), (200, 2));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_23_a_long_or_unscreened_reply_and_an_email_asker_send_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let contact = Uuid::new_v4();
    let request = asked(&mut db, Asker::Homeowner(contact), Some(contact)).await;
    let other = Uuid::new_v4();
    let no_consent = asked(&mut db, Asker::Homeowner(other), None).await;
    let business = asked(&mut db, Asker::Business(Uuid::new_v4()), None).await;
    let email = asked(&mut db, Asker::Email, None).await;
    let jar = signed_in(&mut db).await;
    let app = served(&pool, true);
    let long = format!("{}..", "help ".repeat(60).trim_end());
    assert_eq!(long.chars().count(), 301);
    let refused = [
        (request, long.as_str(), "over 300 characters"),
        (request, "Call me at 928-555-0142 today.", "Take out"),
        (
            request,
            "See https://example.com/help for more.",
            "Take out",
        ),
        (request, " ", "Type a reply first."),
        (no_consent, "We sent it again.", "by email for now"),
        (business, "We sent it again.", "by email for now"),
        (email, "We sent it again.", "by email for now"),
        (Uuid::new_v4(), "We sent it again.", "That request wasn"),
    ];
    for (request, text, line) in refused {
        let (status, page) = reply(&app, &jar, (request, None), text).await;
        assert!(
            status == 400 && page.contains(line),
            "{text}: {status} {page}"
        );
    }
    let (status, page) = reply(&served(&pool, false), &jar, (request, None), "We sent it.").await;
    assert!(status == 400 && page.contains("set up yet"), "{page}");
    let form = format!("csrf=fake&request={request}&press={request}&text=hello");
    let (status, _) = send(&app, &CookieJar::new(), OWNER_HELP_REPLY_ROUTE, Some(form)).await;
    assert_eq!(status, 403);
    assert_eq!(
        (count(&pool, OUTBOX).await, count(&pool, REPLIES).await),
        (0, 0)
    );
    let kept = "SELECT count(*) FROM privacy.message_body";
    assert_eq!(count(&pool, kept).await, 4);
}

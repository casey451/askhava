//! Twilio's inbound-text and status routes on a real database (TD-TXT-04, TD-TXT-11, TD-INT-08,
//! TD-CON-05; TXT-012, TXT-031, INT-019, CON-008, SEC-016, SEC-039; T241).
//!
//! The routes under test are `app/server/src/inbound_text.rs`, compiled into this crate with
//! `#[path]` (a test crate may not depend on `server`), driven through `tower::ServiceExt` with
//! deliveries signed by a fake Twilio token over the fake `ASKHAVA_PUBLIC_BASE_URL` plus the
//! route path and query. Proves: an unsigned or wrongly signed delivery is 403 and changes
//! nothing; the same MessageSid three times opens one request with one consent text, and a
//! status callback is recorded once per (MessageSid, MessageStatus); an opt-out word ends the
//! text at `messaging`, and later texts from that number reach no request until START; HELP gets
//! only the HELP reply; a reply restarts the 72-hour wait; LIST gets the LIST text, and a LIST
//! whose list cannot be built is heard as a reply (D-1310); a business phone's text reaches no
//! homeowner path; the base URL setting's rules. All data is fake.

use std::sync::Arc;

use askhava_core::api::{ManualClock, Settings, SettingsError};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_messaging::api::sender_add;
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{
    DayPart, Format, Price, QuoteForm, QuoteRules, parse_quote, submit_quote,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, STATUS_PATH, Webhooks, routes};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The routes under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HOME: &str = "+19285550142";
const SHOP: &str = "+19285550150";
const ASK_HAVA: &str = "+19285550100";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn hooks(pool: &PgPool, names: Vec<(Uuid, String)>) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        names,
        intake: IntakeSettings::default(),
    })
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// POSTs `fields` to `path` (with its query), signed over `signed_base` + `path`; the status.
async fn post(
    hooks: &Arc<Webhooks>,
    path: &str,
    fields: &[(&str, &str)],
    signed_base: &str,
) -> StatusCode {
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{signed_base}{path}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    routes(Arc::clone(hooks))
        .oneshot(request)
        .await
        .expect("answer")
        .status()
}

async fn text(hooks: &Arc<Webhooks>, sid: u32, from: &str, words: &str) -> StatusCode {
    let sid = format!("SM{sid:032}");
    let fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    post(hooks, INBOUND_PATH, &fields, BASE).await
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn templates(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT template_id FROM messaging.outbox ORDER BY created_at, template_id")
        .fetch_all(pool)
        .await
        .expect("outbox")
}

/// TD-TXT-04 (SEC-016, SEC-039): unsigned, forged or signed over another URL is 403 and records
/// nothing; a signed URL includes its query string.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_only_a_signed_delivery_reaches_the_app(pool: PgPool) {
    let h = hooks(&pool, Vec::new());
    let fields = [
        ("MessageSid", "SM00000000000000000000000000000001"),
        ("From", HOME),
        ("To", ASK_HAVA),
        ("Body", "hi"),
    ];
    assert_eq!(
        post(&h, INBOUND_PATH, &fields, "https://other.example.test").await,
        StatusCode::FORBIDDEN
    );
    let unsigned = Request::post(INBOUND_PATH)
        .body(Body::from("MessageSid=SM1"))
        .expect("request");
    let status = routes(Arc::clone(&h))
        .oneshot(unsigned)
        .await
        .expect("answer")
        .status();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.inbox").await,
        0
    );
    let path = format!("{STATUS_PATH}?attempt=1");
    let fields = [
        ("MessageSid", "SM00000000000000000000000000000009"),
        ("MessageStatus", "delivered"),
    ];
    assert_eq!(post(&h, &path, &fields, BASE).await, StatusCode::OK);
}

/// TD-TXT-04 (TXT-012, SEC-039): the same inbound MessageSid three times opens one request with
/// one consent text; a status callback three times is recorded once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_the_same_message_id_three_times_is_handled_once(pool: PgPool) {
    let h = hooks(&pool, Vec::new());
    for _ in 0..3 {
        assert_eq!(
            text(&h, 1, HOME, "my kitchen sink is leaking").await,
            StatusCode::OK
        );
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.inbox").await,
        1
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM intake.conversation").await,
        1
    );
    assert_eq!(templates(&pool).await, ["consent"]);
    let fields = [
        ("MessageSid", "SM00000000000000000000000000000002"),
        ("MessageStatus", "delivered"),
    ];
    for _ in 0..3 {
        assert_eq!(post(&h, STATUS_PATH, &fields, BASE).await, StatusCode::OK);
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.delivery_event").await,
        1
    );
}

/// TD-CON-05 (CON-008, CON-013, CON-016, CON-018): STOP ends at `messaging` with one
/// confirmation and opens nothing; a later text reaches no request; HELP gets only its reply;
/// the provider's START opens a new request from the consent text (CON-015).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_05_an_opt_out_ends_the_text_at_messaging(pool: PgPool) {
    let h = hooks(&pool, Vec::new());
    assert_eq!(text(&h, 1, HOME, "STOP").await, StatusCode::OK);
    assert_eq!(
        text(&h, 2, HOME, "my sink is leaking").await,
        StatusCode::OK
    );
    assert_eq!(text(&h, 3, HOME, "HELP").await, StatusCode::OK);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM intake.conversation").await,
        0
    );
    assert_eq!(
        templates(&pool).await,
        ["opt_out_confirmation", "help_reply"]
    );
    let sid = format!("SM{:032}", 4);
    let start = [
        ("MessageSid", sid.as_str()),
        ("From", HOME),
        ("To", ASK_HAVA),
        ("Body", "START"),
        ("OptOutType", "START"),
    ];
    assert_eq!(post(&h, INBOUND_PATH, &start, BASE).await, StatusCode::OK);
    assert_eq!(
        templates(&pool).await,
        ["opt_out_confirmation", "help_reply", "consent"]
    );
}

/// TD-INT-08 (INT-019) and the business phone (D-492): a reply through the route restarts the
/// wait; a linked business phone's text is recorded and opens no request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_a_reply_restarts_the_wait_and_a_business_text_opens_nothing(pool: PgPool) {
    let h = hooks(&pool, Vec::new());
    let mut db = pool.acquire().await.expect("connection");
    sender_add(
        &mut db,
        &h.vault,
        (Uuid::from_u128(7), Uuid::from_u128(8)),
        SHOP,
    )
    .await
    .expect("shop");
    assert_eq!(
        text(&h, 1, SHOP, "my sink is leaking").await,
        StatusCode::OK
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.inbox").await,
        1
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM intake.conversation").await,
        0
    );
    let job = job(&pool, &h).await;
    assert_eq!(
        text(&h, 3, HOME, "it drips under the cabinet").await,
        StatusCode::OK
    );
    let due: Vec<jiff_sqlx::Timestamp> =
        sqlx::query_scalar("SELECT run_at FROM core.task WHERE kind = $1 AND payload->>'job' = $2")
            .bind(askhava_intake::api::NO_REPLY_KIND)
            .bind(job.to_string())
            .fetch_all(&pool)
            .await
            .expect("steps");
    let three_days = at() + jiff::SignedDuration::from_hours(72);
    assert!(due.iter().any(|t| t.to_jiff() == three_days));
}

/// A request opened by a route text one hour before [`at`], with one plumbing job.
async fn job(pool: &PgPool, h: &Arc<Webhooks>) -> Uuid {
    assert_eq!(
        text(h, 2, HOME, "my kitchen sink is leaking").await,
        StatusCode::OK
    );
    let mut db = pool.acquire().await.expect("connection");
    let id: Uuid = sqlx::query_scalar("SELECT id FROM intake.conversation")
        .fetch_one(pool)
        .await
        .expect("request");
    let start = askhava_intake::api::JobStart {
        conversation: id,
        words: "my kitchen sink is leaking",
        emergency: false,
        trade: "plumbing_repair",
        at: at() - jiff::SignedDuration::from_hours(1),
    };
    match askhava_intake::api::begin_job(&mut db, &start)
        .await
        .expect("job")
    {
        askhava_intake::api::Opened::Created(job) => job,
        _ => panic!("a new job"),
    }
}

/// A request with one job opened through the route, moved to Open, holding one shown $190 quote
/// from fake business 1.
async fn quoted_job(pool: &PgPool, h: &Arc<Webhooks>) -> Uuid {
    let job = job(pool, h).await;
    let mut db = pool.acquire().await.expect("connection");
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        at(),
        Actor::System,
    )
    .await;
    assert!(moved.expect("open").applied().is_some());
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(Uuid::from_u128(1))
        .bind(at().to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: "Sample wording".to_owned(),
        ai_read: false,
        earliest_visit: Some((jiff::civil::date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    assert!(
        submit_quote(&mut db, (job, Uuid::from_u128(1)), &checked, at())
            .await
            .is_ok()
    );
    job
}

/// TD-TXT-11 (TXT-031): LIST through the route gets the LIST text naming the quoting business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_11_list_through_the_route_gets_the_list_text(pool: PgPool) {
    let names = vec![(Uuid::from_u128(1), "Sample Business 1".to_owned())];
    let h = hooks(&pool, names);
    quoted_job(&pool, &h).await;
    assert_eq!(text(&h, 4, HOME, "LIST").await, StatusCode::OK);
    let body: String =
        sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = 'quote_list'")
            .fetch_one(&pool)
            .await
            .expect("list");
    assert!(body.contains("1. Sample Business 1: $190."), "{body}");
}

/// TD-TXT-11, TD-INT-08 (TXT-031, INT-019, D-1310): LIST with a quote waiting but no public name
/// for its business (production's empty `names`) is not lost: no list goes, and the text is
/// heard as a reply, so the request's Intake job's 72-hour wait restarts at it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_11_list_whose_list_cannot_be_built_is_heard_as_a_reply(pool: PgPool) {
    let h = hooks(&pool, Vec::new());
    quoted_job(&pool, &h).await;
    let mut db = pool.acquire().await.expect("connection");
    let request: Uuid = sqlx::query_scalar("SELECT id FROM intake.conversation")
        .fetch_one(&pool)
        .await
        .expect("request");
    let start = askhava_intake::api::JobStart {
        conversation: request,
        words: "the porch light flickers",
        emergency: false,
        trade: "electrical",
        at: at() - jiff::SignedDuration::from_hours(1),
    };
    let Ok(askhava_intake::api::Opened::Created(waiting)) =
        askhava_intake::api::begin_job(&mut db, &start).await
    else {
        panic!("a second job");
    };
    assert_eq!(text(&h, 4, HOME, "LIST").await, StatusCode::OK);
    let lists = "SELECT count(*) FROM messaging.outbox WHERE template_id LIKE 'quote_list%'";
    assert_eq!(count(&pool, lists).await, 0);
    let job = askhava_jobs::api::current(&mut db, waiting).await;
    assert_eq!(job.expect("job").timer_from, at());
}

/// `Settings::from_pairs` in `env` with `ASKHAVA_PUBLIC_BASE_URL` set to `url`; its value.
fn parse_base(env: &str, url: &str) -> Result<Option<String>, SettingsError> {
    let test =
        "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
    let live =
        "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_PUBLIC_BASE_URL", url),
    ];
    if env == "live" {
        pairs.extend([
            ("ASKHAVA_ALLOW_LIST_TEST", test),
            ("ASKHAVA_ALLOW_LIST_LIVE", live),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01"),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
            ("ASKHAVA_RESEND_DOMAIN", "mail.example.com"),
            ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
        ]);
    }
    let pairs = pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned()));
    Settings::from_pairs(pairs).map(|s| s.public_base_url().map(str::to_owned))
}

/// SEC-016, D-1216: `ASKHAVA_PUBLIC_BASE_URL` is optional, loses a trailing slash, must be an
/// `http(s)` origin with no query, and in live only `https` to a non-local host.
#[test]
fn the_public_base_url_setting() {
    let parse = parse_base;
    assert_eq!(parse("dev", "").expect("unset"), None);
    assert_eq!(
        parse("dev", "http://127.0.0.1:3000/").expect("dev"),
        Some("http://127.0.0.1:3000".to_owned())
    );
    assert_eq!(
        parse("live", "https://hooks.example.test").expect("live"),
        Some(BASE.to_owned())
    );
    for bad in [
        "file:///tmp/x",
        "ftp://hooks.example.test",
        "https://hooks.example.test/?a=1",
        "https://",
    ] {
        let refused = parse("dev", bad).map(|_| ());
        assert_eq!(
            refused,
            Err(SettingsError::Invalid("ASKHAVA_PUBLIC_BASE_URL")),
            "{bad}"
        );
    }
    let local = parse("live", "http://127.0.0.1:3000").map(|_| ());
    assert_eq!(
        local,
        Err(SettingsError::StandInInLive("ASKHAVA_PUBLIC_BASE_URL"))
    );
}

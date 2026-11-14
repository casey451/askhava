//! SEC-015's all-numbers limit counts 911-list first texts through the served inbound text route,
//! and never refuses one (SEC-015, SEC-014, INT-002, INT-004; D-1951, D-3083, D-3581, D-3613;
//! T449).
//!
//! The route under test is `app/server/src/inbound_text.rs`, compiled into this crate with
//! `#[path]` (a test crate may not depend on `server`), driven through `tower::ServiceExt` with
//! deliveries signed by a fake Twilio token over the fake `ASKHAVA_PUBLIC_BASE_URL`, set up as
//! T317's `inbound_text_guards.rs`. Proves: 101 emergency first texts from fake numbers in one
//! hour each open their request and queue their 911 text, and raise one High
//! `intake.requests_all` alert with no subject; with the counter failing (a fake trigger on its
//! table), an emergency first text still opens its request and queues its 911 text, and the
//! failure is logged with no phone or body. All data is fake.

use std::sync::{Arc, Mutex};

use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::REQUESTS_ALL;
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_privacy::api::Vault;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::Timestamp;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HOME: &str = "+19285550142";
const ASK_HAVA: &str = "+19285550100";
const GAS: &str = "I smell gas in the kitchen";
const SAFETY: &str = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'emergency_911'";
const REQUESTS: &str = "SELECT count(*) FROM intake.conversation";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: Vault::new(&key(0x53), &key(0x63), &url).expect("vault"),
        environment: Environment::Test,
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

/// Text `words` from `from` as MessageSid number `sid`, signed; the route's status.
async fn text(hooks: &Arc<Webhooks>, sid: u32, from: &str, words: &str) -> StatusCode {
    let sid = format!("SM{sid:032}");
    let mut fields = vec![
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(hooks)).oneshot(request).await;
    answer.expect("answer").status()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// The all-numbers alerts: severity and subject.
async fn burst_alerts(pool: &PgPool) -> Vec<(String, Option<Uuid>)> {
    sqlx::query_as("SELECT severity, subject_id FROM ops.alert WHERE kind = $1")
        .bind(REQUESTS_ALL.scope)
        .fetch_all(pool)
        .await
        .expect("alerts")
}

/// Fake number `n` (0 to 199): `+1928555` or `+1602555` with `0100` to `0199`.
fn fake_phone(n: u32) -> String {
    let area = if n < 100 { 928 } else { 602 };
    format!("+1{area}5550{}", 100 + n % 100)
}

/// SEC-015, INT-002, INT-004, D-3613: 101 emergency first texts from fake numbers in one hour
/// each open their request and queue their 911 text, never refused, and the all-numbers limit
/// passed raises one High alert with no subject.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_015_a_burst_of_emergency_first_texts_is_alerted_never_refused(pool: PgPool) {
    let h = hooks(&pool);
    for n in 0..101 {
        assert_eq!(text(&h, n + 1, &fake_phone(n), GAS).await, StatusCode::OK);
    }
    assert_eq!(
        count(&pool, REQUESTS).await,
        101,
        "every one opens its request"
    );
    assert_eq!(
        count(&pool, SAFETY).await,
        101,
        "every one gets the 911 text"
    );
    let hits: i64 =
        sqlx::query_scalar("SELECT hits::bigint FROM core.rate_limit WHERE bucket = $1")
            .bind(format!("{}:all", REQUESTS_ALL.scope))
            .fetch_one(&pool)
            .await
            .expect("hits");
    assert_eq!(hits, 101, "each one is counted");
    assert_eq!(burst_alerts(&pool).await, vec![("high".to_owned(), None)]);
}

/// Every app (non-sqlx) log line's fields, for the test's thread.
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
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        !meta.target().starts_with("sqlx")
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

/// INT-002, INT-004, D-3613: with the all-numbers counter failing (a fake trigger on its table),
/// an emergency first text still opens its request and queues its 911 text, the route answers
/// 200, and the failure is logged once, with no phone or body.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn int_002_a_failing_counter_never_changes_the_emergency(pool: PgPool) {
    let fail = "CREATE FUNCTION public.t449_fail() RETURNS trigger LANGUAGE plpgsql AS \
                $$ BEGIN RAISE EXCEPTION 'fake failure'; END $$";
    let trigger = "CREATE TRIGGER t449_fail BEFORE INSERT OR UPDATE ON core.rate_limit \
                   FOR EACH ROW WHEN (NEW.bucket LIKE 'intake.requests_all%') \
                   EXECUTE FUNCTION public.t449_fail()";
    for sql in [fail, trigger] {
        sqlx::query(sql).execute(&pool).await.expect("trigger");
    }
    let h = hooks(&pool);
    let lines = Lines::default();
    let status = {
        let _guard = tracing::subscriber::set_default(lines.clone());
        text(&h, 1, HOME, GAS).await
    };
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&pool, REQUESTS).await, 1, "the request opens");
    assert_eq!(count(&pool, SAFETY).await, 1, "the 911 text is queued");
    assert!(burst_alerts(&pool).await.is_empty());
    let logged = lines.all();
    let failed = "emergency request count failed";
    assert_eq!(logged.matches(failed).count(), 1, "logged once: {logged}");
    assert!(logged.contains("error=\"Counter\""), "{logged}");
    assert!(
        !logged.contains("5550142") && !logged.contains("gas"),
        "{logged}"
    );
}

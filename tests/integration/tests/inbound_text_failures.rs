//! TD-TXT-34 through Twilio's inbound route on a real database (TXT-012, D-801, D-1311, D-2207,
//! T325): a text a module error stops after the repeat check (`Halt::Failed`) is answered 200 and
//! raises one High `messaging.inbound_text_failed` alert about its inbox row, the failed step in
//! the alert's audit row, with no body, phone or error text in either; a resend of the same
//! MessageSid stops at the repeat check and raises nothing.
//!
//! The route is `app/server/src/inbound_text.rs`, compiled in with `#[path]` as in
//! `inbound_scrub_context.rs`. The module error is the one D-2088 names: her stated name was
//! sealed under one `VAULT_KEY` and the route's vault holds another, so `privacy` cannot open it
//! (`Halt::Failed("vault", …)`). The warn line is caught by a small `tracing` subscriber for the
//! test's thread, to read the error's variant name. Every name and phone is fictional.

use std::sync::{Arc, Mutex};

use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_privacy::api::Vault;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{FAILED_ALERT, INBOUND_PATH, Webhooks, routes};
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
const HER: &str = "+19285550181";
const ASK_HAVA: &str = "+19285550100";

fn vault(seal_byte: u8) -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(seal_byte).into(), &key(0x6a).into(), &url).expect("vault")
}

fn hooks(pool: &PgPool, vault: Vault) -> Arc<Webhooks> {
    let at: Timestamp = "2026-11-12T17:00:00Z".parse().expect("instant");
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault,
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

/// POSTs inbound text `n` (MessageSid `SM` + `n`) from her, signed by the fake token.
async fn text(h: &Arc<Webhooks>, n: u32, words: &str) -> StatusCode {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", HER),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    let encode = |v: &str| -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
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
    let answer = routes(Arc::clone(h))
        .oneshot(request)
        .await
        .expect("answer");
    answer.status()
}

/// Every log line's fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Subscriber for Lines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

impl Lines {
    /// The `error` value of the step-failed warn line.
    fn failed_error(&self) -> String {
        let lines = self.0.lock().expect("lines");
        let line = lines
            .iter()
            .find(|l| l.contains("the text was not handled"))
            .expect("the warn line");
        let tail = line.split(" error=\"").nth(1).expect("an error field");
        tail.split('"').next().expect("its value").to_owned()
    }
}

/// Every `ops.alert` and `ops.audit_log` row about this alert kind, as JSON text.
async fn failure_rows(pool: &PgPool) -> (Vec<String>, Vec<String>) {
    let alerts = sqlx::query_scalar(
        "SELECT row_to_json(a)::text FROM ops.alert a WHERE kind = $1 ORDER BY raised_at",
    )
    .bind(FAILED_ALERT)
    .fetch_all(pool)
    .await
    .expect("alerts");
    let audits = sqlx::query_scalar(
        "SELECT row_to_json(l)::text FROM ops.audit_log l WHERE action = $1 ORDER BY id",
    )
    .bind(FAILED_ALERT)
    .fetch_all(pool)
    .await
    .expect("audit rows");
    (alerts, audits)
}

/// TD-TXT-34 (TXT-012, D-801, D-1311, D-2207): a text the vault cannot read past the opt-out and
/// HELP steps is answered 200 with one High alert about its inbox row, its audit row naming the
/// `vault` step; neither row holds the body, her phone or the error; a resend of the same
/// MessageSid is 200 and raises nothing more; a text handled to the end raises no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_34_a_failed_text_raises_one_body_free_alert(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    assert_eq!(
        text(&hooks(&pool, vault(0x5a)), 1, "I'm Zebulon").await,
        StatusCode::OK
    );
    assert_eq!(
        failure_rows(&pool).await,
        (vec![], vec![]),
        "a handled text"
    );
    let other_key = hooks(&pool, vault(0x5c));
    assert_eq!(
        text(&other_key, 2, "zebulon's sink leaks").await,
        StatusCode::OK
    );
    let inbox: Uuid = sqlx::query_scalar("SELECT id FROM messaging.inbox WHERE message_sid = $1")
        .bind(format!("SM{:032}", 2))
        .fetch_one(&pool)
        .await
        .expect("its inbox row");
    let alert: (Uuid, String, String, Uuid) = sqlx::query_as(
        "SELECT id, severity, subject_kind, subject_id FROM ops.alert WHERE kind = $1",
    )
    .bind(FAILED_ALERT)
    .fetch_one(&pool)
    .await
    .expect("one alert");
    assert_eq!(
        (alert.1.as_str(), alert.2.as_str(), alert.3),
        ("high", "messaging.inbox", inbox)
    );
    let audit: (String, String, Uuid, Option<String>) = sqlx::query_as(
        "SELECT actor_kind, subject_kind, subject_id, field FROM ops.audit_log WHERE action = $1",
    )
    .bind(FAILED_ALERT)
    .fetch_one(&pool)
    .await
    .expect("one audit row");
    assert_eq!(
        audit,
        (
            "system".to_owned(),
            "ops.alert".to_owned(),
            alert.0,
            Some("vault".to_owned())
        )
    );
    assert_eq!(
        text(&other_key, 2, "zebulon's sink leaks").await,
        StatusCode::OK
    );
    let (alerts, audits) = failure_rows(&pool).await;
    assert_eq!(
        (alerts.len(), audits.len()),
        (1, 1),
        "a resend raises nothing"
    );
    let error = lines.failed_error();
    let rows = format!("{alerts:?}{audits:?}");
    for leak in [
        "zebulon",
        "Zebulon",
        "sink",
        "leaks",
        "9285550181",
        error.as_str(),
    ] {
        assert!(!rows.contains(leak), "the alert rows hold {leak:?}");
    }
}

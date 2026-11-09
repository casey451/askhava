//! TD-AI-19 through Twilio's inbound route on a real database (AI-003, AI-020, PRIV-001, PRIV-003,
//! PRIV-007, PRIV-024; D-1752, T257): a name a homeowner stated, and her vault name, are removed
//! from her later texts, across a restart, never from another homeowner's, and never reach an AI
//! request, a row outside `privacy` or a log line.
//!
//! The route is `app/server/src/inbound_text.rs`, compiled in with `#[path]` as in
//! `inbound_first_contact.rs`. The route does not call the AI; what it would send is the
//! `ScrubbedText` `privacy::api::read_inbound_for` returns for her next text, read here in a
//! transaction that is rolled back, and handed to `ai::api::read_intake` over a recording stand-in
//! transport. Log lines are caught by a small `tracing` subscriber for the test's thread. Every
//! name, phone and address is fictional.

use std::sync::{Arc, Mutex};

use askhava_ai::api::{
    AiConfig, Endpoint, Fault, IntakeCall, OpenTrades, Origin, Reply, Transport, read_intake,
};
use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_privacy::api::{
    InboundWords, NewContact, Vault, delete_my_data, find_by_phone, purge_contact,
    read_inbound_for, rotate_batch, store_contact,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, Connection, PgPool};
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HER: &str = "+19285550171";
const OTHER: &str = "+19285550172";
const VAULT_HER: &str = "+19285550173";
const ASK_HAVA: &str = "+19285550100";
const NAMES: [&str; 4] = ["zebulon", "quartermaine", "ottoline", "vexley"];
const WORDS: InboundWords<'static> = InboundWords {
    emergency: &[],
    opt_out: &["stop"],
    help: &["help"],
    start: &["start"],
};

fn key(version: u8, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn vault(seal: &str, index: &str) -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&seal.to_owned().into(), &index.to_owned().into(), &url).expect("vault")
}

fn v1() -> Vault {
    vault(&key(1, 0x5a), &key(1, 0x6a))
}

fn hooks(pool: &PgPool, vault: Vault) -> Arc<Webhooks> {
    let at: Timestamp = "2026-11-09T17:00:00Z".parse().expect("instant");
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault,
        names: Vec::new(),
        intake: IntakeSettings::default(),
    })
}

/// POSTs inbound text `n` from `from`, signed by the fake token over `BASE` + the path.
async fn text(h: &Arc<Webhooks>, n: u32, from: &str, words: &str) -> StatusCode {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
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

/// The stand-in AI's recording transport: every request body kept, a fixed valid reading back.
#[derive(Default)]
struct Recording(Mutex<Vec<String>>);

impl Transport for Recording {
    async fn post(&self, _: Endpoint, _: &str, _: Uuid, body: &Value) -> Result<Reply, Fault> {
        self.0.lock().expect("lock").push(body.to_string());
        let fields = json!({"trade": "plumbing_repair", "trade_confidence": 0.9,
            "job_type": "leak repair", "job_type_confidence": 0.8, "emergency": false,
            "urgent": false, "language": "en", "major": false, "missing_info": []});
        let body = json!({"id": "msg_fake19", "stop_reason": "end_turn",
            "content": [{"type": "text", "text": fields.to_string()}]});
        Ok(Reply {
            body,
            request_id: Some("req_fake19".to_owned()),
        })
    }
}

/// What the AI would be sent for `raw` from the homeowner of `phone`: her contact-aware read in a
/// rolled-back transaction, then one gated intake reading over `model` (Dev, so the AI may run).
async fn ai_sees(pool: &PgPool, vault: &Vault, model: &Recording, phone: &str, raw: &str) {
    let mut db = pool.acquire().await.expect("connection");
    let who = find_by_phone(&mut db, vault, phone)
        .await
        .expect("lookup")
        .expect("contact");
    let mut tx = db.begin().await.expect("tx");
    let read = read_inbound_for(&mut tx, vault, who, raw, &WORDS)
        .await
        .expect("read");
    tx.rollback().await.expect("rollback");
    let open = OpenTrades::new([("plumbing_repair".to_owned(), vec!["Leak repair".to_owned()])]);
    let call = IntakeCall {
        subject: Uuid::from_u128(19),
        origin: Origin::Text,
        text: &read.text,
        open: &open,
    };
    let config = AiConfig::from_pairs([]).expect("config");
    read_intake(&mut db, model, &config, Environment::Dev, &call)
        .await
        .expect("reading");
}

fn holds_a_name(text: &str) -> bool {
    let lower = text.to_lowercase();
    NAMES.iter().any(|n| lower.contains(n))
}

/// Every table outside `privacy` whose rows, as text, hold any of [`NAMES`].
async fn tables_with_a_name(pool: &PgPool) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT format('%I.%I', schemaname, tablename) FROM pg_tables \
         WHERE schemaname NOT IN ('pg_catalog', 'information_schema', 'privacy')",
    )
    .fetch_all(pool)
    .await
    .expect("tables");
    let mut found = Vec::new();
    for table in tables {
        let sql = format!("SELECT count(*) FROM {table} t WHERE lower(t::text) ~ $1");
        let hits: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(NAMES.join("|"))
            .fetch_one(pool)
            .await
            .expect("scan");
        if hits > 0 {
            found.push(table);
        }
    }
    found
}

async fn stated(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM privacy.stated_name")
        .fetch_one(pool)
        .await
        .expect("stated")
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

impl Subscriber for Lines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// TD-AI-19 (AI-020, PRIV-001, PRIV-003): "I'm Zebulon Quartermaine" then "zebulon's sink
/// leaks" through the route: both names are kept sealed for her, still removed after a restart
/// (new pool, vault and router) in any case, possessive or NFKC form, absent from every AI request,
/// every row outside `privacy` and every log line; another homeowner's same word is untouched.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_a_stated_name_stays_out_through_the_route(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let h = hooks(&pool, v1());
    assert_eq!(
        text(&h, 1, HER, "I'm Zebulon Quartermaine").await,
        StatusCode::OK
    );
    assert_eq!(
        text(&h, 2, HER, "zebulon's sink leaks").await,
        StatusCode::OK
    );
    assert_eq!(stated(&pool).await, 2, "both words, sealed, once each");
    drop(h);
    let fresh = PgPool::connect_with(pool.connect_options().as_ref().clone())
        .await
        .expect("new pool");
    let h = hooks(&fresh, v1());
    assert_eq!(text(&h, 3, HER, "ZEBULON again").await, StatusCode::OK);
    assert_eq!(
        text(&h, 4, OTHER, "my zebulon brand heater").await,
        StatusCode::OK
    );
    assert_eq!(stated(&fresh).await, 2, "a known name adds no row");
    let model = Recording::default();
    for later in [
        "zebulon's sink leaks",
        "Ｚｅｂｕｌｏｎ ＱＵＡＲＴＥＲＭＡＩＮＥ here",
    ] {
        ai_sees(&fresh, &h.vault, &model, HER, later).await;
    }
    let bodies = model.0.lock().expect("bodies").clone();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies.iter().all(|b| !holds_a_name(b)),
        "no name reaches the AI"
    );
    let other = Recording::default();
    ai_sees(&fresh, &h.vault, &other, OTHER, "my zebulon brand heater").await;
    let theirs = other.0.lock().expect("bodies").concat();
    assert!(
        theirs.contains("zebulon"),
        "another homeowner's text is untouched"
    );
    assert_eq!(tables_with_a_name(&fresh).await, Vec::<String>::new());
    let logged = lines.0.lock().expect("lines").concat();
    assert!(!holds_a_name(&logged), "no log line holds the name");
}

/// TD-AI-19 (AI-003, AI-020): her vault name is removed with no introduction, and a Spanish
/// introduction through the route ("me llamo …", "soy …") is remembered.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_the_vault_name_and_spanish_introductions(pool: PgPool) {
    let h = hooks(&pool, v1());
    let mut db = pool.acquire().await.expect("connection");
    let named = NewContact {
        name: Some(SecretString::from("Ottoline Vexley".to_owned())),
        phone: Some(SecretString::from(VAULT_HER.to_owned())),
        ..NewContact::default()
    };
    store_contact(&mut db, &h.vault, &named)
        .await
        .expect("contact");
    assert_eq!(
        text(&h, 1, VAULT_HER, "hola, me llamo Quillon").await,
        StatusCode::OK
    );
    assert_eq!(text(&h, 2, VAULT_HER, "soy Xantipa").await, StatusCode::OK);
    let model = Recording::default();
    ai_sees(
        &pool,
        &h.vault,
        &model,
        VAULT_HER,
        "Vexley here: ottoline, quillon y xantipa",
    )
    .await;
    let body = model.0.lock().expect("bodies").concat().to_lowercase();
    assert!(!holds_a_name(&body), "the vault name is removed");
    assert!(
        !body.contains("quillon") && !body.contains("xantipa"),
        "Spanish names remembered"
    );
    assert_eq!(tables_with_a_name(&pool).await, Vec::<String>::new());
}

/// TD-AI-19 (PRIV-007, PRIV-011, PRIV-024, D-1752): names stated through the route still match
/// after a key rotation, and are gone after the PRIV-007 purge and "Delete my data".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_rotation_purge_and_delete_my_data(pool: PgPool) {
    let h = hooks(&pool, v1());
    assert_eq!(text(&h, 1, HER, "I'm Zebulon").await, StatusCode::OK);
    assert_eq!(
        text(&h, 2, OTHER, "this is Quartermaine").await,
        StatusCode::OK
    );
    let new = vault(
        &format!("{},{}", key(2, 0x5b), key(1, 0x5a)),
        &format!("{},{}", key(2, 0x6b), key(1, 0x6a)),
    );
    let mut db = pool.acquire().await.expect("connection");
    while rotate_batch(&mut db, &new, 10).await.expect("rotate") > 0 {}
    let model = Recording::default();
    ai_sees(&pool, &new, &model, HER, "zebulon's tub").await;
    assert!(!holds_a_name(&model.0.lock().expect("bodies").concat()));
    let her = find_by_phone(&mut db, &new, HER)
        .await
        .expect("find")
        .expect("her");
    let other = find_by_phone(&mut db, &new, OTHER)
        .await
        .expect("find")
        .expect("other");
    assert!(purge_contact(&mut db, her).await.expect("purge"));
    delete_my_data(&mut db, other, &[]).await.expect("delete");
    assert_eq!(stated(&pool).await, 0, "no stated name is left");
}

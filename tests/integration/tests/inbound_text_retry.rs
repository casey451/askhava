//! TD-TXT-35 through Twilio's inbound route, the worker's retry handler and the Twilio stand-in on
//! a real database (TXT-012, TXT-020, CON-008, CON-018, QH-001; D-801, D-2307, T336).
//!
//! A text a module error stopped (`Halt::Failed`) queues one `messaging.inbound_retry` task; the
//! retry reads the body back from the stand-in's copy, scrubs it and runs the route's steps once,
//! keeping no body in any table or log line. The route and the handler are
//! `app/server/src/inbound_text.rs` and `app/server/src/inbound_retry.rs`, compiled in with
//! `#[path]`. Two module errors are used: D-2088's (her stated name was sealed under one
//! `VAULT_KEY` and the route's vault holds another, `Halt::Failed("vault")`, after the opt-out and
//! HELP steps), and, for an error before the opt-out step, a signed delivery whose `From` is not a
//! US number while the stand-in's record holds her number (`Halt::Failed("messaging")` at the
//! sender lookup); the retry reads `From` from Twilio's record. Every name and phone is fictional.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{TwilioConfig, TwilioMessageReader};
use askhava_core::api::{
    Clock, Environment, Lease, ManualClock, Registry, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_integration::set_app_role;
use askhava_messaging::api::PAUSED_EMERGENCY_ALERT;
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_privacy::api::Vault;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_retry::{HANDLED, RetryKeys, register};
use inbound_text::{FAILED_ALERT, INBOUND_PATH, RETRY_KIND, Webhooks, retry_key, routes};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

/// The retry handler under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_retry.rs"]
mod inbound_retry;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake-auth-token";
const HER: &str = "+19285550181";
const ASK_HAVA: &str = "+19285550100";
const NOT_US: &str = "+4420";
const SEAL: u8 = 0x5a;
const OTHER_SEAL: u8 = 0x5c;
/// 10:00 in Arizona.
const MORNING: &str = "2026-11-12T17:00:00Z";

/// The stand-in server on a port it picked itself (its first line names it); killed on drop.
struct StandIn(Child, u16);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("listening line");
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let port = line
        .trim_end()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok());
    StandIn(
        child,
        port.unwrap_or_else(|| panic!("stand-in said {line:?}")),
    )
}

/// One raw HTTP exchange with the stand-in; the answer's JSON.
async fn stand_in_call(port: u16, head: &str, body: &str) -> serde_json::Value {
    let request = format!(
        "{head} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        STANDARD.encode(format!("AC{}:{TOKEN}", "0".repeat(32))),
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, json) = answer.split_once("\r\n\r\n").expect("http answer");
    serde_json::from_str(json).expect("json")
}

/// She texts `words` to the stand-in; the webhook fields Twilio would post.
async fn sms(port: u16, words: &str) -> Vec<(String, String)> {
    let body = json!({ "from": HER, "to": ASK_HAVA, "body": words }).to_string();
    let parsed = stand_in_call(port, "POST /_stand_in/twilio/inbound", &body).await;
    let names = ["MessageSid", "From", "To", "Body", "OptOutType"];
    names
        .iter()
        .filter_map(|n| {
            parsed["params"][n]
                .as_str()
                .map(|v| ((*n).to_owned(), v.to_owned()))
        })
        .collect()
}

/// Redacts message `sid`'s body at the stand-in, as the day-31 purge does.
async fn redact(port: u16, sid: &str) {
    let path = format!(
        "POST /twilio/2010-04-01/Accounts/AC{}/Messages/{sid}.json",
        "0".repeat(32)
    );
    stand_in_call(port, &path, "Body=").await;
}

fn vault(seal_byte: u8) -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(seal_byte).into(), &key(0x6a).into(), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn hooks(pool: &PgPool, clock: &Arc<ManualClock>, seal: u8) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::clone(clock) as _,
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(seal),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

/// The worker's registry with the retry handler over the stand-in, its vault sealed with `seal`.
fn worker(port: u16, clock: &Arc<ManualClock>, seal: u8) -> Registry {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", "+19285550110,+19285550111"),
        ("ASKHAVA_MASKED_POOL_SIZE", "2"),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("twilio config");
    let reader = TwilioMessageReader::new(&config).expect("reader");
    let keys = RetryKeys {
        id_secret: secret(),
        vault: vault(seal),
        intake: IntakeSettings::default(),
        clock: Arc::clone(clock) as _,
    };
    let mut registry = Registry::new();
    register(&mut registry, Arc::new(reader), Arc::new(keys));
    registry
}

/// POSTs `fields` to the route, signed by the fake token.
async fn post(h: &Arc<Webhooks>, fields: &[(String, String)]) -> StatusCode {
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
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in sorted {
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
    routes(Arc::clone(h))
        .oneshot(request)
        .await
        .expect("answer")
        .status()
}

/// She texts `words`; Twilio posts it to the route over `h`. Returns the MessageSid.
async fn deliver(h: &Arc<Webhooks>, port: u16, words: &str) -> String {
    let fields = sms(port, words).await;
    assert_eq!(post(h, &fields).await, StatusCode::OK);
    sid_of(&fields)
}

/// As [`deliver`], but the posted `From` is not a US number, so the sender lookup fails before
/// the opt-out step; the stand-in's record keeps her number.
async fn deliver_unreadable_sender(h: &Arc<Webhooks>, port: u16, words: &str) -> String {
    let mut fields = sms(port, words).await;
    for (name, value) in &mut fields {
        if name == "From" {
            NOT_US.clone_into(value);
        }
    }
    assert_eq!(post(h, &fields).await, StatusCode::OK);
    sid_of(&fields)
}

fn sid_of(fields: &[(String, String)]) -> String {
    let found = fields.iter().find(|(n, _)| n == "MessageSid");
    found.expect("a MessageSid").1.clone()
}

async fn inbox(pool: &PgPool, sid: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM messaging.inbox WHERE message_sid = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .expect("its inbox row")
}

/// Every retry task: dedupe key, payload, due time, done.
async fn retries(pool: &PgPool) -> Vec<(String, serde_json::Value, Timestamp, bool)> {
    let rows: Vec<(String, serde_json::Value, String, bool)> = sqlx::query_as(
        "SELECT dedupe_key, payload, to_char(run_at AT TIME ZONE 'UTC', \
         'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), done_at IS NOT NULL FROM core.task WHERE kind = $1 ORDER BY id",
    )
    .bind(RETRY_KIND)
    .fetch_all(pool)
    .await
    .expect("retry tasks");
    rows.into_iter()
        .map(|(k, p, at, done)| (k, p, at.parse().expect("instant"), done))
        .collect()
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn outbox_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(pool)
        .await
        .expect("outbox")
}

/// Runs every task due at the clock's now through `registry` on an `askhava_app` pool (R9).
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let app = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let owner = WorkerId::new(&app).await.expect("worker");
    let tasks = claim(&app, owner, clock.now(), SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    for task in tasks {
        registry
            .run(&app, clock, owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

/// Every row in every application table whose text holds `needle`, as `schema.table`.
async fn rows_holding(pool: &PgPool, needle: &str) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT quote_ident(table_schema) || '.' || quote_ident(table_name) FROM information_schema.tables \
         WHERE table_type = 'BASE TABLE' AND table_schema NOT IN ('pg_catalog', 'information_schema')",
    )
    .fetch_all(pool)
    .await
    .expect("tables");
    let mut found = Vec::new();
    for table in tables {
        let sql = format!("SELECT count(*) FROM {table} t WHERE t::text ILIKE '%' || $1 || '%'");
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(needle)
            .fetch_one(pool)
            .await
            .expect("scan");
        if n > 0 {
            found.push(table);
        }
    }
    found
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

fn clock_at(at: &str) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(at.parse().expect("instant")))
}

/// Her first text states her name, sealed under [`SEAL`], so a vault with another key cannot read
/// her later texts (D-2088).
async fn introduce(pool: &PgPool, clock: &Arc<ManualClock>, port: u16) {
    deliver(&hooks(pool, clock, SEAL), port, "I'm Zebulon").await;
}

/// TD-TXT-35 (TXT-012, TXT-020, D-801, D-2307): a text stopped by a module error queues one retry
/// (its inbox row ID only, due in 5 minutes); the retry reads the body by SID, scrubs it and runs
/// the steps once: a paused emergency's alert, committed before the failure, is not raised again.
/// No table and no log line holds the body or her phone afterwards.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_failed_text_is_retried_once_and_its_body_is_kept_nowhere(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (server, clock) = (stand_in(), clock_at(MORNING));
    introduce(&pool, &clock, server.1).await;
    let mut db = pool.acquire().await.expect("connection");
    set_switch(&mut db, Switch::Texting, true, Actor::System)
        .await
        .expect("pause");
    let sid = deliver(
        &hooks(&pool, &clock, OTHER_SEAL),
        server.1,
        "Zebulon here, I smell gas by the kumquat tree",
    )
    .await;
    let id = inbox(&pool, &sid).await;
    let due = clock.now() + SignedDuration::from_mins(5);
    assert_eq!(
        retries(&pool).await,
        vec![(retry_key(id, 1), json!({ "inbox": id }), due, false)]
    );
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $2 AND subject_id = $1";
    let paused = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i64>(alerts)
            .bind(id)
            .bind(PAUSED_EMERGENCY_ALERT)
            .fetch_one(&pool)
            .await
            .expect("alerts")
    };
    assert_eq!(
        paused(pool.clone()).await,
        1,
        "committed before the failure"
    );
    clock.advance(SignedDuration::from_mins(5));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    assert_eq!(
        paused(pool.clone()).await,
        1,
        "a committed step is not repeated"
    );
    let tasks = retries(&pool).await;
    assert_eq!(
        tasks.len(),
        1,
        "a retry that ran to the end queues nothing: {tasks:?}"
    );
    assert!(tasks[0].3, "the retry is done");
    for needle in ["kumquat", "Zebulon", "5550181"] {
        assert_eq!(
            rows_holding(&pool, needle).await,
            Vec::<String>::new(),
            "{needle}"
        );
        let logged = lines
            .0
            .lock()
            .expect("lines")
            .iter()
            .any(|l| l.contains(needle));
        assert!(!logged, "a log line holds {needle}");
    }
}

/// TD-TXT-35 (D-2307): each failed try queues the next (30 minutes, then 2 hours); the third
/// failure queues nothing more and T325's one alert stays open.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_the_third_failure_leaves_the_alert_and_queues_nothing(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    introduce(&pool, &clock, server.1).await;
    let sid = deliver(
        &hooks(&pool, &clock, OTHER_SEAL),
        server.1,
        "zebulon's sink leaks",
    )
    .await;
    let id = inbox(&pool, &sid).await;
    let failing = worker(server.1, &clock, OTHER_SEAL);
    let mut expected = Vec::new();
    for (n, wait) in [(1, 5), (2, 30), (3, 120)] {
        clock.advance(SignedDuration::from_mins(wait));
        run_due(&pool, &failing, &clock).await;
        expected.push(retry_key(id, n));
    }
    clock.advance(SignedDuration::from_hours(5));
    run_due(&pool, &failing, &clock).await;
    let tasks = retries(&pool).await;
    let keys: Vec<String> = tasks.iter().map(|t| t.0.clone()).collect();
    assert_eq!(keys, expected);
    assert!(tasks.iter().all(|t| t.3 && t.1 == json!({ "inbox": id })));
    let open = "SELECT count(*) FROM ops.alert WHERE kind = 'messaging.inbound_text_failed' \
                AND subject_id = $1 AND resolved_at IS NULL";
    assert_eq!(count(&pool, open, id).await, 1);
    let all = "SELECT count(*) FROM ops.alert WHERE subject_id = $1";
    assert_eq!(count(&pool, all, id).await, 1, "no alert but T325's");
    assert_eq!(FAILED_ALERT, "messaging.inbound_text_failed");
}

/// TD-TXT-35 (TXT-020, D-2307): a message whose body Twilio redacted, or one it never had, closes
/// the retry with no error and nothing queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_redacted_or_unknown_message_closes_the_retry(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    introduce(&pool, &clock, server.1).await;
    let route = hooks(&pool, &clock, OTHER_SEAL);
    let redacted = deliver(&route, server.1, "zebulon's gate is stuck").await;
    redact(server.1, &redacted).await;
    let mut unknown = sms(server.1, "zebulon's fence").await;
    for (name, value) in &mut unknown {
        if name == "MessageSid" {
            *value = format!("SM{}", "e".repeat(32));
        }
    }
    assert_eq!(post(&route, &unknown).await, StatusCode::OK);
    assert_eq!(retries(&pool).await.len(), 2);
    clock.advance(SignedDuration::from_mins(5));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    let tasks = retries(&pool).await;
    assert_eq!(tasks.len(), 2, "nothing more queued: {tasks:?}");
    assert!(tasks.iter().all(|t| t.3), "both closed without error");
}

/// TD-TXT-35 (D-2307): a `BadWebhook` delivery and a listed sender's failure are not retried.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_bad_webhook_or_a_listed_sender_is_not_retried(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    let route = hooks(&pool, &clock, SEAL);
    let mut bad = sms(server.1, "fake words").await;
    for (name, value) in &mut bad {
        if name == "MessageSid" {
            "SMbad".clone_into(value);
        }
    }
    assert_eq!(post(&route, &bad).await, StatusCode::OK);
    deliver(&route, server.1, "STOP").await;
    let mut help = sms(server.1, "HELP").await;
    for (name, value) in &mut help {
        if name == "To" {
            NOT_US.clone_into(value);
        }
    }
    assert_eq!(post(&route, &help).await, StatusCode::OK);
    let id = inbox(&pool, &sid_of(&help)).await;
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = 'messaging.inbound_text_failed' \
                  AND subject_id = $1";
    assert_eq!(
        count(&pool, alerts, id).await,
        1,
        "the listed sender's failure is alerted"
    );
    assert_eq!(retries(&pool).await, vec![], "and neither is retried");
}

/// TD-TXT-35 (CON-008, CON-030, D-2307): a STOP whose first try failed before the opt-out step is
/// applied by the retry even after a newer text was handled; Twilio answered the STOP, so no
/// second confirmation is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_stop_retried_after_a_newer_text_is_applied(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    let route = hooks(&pool, &clock, SEAL);
    let stop = deliver_unreadable_sender(&route, server.1, "STOP").await;
    let id = inbox(&pool, &stop).await;
    let tasks = retries(&pool).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].1,
        json!({ "inbox": id, "opt_out_type": "STOP" }),
        "the webhook's OptOutType rides in the payload, never re-matched (CON-030, D-3172)"
    );
    clock.advance(SignedDuration::from_mins(1));
    deliver(&route, server.1, "my sink drips").await;
    let listed = "SELECT count(*) FROM messaging.optout_event WHERE inbox_id = $1";
    assert_eq!(count(&pool, listed, id).await, 0);
    clock.advance(SignedDuration::from_mins(4));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    assert_eq!(count(&pool, listed, id).await, 1, "the STOP is applied");
    let suppressed: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.suppression")
        .fetch_one(&pool)
        .await
        .expect("suppression");
    assert!(suppressed >= 1, "her number is listed");
    let confirmations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE template_id = 'opt_out_confirmation'",
    )
    .fetch_one(&pool)
    .await
    .expect("outbox");
    assert_eq!(confirmations, 0, "Twilio sent the one confirmation");
}

/// TD-TXT-35 (CON-018, D-2307): when a newer text from her was handled, the retry runs only the
/// opt-out and HELP steps and closes: a stale HELP is answered, a stale request changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_stale_retry_runs_only_opt_out_and_help(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    introduce(&pool, &clock, server.1).await;
    let route = hooks(&pool, &clock, SEAL);
    let help = deliver_unreadable_sender(&route, server.1, "HELP").await;
    let stale = deliver(
        &hooks(&pool, &clock, OTHER_SEAL),
        server.1,
        "zebulon's roof leaks",
    )
    .await;
    clock.advance(SignedDuration::from_mins(1));
    deliver(&route, server.1, "never mind, all fixed").await;
    let before = outbox_rows(&pool).await;
    let replies = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'help_reply' \
                   AND $1 = ANY(subject_ids)";
    let help_id = inbox(&pool, &help).await;
    assert_eq!(count(&pool, replies, help_id).await, 0);
    clock.advance(SignedDuration::from_mins(4));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    assert_eq!(
        count(&pool, replies, help_id).await,
        1,
        "the stale HELP is answered"
    );
    assert_eq!(
        outbox_rows(&pool).await,
        before + 1,
        "and nothing else is queued"
    );
    let tasks = retries(&pool).await;
    assert_eq!(tasks.len(), 2, "both close: {tasks:?}");
    assert!(tasks.iter().all(|t| t.3));
    let stale_id = inbox(&pool, &stale).await;
    let open = "SELECT count(*) FROM ops.alert WHERE subject_id = $1 AND resolved_at IS NULL";
    assert_eq!(
        count(&pool, open, stale_id).await,
        1,
        "T325's alert stays the record"
    );
}

/// TD-TXT-35 (QH-001, QH-006, D-2307): a first text whose retry runs at 22:00 Arizona time runs
/// only the opt-out and HELP steps and queues the rest for 08:00:00, which then queues her consent
/// reply; nothing is queued at night.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_retry_at_22_00_waits_for_8_a_m(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at("2026-11-13T04:55:00Z"));
    let route = hooks(&pool, &clock, SEAL);
    let sid = deliver_unreadable_sender(&route, server.1, "my tap drips").await;
    let id = inbox(&pool, &sid).await;
    let before = outbox_rows(&pool).await;
    clock.advance(SignedDuration::from_mins(5));
    let retry = worker(server.1, &clock, SEAL);
    run_due(&pool, &retry, &clock).await;
    assert_eq!(outbox_rows(&pool).await, before, "nothing queued at 22:00");
    let eight: Timestamp = "2026-11-13T15:00:00Z".parse().expect("08:00 Arizona");
    let held = format!("{RETRY_KIND}:{id}:held");
    let tasks = retries(&pool).await;
    assert!(
        tasks.iter().any(|t| t.0 == held && t.2 == eight && !t.3),
        "{tasks:?}"
    );
    clock.set(eight);
    run_due(&pool, &retry, &clock).await;
    assert_eq!(
        outbox_rows(&pool).await,
        before + 1,
        "her reply is queued at 08:00"
    );
    let consent: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = 'consent'")
            .fetch_one(&pool)
            .await
            .expect("outbox");
    assert_eq!(consent, 1, "the reply is her consent text");
    assert!(retries(&pool).await.iter().all(|t| t.3));
}

/// Counts the [`HANDLED`] marks about inbox row `id` (D-3172).
async fn handled(pool: &PgPool, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1 AND subject_id = $2")
        .bind(HANDLED)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("marks")
}

/// Moves try 1 of inbox row `id` an hour later, as a claim that failed once would.
async fn delay_first_try(pool: &PgPool, id: Uuid) {
    sqlx::query("UPDATE core.task SET run_at = run_at + interval '1 hour' WHERE dedupe_key = $1")
        .bind(retry_key(id, 1))
        .execute(pool)
        .await
        .expect("delay");
}

async fn template_rows(pool: &PgPool, template: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
        .bind(template)
        .fetch_one(pool)
        .await
        .expect("outbox")
}

/// TD-TXT-35 (CON-008, CON-018, D-2307, D-3172): a newer text that failed and was then handled by
/// its own retry counts as processed (its T325 alert stays open, its `HANDLED` mark says so), so
/// the older text's later retry runs only the opt-out and HELP steps and queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_newer_text_handled_by_its_own_retry_makes_an_older_retry_stale(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at(MORNING));
    let route = hooks(&pool, &clock, SEAL);
    let older = deliver_unreadable_sender(&route, server.1, "my tap drips").await;
    clock.advance(SignedDuration::from_mins(1));
    let newer = deliver_unreadable_sender(&route, server.1, "my sink leaks too").await;
    let (older, newer) = (inbox(&pool, &older).await, inbox(&pool, &newer).await);
    delay_first_try(&pool, older).await;
    clock.advance(SignedDuration::from_mins(5));
    let retry = worker(server.1, &clock, SEAL);
    run_due(&pool, &retry, &clock).await;
    assert_eq!(
        template_rows(&pool, "consent").await,
        1,
        "the newer text ran every step"
    );
    assert_eq!(handled(&pool, newer).await, 1, "and is marked handled");
    let open = "SELECT count(*) FROM ops.alert WHERE subject_id = $1 AND resolved_at IS NULL";
    assert_eq!(
        count(&pool, open, newer).await,
        1,
        "its alert stays the record"
    );
    let before = outbox_rows(&pool).await;
    clock.advance(SignedDuration::from_hours(1));
    run_due(&pool, &retry, &clock).await;
    assert_eq!(
        outbox_rows(&pool).await,
        before,
        "the stale text changes nothing"
    );
    assert_eq!(
        handled(&pool, older).await,
        0,
        "a stale retry is not marked handled"
    );
    assert_eq!(count(&pool, open, older).await, 1);
    let tasks = retries(&pool).await;
    assert!(tasks.iter().all(|t| t.3), "both close: {tasks:?}");
}

/// TD-TXT-35 (CON-008, CON-018, CON-030, QH-001, QH-004, D-2307): a HELP and a STOP whose retries
/// run at 22:00 Arizona time are applied at once (the HELP reply is an any-hour text; Twilio sent
/// the STOP's one confirmation), and nothing is held for 8 a.m. or queued besides the HELP reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_a_help_or_stop_retried_at_22_00_runs_now_and_sends_nothing_else(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at("2026-11-13T04:55:00Z"));
    let route = hooks(&pool, &clock, SEAL);
    let help = deliver_unreadable_sender(&route, server.1, "HELP").await;
    let stop = deliver_unreadable_sender(&route, server.1, "STOP").await;
    let (help, stop) = (inbox(&pool, &help).await, inbox(&pool, &stop).await);
    let before = outbox_rows(&pool).await;
    clock.advance(SignedDuration::from_mins(5));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    let replies = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'help_reply' \
                   AND $1 = ANY(subject_ids)";
    assert_eq!(
        count(&pool, replies, help).await,
        1,
        "HELP answered at 22:00"
    );
    let listed = "SELECT count(*) FROM messaging.optout_event WHERE inbox_id = $1";
    assert_eq!(count(&pool, listed, stop).await, 1, "STOP applied at 22:00");
    assert_eq!(template_rows(&pool, "opt_out_confirmation").await, 0);
    assert_eq!(outbox_rows(&pool).await, before + 1, "only the HELP reply");
    let tasks = retries(&pool).await;
    assert!(
        tasks.iter().all(|t| t.3 && !t.0.ends_with(":held")),
        "{tasks:?}"
    );
}

/// TD-TXT-35 (QH-001, QH-004, INT-004, D-2307): an emergency text whose retry runs at 22:00
/// Arizona time runs every step at once (its 911 text is an any-hour text) and holds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_35_an_emergency_retried_at_night_runs_every_step(pool: PgPool) {
    let (server, clock) = (stand_in(), clock_at("2026-11-13T04:55:00Z"));
    let route = hooks(&pool, &clock, SEAL);
    let sid = deliver_unreadable_sender(&route, server.1, "I smell gas in my kitchen").await;
    let id = inbox(&pool, &sid).await;
    let before = outbox_rows(&pool).await;
    clock.advance(SignedDuration::from_mins(5));
    run_due(&pool, &worker(server.1, &clock, SEAL), &clock).await;
    let events = "SELECT count(*) FROM intake.emergency_event WHERE inbox_id = $1";
    assert_eq!(
        count(&pool, events, id).await,
        1,
        "the emergency step ran at night"
    );
    assert!(
        outbox_rows(&pool).await > before,
        "its any-hour text is queued"
    );
    assert_eq!(handled(&pool, id).await, 1);
    let tasks = retries(&pool).await;
    assert!(
        tasks.iter().all(|t| t.3 && !t.0.ends_with(":held")),
        "{tasks:?}"
    );
}

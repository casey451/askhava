//! Fault modes, call records, the daily cap and the web-chat rule (TD-AI-10, TD-AI-13, TD-AI-14;
//! AI-001, AI-021, AI-023, AI-024).
//!
//! [`Scripted`] plays the stand-in's fault modes (`stand_ins/ai.py` `FAULTS`, the `refusal` and
//! `max_tokens` stops) one attempt at a time under Tokio's paused clock, since the adapter's
//! client cannot pick a stand-in fault header (D-810); the last test reaches the real stand-in
//! over HTTP through `adapters`' client.

use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use askhava_adapters::api::StandInAi;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tokio::time::Instant;

use super::schemas::{open, reply_fields, scrubbed};
use crate::models::{AdapterTransport, AiConfig, DAILY_CAP_KEY, Endpoint, Fault, Reply, Transport};
use crate::schemas::OpenTrades;
use crate::service::{AiOff, CallEnd, Intake, IntakeCall, Origin, read_intake, send};

/// One scripted attempt.
#[derive(Clone)]
enum Step {
    Answer(Value),
    Fail(Fault),
    Hang,
}

/// A transport that plays `steps` in order and records every path it was asked for.
struct Scripted {
    steps: Mutex<VecDeque<Step>>,
    paths: Mutex<Vec<&'static str>>,
}

impl Scripted {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        let steps = Mutex::new(steps.into_iter().collect());
        Self {
            steps,
            paths: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.paths.lock().unwrap().len()
    }
}

impl Transport for Scripted {
    async fn post(&self, endpoint: Endpoint, _: Uuid, _: &Value) -> Result<Reply, Fault> {
        self.paths.lock().unwrap().push(endpoint.path());
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("an unscripted attempt");
        match step {
            Step::Answer(body) => Ok(Reply {
                body,
                request_id: Some("req_fake01".to_owned()),
            }),
            Step::Fail(fault) => Err(fault),
            Step::Hang => std::future::pending().await,
        }
    }
}

fn status(status: u16, retry_after: Option<u64>) -> Step {
    Step::Fail(Fault::Status {
        status,
        retry_after,
        spend_limit: status == 400,
    })
}

fn good_reply(stop: &str) -> Step {
    let text = Value::Object(reply_fields()).to_string();
    Step::Answer(
        json!({"id": "msg_fake01", "stop_reason": stop, "content": [{"type": "text", "text": text}]}),
    )
}

async fn run(steps: Vec<Step>) -> (Result<Reply, CallEnd>, u32, Duration, Scripted) {
    let transport = Scripted::new(steps);
    let started = Instant::now();
    let (result, attempts) = send(
        &transport,
        &AiConfig::from_pairs([]).unwrap(),
        Uuid::nil(),
        &json!({}),
    )
    .await;
    (result, attempts, started.elapsed(), transport)
}

/// TD-AI-14 (AI-023): a 500, 504, 529 or connection error is retried at most twice with backoff
/// (1 s, then 2 s), and every request goes to `/v1/messages`.
#[tokio::test(start_paused = true)]
async fn retryable_faults_are_retried_twice_with_backoff() {
    for fault in [
        status(500, None),
        status(504, None),
        status(529, None),
        Step::Fail(Fault::Connection),
    ] {
        let (result, attempts, waited, transport) =
            run(vec![fault.clone(), fault.clone(), fault]).await;
        assert!(matches!(
            result,
            Err(CallEnd::Http { alert: None, .. } | CallEnd::Connection)
        ));
        assert_eq!((attempts, waited), (3, Duration::from_secs(3)));
        assert!(
            transport
                .paths
                .lock()
                .unwrap()
                .iter()
                .all(|p| *p == "/v1/messages")
        );
    }
    let (result, attempts, _, _) = run(vec![status(529, None), good_reply("end_turn")]).await;
    assert!(result.is_ok());
    assert_eq!(attempts, 2);
}

/// TD-AI-14 (AI-023): a 429 with `retry-after: 3` waits 3 seconds; a 429 without it, a 400, a
/// 401 or a 403 is not retried, and all but a plain 400 raise the owner alert.
#[tokio::test(start_paused = true)]
async fn rate_limit_waits_and_key_or_spend_errors_alert_without_retry() {
    let (result, attempts, waited, _) =
        run(vec![status(429, Some(3)), good_reply("end_turn")]).await;
    assert!(result.is_ok());
    assert_eq!((attempts, waited), (2, Duration::from_secs(3)));
    let cases = [
        (status(429, None), Some("ai.spend_limit")),
        (status(400, None), Some("ai.spend_limit")),
        (
            Step::Fail(Fault::Status {
                status: 400,
                retry_after: None,
                spend_limit: false,
            }),
            None,
        ),
        (status(401, None), Some("ai.key_rejected")),
        (status(403, None), Some("ai.key_rejected")),
    ];
    for (fault, expected) in cases {
        let (result, attempts, _, _) = run(vec![fault]).await;
        assert_eq!(attempts, 1);
        assert!(matches!(result, Err(CallEnd::Http { alert, .. }) if alert == expected));
    }
}

/// TD-AI-14 (AI-023): a call past 20 seconds ends, retries included.
#[tokio::test(start_paused = true)]
async fn a_call_past_twenty_seconds_ends() {
    let (result, _, waited, _) = run(vec![Step::Hang]).await;
    assert_eq!(
        (result, waited),
        (Err(CallEnd::TimedOut), Duration::from_secs(20))
    );
    let (result, attempts, _, _) = run(vec![status(429, Some(30))]).await;
    assert_eq!((result, attempts), (Err(CallEnd::TimedOut), 1));
}

async fn intake(
    pool: &PgPool,
    transport: &impl Transport,
    config: &AiConfig,
    origin: Origin,
) -> Intake {
    let text = scrubbed("water leak under the kitchen sink");
    let open: OpenTrades = open();
    let call = IntakeCall {
        subject: Uuid::nil(),
        origin,
        text: &text,
        open: &open,
    };
    let mut db = pool.acquire().await.unwrap();
    read_intake(&mut db, transport, config, &call)
        .await
        .unwrap()
}

/// TD-AI-14 (AI-023, AI-005): a `refusal` or `max_tokens` stop, an output that breaks the schema
/// and an error each return the AI-off result; the 401 raises one owner alert a day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_ending_returns_the_ai_off_result(pool: PgPool) {
    let config = AiConfig::from_pairs([]).unwrap();
    let broken = Step::Answer(
        json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "{\"trade\": 1}"}]}),
    );
    for step in [
        good_reply("refusal"),
        good_reply("max_tokens"),
        broken,
        status(401, None),
        status(401, None),
    ] {
        let outcome = intake(&pool, &Scripted::new([step]), &config, Origin::Text).await;
        assert_eq!(outcome, Intake::AiOff(AiOff::CallEnded));
    }
    let outcomes = sqlx::query_scalar!("SELECT outcome FROM ai.call_stat ORDER BY at, outcome")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 5);
    let alerts = sqlx::query_scalar!(
        r#"SELECT count(alert_id) AS "n!" FROM ai.call_stat WHERE alert = 'ai.key_rejected'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(alerts, 1);
}

/// TD-AI-13 (AI-024): every call record holds the model ID from the setting, the instruction
/// version and the provider's request ID; the record is append-only for the app role.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_records_hold_model_instruction_version_and_request_id(pool: PgPool) {
    let config =
        AiConfig::from_pairs([("ASKHAVA_AI_MODEL".to_owned(), "claude-test-9-9".to_owned())])
            .unwrap();
    let outcome = intake(
        &pool,
        &Scripted::new([good_reply("end_turn")]),
        &config,
        Origin::Text,
    )
    .await;
    assert!(matches!(outcome, Intake::Reading(r) if r.job_type == "leak repair"));
    let row =
        sqlx::query!("SELECT model, instruction_version, request_id, outcome FROM ai.call_stat")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.model, "claude-test-9-9");
    assert_eq!(row.instruction_version, "intake_reading.v1");
    assert_eq!(row.request_id.as_deref(), Some("req_fake01"));
    assert_eq!(row.outcome, "ok");
    let mut tx = pool.begin().await.unwrap();
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query!("DELETE FROM ai.call_stat")
            .execute(&mut *tx)
            .await
            .is_err()
    );
}

/// TD-AI-10 (AI-021): an unverified web-chat session makes zero adapter calls; the daily cap
/// stops the next call and raises the alert once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn unverified_web_chat_and_the_daily_cap_make_no_call(pool: PgPool) {
    let config = AiConfig::from_pairs([(DAILY_CAP_KEY.to_owned(), "1".to_owned())]).unwrap();
    let none = Scripted::new([]);
    let unverified = Origin::WebChat {
        code_verified: false,
    };
    assert_eq!(
        intake(&pool, &none, &config, unverified).await,
        Intake::AiOff(AiOff::NotVerified)
    );
    assert_eq!(none.calls(), 0);
    let one = Scripted::new([good_reply("end_turn")]);
    let verified = Origin::WebChat {
        code_verified: true,
    };
    assert!(matches!(
        intake(&pool, &one, &config, verified).await,
        Intake::Reading(_)
    ));
    for _ in 0..2 {
        assert_eq!(
            intake(&pool, &none, &config, Origin::Text).await,
            Intake::AiOff(AiOff::DailyCap)
        );
    }
    assert_eq!((one.calls(), none.calls()), (1, 0));
    let alerts = sqlx::query_scalar!(
        r#"SELECT count(alert_id) AS "n!" FROM ai.call_stat WHERE alert = 'ai.daily_cap'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(alerts, 1);
}

struct StandIn(Child, u16);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", &port.to_string()])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::null())
        .spawn();
    let server = StandIn(child.unwrap(), port);
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("stand-in server did not start");
}

/// TD-AI-13 (AI-001, AI-022, AI-009): through `adapters`' client the stand-in accepts the
/// request (no tools, the schema in `output_config`) and its schema-checked answer is read.
#[tokio::test]
async fn the_stand_in_answers_the_request_over_http() {
    let server = stand_in();
    let client = StandInAi::new(&format!("http://127.0.0.1:{}/anthropic", server.1)).unwrap();
    let transport = AdapterTransport::new(client, SecretString::from("fake-id-secret"));
    let config = AiConfig::from_pairs([]).unwrap();
    let body = crate::service::intake_request(&config, &scrubbed("my faucet has a leak"));
    let (result, attempts) = send(&transport, &config, Uuid::nil(), &body).await;
    let reply = result.unwrap();
    assert_eq!(attempts, 1);
    assert!(reply.request_id.is_some_and(|id| id.starts_with("msg_")));
    let stand_in_trades = OpenTrades::new([("plumbing".to_owned(), vec!["plumbing".to_owned()])]);
    let reading = crate::service::reading_from_reply(&reply.body, &stand_in_trades).unwrap();
    assert_eq!(reading.trade, "plumbing");
}

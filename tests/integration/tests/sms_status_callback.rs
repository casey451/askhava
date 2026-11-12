//! TD-TXT-36: every text asks Twilio for status callbacks, and the route takes the callback
//! (TXT-013, TXT-014, CARD-019, SEC-016, OPS-017; D-2761, T377).
//!
//! Twilio posts a message's status only to the `StatusCallback` URL its create request gave, so
//! `adapters::api::TwilioSms` sends `ASKHAVA_PUBLIC_BASE_URL` plus `STATUS_PATH` on every create
//! once the composition set it. Here the real texting client sends through the served stand-in
//! (`stand_ins/server.py`), the stand-in moves the message to `undelivered` with an error code and
//! answers with the URL the create gave, the params and Twilio's signature over that URL, and the
//! status route (`app/server/src/inbound_text.rs`, compiled in with `#[path]`) accepts that exact
//! delivery and records it. A client with no status URL asks for nothing, and the stand-in then
//! has no callback to give (409). Every phone number is fictional.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_adapters::api::{IdempotencyKey, SmsAdapter, TwilioConfig, TwilioSms, twilio_clients};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_privacy::api::Vault;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use inbound_text::{STATUS_PATH, Webhooks, routes};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

/// The routes under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake-auth-token";
const HER: &str = "+19285550142";
const FROM: &str = "+15005550006";

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

/// The real Twilio texting client pointed at the stand-in, as `twilio_clients` builds it.
fn texting(port: u16) -> TwilioSms {
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
    let (sms, _, _) = twilio_clients(&config).expect("clients");
    sms
}

/// Sends one fake text through `sms`; the message SID.
async fn send(sms: &TwilioSms, n: &str) -> String {
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let key = IdempotencyKey::derive(&secret, "test.status_callback", &[n]);
    let sent = sms
        .send_text(&key, HER, FROM, "Ask Hava: a fake test text")
        .await
        .expect("sent");
    sent.sid
}

/// Asks the stand-in to move message `sid` to `undelivered` (error 30003); its status and answer.
async fn move_status(port: u16, sid: &str) -> (u16, Value) {
    let body = json!({ "sid": sid, "status": "undelivered", "error_code": 30003 }).to_string();
    let request = format!(
        "POST /_stand_in/twilio/status HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (head, json) = answer.split_once("\r\n\r\n").expect("http answer");
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok());
    (
        status.expect("status line"),
        serde_json::from_str(json).expect("json"),
    )
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(
            "2026-11-12T17:00:00Z".parse().expect("instant"),
        )),
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

/// Posts the stand-in's callback to the path of the URL it was aimed at, with its signature.
async fn deliver(h: &Arc<Webhooks>, callback: &Value) -> StatusCode {
    let url = callback["url"].as_str().expect("url");
    let path = url.strip_prefix(BASE).expect("aimed at the public base");
    let params = callback["params"].as_object().expect("params");
    let body: Vec<String> = params
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v.as_str().expect("text"))))
        .collect();
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            callback["signature"].as_str().expect("signature"),
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

/// TD-TXT-36 (TXT-013, TXT-014, CARD-019, SEC-016, D-2761): a text sent with the public base URL
/// set carries `StatusCallback` = that URL plus `/webhooks/twilio/status`; the route accepts the
/// signed callback the stand-in gives for it and records the failure with its error code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_36_every_text_asks_for_the_status_callback_the_route_accepts(pool: PgPool) {
    let fake = stand_in();
    let sms = texting(fake.1).with_status_callback(format!("{BASE}{STATUS_PATH}"));
    let sid = send(&sms, "1").await;
    let (status, callback) = move_status(fake.1, &sid).await;
    assert_eq!(status, 200, "the create carried no StatusCallback");
    assert_eq!(
        callback["url"],
        "https://hooks.example.test/webhooks/twilio/status"
    );
    assert_eq!(callback["params"]["ErrorCode"], "30003");
    let h = hooks(&pool);
    assert_eq!(deliver(&h, &callback).await, StatusCode::OK);
    let recorded: (String, String, Option<i32>) =
        sqlx::query_as("SELECT provider_sid, status, error_code FROM messaging.delivery_event")
            .fetch_one(&pool)
            .await
            .expect("one delivery event");
    assert_eq!(recorded, (sid, "undelivered".to_owned(), Some(30003)));
}

/// TD-TXT-36 (TXT-013, D-2761): a client with no status URL (no public base URL) asks for no
/// callback, so Twilio, like the stand-in, has none to give.
#[tokio::test]
async fn td_txt_36_no_status_url_asks_for_no_callback() {
    let fake = stand_in();
    let sid = send(&texting(fake.1), "2").await;
    let (status, refused) = move_status(fake.1, &sid).await;
    assert_eq!(status, 409);
    assert!(refused.get("url").is_none() && refused.get("params").is_none());
}

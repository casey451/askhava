//! TD-CALL-22: every placed call carries its TwiML and status callback, and the safety call's
//! answer and key press are received (CALL-006, CALL-022, OPS-028, INT-004, SEC-016, CON-035;
//! D-2762, D-2771, D-3231, T378).
//!
//! The real voice client places calls through the served stand-in (`stand_ins/server.py`), which
//! refuses a create with no script; `/_stand_in/twilio/call-progress` answers with the script the
//! create gave and the callbacks Twilio would post as the call ends, each signed over the URL the
//! create gave. Those are posted to `server`'s `voice_status` routes (compiled in with `#[path]`).
//! Every phone number is fictional.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_adapters::api::{TwilioConfig, TwilioVoice, twilio_clients};
use askhava_calls::api::{
    Approval, AutomatedCall, CallGate, Conditions, SAFETY_KEY_PATH, SAFETY_RESULT_ACTION,
    SAFETY_STATUS_PATH, SafetyCall, SafetyDialer, SafetyOutcome, SafetySwitch, automated_call_gate,
    hangup_twiml, owner_bridge_twiml, place_safety_call, ring_owner,
};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_ops::api::{ALERT_ACKNOWLEDGED_ACTION, NewAlert, Severity, raise_alert};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::Webhooks;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

/// The shared Twilio webhook state, compiled in for `voice_status`.
#[path = r"../../../app/server/src/inbound_text.rs"]
#[allow(
    dead_code,
    reason = "only `Webhooks` is used; the text routes are not served here"
)]
mod inbound_text;
/// The routes under test.
#[path = r"../../../app/server/src/voice_status.rs"]
mod voice_status;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake-auth-token";
const MAIN: &str = "+19285550100";
const CASEY: &str = "+19285550199";

fn ts() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

/// The stand-in server on a port it picked itself; killed on drop.
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

/// The real voice client pointed at the stand-in, and the pool's first number.
fn voice(port: u16) -> (TwilioVoice, askhava_adapters::api::PoolNumber) {
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
    let (_, voice, _) = twilio_clients(&config).expect("clients");
    (voice, config.pool().numbers()[0].clone())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    let at = ts() + SignedDuration::from_mins(6);
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

fn switched_on() -> SafetySwitch {
    let approval = Approval {
        date: "2026-10-30".parse().expect("date"),
        reference: "FAKE-REF-1".to_owned(),
    };
    let met = Conditions {
        narrowed_trigger: true,
        recent_only: true,
        script: true,
        other_number: true,
        list_checked: true,
        consent_text: true,
        logged: true,
    };
    SafetySwitch::turn_on(Some(&approval), &met).expect("on")
}

/// A stored contact with the fake phone `phone` and an open critical alert about it.
async fn emergency(db: &mut PgConnection, phone: &str) -> SafetyCall {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let alert = NewAlert {
        kind: "messaging.opted_out_emergency",
        severity: Severity::Critical,
        subject: Some(("privacy.contact", contact)),
    };
    let alert = raise_alert(db, &alert).await.expect("alert");
    let (message, arrived_at) = (Uuid::new_v4(), ts());
    SafetyCall {
        contact,
        message,
        arrived_at,
        alert,
    }
}

/// Places the safety call about `call` through the stand-in; its call SID.
async fn place(db: &mut PgConnection, port: u16, call: &SafetyCall) -> String {
    let (voice, from) = voice(port);
    let key = secret();
    let dialer = SafetyDialer {
        voice: &voice,
        from: &from,
        main: MAIN,
        switch: switched_on(),
        secret: &key,
        public_base: BASE,
    };
    let now = ts() + SignedDuration::from_mins(5);
    match place_safety_call(db, &vault(), &dialer, call, now).await {
        Ok(SafetyOutcome::Placed(sid)) => sid,
        other => panic!("not placed: {other:?}"),
    }
}

/// Asks the stand-in to end call `sid` with `outcome`: the script and the signed callbacks.
async fn progress(port: u16, sid: &str, outcome: &str) -> Value {
    let body = json!({ "sid": sid, "outcome": outcome }).to_string();
    let request = format!(
        "POST /_stand_in/twilio/call-progress HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Twilio's `X-Twilio-Signature` over `url` and `params`.
fn sign(url: &str, params: &[(String, String)]) -> String {
    let mut sorted = params.to_vec();
    sorted.sort();
    let data: String = sorted
        .iter()
        .fold(url.to_owned(), |acc, (k, v)| acc + k + v);
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(data.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Posts `params` to `url`'s path and query with `signature`; the status and body.
async fn post(
    h: &Arc<Webhooks>,
    url: &str,
    params: &[(String, String)],
    signature: &str,
) -> (StatusCode, String) {
    let path = url.strip_prefix(BASE).expect("aimed at the public base");
    let body: Vec<String> = params
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let request = Request::post(path)
        .header("X-Twilio-Signature", signature)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = voice_status::routes(Arc::clone(h))
        .oneshot(request)
        .await
        .expect("answer");
    let status = answer.status();
    let bytes = axum::body::to_bytes(answer.into_body(), 1 << 16)
        .await
        .expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

/// Delivers every callback the stand-in gave, as Twilio would; each must be taken.
async fn deliver(h: &Arc<Webhooks>, given: &Value) {
    for callback in given["callbacks"].as_array().expect("callbacks") {
        let params: Vec<(String, String)> = callback["params"]
            .as_object()
            .expect("params")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("text").to_owned()))
            .collect();
        let url = callback["url"].as_str().expect("url");
        let signature = callback["signature"].as_str().expect("signature");
        assert_eq!(
            post(h, url, &params, signature).await.0,
            StatusCode::OK,
            "{url}"
        );
    }
}

async fn count(pool: &PgPool, action: &str, to: Option<&str>) -> i64 {
    let sql = "SELECT count(*) FROM ops.audit_log WHERE action = $1 \
               AND ($2::text IS NULL OR to_status = $2)";
    sqlx::query_scalar(sql)
        .bind(action)
        .bind(to)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn resolved(pool: &PgPool, alert: Uuid) -> bool {
    sqlx::query_scalar("SELECT resolved_at IS NOT NULL FROM ops.alert WHERE id = $1")
        .bind(alert)
        .fetch_one(pool)
        .await
        .expect("alert")
}

/// The `Gather` action URL in `twiml`, its `&amp;` read back as `&`, as Twilio would post to it.
fn key_action(twiml: &str) -> String {
    let (_, rest) = twiml.split_once("<Gather").expect("a Gather");
    let (_, rest) = rest.split_once("action=\"").expect("an action");
    let (url, _) = rest.split_once('"').expect("a closing quote");
    url.replace("&amp;", "&")
}

/// TD-CALL-22 (CALL-022, OPS-028, SEC-016, D-2762): the safety call carries `safety_twiml` inline
/// with its absolute key action (a 9 posted to the very URL it names is taken) and asks for status and detection callbacks on the public base; a
/// `human` result acknowledges the alert once and a repeat of every callback changes nothing; an
/// unsigned callback is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_22_a_safety_call_carries_its_twiml_and_a_person_acknowledges_once(pool: PgPool) {
    assert_eq!(
        (voice_status::STATUS_PATH, voice_status::KEY_PATH),
        (SAFETY_STATUS_PATH, SAFETY_KEY_PATH)
    );
    let fake = stand_in();
    let mut db = pool.acquire().await.expect("connection");
    let call = emergency(&mut db, "+19285550171").await;
    let sid = place(&mut db, fake.1, &call).await;
    let given = progress(fake.1, &sid, "human").await;
    let key_url = key_action(given["twiml"].as_str().expect("the create carried Twiml"));
    assert_eq!(key_url, format!("{BASE}{SAFETY_KEY_PATH}?{}", call.query()));
    let status_url = format!("{BASE}/webhooks/twilio/voice/status?{}", call.query());
    let urls: Vec<&str> = given["callbacks"]
        .as_array()
        .expect("list")
        .iter()
        .map(|c| c["url"].as_str().expect("url"))
        .collect();
    assert_eq!(urls, vec![status_url.as_str(); 5]);
    let h = hooks(&pool);
    deliver(&h, &given).await;
    deliver(&h, &given).await;
    assert!(resolved(&pool, call.alert).await);
    assert_eq!(count(&pool, ALERT_ACKNOWLEDGED_ACTION, None).await, 1);
    assert_eq!(count(&pool, SAFETY_RESULT_ACTION, Some("person")).await, 1);
    let taken = press_at(&h, &key_url, "9").await;
    assert_eq!(taken, (StatusCode::OK, hangup_twiml()), "{key_url}");
    assert_eq!(count(&pool, "calls.safety_call_press_9", None).await, 1);
    let forged = [
        ("CallSid".to_owned(), sid),
        ("AnsweredBy".to_owned(), "human".to_owned()),
    ];
    assert_eq!(
        post(&h, &status_url, &forged, "bm90LWEtc2lnbmF0dXJl")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

/// TD-CALL-22 (OPS-028, D-2771): `answered` then `machine_end_beep` never acknowledges the alert;
/// a later call's `no-answer` logs `nobody`, and neither is decided twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_22_a_machine_or_nobody_never_acknowledges(pool: PgPool) {
    let fake = stand_in();
    let h = hooks(&pool);
    let mut db = pool.acquire().await.expect("connection");
    let machine = emergency(&mut db, "+19285550172").await;
    let sid = place(&mut db, fake.1, &machine).await;
    let given = progress(fake.1, &sid, "machine_end_beep").await;
    deliver(&h, &given).await;
    assert!(!resolved(&pool, machine.alert).await);
    assert_eq!(count(&pool, SAFETY_RESULT_ACTION, Some("machine")).await, 1);
    let nobody = emergency(&mut db, "+19285550173").await;
    let sid = place(&mut db, fake.1, &nobody).await;
    let given = progress(fake.1, &sid, "no-answer").await;
    deliver(&h, &given).await;
    deliver(&h, &given).await;
    assert!(!resolved(&pool, nobody.alert).await);
    assert_eq!(count(&pool, SAFETY_RESULT_ACTION, Some("nobody")).await, 1);
    assert_eq!(count(&pool, SAFETY_RESULT_ACTION, None).await, 2);
    assert_eq!(count(&pool, ALERT_ACKNOWLEDGED_ACTION, None).await, 0);
}

/// The key press `digits` as Twilio posts it to a `Gather` action.
fn key_params(digits: &str) -> [(String, String); 2] {
    [
        (
            "CallSid".to_owned(),
            "CA0000000000000000000000000000009a".to_owned(),
        ),
        ("Digits".to_owned(), digits.to_owned()),
    ]
}

/// Posts the key `digits` to the key action `url`, signed as Twilio would.
async fn press_at(h: &Arc<Webhooks>, url: &str, digits: &str) -> (StatusCode, String) {
    let params = key_params(digits);
    post(h, url, &params, &sign(url, &params)).await
}

/// Posts the key `digits` for `call` to the safety-key route, signed as Twilio would.
async fn press(h: &Arc<Webhooks>, call: &SafetyCall, digits: &str) -> (StatusCode, String) {
    let url = format!("{BASE}/webhooks/twilio/voice/safety-key?{}", call.query());
    press_at(h, &url, digits).await
}

/// TD-CALL-22 (CALL-022, CON-035, D-2771): a 9 on the safety-key route lists her number and answers
/// hang-up TwiML, a repeat changes nothing; another key lists nothing; a forged 9 is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_22_a_9_on_the_safety_key_route_lists_her_number(pool: PgPool) {
    let h = hooks(&pool);
    let mut db = pool.acquire().await.expect("connection");
    let (stopped, other) = (
        emergency(&mut db, "+19285550174").await,
        emergency(&mut db, "+19285550175").await,
    );
    let now = ts() + SignedDuration::from_mins(6);
    let gate = |c: Uuid| (c, AutomatedCall::Safety);
    assert_eq!(
        press(&h, &other, "1").await,
        (StatusCode::OK, hangup_twiml())
    );
    assert_eq!(
        press(&h, &other, "9").await,
        (StatusCode::OK, hangup_twiml()),
        "a repeat"
    );
    let listed = automated_call_gate(&mut db, &vault(), gate(other.contact), now).await;
    assert_eq!(listed.expect("gate"), CallGate::Dial);
    let url = format!("{BASE}{}?{}", voice_status::KEY_PATH, stopped.query());
    let forged = post(&h, &url, &key_params("9"), "bm90LWEtc2lnbmF0dXJl").await;
    assert_eq!(forged.0, StatusCode::FORBIDDEN);
    let listed = automated_call_gate(&mut db, &vault(), gate(stopped.contact), now).await;
    assert_eq!(
        listed.expect("gate"),
        CallGate::Dial,
        "a forged 9 lists nothing"
    );
    assert_eq!(count(&pool, "calls.safety_call_press_9", None).await, 0);
    assert_eq!(
        press(&h, &stopped, "9").await,
        (StatusCode::OK, hangup_twiml())
    );
    let listed = automated_call_gate(&mut db, &vault(), gate(stopped.contact), now).await;
    assert_eq!(listed.expect("gate"), CallGate::OptedOut);
    assert_eq!(count(&pool, "calls.safety_call_press_9", None).await, 1);
    assert_eq!(press(&h, &stopped, "9").await.0, StatusCode::OK);
    assert_eq!(count(&pool, "calls.safety_call_press_9", None).await, 1);
}

/// TD-CALL-22 (OPS-028, CALL-006, D-2762): the owner bridge carries `owner_bridge_twiml` inline, so
/// her number sits in no URL, and its callbacks change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_22_the_owner_bridge_carries_its_twiml_inline(pool: PgPool) {
    let fake = stand_in();
    let (voice, from) = voice(fake.1);
    let (casey, her) = (
        SecretString::from(CASEY),
        SecretString::from("+19285550176"),
    );
    let words = "Ask Hava: leave now and call 911.";
    let alert = Uuid::new_v4();
    let who = (&casey, &from);
    let sid = ring_owner(
        &voice,
        &secret(),
        who,
        (alert, Uuid::new_v4()),
        (&her, words),
        BASE,
    )
    .await
    .expect("rung");
    let given = progress(fake.1, &sid, "no-answer").await;
    let expected = owner_bridge_twiml(&from, &her, words).expect("bridge");
    assert_eq!(given["twiml"].as_str(), Some(expected.expose_secret()));
    let url = format!("{BASE}/webhooks/twilio/voice/status?bridge={alert}");
    assert!(
        given["callbacks"]
            .as_array()
            .expect("list")
            .iter()
            .all(|c| c["url"] == url.as_str())
    );
    deliver(&hooks(&pool), &given).await;
    assert_eq!(count(&pool, SAFETY_RESULT_ACTION, None).await, 0);
}

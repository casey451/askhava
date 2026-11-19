//! A "press 9" on the safety call is never lost to a database error (CON-035, CALL-019, CALL-022,
//! PRIV-016; D-4051, D-4064, D-4088, T571).
//!
//! `server`'s `voice_status` routes are compiled in with `#[path]`, as in `voice_call_plan.rs`
//! (TD-CALL-22). The step's failure is made here only: a test-created trigger on
//! `messaging.suppression` that raises, dropped before the retry; the double failure uses a closed
//! pool. The Twilio stand-in models no `Gather` or `Redirect` (D-2763), so the cases read the
//! returned TwiML. Every number is fictional.

use std::sync::Arc;

use askhava_calls::api::{
    SAFETY_KEY_LOST_ALERT, SAFETY_KEY_PATH, SAFETY_KEY_RETRY_ATTEMPTS, SAFETY_KEY_RETRY_KIND,
    SafetyCall, hangup_twiml, run_safety_key_retry,
};
use askhava_core::api::{Claimed, Environment, ManualClock};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_integration::log_capture;
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::Body;
use axum::http::Request;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::Webhooks;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
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
const PHONE: &str = "+19285550181";
const PHONE_DIGITS: &str = "9285550181";

fn ts() -> Timestamp {
    "2026-11-02T17:05:00Z".parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(ts())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

/// A stored contact with the fake phone and an open critical alert about it.
async fn emergency(pool: &PgPool) -> SafetyCall {
    let mut db = pool.acquire().await.expect("connection");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let alert = NewAlert {
        kind: "messaging.opted_out_emergency",
        severity: Severity::Critical,
        subject: Some(("privacy.contact", contact)),
    };
    let alert = raise_alert(&mut db, &alert).await.expect("alert");
    let arrived_at = "2026-11-02T17:00:00Z".parse().expect("instant");
    SafetyCall {
        contact,
        message: Uuid::new_v4(),
        arrived_at,
        alert,
    }
}

/// Posts `params` to `url`, signed as Twilio would; the TwiML answered (always 200).
async fn post(h: &Arc<Webhooks>, url: &str, params: &[(&str, &str)]) -> String {
    let mut sorted = params.to_vec();
    sorted.sort_unstable();
    let data = sorted
        .iter()
        .fold(url.to_owned(), |acc, (k, v)| acc + k + v);
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(data.as_bytes());
    let signature = STANDARD.encode(mac.finalize().into_bytes());
    let body: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let request = Request::post(url.strip_prefix(BASE).expect("public base"))
        .header("X-Twilio-Signature", signature)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = voice_status::routes(Arc::clone(h))
        .oneshot(request)
        .await
        .expect("answer");
    assert_eq!(answer.status(), 200);
    let bytes = axum::body::to_bytes(answer.into_body(), 1 << 16).await;
    String::from_utf8(bytes.expect("body").to_vec()).expect("utf-8")
}

const CALL_SID: (&str, &str) = ("CallSid", "CA0000000000000000000000000000071a");

fn key_url(call: &SafetyCall) -> String {
    format!("{BASE}{SAFETY_KEY_PATH}?{}", call.query())
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn listed(pool: &PgPool) -> i64 {
    count(pool, "SELECT count(*) FROM messaging.suppression").await
}

/// The queued retry tasks' payloads.
async fn queued(pool: &PgPool) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
        .bind(SAFETY_KEY_RETRY_KIND)
        .fetch_all(pool)
        .await
        .expect("tasks")
}

async fn break_suppression(pool: &PgPool, on: bool) {
    let sql: &[&'static str] = if on {
        &[
            "CREATE FUNCTION public.t571_fail() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'fake test failure'; END $$",
            "CREATE TRIGGER t571_fail BEFORE INSERT ON messaging.suppression \
             FOR EACH ROW EXECUTE FUNCTION public.t571_fail()",
        ]
    } else {
        &[
            "DROP TRIGGER t571_fail ON messaging.suppression",
            "DROP FUNCTION public.t571_fail()",
        ]
    };
    for statement in sql {
        sqlx::query(*statement)
            .execute(pool)
            .await
            .expect("trigger");
    }
}

async fn retry(pool: &PgPool, payload: &Value, attempts: i32) -> bool {
    let task = Claimed {
        id: 1,
        kind: SAFETY_KEY_RETRY_KIND.to_owned(),
        payload: payload.clone(),
        attempts,
    };
    let done = run_safety_key_retry(pool.clone(), Some(&vault()), task, ts()).await;
    done.is_ok()
}

/// D-4051 (CON-035, PRIV-016): a 9 whose step fails on a database error is queued with IDs only
/// and answered with a hang-up; the last failed retry raises one High alert; once the database
/// works the retry lists her number, and a second run changes nothing. No log line holds her
/// phone. Fails on `main`, where the key is only logged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_nine_lost_to_a_database_error_is_queued_and_taken(pool: PgPool) {
    let lines = log_capture::start();
    let call = emergency(&pool).await;
    break_suppression(&pool, true).await;
    let answer = post(&hooks(&pool), &key_url(&call), &[CALL_SID, ("Digits", "9")]).await;
    assert_eq!(answer, hangup_twiml());
    assert_eq!(listed(&pool).await, 0);
    let tasks = queued(&pool).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["digits"], "9");
    assert!(!retry(&pool, &tasks[0], SAFETY_KEY_RETRY_ATTEMPTS).await);
    assert!(!retry(&pool, &tasks[0], SAFETY_KEY_RETRY_ATTEMPTS).await);
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = 'calls.safety_key_lost'";
    assert_eq!(SAFETY_KEY_LOST_ALERT, "calls.safety_key_lost");
    assert_eq!(count(&pool, alerts).await, 1);
    break_suppression(&pool, false).await;
    assert!(retry(&pool, &tasks[0], 1).await);
    assert_eq!(listed(&pool).await, 1);
    let events = "SELECT count(*) FROM messaging.optout_event";
    let before = count(&pool, events).await;
    assert!(retry(&pool, &tasks[0], 2).await);
    assert_eq!(
        (listed(&pool).await, count(&pool, events).await),
        (1, before)
    );
    let alert_rows =
        "SELECT count(*) FROM ops.alert alert WHERE row_to_json(alert)::text LIKE '%' || $1 || '%'";
    let in_alerts: i64 = sqlx::query_scalar(alert_rows)
        .bind(PHONE_DIGITS)
        .fetch_one(&pool)
        .await
        .expect("alerts");
    assert_eq!(in_alerts, 0);
    assert!(!tasks[0].to_string().contains(PHONE_DIGITS));
    let logged = lines.lines();
    assert!(logged.iter().any(|l| l.contains("step failed")));
    assert!(logged.iter().all(|l| !l.contains(PHONE_DIGITS)));
}

/// CON-035: a key whose step succeeds lists her number and queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_key_taken_at_once_queues_nothing(pool: PgPool) {
    let call = emergency(&pool).await;
    let answer = post(&hooks(&pool), &key_url(&call), &[CALL_SID, ("Digits", "9")]).await;
    assert_eq!(answer, hangup_twiml());
    assert_eq!(listed(&pool).await, 1);
    assert!(queued(&pool).await.is_empty());
}

/// D-4064: a redirected post carries `Digits` and `attempt` only in the query; the route strips
/// them before `SafetyCall::from_query` and lists her number. Fails on `main`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_redirected_nine_in_the_query_is_taken(pool: PgPool) {
    let call = emergency(&pool).await;
    let url = format!("{}&Digits=9&attempt=1", key_url(&call));
    let answer = post(&hooks(&pool), &url, &[CALL_SID]).await;
    assert_eq!(answer, hangup_twiml());
    assert_eq!(listed(&pool).await, 1);
}

/// D-4051: when the step and the queue both fail (a closed pool) the answer is a POST redirect
/// back to the key route carrying the key, at most `KEY_ATTEMPTS` times, then the STOP line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_double_failure_redirects_at_most_three_times(pool: PgPool) {
    let call = emergency(&pool).await;
    let closed = PgPoolOptions::new().connect_lazy_with((*pool.connect_options()).clone());
    closed.close().await;
    let h = hooks(&closed);
    let mut answer = post(&h, &key_url(&call), &[CALL_SID, ("Digits", "9")]).await;
    for attempt in 1..=voice_status::KEY_ATTEMPTS {
        assert!(answer.contains("<Pause length=\"2\"/><Redirect method=\"POST\">"));
        let rest = answer.split_once("method=\"POST\">").expect("redirect").1;
        let url = rest
            .split_once("</Redirect>")
            .expect("end")
            .0
            .replace("&amp;", "&");
        assert_eq!(
            url,
            format!("{}&Digits=9&attempt={attempt}", key_url(&call))
        );
        answer = post(&h, &url, &[CALL_SID]).await;
    }
    assert!(!answer.contains("<Redirect"));
    assert!(answer.contains(&format!("<Say>{}</Say><Hangup/>", voice_status::STOP_LINE)));
    assert_eq!(listed(&pool).await, 0);
}

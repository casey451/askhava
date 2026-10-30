//! Replayed webhooks change nothing (TD-SEC-03, TD-SEC-18, TD-SEC-27; SEC-016, SEC-017, SEC-039,
//! SEC-051; D-820).
//!
//! A signed timestamp 300 seconds old passes and 301 is refused, for Stripe, Svix and Lob, and so
//! is one 301 seconds ahead. A genuine delivery replayed inside the window still verifies, so the
//! dedupe is what stops it: a verified Stripe event delivered 100 times, and a Svix message, each
//! have one effect, and verified Twilio status callbacks queued, sent, delivered, delivered have 3,
//! through a probe module table built as `askhava_core::api::Dedupe` documents. All data is fake.

use askhava_adapters::api::{
    Delivery, SignatureError, VerifiedWebhook, WebhookVendor, verify_webhook,
};
use askhava_core::api::{Dedupe, EventKey};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::SecretString;
use sha1::Sha1;
use sha2::Sha256;
use sqlx::PgPool;

const NOW: i64 = 1_793_000_000;
const URL: &str = "https://hooks.example.test/webhook";
const EVENT: &[u8] = br#"{"id":"evt_fake_replay","type":"payment_intent.succeeded"}"#;
/// The fake Svix key; the `whsec_` text is built at run time so no secret-shaped literal is
/// committed.
const KEY: &[u8] = b"fake-replay-key";

/// One vendor's check of a delivery signed at `t`, received at `now`.
type Check = fn(i64, i64) -> Result<VerifiedWebhook, SignatureError>;

fn secret_text() -> String {
    format!("whsec_{}", STANDARD.encode(KEY))
}

fn verify(
    vendor: WebhookVendor,
    secret: &str,
    body: &[u8],
    headers: &[(&str, &str)],
    now: i64,
) -> Result<VerifiedWebhook, SignatureError> {
    let delivery = Delivery {
        url: URL,
        body,
        headers,
    };
    verify_webhook(vendor, &SecretString::from(secret), delivery, now)
}

fn mac256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).unwrap();
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Hex HMAC-SHA256 under the secret text over `<t>.<EVENT>`, as Stripe and Lob sign.
fn hex_sig(t: i64) -> String {
    let message = [format!("{t}.").as_bytes(), EVENT].concat();
    let mac = mac256(secret_text().as_bytes(), &message);
    mac.iter().map(|b| format!("{b:02x}")).collect()
}

fn stripe(t: i64, now: i64) -> Result<VerifiedWebhook, SignatureError> {
    let header = format!("t={t},v1={}", hex_sig(t));
    let headers = [("stripe-signature", header.as_str())];
    verify(WebhookVendor::Stripe, &secret_text(), EVENT, &headers, now)
}

fn svix(t: i64, now: i64) -> Result<VerifiedWebhook, SignatureError> {
    let message = [format!("msg_fake_replay.{t}.").as_bytes(), EVENT].concat();
    let sig = format!("v1,{}", STANDARD.encode(mac256(KEY, &message)));
    let stamp = t.to_string();
    let headers = [
        ("svix-id", "msg_fake_replay"),
        ("svix-timestamp", stamp.as_str()),
        ("svix-signature", sig.as_str()),
    ];
    verify(WebhookVendor::Resend, &secret_text(), EVENT, &headers, now)
}

fn lob(t: i64, now: i64) -> Result<VerifiedWebhook, SignatureError> {
    let (sig, stamp) = (hex_sig(t), t.to_string());
    let headers = [
        ("lob-signature", sig.as_str()),
        ("lob-signature-timestamp", stamp.as_str()),
    ];
    verify(WebhookVendor::Lob, &secret_text(), EVENT, &headers, now)
}

/// TD-SEC-03, TD-SEC-27; SEC-016, SEC-051: 300 seconds old is accepted, 301 refused, either way.
#[test]
fn timestamp_tolerance_is_five_minutes() {
    let checks: [Check; 3] = [stripe, svix, lob];
    for check in checks {
        assert!(check(NOW - 300, NOW).is_ok());
        assert!(check(NOW + 300, NOW).is_ok());
        assert_eq!(check(NOW - 301, NOW).unwrap_err(), SignatureError::Stale);
        assert_eq!(check(NOW + 301, NOW).unwrap_err(), SignatureError::Stale);
        assert_eq!(check(i64::MIN, NOW).unwrap_err(), SignatureError::Stale);
    }
}

/// A probe module's `webhook_event` and effect tables, as a module's migration would make them.
async fn probe_tables(pool: &PgPool) {
    for sql in [
        "CREATE TABLE core.replay_webhook_event (source text NOT NULL, event_key text NOT NULL, \
         received_at timestamptz NOT NULL, PRIMARY KEY (source, event_key))",
        "CREATE TABLE core.replay_effect (event_key text NOT NULL)",
    ] {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
}

/// One verified delivery as a module applies it: dedupe insert, and the effect on the first.
async fn apply(pool: &PgPool, source: &str, key: &EventKey) -> Dedupe {
    let mut tx = pool.begin().await.unwrap();
    let row: Option<String> = sqlx::query_scalar(
        "INSERT INTO core.replay_webhook_event (source, event_key, received_at) \
         VALUES ($1, $2, now()) ON CONFLICT (source, event_key) DO NOTHING RETURNING event_key",
    )
    .bind(source)
    .bind(key.as_str())
    .fetch_optional(&mut *tx)
    .await
    .unwrap();
    let outcome = Dedupe::from_returning(row);
    if outcome.is_first() {
        sqlx::query("INSERT INTO core.replay_effect (event_key) VALUES ($1)")
            .bind(key.as_str())
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    outcome
}

async fn effects(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.replay_effect")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// TD-SEC-03, TD-SEC-27; SEC-017, SEC-051: one signed event replayed 100 times inside the window
/// verifies each time and has one effect.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn one_event_delivered_100_times_has_one_effect(pool: PgPool) {
    probe_tables(&pool).await;
    for n in 0..100 {
        let event = stripe(NOW, NOW + n).unwrap();
        let outcome = apply(&pool, "stripe", event.event_key().unwrap()).await;
        assert_eq!(outcome.is_first(), n == 0);
    }
    for n in 0..3 {
        let message = svix(NOW, NOW).unwrap();
        let outcome = apply(&pool, "resend", message.event_key().unwrap()).await;
        assert_eq!(outcome.is_first(), n == 0);
    }
    assert_eq!(effects(&pool).await, 2);
}

/// A Twilio status callback for one message, signed with the fake token.
fn status_callback(status: &str) -> VerifiedWebhook {
    let sid = "SM00000000000000000000000000000002";
    let body = format!("MessageSid={sid}&MessageStatus={status}");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(b"fake_twilio_auth_token").unwrap();
    mac.update(format!("{URL}MessageSid{sid}MessageStatus{status}").as_bytes());
    let sig = STANDARD.encode(mac.finalize().into_bytes());
    let headers = [("x-twilio-signature", sig.as_str())];
    let token = "fake_twilio_auth_token";
    verify(WebhookVendor::Twilio, token, body.as_bytes(), &headers, NOW).unwrap()
}

/// TD-SEC-18; SEC-039: verified status callbacks queued, sent, delivered, delivered, keyed on
/// (MessageSid, MessageStatus), have exactly 3 effects.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn twilio_status_sequence_has_three_effects(pool: PgPool) {
    probe_tables(&pool).await;
    let sequence = [
        ("queued", true),
        ("sent", true),
        ("delivered", true),
        ("delivered", false),
    ];
    for (status, first) in sequence {
        let callback = status_callback(status);
        let field = |name: &str| {
            let found = callback.params().iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap()
        };
        let key = EventKey::pair(&field("MessageSid"), &field("MessageStatus")).unwrap();
        assert_eq!(apply(&pool, "twilio_status", &key).await.is_first(), first);
    }
    assert_eq!(effects(&pool).await, 3);
}

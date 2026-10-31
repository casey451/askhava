//! Inbox de-duplication and delivery statuses on a real database (TD-TXT-04; TXT-012, TXT-013).
//!
//! Every webhook here is signed with a fake Twilio token and passed through `verify_webhook`, so
//! the module sees only what a route would hand it. Proves that the same inbound MessageSid three
//! times makes one inbox row, also when the three arrive at once; that a sent text's status
//! follows its callbacks (queued, sent, delivered, a repeat and a late `sent` or `accepted` changing
//! nothing),
//! a failure keeps Twilio's error code, a callback that came before the send was marked still
//! counts, and an unsent row reads queued or sending; and that an ill-shaped
//! delivery is refused. All data is fake.

use askhava_adapters::api::{
    AdapterError, Delivery, IdempotencyKey, SentText, SmsAdapter, VerifiedWebhook, WebhookVendor,
    verify_webhook,
};
use askhava_messaging::api::{
    Callback, InboxError, NewText, Relation, To, delivery_status, enqueue, receive_inbound,
    receive_status, send,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;

const URL: &str = "https://hooks.example.test/twilio";
const TOKEN: &str = "fake_twilio_auth_token";
const INBOUND_SID: &str = "SM0000000000000000000000000000ab01";
const SENT_SID: &str = "SM0000000000000000000000000000cd02";

/// A Twilio delivery of `fields`, signed with the fake token and verified as a route would.
fn twilio(fields: &[(&str, &str)]) -> VerifiedWebhook {
    let body: Vec<String> = fields.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let body = body.join("&");
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(URL.as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let sig = STANDARD.encode(mac.finalize().into_bytes());
    let headers = [("X-Twilio-Signature", sig.as_str())];
    let delivery = Delivery {
        url: URL,
        body: body.as_bytes(),
        headers: &headers,
    };
    verify_webhook(
        WebhookVendor::Twilio,
        &SecretString::from(TOKEN),
        delivery,
        0,
    )
    .expect("signed")
}

fn inbound(sid: &str) -> VerifiedWebhook {
    twilio(&[("MessageSid", sid), ("Body", "fake-reply")])
}

fn status(sid: &str, value: &str) -> VerifiedWebhook {
    twilio(&[("MessageSid", sid), ("MessageStatus", value)])
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// A carrier that accepts every text under one fake SID.
struct FakeCarrier;

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        Ok(SentText {
            sid: SENT_SID.to_owned(),
            status: "queued".to_owned(),
        })
    }
}

/// Queues one business text for job `job`; returns its outbox ID.
async fn queued(pool: &PgPool, job: u128) -> Uuid {
    let subject = [Uuid::from_u128(job)];
    let text = NewText {
        template_id: "first_quote",
        subject: &subject,
        recipient: Uuid::from_u128(0x0b0),
        relation: Relation::Business,
        body: "Ask Hava: fake body",
    };
    let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
    let mut db = pool.acquire().await.expect("connection");
    enqueue(&mut db, &secret, &text).await.expect("queued").0
}

async fn sent(pool: &PgPool, id: Uuid) {
    let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
    let mut db = pool.acquire().await.expect("connection");
    let to = (To::Business("+19285550142"), "+19285550100");
    send(&mut db, (&secret, &FakeCarrier), id, to)
        .await
        .expect("sent");
}

/// A callback's outcome as text: the recorded status's stored name, `repeat` or `ignored`
/// (compared as names, so no status variant is matched outside its module, SEC-031).
fn label(outcome: Callback) -> &'static str {
    match outcome {
        Callback::Recorded(recorded) => recorded.as_str(),
        Callback::Repeat => "repeat",
        Callback::Ignored => "ignored",
    }
}

async fn record(pool: &PgPool, value: &str) -> &'static str {
    let mut db = pool.acquire().await.expect("connection");
    let outcome = receive_status(&mut db, &status(SENT_SID, value)).await;
    label(outcome.expect("recorded"))
}

async fn now_status(pool: &PgPool, id: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("connection");
    delivery_status(&mut db, id).await.expect("status").as_str()
}

/// TD-TXT-04, TXT-012: the same inbound message ID three times is handled once, in turn and at
/// once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_the_same_inbound_message_id_three_times_is_handled_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let first = receive_inbound(&mut db, &inbound(INBOUND_SID)).await;
    assert!(matches!(first, Ok(Some(_))));
    for _ in 0..2 {
        assert_eq!(
            receive_inbound(&mut db, &inbound(INBOUND_SID)).await,
            Ok(None)
        );
    }
    let racing = "SM0000000000000000000000000000ab02";
    let runs: Vec<_> = (0..3)
        .map(|_| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let mut db = pool.acquire().await.expect("connection");
                receive_inbound(&mut db, &inbound(racing)).await
            })
        })
        .collect();
    let mut firsts = 0;
    for run in runs {
        firsts += usize::from(run.await.expect("joined").expect("recorded").is_some());
    }
    assert_eq!(firsts, 1);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM messaging.inbox").await,
        2
    );
}

/// TD-TXT-04, TXT-012, TXT-013: delivery statuses are recorded once each, a late earlier status
/// never moves a text back, and an unsent row reads queued or sending.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_04_delivery_statuses_are_recorded(pool: PgPool) {
    let id = queued(&pool, 1).await;
    assert_eq!(now_status(&pool, id).await, "queued");
    assert_eq!(record(&pool, "queued").await, "queued");
    sent(&pool, id).await;
    assert_eq!(now_status(&pool, id).await, "sent");
    assert_eq!(record(&pool, "sent").await, "sent");
    assert_eq!(record(&pool, "delivered").await, "delivered");
    assert_eq!(record(&pool, "delivered").await, "repeat");
    assert_eq!(record(&pool, "sent").await, "repeat");
    assert_eq!(record(&pool, "accepted").await, "queued");
    assert_eq!(record(&pool, "received").await, "ignored");
    assert_eq!(now_status(&pool, id).await, "delivered");
    let events = "SELECT count(*) FROM messaging.delivery_event";
    assert_eq!(count(&pool, events).await, 4);
    let other = queued(&pool, 2).await;
    sqlx::query("UPDATE messaging.outbox SET status = 'sending' WHERE id = $1")
        .bind(other)
        .execute(&pool)
        .await
        .expect("claimed");
    assert_eq!(now_status(&pool, other).await, "sending");
}

/// TXT-013: a failure keeps Twilio's error code and is the text's status.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_text_keeps_its_error_code(pool: PgPool) {
    let id = queued(&pool, 3).await;
    sent(&pool, id).await;
    let hook = twilio(&[
        ("MessageSid", SENT_SID),
        ("MessageStatus", "undelivered"),
        ("ErrorCode", "30003"),
    ]);
    let mut db = pool.acquire().await.expect("connection");
    let outcome = receive_status(&mut db, &hook).await.expect("recorded");
    assert_eq!(label(outcome), "undelivered");
    assert_eq!(now_status(&pool, id).await, "undelivered");
    let code: Option<i32> = sqlx::query_scalar("SELECT error_code FROM messaging.delivery_event")
        .fetch_one(&pool)
        .await
        .expect("event");
    assert_eq!(code, Some(30003));
}

/// TXT-012: a delivery with no well-formed MessageSid, status or error code is refused and
/// records nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ill_shaped_deliveries_are_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let bad = BadCases::new();
    for hook in &bad.inbound {
        assert_eq!(
            receive_inbound(&mut db, hook).await,
            Err(InboxError::BadWebhook)
        );
    }
    for hook in &bad.status {
        assert_eq!(
            receive_status(&mut db, hook).await,
            Err(InboxError::BadWebhook)
        );
    }
    let events = "SELECT count(*) FROM messaging.webhook_event";
    assert_eq!(count(&pool, events).await, 0);
}

/// The refused deliveries of `ill_shaped_deliveries_are_refused`.
struct BadCases {
    inbound: Vec<VerifiedWebhook>,
    status: Vec<VerifiedWebhook>,
}

impl BadCases {
    fn new() -> Self {
        Self {
            inbound: vec![
                twilio(&[("Body", "fake-reply")]),
                inbound("SM-not-a-sid"),
                inbound("XX0000000000000000000000000000ab01"),
            ],
            status: vec![
                twilio(&[("MessageSid", SENT_SID)]),
                status(SENT_SID, "bad:status"),
                twilio(&[
                    ("MessageSid", SENT_SID),
                    ("MessageStatus", "failed"),
                    ("ErrorCode", "x1"),
                ]),
            ],
        }
    }
}

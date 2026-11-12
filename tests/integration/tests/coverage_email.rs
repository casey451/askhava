//! The owner-inbox email for each request no business covers (TD-OPS-18, TD-INT-07; T215,
//! D-435, D-3332, D-3334; OPS-041, INT-016, INT-017, INT-018).
//!
//! A boat-lift request (a trade not open) closes as Not covered with the INT-017 reply, one
//! demand row and one `intake.not_covered` task holding the trade, town, request number and the
//! month's count, never the homeowner's contact or words; a second run queues nothing more; a
//! second request that month counts 2. `businesses`' handler emails the owner inbox once through a
//! fake provider, however often it runs, and the email holds no homeowner detail. A town off the
//! list closes early with no task. The digest's roll-up is TD-OPS-09's; the count line TD-INT-29's.
//! Fake IDs, addresses and texts only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    BAD_COVERAGE_PAYLOAD, CoverageMailer, NO_COVERAGE_SENDER, run_coverage_notice,
};
use askhava_core::api::{Claimed, HandlerError, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, NOT_COVERED_KIND, NOT_COVERED_TEMPLATE, Opened, Outcome, Request, Routing, TextIn,
    Why, apply_routing, begin_job, receive_text, recruiting_demand, route, trade_is_open,
};
use askhava_jobs::api::{current, job_number};
use askhava_messaging::api::{COVERAGE_EMAIL_TEMPLATE, DailyLimit, Language, MailboxKind};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const AREA: [&str; 1] = [TOWN];
const WORDS: &str = "my boat lift is stuck at 12 Fake Lane";
const OWNER_INBOX: &str = "owner-inbox@example.test";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at() -> Timestamp {
    "2026-11-02T10:00:00-07:00".parse().expect("time")
}

fn text(contact: Uuid) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::from_u128(0x4_6000 + contact.as_u128()),
        contact,
        inbound: Inbound {
            words: WORDS,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(),
    }
}

/// Opens a request with one `trade` job for `contact` and routes it in `town`.
async fn routed(db: &mut PgConnection, contact: Uuid, town: &str) -> (Uuid, Uuid, Outcome) {
    let msg = text(contact);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let conversation = handled.conversation.expect("request");
    let start = JobStart {
        conversation,
        words: WORDS,
        emergency: false,
        trade: "boat_lift",
        at: at(),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let outcome = route_again(db, conversation, &msg, town).await;
    (conversation, job, outcome)
}

async fn route_again(
    db: &mut PgConnection,
    conversation: Uuid,
    msg: &TextIn<'_>,
    town: &str,
) -> Outcome {
    let req = Request {
        town,
        area: &AREA,
        trade: "boat_lift",
        trade_open: trade_is_open(db, "boat_lift").await.expect("open"),
        words: WORDS,
        move_states: None,
        job_types: &[],
    };
    let outcome = route(&req);
    let routing = Routing {
        conversation,
        msg,
        language: Language::En,
        spanish_on: false,
        label: "boat lift",
        job_type: None,
    };
    let queued = apply_routing(db, &secret(), (&req, outcome), &routing).await;
    let queued = queued.expect("apply");
    if outcome == Outcome::NotCovered(Why::TradeNotOpen)
        && let Some(sent) = queued
    {
        assert_eq!(sent.template, NOT_COVERED_TEMPLATE);
    }
    outcome
}

async fn tasks(db: &mut PgConnection) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(NOT_COVERED_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

#[derive(Default)]
struct FakeMail {
    sent: Mutex<Vec<(String, String, String)>>,
}

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push((e.to.to_owned(), e.subject.to_owned(), e.text.to_owned()));
        Ok(format!("re_{:08}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn mailer() -> Arc<CoverageMailer<FakeMail>> {
    Arc::new(CoverageMailer {
        secret: secret(),
        postal: (
            "PO Box 100, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
        provider: FakeMail::default(),
        limit: DailyLimit::new(1000, 80).expect("limit"),
        owner_inbox: SecretString::from(OWNER_INBOX.to_owned()),
        clock: Arc::new(SystemClock),
    })
}

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: NOT_COVERED_KIND.to_owned(),
        payload,
        attempts: 1,
    }
}

/// TD-INT-07, TD-OPS-18 (INT-016, INT-017, OPS-041): a trade not open closes the request as Not
/// covered with one demand row and one owner-email task with trade, town, request number and the
/// month's count and no homeowner detail; a rerun adds nothing; a second request counts 2.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_07_a_trade_not_open_queues_one_owner_email_task(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let contact = Uuid::from_u128(0x7201);
    let (conversation, job, outcome) = routed(&mut db, contact, TOWN).await;
    assert_eq!(outcome, Outcome::NotCovered(Why::TradeNotOpen));
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "not_covered"
    );
    assert_eq!(
        recruiting_demand(&mut db, conversation)
            .await
            .expect("rows")
            .len(),
        1
    );
    let number = job_number(&mut db, job).await.expect("number");
    let want = json!({"conversation": conversation, "trade": "boat_lift", "label": "boat lift",
        "town": TOWN, "number": number, "month_count": 1});
    assert_eq!(tasks(&mut db).await, vec![want.clone()]);
    let raw = want.to_string();
    assert!(!raw.contains(&contact.to_string()) && !raw.contains("Fake Lane"));
    route_again(&mut db, conversation, &text(contact), TOWN).await;
    assert_eq!(
        tasks(&mut db).await.len(),
        1,
        "a rerun queues no second task"
    );
    routed(&mut db, Uuid::from_u128(0x7202), TOWN).await;
    let all = tasks(&mut db).await;
    assert_eq!(all.len(), 2);
    assert_eq!(all[1]["month_count"], 2, "the month's count so far");
}

/// TD-INT-07 (INT-022): a town off the list closes early and queues no owner email task.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_07_a_town_off_the_list_queues_no_owner_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let (_, job, outcome) = routed(&mut db, Uuid::from_u128(0x7203), "needles").await;
    assert_eq!(outcome, Outcome::OutOfArea);
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "closed_early"
    );
    assert!(tasks(&mut db).await.is_empty());
}

/// TD-OPS-18 (OPS-041, SEC-018, SEC-034): the handler emails the owner inbox once with the
/// trade, town, request number and month's count; a rerun stores and sends nothing more; no
/// homeowner contact, words or address is in the email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_18_one_owner_inbox_email_with_no_homeowner_detail(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let contact = Uuid::from_u128(0x7204);
    let (_, job, _) = routed(&mut db, contact, TOWN).await;
    let number = job_number(&mut db, job).await.expect("number");
    let payload = tasks(&mut db).await.remove(0);
    let mail = mailer();
    for _ in 0..2 {
        run_coverage_notice(
            pool.clone(),
            Some(Arc::clone(&mail)),
            claimed(payload.clone()),
        )
        .await
        .expect("emailed");
    }
    let sent = mail.provider.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1, "one email however often the task runs");
    let (to, subject, body) = &sent[0];
    assert_eq!(to, OWNER_INBOX);
    assert_eq!(
        subject,
        "Ask Hava: No business covers boat lift work in Lake Havasu City yet"
    );
    for line in [
        "Trade: boat lift".to_owned(),
        "Town: Lake Havasu City".to_owned(),
        format!("Request number: {number}"),
        "All requests not covered this month, counting this one: 1".to_owned(),
    ] {
        assert!(body.contains(&line), "{line}");
    }
    for detail in [
        contact.to_string(),
        "Fake Lane".to_owned(),
        "stuck".to_owned(),
    ] {
        assert!(
            !body.contains(&detail) && !subject.contains(&detail),
            "{detail}"
        );
    }
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.email_outbox WHERE template_id = $1")
            .bind(COVERAGE_EMAIL_TEMPLATE)
            .fetch_one(&mut *db)
            .await
            .expect("rows");
    assert_eq!(rows, 1);
}

/// TD-OPS-18: with no sender the task fails and is tried again; a payload `intake` never writes
/// (a phone number as the trade, a missing count) is refused before anything is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_18_no_sender_or_a_bad_payload_sends_nothing(pool: PgPool) {
    let good = json!({"conversation": Uuid::from_u128(0x7205), "trade": "boat_lift",
        "label": "boat lift", "town": TOWN, "number": null, "month_count": 3});
    let none: Option<Arc<CoverageMailer<FakeMail>>> = None;
    let refused = run_coverage_notice(pool.clone(), none, claimed(good.clone())).await;
    assert_eq!(refused, Err(HandlerError(NO_COVERAGE_SENDER)));
    let mut phone = good.clone();
    phone["label"] = json!("call 928 555 0100");
    let mut no_count = good.clone();
    no_count["month_count"] = Value::Null;
    let mut bad_town = good.clone();
    bad_town["town"] = json!("Lake Havasu City");
    for bad in [phone, no_count, bad_town] {
        let mail = mailer();
        let refused =
            run_coverage_notice(pool.clone(), Some(Arc::clone(&mail)), claimed(bad)).await;
        assert_eq!(refused, Err(HandlerError(BAD_COVERAGE_PAYLOAD)));
        assert!(mail.provider.sent.lock().expect("lock").is_empty());
    }
    let mail = mailer();
    run_coverage_notice(pool.clone(), Some(Arc::clone(&mail)), claimed(good))
        .await
        .expect("no number is fine");
    let sent = mail.provider.sent.lock().expect("lock").clone();
    assert!(
        !sent[0].2.contains("Request number"),
        "no job, no number line"
    );
}

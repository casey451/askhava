//! TD-CON-34 (CON-016, CON-030, CON-009; T360, D-2629): an opt-out while texting is off is
//! confirmed by email once per opt-out event when an email is on file and not at all without one;
//! a listed homeowner who writes again is emailed at most once per Arizona day, or with no email on
//! file listed once per message by an `ops` audit row `messaging.optout_wrote_again`; no audit row,
//! stored email or log line holds her phone, email or words. Fake data only.

use std::sync::{Arc, Mutex};

use askhava_core::api::MIGRATOR;
use askhava_messaging::api::{
    Language, MailboxKind, OPTOUT_CONFIRMATION_EMAIL, OPTOUT_WROTE_AGAIN_EMAIL, OptOutEmailSender,
    WROTE_AGAIN_ACTION, WroteAgain, queue_optout_confirmation_email, queue_optout_wrote_again,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// 10:00 Arizona on a Tuesday.
const NOW: i64 = 1_793_638_800;
const PHONE: &str = "+19285550173";
const OTHER_PHONE: &str = "+19285550174";
const EMAIL: &str = "dana.testperson@example.com";
const WORDS: &str = "is anyone coming about my leaking water heater";

fn at(hours: i64) -> Timestamp {
    Timestamp::from_second(NOW).expect("time") + SignedDuration::from_hours(hours)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x60), &key(0x61), &url).expect("vault")
}

fn sender() -> OptOutEmailSender {
    let secret = SecretString::from("fake-email-key-secret-0000000360".to_owned());
    let postal = (
        "PO Box 1234, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    );
    OptOutEmailSender::new(secret, postal, vault()).expect("sender")
}

/// Her vault contact, with the fake email when `email`.
async fn her(pool: &PgPool, email: bool) -> Uuid {
    contact(pool, email, PHONE).await
}

/// A vault contact with `phone`, and the fake email when `email`.
async fn contact(pool: &PgPool, email: bool, phone: &str) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(phone)),
        email: email.then(|| SecretString::from(EMAIL)),
        address: None,
        onsite_contact: None,
    };
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

/// Every stored email of `template` to `contact`, as JSON text.
async fn emails(pool: &PgPool, template: &str, contact: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT row_to_json(e)::text FROM messaging.email_outbox e \
         WHERE template_id = $1 AND recipient_id = $2 ORDER BY queued_at",
    )
    .bind(template)
    .bind(contact)
    .fetch_all(pool)
    .await
    .expect("emails")
}

/// Every wrote-again audit row, as JSON text.
async fn listed(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT row_to_json(l)::text FROM ops.audit_log l WHERE action = $1 ORDER BY id",
    )
    .bind(WROTE_AGAIN_ACTION)
    .fetch_all(pool)
    .await
    .expect("audit rows")
}

/// TD-CON-34 (CON-030, CON-009): with an email on file one confirmation email per opt-out event,
/// none on a repeat, the CON-009 words in her language.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_34_one_confirmation_email_per_opt_out_event(pool: PgPool) {
    let contact = her(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    let to = (contact, Language::En);
    let stored = queue_optout_confirmation_email(&mut db, &sender(), to, first, at(0)).await;
    assert!(stored.expect("stored"));
    let again = queue_optout_confirmation_email(&mut db, &sender(), to, first, at(1)).await;
    assert!(!again.expect("read"), "a repeat stores nothing");
    let es = (contact, Language::Es);
    let other = queue_optout_confirmation_email(&mut db, &sender(), es, second, at(2)).await;
    assert!(other.expect("stored"), "a new event gets its own email");
    let rows = emails(&pool, OPTOUT_CONFIRMATION_EMAIL, contact).await;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains("You're unsubscribed. You won't get more texts"));
    assert!(rows[1].contains("Se canceló su suscripción."));
    assert!(
        rows[0].contains(&first.to_string()),
        "the event is its subject"
    );
}

/// TD-CON-34 (CON-030): without an email on file nothing is stored, and never a text.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_34_no_confirmation_email_without_an_email(pool: PgPool) {
    let contact = her(&pool, false).await;
    let mut db = pool.acquire().await.expect("db");
    let to = (contact, Language::En);
    let event = Uuid::new_v4();
    let stored = queue_optout_confirmation_email(&mut db, &sender(), to, event, at(0)).await;
    assert!(!stored.expect("read"));
    assert!(
        emails(&pool, OPTOUT_CONFIRMATION_EMAIL, contact)
            .await
            .is_empty()
    );
    let texts: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&pool)
        .await
        .expect("texts");
    assert_eq!(texts, 0);
}

/// TD-CON-34 (CON-016): a listed homeowner's message gives one email, a second the same Arizona
/// day (even after midnight UTC) none, the next day one; no audit row.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_34_wrote_again_emails_at_most_once_a_day(pool: PgPool) {
    let contact = her(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let to = (contact, Language::En);
    let mut wrote = Vec::new();
    for hours in [0, 13, 24] {
        let inbox = Uuid::new_v4();
        let done = queue_optout_wrote_again(&mut db, &sender(), to, inbox, at(hours)).await;
        wrote.push(done.expect("handled"));
    }
    let want = [true, false, true].map(WroteAgain::Emailed);
    assert_eq!(wrote, want, "13:00 later is 23:00 the same Arizona day");
    let rows = emails(&pool, OPTOUT_WROTE_AGAIN_EMAIL, contact).await;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains("You opted out of texts from Ask Hava"));
    assert!(rows[0].contains("send START") && rows[0].contains("quote page"));
    assert!(listed(&pool).await.is_empty());
}

/// TD-CON-34 (CON-016): without an email one audit row per message (a retry adds none), no email.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_34_wrote_again_without_an_email_is_listed_per_message(pool: PgPool) {
    let contact = her(&pool, false).await;
    let mut db = pool.acquire().await.expect("db");
    let to = (contact, Language::Es);
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    let mut wrote = Vec::new();
    for inbox in [first, first, second] {
        let done = queue_optout_wrote_again(&mut db, &sender(), to, inbox, at(0)).await;
        wrote.push(done.expect("handled"));
    }
    assert_eq!(wrote, [true, false, true].map(WroteAgain::Listed));
    let rows = listed(&pool).await;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains(&first.to_string()) && rows[0].contains("messaging.inbox"));
    assert!(
        emails(&pool, OPTOUT_WROTE_AGAIN_EMAIL, contact)
            .await
            .is_empty()
    );
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

/// TD-CON-34 (PRIV-016, SEC-018): after every path, no audit row, stored email or log line holds
/// her phone, her email or her words.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_34_no_body_phone_or_email_in_any_row_or_log_line(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (with, without) = (
        her(&pool, true).await,
        contact(&pool, false, OTHER_PHONE).await,
    );
    let mut db = pool.acquire().await.expect("db");
    for contact in [with, without] {
        let to = (contact, Language::En);
        let event = Uuid::new_v4();
        queue_optout_confirmation_email(&mut db, &sender(), to, event, at(0))
            .await
            .expect("confirmation");
        queue_optout_wrote_again(&mut db, &sender(), to, Uuid::new_v4(), at(1))
            .await
            .expect("wrote again");
    }
    let audits: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(l)::text FROM ops.audit_log l")
            .fetch_all(&pool)
            .await
            .expect("audit rows");
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(e)::text FROM messaging.email_outbox e")
            .fetch_all(&pool)
            .await
            .expect("emails");
    assert_eq!((listed(&pool).await.len(), stored.len()), (1, 2));
    let logged = lines.0.lock().expect("lines").clone();
    for text in audits.iter().chain(&stored).chain(&logged) {
        for secret in [
            PHONE,
            &PHONE[2..],
            OTHER_PHONE,
            &OTHER_PHONE[2..],
            EMAIL,
            WORDS,
            "Dana",
        ] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
    }
}

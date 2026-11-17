//! PICK-046's "couldn't take it" notice goes by email when the homeowner can't be texted (T446;
//! PICK-046, CON-028, CON-021, PRIV-004; D-3579, D-3880).
//!
//! A homeowner texts in (consent text and record) and a plumbing job walks to Quotes ready with
//! quotes from named businesses; `intake::api::job_text_step` then runs the `pick_lapsed` event
//! for the first business, with keys that carry the footer postal line. An opted-out number with
//! an email on file gets one email and no text, a rerun stores nothing more, no email on file gets
//! nothing, and a textable number gets the text and no email. Every number (555-01xx), address,
//! name, ID and word is fake.

use askhava_core::api::{IdSecret, brand};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    JobStart, JobText, JobTextKeys, JobTextSkip, JobTextStep, Opened, PICK_LAPSED_EMAIL, TextIn,
    begin_job, job_text_step, receive_text,
};
use askhava_jobs::api::{JobStatus, current, move_to, record_job_facts};
use askhava_messaging::api::{MailboxKind, PostalAddress, RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_picks::api::PICK_LAPSED_TEMPLATE;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ID_SECRET: &str = "test-only-id-secret-for-t446-not-a-real-key";
const PHONE: &str = "+19285550146";
const EMAIL: &str = "sample.homeowner@example.invalid";
/// 10:00 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
const NAMES: [&str; 3] = [
    "Sample Drain Co A",
    "Sample Drain Co B",
    "Sample Drain Co C",
];
const PICK: Uuid = Uuid::from_u128(0x446_0001);

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn business(n: usize) -> Uuid {
    Uuid::from_u128(0x446_1000 + u128::try_from(n).expect("small"))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x46), &key(0x64), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

fn keys() -> JobTextKeys {
    let link = IdSecret::new(1, secret()).expect("link key");
    let postal = PostalAddress::new("PO Box 0146, Sample City, AZ 86400", MailboxKind::PoBox);
    JobTextKeys::new(link, secret(), vault()).with_postal(postal.expect("postal"))
}

/// `count` named businesses, each with a confirmed quote on `job`, in arrival order.
async fn quoted(db: &mut PgConnection, job: Uuid, count: usize, at: Timestamp) {
    for (i, name) in NAMES.iter().take(count).enumerate() {
        let id = business(i + 1);
        sqlx::query(
            "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
             VALUES ($1, 'active', now(), now())",
        )
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("business");
        sqlx::query(
            "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
             VALUES ($1, $2, now())",
        )
        .bind(id)
        .bind(*name)
        .execute(&mut *db)
        .await
        .expect("name");
        let received = at + SignedDuration::from_mins(i64::try_from(i).expect("small"));
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(id)
        .bind(received.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
}

/// Stores `job`'s facts and walks it from Intake to Quotes ready with `count` quotes.
async fn walk_to_quotes_ready(db: &mut PgConnection, job: Uuid, count: usize, at: Timestamp) {
    let types = ["drain cleaning".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{step:?}");
    }
    quoted(db, job, count, at).await;
}

/// A homeowner (with an email when `email`) texts in and her job walks to Quotes ready with
/// `count` quotes; returns the `pick_lapsed` event for business 1's ended pick.
async fn lapsed_event(db: &mut PgConnection, email: bool, count: usize) -> JobText {
    let at = ts(DAY);
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: email.then(|| SecretString::from(EMAIL.to_owned())),
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen drain is backing up";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: at,
    };
    let handled = receive_text(db, &secret(), &msg, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    walk_to_quotes_ready(db, job, count, at).await;
    let version = current(db, job).await.expect("job").version;
    JobText {
        job,
        version,
        template: PICK_LAPSED_TEMPLATE,
        values: json!({"job": job, "pick": PICK, "business": business(1)}),
    }
}

async fn opt_out(db: &mut PgConnection) {
    let channel = (RevokeChannel::Portal, "portal");
    suppress(db, &vault(), PHONE, channel, ts(DAY))
        .await
        .expect("suppress");
}

async fn step(db: &mut PgConnection, text: &JobText) -> JobTextStep {
    job_text_step(db, &keys(), text, ts(DAY))
        .await
        .expect("step")
}

/// The stored PICK-046 emails' subjects and plain parts.
async fn emails(db: &mut PgConnection) -> Vec<(String, String)> {
    sqlx::query_as("SELECT subject, plain FROM messaging.email_outbox WHERE template_id = $1")
        .bind(PICK_LAPSED_EMAIL)
        .fetch_all(db)
        .await
        .expect("emails")
}

async fn texts(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
        .bind(PICK_LAPSED_TEMPLATE)
        .fetch_one(db)
        .await
        .expect("texts")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_opted_out_with_an_email_gets_one_email_and_no_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let text = lapsed_event(&mut db, true, 3).await;
    opt_out(&mut db).await;
    let got = step(&mut db, &text).await;
    assert_eq!(got, JobTextStep::Skipped(JobTextSkip::OptedOut));
    assert_eq!(texts(&mut db).await, 0, "no text after an opt-out");
    let stored = emails(&mut db).await;
    assert_eq!(stored.len(), 1, "one email");
    let (subject, plain) = &stored[0];
    let short = brand().short_name();
    assert_eq!(
        subject,
        &format!("{short}: {} couldn't take it this time", NAMES[0])
    );
    let link = format!("https://{}/q/", brand().domain());
    assert!(
        plain.starts_with(&format!(
            "{} couldn't take it this time. You still have other quotes. \
             You can pick another one on your quote page: {link}",
            NAMES[0]
        )),
        "{plain}"
    );
    for other in &NAMES[1..] {
        assert!(!plain.contains(other), "no other business: {plain}");
    }
    assert!(!plain.contains("190") && !plain.contains('$'), "no amount");
    assert!(
        !plain.contains(PHONE) && !plain.contains(EMAIL),
        "no contact"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_with_no_other_quote_the_email_says_we_will_email_you(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let text = lapsed_event(&mut db, true, 1).await;
    opt_out(&mut db).await;
    step(&mut db, &text).await;
    let stored = emails(&mut db).await;
    assert_eq!(stored.len(), 1, "one email");
    let want = format!(
        "{} couldn't take it this time. We'll email you when another quote comes in. \
         If it can't wait, it's fine to call a company you know.\n",
        NAMES[0]
    );
    assert!(stored[0].1.starts_with(&want), "{}", stored[0].1);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_no_email_on_file_gets_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let text = lapsed_event(&mut db, false, 3).await;
    opt_out(&mut db).await;
    let got = step(&mut db, &text).await;
    assert_eq!(got, JobTextStep::Skipped(JobTextSkip::OptedOut));
    assert_eq!(texts(&mut db).await, 0, "no text");
    assert!(emails(&mut db).await.is_empty(), "no email");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_a_rerun_stores_one_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let text = lapsed_event(&mut db, true, 3).await;
    opt_out(&mut db).await;
    step(&mut db, &text).await;
    step(&mut db, &text).await;
    assert_eq!(emails(&mut db).await.len(), 1, "one email after a rerun");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_a_textable_homeowner_gets_the_text_and_no_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let text = lapsed_event(&mut db, true, 3).await;
    let got = step(&mut db, &text).await;
    assert!(matches!(got, JobTextStep::Queued(_)), "{got:?}");
    assert_eq!(texts(&mut db).await, 1, "the text");
    assert!(emails(&mut db).await.is_empty(), "never both");
}

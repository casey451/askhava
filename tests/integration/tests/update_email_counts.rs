//! Quote updates reach Email and web-and-email homeowners, and the count told is the count of the
//! update last sent (T840; TXT-017, TXT-018, TXT-021, TXT-034, CON-035; D-5617, D-5639, D-5640).
//!
//! In PostgreSQL, with a stand-in for the email provider: a homeowner (fake 555 number) opens a
//! plumbing job, chooses her update channel and gets the first-quote update through
//! `intake::job_text_step`; each later quote runs `intake::quote_update_step`. Rows queued by the
//! database clock are stamped back to the test's instant; a bounce, a drop or a delivery is the
//! provider's event row. Every number, name, address and word is fake.

use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_core::api::IdSecret;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, JobTextKeys, JobTextSkip, JobTextStep, Opened, QuoteUpdateStep, TextIn, begin_job,
    job_text_step, parse_job_text, quote_update_step, receive_text,
};
use askhava_jobs::api::{JobStatus, first_quote, move_to};
use askhava_messaging::api::{
    DailyLimit, EmailSent, EmailTo, MailboxKind, PostalAddress, RevokeChannel, UpdateChannel,
    last_quote_text, send_email, set_update_channel, set_update_footer, suppress,
};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, UpdateAddress, Vault, store_contact, store_update_address};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ID_KEY: &str = "fake-id-secret-for-tests-00000840";
/// 10:00 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
const HOUR: i64 = 3600;
/// 2:00 a.m. Arizona the next day: text quiet hours.
const NIGHT: i64 = 16 * HOUR;

fn at(secs: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("day") + SignedDuration::from_secs(secs)
}

fn key() -> SecretString {
    SecretString::from(ID_KEY.to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x84), &key(0x40), &url).expect("vault")
}

fn keys() -> JobTextKeys {
    JobTextKeys::new(IdSecret::new(1, key()).expect("secret"), key(), vault())
}

/// Homeowner `n` (+1928555 01nn): her contact, its phone and her open plumbing job with one quote
/// moved to Quotes ready; `channel` her update channel, with a fake address stored.
async fn homeowner(db: &mut PgConnection, n: u128, channel: UpdateChannel) -> (Uuid, String, Uuid) {
    let postal = PostalAddress::new("PO Box 0840, Sample City, AZ 86400", MailboxKind::PoBox);
    set_update_footer(postal.expect("postal"));
    let phone = format!("+1928555{:04}", 100 + n);
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.clone())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let job = open_job(db, n, contact).await;
    let address = UpdateAddress::parse(&format!("pat{n}@mail.test")).expect("address");
    store_update_address(db, &vault(), contact, &address, at(0))
        .await
        .expect("address");
    set_update_channel(db, contact, channel, at(0))
        .await
        .expect("channel");
    quote(db, job, 1).await;
    assert!(
        first_quote(db, job, at(0))
            .await
            .expect("first")
            .applied()
            .is_some()
    );
    (contact, phone, job)
}

/// Her request (texted in at `DAY`) and its plumbing job, moved to Open.
async fn open_job(db: &mut PgConnection, n: u128, contact: Uuid) -> Uuid {
    let inbound = Inbound {
        words: "my kitchen sink is leaking under the cabinet",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x840_0000 + n),
        contact,
        inbound,
        received_at: at(0),
    };
    let handled = receive_text(db, &key(), &msg, Settings::default()).await;
    let start = JobStart {
        conversation: handled.expect("open").conversation.expect("request"),
        words: "my kitchen sink is leaking",
        emergency: false,
        trade: "plumbing",
        at: at(0),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let moved = move_to(
        db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        at(0),
        Actor::System,
    )
    .await;
    assert!(moved.expect("open").applied().is_some());
    job
}

/// The web-chat homeowner who gave no texting consent (TXT-034): her consent record is removed.
async fn no_consent(db: &mut PgConnection, contact: Uuid) {
    sqlx::query(
        "DELETE FROM intake.consent_record r USING intake.homeowner h \
         WHERE h.id = r.homeowner_id AND h.contact_id = $1",
    )
    .bind(contact)
    .execute(db)
    .await
    .expect("fixture: no consent");
}

/// Business `n`'s confirmed quote on `job`.
async fn quote(db: &mut PgConnection, job: Uuid, n: u128) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0x840_1000 + n))
    .bind(at(0).to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

/// The first-quote event the move queued for `job`, run at `now`; the rows it queued stamped at `now`.
async fn first(db: &mut PgConnection, (contact, job): (Uuid, Uuid), now: Timestamp) -> JobTextStep {
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'jobs.homeowner_text' \
         AND payload->>'template' = 'first_quote' AND payload->>'job' = $1::text",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .expect("first-quote event");
    let text = parse_job_text(&payload).expect("payload");
    let step = job_text_step(db, &keys(), &text, now).await.expect("step");
    stamp(db, contact, now).await;
    step
}

/// `quote_update_step` for `job` at `now`; the rows it queued stamped at `now`.
async fn evaluate(
    db: &mut PgConnection,
    (contact, job): (Uuid, Uuid),
    now: Timestamp,
) -> QuoteUpdateStep {
    let step = quote_update_step(db, &keys(), (job, false), now).await;
    stamp(db, contact, now).await;
    step.expect("step")
}

/// Rows queued by the database clock, later than `now`, are given `now` as their queue time.
async fn stamp(db: &mut PgConnection, contact: Uuid, now: Timestamp) {
    for sql in [
        "UPDATE messaging.outbox SET created_at = $1 WHERE recipient_id = $2 AND created_at > $1",
        "UPDATE messaging.email_outbox SET queued_at = $1, not_before = $1 \
         WHERE recipient_id = $2 AND queued_at > $1",
    ] {
        sqlx::query(sql)
            .bind(now.to_sqlx())
            .bind(contact)
            .execute(&mut *db)
            .await
            .expect("fixture: queue time");
    }
}

/// `contact`'s text rows of `template`: their subject IDs.
async fn texts(db: &mut PgConnection, contact: Uuid, template: &str) -> Vec<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT subject_ids FROM messaging.outbox WHERE recipient_id = $1 AND template_id = $2",
    )
    .bind(contact)
    .bind(template)
    .fetch_all(db)
    .await
    .expect("texts")
}

/// `contact`'s update email rows of `template` (`update.` prefixed): ID and subject IDs, oldest
/// first.
async fn emails(db: &mut PgConnection, contact: Uuid, template: &str) -> Vec<(Uuid, Vec<Uuid>)> {
    sqlx::query_as(
        "SELECT id, subject_ids FROM messaging.email_outbox WHERE recipient_id = $1 \
         AND template_id = 'update.' || $2 ORDER BY queued_at, id",
    )
    .bind(contact)
    .bind(template)
    .fetch_all(db)
    .await
    .expect("emails")
}

/// The email provider stand-in: answers each send with a fake ID.
#[derive(Clone, Default)]
struct StandIn;

static SENT: AtomicUsize = AtomicUsize::new(0);

impl EmailAdapter for StandIn {
    async fn send_email(&self, _: &IdempotencyKey, _: &Email<'_>) -> Result<String, AdapterError> {
        Ok(format!("re_t840_{}", SENT.fetch_add(1, Ordering::Relaxed)))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// Sends email row `id` through the stand-in at `now`; with `event` the provider then reports
/// it (`email.bounced`, `email.delivered`) at `now`.
async fn deliver(db: &mut PgConnection, id: Uuid, now: Timestamp, event: Option<&str>) {
    let limit = DailyLimit::new(100, 90).expect("limit");
    let vault = vault();
    let sent = send_email(
        db,
        (&key(), &StandIn, limit),
        id,
        EmailTo::Homeowner(&vault),
        now,
    )
    .await;
    let Ok(EmailSent::Sent(provider)) = sent else {
        panic!("sent: {sent:?}");
    };
    if let Some(kind) = event {
        sqlx::query(
            "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at) \
             VALUES ($1, $2, $3, $3)",
        )
        .bind(provider)
        .bind(kind)
        .bind(now.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("fixture: provider event");
    }
}

/// A hard bounce of email `id` at `now` and T730's reset of her channel to text.
async fn bounce(db: &mut PgConnection, (contact, id): (Uuid, Uuid), now: Timestamp) {
    deliver(db, id, now, Some("email.bounced")).await;
    set_update_channel(db, contact, UpdateChannel::Text, now)
        .await
        .expect("reset to text");
}

fn count(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn queued(step: &QuoteUpdateStep) -> bool {
    matches!(step, QuoteUpdateStep::Queued(_))
}

/// TXT-018, TXT-017, D-5639: an Email homeowner's first quote and second quote are update emails
/// and never a text; after a hard bounce of the second and the reset to text, the next evaluation
/// tells the current count, 2, by text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_email_update_is_no_text_and_after_a_bounce_the_count_goes_by_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (contact, _, job) = homeowner(&mut db, 1, UpdateChannel::Email).await;
    assert!(matches!(
        first(&mut db, (contact, job), at(0)).await,
        JobTextStep::Queued(_)
    ));
    quote(&mut db, job, 2).await;
    assert!(queued(
        &evaluate(&mut db, (contact, job), at(4 * HOUR)).await
    ));
    let sent = emails(&mut db, contact, "quote_update").await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1[..2], [job, count(2)]);
    assert!(texts(&mut db, contact, "quote_update").await.is_empty());
    assert!(texts(&mut db, contact, "first_quote").await.is_empty());
    bounce(&mut db, (contact, sent[0].0), at(4 * HOUR + 60)).await;
    let step = evaluate(&mut db, (contact, job), at(5 * HOUR)).await;
    assert!(queued(&step), "{step:?}");
    assert_eq!(
        texts(&mut db, contact, "quote_update").await,
        [vec![job, count(2)]]
    );
}

/// TXT-034, TXT-021, CON-035: a web-and-email homeowner with no texting consent, and an Email
/// homeowner after STOP, get the update email and no text row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_consent_and_stop_still_get_the_email_and_no_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for n in [2, 3] {
        let (contact, phone, job) = homeowner(&mut db, n, UpdateChannel::Email).await;
        if n == 2 {
            no_consent(&mut db, contact).await;
        } else {
            let how = (RevokeChannel::Portal, "portal");
            suppress(&mut db, &vault(), &phone, how, at(60))
                .await
                .expect("stop");
        }
        assert!(matches!(
            first(&mut db, (contact, job), at(120)).await,
            JobTextStep::Queued(_)
        ));
        quote(&mut db, job, 2).await;
        assert!(queued(
            &evaluate(&mut db, (contact, job), at(4 * HOUR)).await
        ));
        assert_eq!(
            emails(&mut db, contact, "quote_update").await.len(),
            1,
            "{n}"
        );
        assert_eq!(
            emails(&mut db, contact, "first_quote").await.len(),
            1,
            "{n}"
        );
        assert!(
            texts(&mut db, contact, "quote_update").await.is_empty(),
            "{n}"
        );
        assert!(
            texts(&mut db, contact, "first_quote").await.is_empty(),
            "{n}"
        );
    }
}

/// CON-035, D-5640: a Both homeowner after STOP gets the update by email alone, at once at 2:00
/// a.m. Arizona; a Both homeowner who may be texted has the whole update held in quiet hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn both_after_stop_is_email_now_and_both_at_night_is_held(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for n in [4, 5] {
        let (contact, phone, job) = homeowner(&mut db, n, UpdateChannel::Both).await;
        assert!(matches!(
            first(&mut db, (contact, job), at(0)).await,
            JobTextStep::Queued(_)
        ));
        assert_eq!(texts(&mut db, contact, "first_quote").await.len(), 1);
        if n == 4 {
            let how = (RevokeChannel::Portal, "portal");
            suppress(&mut db, &vault(), &phone, how, at(60))
                .await
                .expect("stop");
        }
        quote(&mut db, job, 2).await;
        let step = evaluate(&mut db, (contact, job), at(NIGHT)).await;
        let updates = emails(&mut db, contact, "quote_update").await.len();
        assert!(
            texts(&mut db, contact, "quote_update").await.is_empty(),
            "{n}"
        );
        if n == 4 {
            assert!(queued(&step), "{step:?}");
            assert_eq!(updates, 1);
        } else {
            assert!(matches!(step, QuoteUpdateStep::Held(_)), "{step:?}");
            assert_eq!(updates, 0, "held whole");
        }
    }
}

/// TXT-017, D-5639: a sent 2-quote update, a dropped 3-quote update, then a fourth quote: the
/// next evaluation tells 4 at once, measured from the 2-quote update, not the dropped one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_dropped_update_is_not_counted_as_told(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (contact, _, job) = homeowner(&mut db, 6, UpdateChannel::Email).await;
    first(&mut db, (contact, job), at(0)).await;
    quote(&mut db, job, 2).await;
    assert!(queued(
        &evaluate(&mut db, (contact, job), at(4 * HOUR)).await
    ));
    let two = emails(&mut db, contact, "quote_update").await[0].0;
    deliver(&mut db, two, at(4 * HOUR), Some("email.delivered")).await;
    quote(&mut db, job, 3).await;
    assert!(queued(
        &evaluate(&mut db, (contact, job), at(7 * HOUR + 60)).await
    ));
    let three = emails(&mut db, contact, "quote_update").await[1].0;
    sqlx::query(
        "UPDATE messaging.email_outbox SET status = 'dropped', subject = NULL, html = NULL, \
         plain = NULL WHERE id = $1",
    )
    .bind(three)
    .execute(&mut *db)
    .await
    .expect("fixture: dropped");
    quote(&mut db, job, 4).await;
    let step = evaluate(&mut db, (contact, job), at(8 * HOUR)).await;
    assert!(queued(&step), "{step:?}");
    let sent = emails(&mut db, contact, "quote_update").await;
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2].1[..2], [job, count(4)]);
}

/// D-4419: the run times of the `quotes.count_update` retries queued for `job`.
async fn retries(db: &mut PgConnection, job: Uuid) -> Vec<Timestamp> {
    let rows: Vec<jiff_sqlx::Timestamp> = sqlx::query_scalar(
        "SELECT run_at FROM core.task WHERE kind = 'quotes.count_update' \
         AND payload->>'job' = $1::text AND (payload->>'retry')::boolean ORDER BY run_at",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("retries");
    rows.into_iter()
        .map(jiff_sqlx::Timestamp::to_jiff)
        .collect()
}

/// TXT-016, TXT-017, D-4419, D-5639, D-5640: a bounced `update.first_quote` email, then the reset
/// to text: the next evaluation queues the first-quote text and, with 2 quotes shown, the one
/// retry 15 minutes later that tells the count; with no texting consent (web-and-email) no text
/// row and no retry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_bounced_first_quote_email_becomes_the_first_quote_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for n in [7, 8] {
        let (contact, _, job) = homeowner(&mut db, n, UpdateChannel::Email).await;
        if n == 8 {
            no_consent(&mut db, contact).await;
        }
        first(&mut db, (contact, job), at(0)).await;
        let email = emails(&mut db, contact, "first_quote").await[0].0;
        bounce(&mut db, (contact, email), at(60)).await;
        quote(&mut db, job, 2).await;
        let step = evaluate(&mut db, (contact, job), at(HOUR)).await;
        let sent = texts(&mut db, contact, "first_quote").await;
        if n == 7 {
            assert!(queued(&step), "{step:?}");
            assert_eq!(sent, [vec![job]]);
            let retry = at(HOUR) + SignedDuration::from_mins(15);
            assert_eq!(retries(&mut db, job).await, [retry]);
        } else {
            assert_eq!(step, QuoteUpdateStep::Skipped(JobTextSkip::NoConsent));
            assert!(sent.is_empty());
            assert!(retries(&mut db, job).await.is_empty());
        }
    }
}

/// TXT-016, QH-001, D-4419, D-5640: the bounced first-quote email's text in quiet hours is held to
/// the window's opening, and the count's retry is queued 15 minutes after it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_bounced_first_quote_email_at_night_holds_the_text_and_queues_the_retry(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (contact, _, job) = homeowner(&mut db, 10, UpdateChannel::Email).await;
    first(&mut db, (contact, job), at(0)).await;
    let email = emails(&mut db, contact, "first_quote").await[0].0;
    bounce(&mut db, (contact, email), at(60)).await;
    quote(&mut db, job, 2).await;
    let step = evaluate(&mut db, (contact, job), at(NIGHT)).await;
    let QuoteUpdateStep::Held(until) = step else {
        panic!("{step:?}");
    };
    assert!(until > at(NIGHT));
    assert!(texts(&mut db, contact, "first_quote").await.is_empty());
    let retry = until + SignedDuration::from_mins(15);
    assert_eq!(retries(&mut db, job).await, [retry]);
}

/// TXT-017, D-5639: a Both homeowner's first-quote text and email queued in one instant tie, and
/// the reader gives the text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn on_a_tie_the_text_wins(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (contact, _, job) = homeowner(&mut db, 11, UpdateChannel::Both).await;
    first(&mut db, (contact, job), at(0)).await;
    assert_eq!(emails(&mut db, contact, "first_quote").await.len(), 1);
    let text: Uuid = sqlx::query_scalar(
        "SELECT id FROM messaging.outbox WHERE recipient_id = $1 AND template_id = 'first_quote'",
    )
    .bind(contact)
    .fetch_one(&mut *db)
    .await
    .expect("text");
    let last = last_quote_text(&mut db, contact, job)
        .await
        .expect("reader")
        .expect("a quote update");
    assert_eq!((last.id, last.template_id.as_str()), (text, "first_quote"));
}

/// Only `UPDATE_TEMPLATES` skip the text gates: a non-update job text to an Email homeowner with
/// no texting consent sends nothing, by text or email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_non_update_job_text_keeps_the_text_gates(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (contact, _, job) = homeowner(&mut db, 9, UpdateChannel::Email).await;
    no_consent(&mut db, contact).await;
    let version: i32 = sqlx::query_scalar("SELECT version FROM jobs.job WHERE id = $1")
        .bind(job)
        .fetch_one(&mut *db)
        .await
        .expect("version");
    let payload = serde_json::json!({"job": job, "version": version,
        "template": "check_in_day3", "values": {"job": 1001}});
    let text = parse_job_text(&payload).expect("payload");
    let step = job_text_step(&mut db, &keys(), &text, at(60))
        .await
        .expect("step");
    assert_eq!(step, JobTextStep::Skipped(JobTextSkip::NoConsent));
    let rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1 AND template_id LIKE '%check_in%') \
         + (SELECT count(*) FROM messaging.email_outbox WHERE recipient_id = $1)",
    )
    .bind(contact)
    .fetch_one(&mut *db)
    .await
    .expect("rows");
    assert_eq!(rows, 0);
}

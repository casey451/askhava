//! The read-back confirm's 24-hour window (PICK-001, PICK-006, D-4052), the quote-count update
//! event (TXT-017, D-4052, D-4064) and the last quote text's delivery read (TXT-017, D-1190).
//! Fake IDs, made-up quotes and made-up message IDs only.
//!
//! A confirm after the card's 24 hours is moved on and shows nothing; each quote newly shown on a
//! job already Quotes ready queues one `quotes.count_update` task holding the job ID only, the
//! first quote and a re-confirm or edit of a shown quote none; `messaging`'s read gives the time
//! TXT-017 measures from and skips a text that never reached her.

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, create, current, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_messaging::api::last_quote_text;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    COUNT_UPDATE_KIND, Format, Price, Quote, QuoteForm, QuoteRules, Submitted, confirm_quote_at,
    parse_quote, shown_quotes, submit_quote,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn quote(ai_read: bool) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(41_000),
        notes: "Haul-away included.".to_owned(),
        original: "410 haul away included".to_owned(),
        ai_read,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    parse_quote(form, &rules).expect("quote")
}

/// An Open job, created and moved at `when`.
async fn open_job(db: &mut Db, when: Timestamp) -> Uuid {
    let id = create(db, when, false, Actor::System).await.expect("job");
    let moved = move_to(
        db,
        id,
        (JobStatus::Intake, JobStatus::Open),
        when,
        Actor::System,
    )
    .await
    .expect("move");
    assert_eq!(moved, Guarded::Applied(()));
    id
}

/// Records business `n`'s card for `job`, sent at `sent` (the fixture stands in for matching).
async fn card(db: &mut Db, job: Uuid, n: u128, sent: Timestamp) -> Uuid {
    let business = Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) \
         VALUES ($1, $2, false, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(sent.to_sqlx())
    .execute(&mut **db)
    .await
    .expect("card");
    business
}

/// The payloads of the job's queued count-update events.
async fn count_events(db: &mut Db, job: Uuid) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY id",
    )
    .bind(COUNT_UPDATE_KIND)
    .bind(job.to_string())
    .fetch_all(&mut **db)
    .await
    .expect("tasks")
}

/// PICK-001, PICK-006, D-4052: a read-back confirm at the card's 24 hours is moved on and shows
/// nothing; one a second earlier is applied and moves the job to Quotes ready.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_confirm_after_the_cards_24_hours_is_moved_on(mut db: Db) {
    let id = open_job(&mut db, at("02T21:00:00")).await;
    let a = card(&mut db, id, 1, at("03T08:00:00")).await;
    let read = submit_quote(&mut db, (id, a), &quote(true), at("03T09:00:00")).await;
    assert_eq!(read, Ok(Submitted::Saved));
    let late = confirm_quote_at(&mut db, (id, a), at("04T08:00:00")).await;
    assert_eq!(late, Ok(Guarded::MovedOn));
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![]));
    let status = current(&mut db, id).await.expect("status").status;
    assert_eq!(status.code(), "open");
    let inside = confirm_quote_at(&mut db, (id, a), at("04T07:59:59")).await;
    assert_eq!(inside, Ok(Guarded::Applied(())));
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![a]));
    let status = current(&mut db, id).await.expect("status").status;
    assert_eq!(status.code(), "quotes_ready");
}

/// TXT-016, TXT-017, D-4052: the first quote queues no count event, a second shown quote (a form
/// quote or an AI-read one at its confirm) queues one with the job ID only, and a form edit or a
/// re-confirm of a shown quote queues none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_newly_shown_quote_after_the_first_queues_one_count_event(mut db: Db) {
    let id = open_job(&mut db, at("02T21:00:00")).await;
    let sent = at("03T08:00:00");
    let (a, b, c) = (
        card(&mut db, id, 1, sent).await,
        card(&mut db, id, 2, sent).await,
        card(&mut db, id, 3, sent).await,
    );
    let first = submit_quote(&mut db, (id, a), &quote(false), at("03T09:00:00")).await;
    assert_eq!(first, Ok(Submitted::Saved));
    assert!(count_events(&mut db, id).await.is_empty());
    let second = submit_quote(&mut db, (id, b), &quote(false), at("03T10:00:00")).await;
    assert_eq!(second, Ok(Submitted::Saved));
    assert_eq!(count_events(&mut db, id).await, vec![json!({ "job": id })]);
    let edit = submit_quote(&mut db, (id, b), &quote(false), at("03T10:30:00")).await;
    assert_eq!(edit, Ok(Submitted::Saved));
    let read = submit_quote(&mut db, (id, c), &quote(true), at("03T11:00:00")).await;
    assert_eq!(read, Ok(Submitted::Saved));
    assert_eq!(count_events(&mut db, id).await.len(), 1);
    let confirm = confirm_quote_at(&mut db, (id, c), at("03T12:00:00")).await;
    assert_eq!(confirm, Ok(Guarded::Applied(())));
    assert_eq!(count_events(&mut db, id).await.len(), 2);
    let again = confirm_quote_at(&mut db, (id, c), at("03T12:30:00")).await;
    assert_eq!(again, Ok(Guarded::Applied(())));
    let reconfirm_a = confirm_quote_at(&mut db, (id, a), at("03T12:40:00")).await;
    assert_eq!(reconfirm_a, Ok(Guarded::Applied(())));
    assert_eq!(count_events(&mut db, id).await.len(), 2);
}

/// One outbox row to `recipient` about `job`: `template`, `status`, queued at `queued`, sent at
/// `sent` (a `sent` row gets a made-up message ID and no body; any other row a fake body).
async fn text(
    db: &mut Db,
    (recipient, job): (Uuid, Uuid),
    (template, status): (&str, &str),
    (queued, sent): (Timestamp, Option<Timestamp>),
) -> String {
    let sid = format!("SM{}", Uuid::new_v4().simple());
    let is_sent = status == "sent";
    sqlx::query(
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, recipient_kind, \
         subject_ids, body, status, provider_sid, created_at, sent_at) \
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), $1, $2, 'homeowner', \
         ARRAY[$3]::uuid[], $4, $5, $6, $7, $8)",
    )
    .bind(template)
    .bind(recipient)
    .bind(job)
    .bind((!is_sent).then_some("Fake quote text."))
    .bind(status)
    .bind(is_sent.then(|| sid.clone()))
    .bind(queued.to_sqlx())
    .bind(sent.map(|t| t.to_sqlx()))
    .execute(&mut **db)
    .await
    .expect("outbox row");
    sid
}

/// Records Twilio's `delivered` event for message `sid` at `when`.
async fn delivered(db: &mut Db, sid: &str, when: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.delivery_event (provider_sid, status, received_at) \
         VALUES ($1, 'delivered', $2)",
    )
    .bind(sid)
    .bind(when.to_sqlx())
    .execute(&mut **db)
    .await
    .expect("delivery event");
}

/// The time `last_quote_text` gives for `recipient` and `job`, if any.
async fn last_at(db: &mut Db, recipient: Uuid, job: Uuid) -> Option<Timestamp> {
    last_quote_text(db, recipient, job)
        .await
        .expect("read")
        .map(|t| t.at)
}

/// TXT-017, D-1190, D-4064: the delivery time, else the send time, else the queue time of an
/// unsent row; none for another job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_last_quote_text_gives_its_delivery_send_or_queue_time(mut db: Db) {
    let (her, job, other_job) = (
        Uuid::from_u128(71),
        Uuid::from_u128(72),
        Uuid::from_u128(73),
    );
    assert_eq!(last_at(&mut db, her, job).await, None);
    let first = (at("03T09:00:00"), Some(at("03T09:00:05")));
    let sid = text(&mut db, (her, job), ("first_quote", "sent"), first).await;
    delivered(&mut db, &sid, at("03T09:00:20")).await;
    assert_eq!(last_at(&mut db, her, job).await, Some(at("03T09:00:20")));
    let update = (at("03T12:00:00"), Some(at("03T12:00:04")));
    text(&mut db, (her, job), ("quote_update", "sent"), update).await;
    assert_eq!(last_at(&mut db, her, job).await, Some(at("03T12:00:04")));
    let waiting = (at("03T15:30:00"), None);
    text(&mut db, (her, job), ("quote_update", "queued"), waiting).await;
    let last = last_quote_text(&mut db, her, job).await.expect("read");
    let last = last.expect("a text");
    assert_eq!(
        (last.template_id.as_str(), last.at),
        ("quote_update", at("03T15:30:00"))
    );
    assert_eq!(last_at(&mut db, her, other_job).await, None);
    assert_eq!(last_at(&mut db, Uuid::from_u128(74), job).await, None);
}

/// TXT-017, D-4064: a failed row never reached her, so the read gives the row before it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_quote_text_is_skipped_for_the_one_before_it(mut db: Db) {
    let (her, job) = (Uuid::from_u128(81), Uuid::from_u128(82));
    let first = (at("03T09:00:00"), Some(at("03T09:00:05")));
    let sid = text(&mut db, (her, job), ("first_quote", "sent"), first).await;
    delivered(&mut db, &sid, at("03T09:00:30")).await;
    let failed = (at("03T13:00:00"), None);
    text(&mut db, (her, job), ("quote_update", "failed"), failed).await;
    assert_eq!(last_at(&mut db, her, job).await, Some(at("03T09:00:30")));
}

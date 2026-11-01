//! "<number> YES" text picks in the database (TD-PICK-07, TD-PICK-14, TD-PICK-40, TD-TERMS-10,
//! TD-CON-12; PICK-015, PICK-016, PICK-031, PICK-032, PICK-036, CON-029). Fake numbers, IDs and
//! wording only.
//!
//! `intake::answer_text_pick` hands the text to `picks` under the homeowner lock: the Terms line
//! goes first and is recorded, the repeat picks with channel `text` and one terms acceptance, a
//! second YES replaces a waiting pick and tells the first business, two jobs ask which, nothing
//! waiting gets the status reply, and a CANCEL pause withdraws the waiting pick and blocks new ones.

use askhava_core::api::Guarded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, Opened, PublishedTerms, TextIn, answer_text_pick, begin_job, on_opt_out,
    receive_text, terms_acceptances,
};
use askhava_jobs::api::{JobStatus, current, job_number, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::rules::Plan;
use askhava_picks::api::{
    CONFIRMED_TEMPLATE, DONE_TEMPLATE, PICK_MADE_KIND, PICK_WITHDRAWN_KIND, TERMS_TEMPLATE,
    WAITING_TEMPLATE, WHICH_TEMPLATE, terms_line_sent,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TERMS_LINE: &str = "By picking, you agree to the Terms: askhava.com/terms";
const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: Uuid, words: &str, minute: i64, opt_out: bool) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128((contact.as_u128() << 32) + u128::from(minute.unsigned_abs())),
        contact,
        inbound: Inbound {
            words,
            opt_out,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// Opens her request and one job per trade, each moved to Quotes ready with `quotes` quotes.
async fn jobs_ready(db: &mut Db, her: Uuid, trades: &[&str], quotes: u128) -> (Uuid, Vec<Uuid>) {
    let first = text(
        her,
        "my kitchen sink is leaking under the cabinet",
        0,
        false,
    );
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let id = handled.expect("open").conversation.expect("request");
    let mut jobs = Vec::new();
    for trade in trades {
        let start = JobStart {
            conversation: id,
            words: first.inbound.words,
            emergency: false,
            trade,
            at: at(1),
        };
        let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
            panic!("a new job");
        };
        for step in [
            (JobStatus::Intake, JobStatus::Open),
            (JobStatus::Open, JobStatus::QuotesReady),
        ] {
            let moved = move_to(db, job, step, at(2), Actor::System).await;
            assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
        }
        for n in 1..=quotes {
            quote(db, job, n).await;
        }
        jobs.push(job);
    }
    (id, jobs)
}

/// Business `n` quotes on `job`, confirmed, arriving `n` minutes in (fake wording).
async fn quote(db: &mut Db, job: Uuid, n: u128) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(business(n))
    .bind(at(i64::try_from(n).expect("small") + 3).to_sqlx())
    .execute(&mut **db)
    .await
    .expect("quote");
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0xb000 + n)
}

async fn yes(db: &mut Db, her: Uuid, words: &str, minute: i64) -> Option<Plan> {
    let msg = text(her, words, minute, false);
    answer_text_pick(db, &secret(), &msg, (Settings::default(), &TERMS))
        .await
        .expect("text pick")
        .map(|d| d.plan)
}

/// The templates and bodies queued for `contact`, oldest first.
async fn texts(db: &mut Db, contact: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT template_id, coalesce(body, '') FROM messaging.outbox WHERE recipient_id = $1 \
         ORDER BY created_at, id",
    )
    .bind(contact)
    .fetch_all(&mut **db)
    .await
    .expect("outbox")
}

/// The job's picks: business, quote number, channel, inbox ID set, status, end reason.
async fn picks(db: &mut Db, job: Uuid) -> Vec<(Uuid, i32, String, bool, String, Option<String>)> {
    sqlx::query_as(
        "SELECT business_id, quote_number, channel, inbox_id IS NOT NULL, status, end_reason \
         FROM picks.pick WHERE job_id = $1 ORDER BY picked_at, id",
    )
    .bind(job)
    .fetch_all(&mut **db)
    .await
    .expect("picks")
}

async fn tasks(db: &mut Db, kind: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(kind)
        .fetch_all(&mut **db)
        .await
        .expect("tasks")
}

async fn status(db: &mut Db, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

/// TD-PICK-40, TD-TERMS-10 (PICK-016, PICK-031, TERMS-016, TERMS-033, D-492): the first "2 YES"
/// gets the Terms line and "Reply 2 YES again", recorded with its template version, and picks
/// nothing; the repeat picks, logged with channel `text`, and stores one acceptance with channel
/// `text_pick` and that inbound text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_40_terms_line_first_then_the_text_pick(mut db: Db) {
    let her = Uuid::from_u128(0x54_0001);
    let (id, jobs) = jobs_ready(&mut db, her, &["plumbing"], 2).await;
    let job = jobs[0];
    assert!(!terms_line_sent(&mut db, job).await.expect("read"));
    assert_eq!(yes(&mut db, her, "2 YES", 10).await, Some(Plan::Terms(job)));
    let sent = texts(&mut db, her).await;
    let (template, body) = sent.last().expect("terms text");
    assert_eq!(template, TERMS_TEMPLATE);
    assert!(body.contains("Reply 2 YES again to pick."));
    assert!(body.ends_with(TERMS_LINE));
    let version: String =
        sqlx::query_scalar("SELECT template_version FROM picks.terms_line WHERE job_id = $1")
            .bind(job)
            .fetch_one(&mut *db)
            .await
            .expect("terms line");
    assert!(version.starts_with("text_pick_terms:en:"));
    assert!(picks(&mut db, job).await.is_empty());
    assert_eq!(status(&mut db, job).await, "quotes_ready");
    assert_eq!(
        yes(&mut db, her, "2 yes please", 11).await,
        Some(Plan::Pick(job))
    );
    let picked = picks(&mut db, job).await;
    assert_eq!(picked.len(), 1);
    assert_eq!((picked[0].0, picked[0].1), (business(2), 2));
    assert_eq!((picked[0].2.as_str(), picked[0].3), ("text", true));
    assert_eq!(status(&mut db, job).await, "awaiting_confirm");
    assert_eq!(tasks(&mut db, PICK_MADE_KIND).await.len(), 1);
    assert_eq!(
        texts(&mut db, her).await.last().expect("done").0,
        DONE_TEMPLATE
    );
    let accepted = terms_acceptances(&mut db, id).await.expect("acceptances");
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].channel, "text_pick");
    assert_eq!(
        accepted[0].inbox_id,
        Some(text(her, "", 11, false).inbox_id)
    );
    assert_eq!(accepted[0].assent_line, TERMS_LINE);
}

/// TD-PICK-07 (PICK-015, PICK-031, D-068): from another phone nothing happens; with two jobs in
/// Quotes ready Ask Hava asks which (ending with the Terms line), and the named request picks.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_07_another_phone_and_two_jobs(mut db: Db) {
    let her = Uuid::from_u128(0x54_0002);
    let (_, jobs) = jobs_ready(&mut db, her, &["plumbing", "electrical"], 2).await;
    let stranger = Uuid::from_u128(0x54_0099);
    assert_eq!(yes(&mut db, stranger, "2 YES", 10).await, None);
    assert!(texts(&mut db, stranger).await.is_empty());
    let numbers = [
        job_number(&mut db, jobs[0]).await.expect("n"),
        job_number(&mut db, jobs[1]).await.expect("n"),
    ];
    assert_eq!(
        yes(&mut db, her, "2 YES", 11).await,
        Some(Plan::Which(numbers.to_vec()))
    );
    let (template, body) = texts(&mut db, her).await.last().cloned().expect("which");
    assert_eq!(template, WHICH_TEMPLATE);
    assert!(body.ends_with(TERMS_LINE));
    assert!(body.contains(&format!("{} 2 YES", numbers[0])));
    for job in &jobs {
        assert!(terms_line_sent(&mut db, *job).await.expect("read"));
        assert!(picks(&mut db, *job).await.is_empty());
    }
    let named = format!("{} 2 YES", numbers[1]);
    assert_eq!(
        yes(&mut db, her, &named, 12).await,
        Some(Plan::Pick(jobs[1]))
    );
    assert_eq!(picks(&mut db, jobs[1]).await.len(), 1);
    assert!(picks(&mut db, jobs[0]).await.is_empty());
}

/// TD-PICK-14 (PICK-032, PICK-036, D-147): "1 YES" then "2 YES" before Confirm moves the pick to
/// quote 2, business 1's pick is cancelled and its notice enqueued; after the business's Confirm a
/// YES gets the confirmed reply with how to close; with no job waiting the status reply goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_14_replace_confirmed_and_status(mut db: Db) {
    let her = Uuid::from_u128(0x54_0003);
    let (_, jobs) = jobs_ready(&mut db, her, &["plumbing"], 2).await;
    let job = jobs[0];
    yes(&mut db, her, "1 YES", 10).await;
    assert_eq!(yes(&mut db, her, "1 YES", 11).await, Some(Plan::Pick(job)));
    assert_eq!(
        yes(&mut db, her, "2 YES", 12).await,
        Some(Plan::Replace(job))
    );
    let picked = picks(&mut db, job).await;
    assert_eq!(picked.len(), 2);
    assert_eq!(
        (picked[0].0, picked[0].4.as_str()),
        (business(1), "cancelled")
    );
    assert_eq!(picked[0].5.as_deref(), Some("replaced"));
    assert_eq!(
        (picked[1].0, picked[1].4.as_str()),
        (business(2), "awaiting_confirm")
    );
    let notices = tasks(&mut db, PICK_WITHDRAWN_KIND).await;
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["business"], serde_json::json!(business(1)));
    assert_eq!(status(&mut db, job).await, "awaiting_confirm");
    let confirm = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let moved = move_to(&mut db, job, confirm, at(13), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "confirm");
    assert_eq!(yes(&mut db, her, "1 YES", 14).await, Some(Plan::Status));
    let (template, body) = texts(&mut db, her).await.last().cloned().expect("reply");
    assert_eq!(template, CONFIRMED_TEMPLATE);
    let number = job_number(&mut db, job).await.expect("n");
    assert!(body.contains(&format!("reply CLOSE {number}")));
    let other = Uuid::from_u128(0x54_0004);
    let (_, fresh) = open_job(&mut db, other).await;
    assert_eq!(status(&mut db, fresh).await, "open");
    assert_eq!(yes(&mut db, other, "2 YES", 20).await, Some(Plan::Status));
    let last = texts(&mut db, other).await.last().cloned().expect("status");
    assert_eq!(last.0, WAITING_TEMPLATE);
}

/// A request for `her` with one job left in Open (no quotes yet).
async fn open_job(db: &mut Db, her: Uuid) -> (Uuid, Uuid) {
    let msg = text(her, "the porch light switch sparks when flipped", 15, false);
    let id = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("reply")
        .conversation
        .expect("request");
    let start = JobStart {
        conversation: id,
        words: msg.inbound.words,
        emergency: false,
        trade: "electrical",
        at: at(15),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at(16), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "open");
    (id, job)
}

/// TD-CON-12 (CON-029, D-129): bare CANCEL or CANCELAR with a pick in Awaiting confirm withdraws
/// the pick and enqueues the business's notice; the job keeps its status and still takes quotes;
/// no new pick is taken and nothing is texted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_12_cancel_withdraws_the_waiting_pick(mut db: Db) {
    for (n, word) in [(5_u128, "CANCEL"), (6, "cancelar")] {
        let her = Uuid::from_u128(0x54_0000 + n);
        let (_, jobs) = jobs_ready(&mut db, her, &["plumbing"], 2).await;
        let job = jobs[0];
        yes(&mut db, her, "1 YES", 10).await;
        assert_eq!(
            yes(&mut db, her, "1 YES", 11).await,
            Some(Plan::Pick(job)),
            "{word}"
        );
        let before = texts(&mut db, her).await.len();
        let stop = text(her, word, 12, true);
        let done = on_opt_out(&mut db, &stop)
            .await
            .expect("intake")
            .expect("live");
        assert!(done.paused, "{word}");
        let picked = picks(&mut db, job).await;
        assert_eq!(picked[0].4, "cancelled", "{word}");
        assert_eq!(picked[0].5.as_deref(), Some("paused"), "{word}");
        let notices = tasks(&mut db, PICK_WITHDRAWN_KIND).await;
        assert!(
            notices.iter().any(|t| t["job"] == serde_json::json!(job)),
            "{word}"
        );
        let still = current(&mut db, job).await.expect("job").status;
        assert_eq!(still.code(), "awaiting_confirm", "{word}");
        assert!(still.takes_quotes(), "{word}");
        assert_eq!(
            yes(&mut db, her, "2 YES", 13).await,
            Some(Plan::Paused(job)),
            "{word}"
        );
        assert_eq!(picks(&mut db, job).await.len(), 1, "{word}");
        assert_eq!(texts(&mut db, her).await.len(), before, "{word}");
    }
}

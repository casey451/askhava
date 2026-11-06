//! The first-quote move to Quotes ready on a fresh database (TD-JOB-11: JOB-009, JOB-011, TXT-016;
//! TD-PICK-40's path to "2 YES"; JOB-006, JOB-012, JOB-025; D-1517). Fake numbers (555-01xx), IDs
//! and wording only.
//!
//! A business's first confirmed quote moves an Open or No quotes yet job to Quotes ready inside
//! the quote's own transaction with one `first_quote` homeowner-text event; nothing else moves it.
//! The old wait then applies nothing, Quotes ready's 3-day close counts from the move, and
//! `intake`'s sender drops a stale no-quotes-yet text and sends the first-quote text with her link.

use askhava_core::api::{Claimed, Guarded, IdSecret, WorkerId, claim, finish};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, JobTextKeys, JobTextSkip, JobTextStep, Opened, PublishedTerms, TextIn,
    answer_text_pick, begin_job, job_text_step, parse_job_text, receive_text,
};
use askhava_jobs::api::{
    JobRow, JobStatus, TEXT_KIND, TIMER_KIND, create, current, move_to, restart_timer, run_timer,
};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_picks::api::rules::Plan;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    Format, Price, Quote, QuoteForm, QuoteRules, Submitted, confirm_quote_at, parse_quote,
    submit_quote,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 2026-11-02 10:00 Arizona time, when each job opens; [`hours`] counts from it (hour 32 is 6 p.m.
/// the next day, hour 48 10 a.m. the day after).
const DAY: &str = "2026-11-02T17:00:00Z";
const PAGE: &str = "https://askhava.com/q/";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn hours(h: i64) -> Timestamp {
    ts(DAY) + SignedDuration::from_hours(h)
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-000".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x54), &key(0x64), &url).expect("vault")
}

fn keys() -> JobTextKeys {
    JobTextKeys::new(IdSecret::new(1, secret()).expect("id"), secret(), vault())
}

/// A made-up firm quote; `ai_read` marks one the AI read from a text (PICK-006).
fn quote(ai_read: bool) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(24_000),
        notes: "Parts and labor included.".to_owned(),
        original: "240 parts and labor".to_owned(),
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

/// An Open job (no homeowner) with cards for businesses 1 and 2, all at [`DAY`].
async fn open_job(db: &mut PgConnection) -> Uuid {
    let id = create(db, ts(DAY), false, Actor::System)
        .await
        .expect("job");
    let step = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, id, step, ts(DAY), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    cards(db, id, ts(DAY)).await;
    id
}

/// Records the cards of businesses 1 and 2 for `job`, sent at `sent` (stands in for matching).
async fn cards(db: &mut PgConnection, job: Uuid, sent: Timestamp) {
    for n in 1..=2 {
        sqlx::query(
            "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) \
             VALUES ($1, $2, false, $3)",
        )
        .bind(job)
        .bind(business(n))
        .bind(sent.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    }
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0xb100 + n)
}

async fn submit(db: &mut PgConnection, job: Uuid, (n, ai_read): (u128, bool), at: Timestamp) {
    let saved = submit_quote(db, (job, business(n)), &quote(ai_read), at).await;
    assert_eq!(saved, Ok(Submitted::Saved));
}

async fn job(db: &mut PgConnection, id: Uuid) -> JobRow {
    current(db, id).await.expect("job")
}

/// The payloads of `template`'s homeowner-text events for `job`.
async fn events(db: &mut PgConnection, job: Uuid, template: &str) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         AND payload->>'template' = $3 ORDER BY id",
    )
    .bind(TEXT_KIND)
    .bind(job.to_string())
    .bind(template)
    .fetch_all(db)
    .await
    .expect("events")
}

/// How many moves into Quotes ready the job's history holds.
async fn moves_to_ready(db: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM jobs.status_history WHERE job_id = $1 AND to_status = 'quotes_ready'",
    )
    .bind(job)
    .fetch_one(db)
    .await
    .expect("history")
}

/// Claims every task due at `now`, finishing each and running the job timers.
async fn fire(pool: &PgPool, now: Timestamp) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_mins(5);
    loop {
        let claimed: Vec<Claimed> = claim(pool, owner, now, lease, 100).await.expect("claim");
        if claimed.is_empty() {
            return;
        }
        for task in claimed {
            finish(pool, owner, &task, now).await.expect("finish");
            if task.kind == TIMER_KIND {
                run_timer(pool.clone(), task).await.expect("timer");
            }
        }
    }
}

/// TD-JOB-11 (JOB-009, TXT-016, D-1517): the first confirmed quote moves the Open job to Quotes
/// ready at the quote's time with one `first_quote` event at the new version; a second quote and an
/// edit move nothing and queue no second text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_the_first_quote_moves_the_job_once_with_one_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    submit(&mut db, id, (1, false), hours(2)).await;
    let moved = job(&mut db, id).await;
    assert_eq!(moved.status.code(), "quotes_ready");
    assert_eq!((moved.status_at, moved.timer_from), (hours(2), hours(2)));
    let sent = events(&mut db, id, "first_quote").await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["version"].as_i64(), Some(i64::from(moved.version)));
    assert!(sent[0]["values"]["job"].as_i64().is_some(), "the number");
    submit(&mut db, id, (2, false), hours(3)).await;
    submit(&mut db, id, (1, false), hours(4)).await;
    assert_eq!(job(&mut db, id).await.version, moved.version);
    assert_eq!(events(&mut db, id, "first_quote").await.len(), 1);
    assert_eq!(moves_to_ready(&mut db, id).await, 1);
}

/// TD-JOB-11 (JOB-009, D-043): a quote in Awaiting confirm moves nothing and queues no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_a_quote_after_the_pick_moves_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    for step in [
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
    ] {
        let moved = move_to(&mut db, id, step, hours(1), Actor::System).await;
        assert_eq!(moved.expect("move"), Guarded::Applied(()));
    }
    let before = job(&mut db, id).await;
    submit(&mut db, id, (1, false), hours(2)).await;
    let after = job(&mut db, id).await;
    assert_eq!(
        (after.status.code(), after.version),
        (before.status.code(), before.version)
    );
    assert!(events(&mut db, id, "first_quote").await.is_empty());
}

/// TD-JOB-11 (JOB-009, R11): a quote row that fails to insert rolls the move and its text back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_a_failing_quote_insert_rolls_the_move_back(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    for stmt in [
        "CREATE FUNCTION public.refuse_quote() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'test refuses the quote row'; END $$",
        "CREATE TRIGGER refuse_quote BEFORE INSERT ON quotes.quote \
         FOR EACH ROW EXECUTE FUNCTION public.refuse_quote()",
    ] {
        sqlx::query(stmt).execute(&mut *db).await.expect("trigger");
    }
    let before = job(&mut db, id).await;
    let refused = submit_quote(&mut db, (id, business(1)), &quote(false), hours(2)).await;
    assert!(refused.is_err());
    let after = job(&mut db, id).await;
    assert_eq!(
        (after.status.code(), after.version),
        ("open", before.version)
    );
    assert!(events(&mut db, id, "first_quote").await.is_empty());
    assert_eq!(moves_to_ready(&mut db, id).await, 0);
}

/// TD-JOB-11 (JOB-006, JOB-012, JOB-025): after the move the Open wait applies nothing, and the
/// 3-day close counts from the move, not from the job's opening.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_the_old_wait_applies_nothing_and_three_days_count_from_the_move(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    submit(&mut db, id, (1, false), hours(5)).await;
    fire(&pool, hours(48)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "quotes_ready");
    assert!(events(&mut db, id, "no_quotes_yet").await.is_empty());
    fire(&pool, hours(5 + 72) - SignedDuration::from_secs(1)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "quotes_ready");
    fire(&pool, hours(5 + 72)).await;
    let closed = job(&mut db, id).await;
    assert_eq!(
        (closed.status.code(), closed.status_at),
        ("closed", hours(77))
    );
    assert_eq!(events(&mut db, id, "closed_no_pick").await.len(), 1);
}

/// TD-JOB-11 (JOB-025, D-807): a wait restarted after the move counts its 3 days from the restart.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_a_restarted_wait_moves_the_three_day_close(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    submit(&mut db, id, (1, false), hours(5)).await;
    let restart = (JobStatus::QuotesReady, hours(29));
    let restarted = restart_timer(&mut db, id, restart).await;
    assert_eq!(restarted.expect("restart"), Guarded::Applied(()));
    fire(&pool, hours(5 + 72)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "quotes_ready");
    fire(&pool, hours(29 + 72)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "closed");
}

/// TD-JOB-11 (JOB-004, JOB-009): two first quotes at once make one move and one text, run after
/// run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_two_first_quotes_at_once_move_once(pool: PgPool) {
    for _ in 0..6 {
        let mut db = pool.acquire().await.expect("db");
        let id = open_job(&mut db).await;
        let (mut a, mut b) = (
            pool.acquire().await.expect("a"),
            pool.acquire().await.expect("b"),
        );
        let (q1, q2) = (quote(false), quote(false));
        let (one, two) = tokio::join!(
            submit_quote(&mut a, (id, business(1)), &q1, hours(2)),
            submit_quote(&mut b, (id, business(2)), &q2, hours(2)),
        );
        assert_eq!((one, two), (Ok(Submitted::Saved), Ok(Submitted::Saved)));
        assert_eq!(job(&mut db, id).await.status.code(), "quotes_ready");
        assert_eq!(moves_to_ready(&mut db, id).await, 1);
        assert_eq!(events(&mut db, id, "first_quote").await.len(), 1);
    }
}

/// TD-JOB-11 (JOB-009, PICK-006, D-149): an AI-read quote moves nothing until the business
/// confirms it; the confirm moves the job and queues the one text, and a second confirm adds none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_an_ai_read_quote_moves_the_job_only_on_confirm(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = open_job(&mut db).await;
    submit(&mut db, id, (1, true), hours(2)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "open");
    assert!(events(&mut db, id, "first_quote").await.is_empty());
    let confirmed = confirm_quote_at(&mut db, (id, business(1)), hours(3)).await;
    assert_eq!(confirmed, Ok(Guarded::Applied(())));
    let moved = job(&mut db, id).await;
    assert_eq!(
        (moved.status.code(), moved.status_at),
        ("quotes_ready", hours(3))
    );
    let again = confirm_quote_at(&mut db, (id, business(1)), hours(4)).await;
    assert_eq!(again, Ok(Guarded::Applied(())));
    assert_eq!(job(&mut db, id).await.version, moved.version);
    assert_eq!(events(&mut db, id, "first_quote").await.len(), 1);
}

/// A homeowner with an Arizona 555 number texts in, gets the consent text and a plumbing job,
/// moved to Open at [`DAY`] with its two cards sent at `sent`; returns her contact and the job.
async fn her_open_job(db: &mut PgConnection, phone: &str, sent: Timestamp) -> (Uuid, Uuid) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let msg = text_in(her, "my kitchen sink is leaking under the cabinet", ts(DAY));
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: msg.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let step = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    cards(db, job, sent).await;
    (her, job)
}

fn text_in(contact: Uuid, words: &str, at: Timestamp) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(contact.as_u128() ^ u128::from(at.as_second().unsigned_abs())),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at,
    }
}

/// TD-JOB-11 (JOB-011, TXT-016, JOB-006, SEC-037): a No quotes yet job moves on its first quote;
/// its stale no-quotes-yet text is dropped and the first-quote text goes once, with her link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_11_a_no_quotes_yet_job_moves_and_its_stale_text_is_dropped(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, id) = her_open_job(&mut db, "+19285550151", hours(30)).await;
    fire(&pool, hours(32)).await;
    assert_eq!(job(&mut db, id).await.status.code(), "no_quotes_yet");
    submit(&mut db, id, (1, false), hours(40)).await;
    let moved = job(&mut db, id).await;
    assert_eq!(
        (moved.status.code(), moved.status_at),
        ("quotes_ready", hours(40))
    );
    let event = |p: &Value| parse_job_text(p).expect("parsed");
    let stale = event(&events(&mut db, id, "no_quotes_yet").await[0]);
    let step = job_text_step(&mut db, &keys(), &stale, hours(48)).await;
    assert_eq!(
        step.expect("step"),
        JobTextStep::Skipped(JobTextSkip::MovedOn)
    );
    let first = events(&mut db, id, "first_quote").await;
    assert!(!first[0].to_string().contains(PAGE), "no link in a payload");
    let first = event(&first[0]);
    let step = job_text_step(&mut db, &keys(), &first, hours(48)).await;
    assert!(matches!(step.expect("step"), JobTextStep::Queued(_)));
    let again = job_text_step(&mut db, &keys(), &first, hours(48)).await;
    assert_eq!(
        again.expect("again"),
        JobTextStep::Skipped(JobTextSkip::AlreadyQueued)
    );
    let bodies: Vec<String> = sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE recipient_id = $1 AND template_id = 'first_quote'",
    )
    .bind(her)
    .fetch_all(&mut *db)
    .await
    .expect("outbox");
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("Your first quote for request"));
    assert!(bodies[0].contains(PAGE));
}

/// TD-PICK-40's path (PICK-016, D-1517): after two quotes, with no manual move, "2 YES" gets the
/// Terms line and the repeat picks the second quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_40_two_yes_is_taken_after_two_quotes_with_no_manual_move(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, id) = her_open_job(&mut db, "+19285550152", ts(DAY)).await;
    submit(&mut db, id, (1, false), hours(2)).await;
    submit(&mut db, id, (2, false), hours(3)).await;
    let terms = PublishedTerms {
        version: "2026-11-01",
        text: "Sample homeowner terms for tests only.",
    };
    let mut yes = async |at: Timestamp| {
        let msg = text_in(her, "2 YES", at);
        let plan = answer_text_pick(&mut db, &secret(), &msg, (Settings::default(), &terms)).await;
        plan.expect("text pick").map(|d| d.plan)
    };
    assert_eq!(yes(hours(4)).await, Some(Plan::Terms(id)));
    assert_eq!(yes(hours(5)).await, Some(Plan::Pick(id)));
    assert_eq!(job(&mut db, id).await.status.code(), "awaiting_confirm");
}

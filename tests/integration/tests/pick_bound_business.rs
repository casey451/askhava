//! A quote keeps the number it was first shown with, and a pick re-reads its quote (T361;
//! TD-PICK-60's storage cases: PICK-008, PRIV-005; D-084, D-2632, D-2912, D-3999). Fake IDs and
//! wording only. T564 adds TD-PICK-60's reading cases to this file.
//!
//! Through `quotes::api` and `picks::api`: business A's AI-read quote arrives first and waits for
//! its read-back, B's form quote arrives later and is shown first, so B is shown #1 and A, once
//! confirmed, #2; neither number moves through a later confirm, an AI-read edit or a withdrawal,
//! while the page keeps arrival order until T441. Migration 0202 numbers rows already stored and
//! rows written by plain SQL. A withdrawal committed while a page pick waits on the job's lock,
//! after the pick's own check, answers `Taken` and stores nothing.

use askhava_businesses::api::create as create_business;
use askhava_core::api::MIGRATOR;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, current, lock_for_quote_change, move_to, record_job_facts};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_picks::api::{PagePick, page_pick};
use askhava_quotes::api::{
    DayPart, Format, Price, Quote, QuoteForm, QuoteRules, Submitted, Withdrawal, confirm_quote_at,
    page_quotes, parse_quote, shown_numbers, submit_quote, withdraw_quote,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-361".to_owned())
}

fn text(words: &str, when: Timestamp) -> TextIn<'_> {
    let second = u128::try_from(when.as_second()).expect("positive");
    TextIn {
        inbox_id: Uuid::from_u128((0x361 << 64) + second),
        contact: Uuid::from_u128(0x361_0001),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: when,
    }
}

/// A firm quote in fake words, read by the AI (waiting for its read-back) or sent by form.
fn quote(words: &str, ai_read: bool) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: words.to_owned(),
        ai_read,
        earliest_visit: Some((date(2026, 11, 9), DayPart::Evening)),
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

/// Her request and one Open plumbing job with its facts; its ID.
async fn open_job(db: &mut Db) -> Uuid {
    let first = text(
        "my kitchen sink is leaking under the cabinet",
        at("02T20:00:00"),
    );
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: first.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at("02T20:01:00"),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at("02T20:02:00"), Actor::System)
        .await
        .expect("open");
    job
}

/// A new business holding `job`'s card; its ID.
async fn card_holder(db: &mut Db, job: Uuid) -> Uuid {
    let id = create_business(db, false, at("01T09:00:00"))
        .await
        .expect("business");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(id)
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    id
}

async fn send(db: &mut Db, (job, business): (Uuid, Uuid), quote: &Quote, when: &str) {
    let saved = submit_quote(db, (job, business), quote, at(when)).await;
    assert_eq!(saved, Ok(Submitted::Saved));
}

/// A AI-read at 9:00, B by form at 9:05; B is shown first. Returns the job, A and B.
async fn a_then_b(db: &mut Db) -> (Uuid, Uuid, Uuid) {
    let job = open_job(db).await;
    let (a, b) = (card_holder(db, job).await, card_holder(db, job).await);
    send(
        db,
        (job, a),
        &quote("Sample wording A", true),
        "03T09:00:00",
    )
    .await;
    send(
        db,
        (job, b),
        &quote("Sample wording B", false),
        "03T09:05:00",
    )
    .await;
    (job, a, b)
}

async fn numbers(db: &mut Db, job: Uuid) -> Vec<(u32, Uuid)> {
    shown_numbers(db, job).await.expect("shown numbers")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_60_a_quote_shown_first_keeps_number_one(mut db: Db) {
    let (job, a, b) = a_then_b(&mut db).await;
    assert_eq!(
        numbers(&mut db, job).await,
        vec![(1, b)],
        "A is not shown yet"
    );
    let confirmed = confirm_quote_at(&mut db, (job, a), at("03T09:10:00")).await;
    assert!(confirmed.is_ok_and(|g| g.applied().is_some()));
    assert_eq!(numbers(&mut db, job).await, vec![(1, b), (2, a)]);
    let page: Vec<Uuid> = page_quotes(&mut db, job)
        .await
        .expect("page")
        .iter()
        .map(|q| q.business)
        .collect();
    assert_eq!(page, vec![a, b], "the page keeps arrival order until T441");

    send(
        &mut db,
        (job, b),
        &quote("Sample wording B2", true),
        "03T09:20:00",
    )
    .await;
    assert_eq!(
        numbers(&mut db, job).await,
        vec![(1, b), (2, a)],
        "an AI-read edit waits"
    );
    let again = confirm_quote_at(&mut db, (job, b), at("03T09:25:00")).await;
    assert!(again.is_ok_and(|g| g.applied().is_some()));
    send(
        &mut db,
        (job, a),
        &quote("Sample wording A2", false),
        "03T09:30:00",
    )
    .await;
    assert_eq!(
        numbers(&mut db, job).await,
        vec![(1, b), (2, a)],
        "edits keep numbers"
    );

    let gone = withdraw_quote(&mut db, (job, b), at("03T09:40:00")).await;
    assert_eq!(gone, Ok(Withdrawal::Withdrawn));
    assert_eq!(
        numbers(&mut db, job).await,
        vec![(1, b), (2, a)],
        "B keeps #1, A #2"
    );
    let c = card_holder(&mut db, job).await;
    send(
        &mut db,
        (job, c),
        &quote("Sample wording C", false),
        "03T09:45:00",
    )
    .await;
    assert_eq!(numbers(&mut db, job).await, vec![(1, b), (2, a), (3, c)]);
}

/// Inserts a confirmed (or waiting) quote by plain SQL, as done tests do.
async fn sql_quote(pool: &PgPool, (job, business): (Uuid, Uuid), confirmed: bool, when: &str) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', NOT $3, $3, $4)",
    )
    .bind(job)
    .bind(business)
    .bind(confirmed)
    .bind(at(when).to_sqlx())
    .execute(pool)
    .await
    .expect("quote row");
}

async fn number_of(pool: &PgPool, job: Uuid, business: Uuid) -> Option<i32> {
    sqlx::query_scalar(
        "SELECT shown_number FROM quotes.quote WHERE job_id = $1 AND business_id = $2",
    )
    .bind(job)
    .bind(business)
    .fetch_one(pool)
    .await
    .expect("number")
}

#[sqlx::test(migrations = false)]
async fn td_pick_60_backfilled_and_trigger_filled_numbers(pool: PgPool) {
    MIGRATOR
        .run_to(201, &pool)
        .await
        .expect("migrations before 0202");
    let job = Uuid::from_u128(0x361_1000);
    let ids: Vec<Uuid> = (1..=4).map(|n| Uuid::from_u128(0x361_2000 + n)).collect();
    sql_quote(&pool, (job, ids[2]), true, "03T09:00:00").await;
    sql_quote(&pool, (job, ids[1]), true, "03T09:00:00").await;
    sql_quote(&pool, (job, ids[0]), false, "03T08:00:00").await;
    MIGRATOR.run(&pool).await.expect("migration 0202");
    assert_eq!(
        number_of(&pool, job, ids[1]).await,
        Some(1),
        "same time: business ID order"
    );
    assert_eq!(number_of(&pool, job, ids[2]).await, Some(2));
    assert_eq!(number_of(&pool, job, ids[0]).await, None, "never shown");

    sql_quote(&pool, (job, ids[3]), true, "03T10:00:00").await;
    assert_eq!(
        number_of(&pool, job, ids[3]).await,
        Some(3),
        "the trigger fills it"
    );
    sqlx::query("UPDATE quotes.quote SET confirmed = true WHERE business_id = $1")
        .bind(ids[0])
        .execute(&pool)
        .await
        .expect("confirm");
    assert_eq!(number_of(&pool, job, ids[0]).await, Some(4));
    sqlx::query(
        "UPDATE quotes.quote SET confirmed = false, shown_number = NULL WHERE business_id = $1",
    )
    .bind(ids[1])
    .execute(&pool)
    .await
    .expect("unconfirm");
    assert_eq!(
        number_of(&pool, job, ids[1]).await,
        Some(1),
        "never cleared"
    );
}

/// Waits until another session waits on a row lock.
async fn lock_waited(pool: &PgPool) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
             AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("activity");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the pick never waited on the job's lock");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_60_a_withdrawal_committed_after_the_check_is_taken(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = open_job(&mut db).await;
    let a = card_holder(&mut db, job).await;
    send(
        &mut db,
        (job, a),
        &quote("Sample wording A", false),
        "03T09:00:00",
    )
    .await;
    let mut held = pool.begin().await.expect("tx");
    lock_for_quote_change(&mut held, job)
        .await
        .expect("job lock");
    let gone = withdraw_quote(&mut held, (job, a), at("03T10:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::Withdrawn));

    let picker = pool.clone();
    let pick = tokio::spawn(async move {
        let mut db = picker.acquire().await.expect("db");
        page_pick(&mut db, (job, false), 1, Actor::System, at("03T10:01:00")).await
    });
    lock_waited(&pool).await;
    held.commit().await.expect("withdrawal commits");
    assert_eq!(pick.await.expect("task"), Ok(PagePick::Taken));

    let picks: i64 = sqlx::query_scalar("SELECT count(*) FROM picks.pick WHERE job_id = $1")
        .bind(job)
        .fetch_one(&pool)
        .await
        .expect("picks");
    assert_eq!(picks, 0, "nothing stored, notified or charged");
    let status = current(&mut db, job).await.expect("job").status;
    assert!(
        status.takes_quote_edits(),
        "still before a pick: the move rolled back"
    );
}

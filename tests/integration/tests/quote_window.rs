//! Quote intake in the database (TD-PICK-01: PICK-001; TD-PICK-02: PICK-002; TD-JOB-03's
//! every-window-ended cases: TXT-042, JOB-025). Fake IDs and made-up quotes only.
//!
//! Each business quotes inside its own card's 24 hours while the job takes quotes; a quote the
//! business has not confirmed never reaches the homeowner's page; once the last card's window ends
//! on a Quotes ready job `quotes` hands the counts to `jobs`, which queues the one all-quotes-in
//! text or closes a job with no choosable quote left.

use askhava_core::api::{Claimed, Guarded};
use askhava_jobs::api::rules::WindowsEnd;
use askhava_jobs::api::{JobStatus, create, current, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    Format, Price, Quote, QuoteForm, QuoteRules, Submitted, WINDOWS_KIND, WindowsCheck,
    confirm_quote, parse_quote, run_windows_end, shown_quotes, submit_quote, windows_ended,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn quote(ai_read: bool) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
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

/// A job moved along `path` from Intake, each move at `when`.
async fn job_at(db: &mut Db, path: &[JobStatus], when: Timestamp) -> Uuid {
    let id = create(db, when, false, Actor::System).await.expect("job");
    let mut from = JobStatus::Intake;
    for to in path {
        let moved = move_to(db, id, (from, *to), when, Actor::System)
            .await
            .expect("move");
        assert_eq!(moved, Guarded::Applied(()));
        from = *to;
    }
    id
}

/// Records business `n`'s card for `job`, sent at `sent` (the fixture stands in for matching).
async fn card(db: &mut Db, job: Uuid, n: u128, sent: Timestamp) -> Uuid {
    let business = Uuid::from_u128(n);
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(business)
        .bind(sent.to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    business
}

async fn tasks(db: &mut Db, kind: &str, job: Uuid) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY id",
    )
    .bind(kind)
    .bind(job.to_string())
    .fetch_all(&mut **db)
    .await
    .expect("tasks")
}

/// TD-PICK-01: a card held overnight and sent at 08:00 takes a quote at 07:59:59 the next day and
/// refuses one at 08:00:00 and 08:00:01; quotes are taken in Awaiting confirm and refused once the
/// picked business has tapped Confirm (D-042, D-043).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_01_each_card_has_its_own_24_hours(mut db: Db) {
    let open = [JobStatus::Open];
    let id = job_at(&mut db, &open, at("02T21:00:00")).await;
    let sent = at("03T08:00:00");
    let (a, b, c) = (
        card(&mut db, id, 1, sent).await,
        card(&mut db, id, 2, sent).await,
        card(&mut db, id, 3, sent).await,
    );
    let q = quote(false);
    assert_eq!(
        submit_quote(&mut db, (id, a), &q, at("04T07:59:59")).await,
        Ok(Submitted::Saved)
    );
    assert_eq!(
        submit_quote(&mut db, (id, b), &q, at("04T08:00:00")).await,
        Ok(Submitted::WindowClosed)
    );
    assert_eq!(
        submit_quote(&mut db, (id, c), &q, at("04T08:00:01")).await,
        Ok(Submitted::WindowClosed)
    );
    let stranger = Uuid::from_u128(9);
    assert_eq!(
        submit_quote(&mut db, (id, stranger), &q, at("03T09:00:00")).await,
        Ok(Submitted::NoCard)
    );
    let picked = [
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
    ];
    let id = job_at(&mut db, &picked, at("02T21:00:00")).await;
    let (a, b) = (
        card(&mut db, id, 1, sent).await,
        card(&mut db, id, 2, sent).await,
    );
    assert_eq!(
        submit_quote(&mut db, (id, a), &q, at("03T12:00:00")).await,
        Ok(Submitted::Saved)
    );
    let confirm = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let confirmed = move_to(&mut db, id, confirm, at("03T12:30:00"), Actor::System).await;
    assert_eq!(confirmed, Ok(Guarded::Applied(())));
    assert_eq!(
        submit_quote(&mut db, (id, b), &q, at("03T13:00:00")).await,
        Ok(Submitted::NotTaking)
    );
}

/// TD-PICK-02: an AI-read quote is not shown until the business confirms it, a form quote is,
/// an edit of an AI-read quote needs confirming again, and the table has no expiry field.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_02_only_confirmed_quotes_are_shown(mut db: Db) {
    let id = job_at(&mut db, &[JobStatus::Open], at("03T08:00:00")).await;
    let (a, b) = (
        card(&mut db, id, 1, at("03T08:00:00")).await,
        card(&mut db, id, 2, at("03T08:00:00")).await,
    );
    submit_quote(&mut db, (id, a), &quote(true), at("03T09:00:00"))
        .await
        .expect("read");
    submit_quote(&mut db, (id, b), &quote(false), at("03T09:00:00"))
        .await
        .expect("form");
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![b]));
    assert_eq!(
        confirm_quote(&mut db, id, a).await,
        Ok(Guarded::Applied(()))
    );
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![a, b]));
    submit_quote(&mut db, (id, a), &quote(true), at("03T10:00:00"))
        .await
        .expect("edit");
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![b]));
    let nobody = Uuid::from_u128(9);
    assert_eq!(
        confirm_quote(&mut db, id, nobody).await,
        Ok(Guarded::MovedOn)
    );
    let expiry: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'quotes' \
         AND table_name = 'quote' AND column_name LIKE '%expir%'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("columns");
    assert_eq!(expiry, 0);
}

/// TD-JOB-03 (TXT-042, JOB-025): the check waits for a later card's window, then queues exactly
/// one "All quotes are in" text with k of N, even when it runs twice; a job with no choosable
/// quote left closes; a job not in Quotes ready is left alone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_all_quotes_in_once_every_window_ended(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let ready = [JobStatus::Open, JobStatus::QuotesReady];
    let id = job_at(&mut db, &ready, at("03T08:00:00")).await;
    let a = card(&mut db, id, 1, at("03T08:00:00")).await;
    card(&mut db, id, 2, at("03T10:00:00")).await;
    submit_quote(&mut db, (id, a), &quote(false), at("03T09:00:00"))
        .await
        .expect("quote");
    let early = windows_ended(&mut db, id, at("04T08:00:00")).await;
    assert_eq!(early, Ok(WindowsCheck::Open(at("04T10:00:00"))));
    let scheduled = tasks(&mut db, WINDOWS_KIND, id).await;
    assert_eq!(scheduled.len(), 1, "one check per job and window end");
    for _ in 0..2 {
        let task = Claimed {
            id: 0,
            kind: WINDOWS_KIND.to_owned(),
            payload: scheduled[0].clone(),
            attempts: 1,
        };
        run_windows_end(pool.clone(), task).await.expect("handler");
    }
    let texts = tasks(&mut db, "jobs.homeowner_text", id).await;
    let all_in: Vec<_> = texts
        .iter()
        .filter(|t| t["template"] == "all_quotes_in")
        .collect();
    assert_eq!(all_in.len(), 1, "sent once per job");
    assert_eq!(
        (
            all_in[0]["values"]["quoted"].as_i64(),
            all_in[0]["values"]["count"].as_i64()
        ),
        (Some(1), Some(2))
    );
    assert_eq!(
        current(&mut db, id).await.expect("job").status,
        JobStatus::QuotesReady
    );
    sqlx::query("UPDATE quotes.quote SET withdrawn_at = $2 WHERE job_id = $1")
        .bind(id)
        .bind(at("04T11:00:00").to_sqlx())
        .execute(&mut *db)
        .await
        .expect("withdraw");
    let closed = windows_ended(&mut db, id, at("04T12:00:00")).await;
    assert_eq!(
        closed,
        Ok(WindowsCheck::Ended(Guarded::Applied(WindowsEnd::Close)))
    );
    assert_eq!(
        current(&mut db, id).await.expect("job").status,
        JobStatus::Closed
    );
    let open = job_at(&mut db, &[JobStatus::Open], at("03T08:00:00")).await;
    card(&mut db, open, 1, at("03T08:00:00")).await;
    let left = windows_ended(&mut db, open, at("04T08:00:00")).await;
    assert_eq!(left, Ok(WindowsCheck::Ended(Guarded::MovedOn)));
    let none = job_at(&mut db, &[JobStatus::Open], at("03T08:00:00")).await;
    assert_eq!(
        windows_ended(&mut db, none, at("04T08:00:00")).await,
        Ok(WindowsCheck::NoCards)
    );
}

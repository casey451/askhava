//! A quote's PICK-040 facts stored with it (TD-PICK-63: PICK-040, PICK-051; T406, D-3134).
//! Fake IDs, made-up job type codes and made-up quotes only.
//!
//! `submit_quote_with_facts` stores a quote and its five answers in one transaction; a quote
//! `submit_quote` refuses stores no answers; an edit replaces them; `stored_facts` reads them with
//! the quote's earliest visit; `last_facts` gives the business's latest answers on one job type.

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    DayPart, Format, Parts, Permit, Price, Quote, QuoteFacts, QuoteForm, QuoteRules, Spanish,
    Submitted, TripFee, last_facts, parse_quote, stored_facts, submit_quote,
    submit_quote_with_facts,
};
use jiff::Timestamp;
use jiff::civil::{Date, date};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

const VISIT: (Date, DayPart) = (date(2026, 11, 6), DayPart::Morning);

fn quote(cents: u64) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(cents),
        notes: "Parts and labor included.".to_owned(),
        original: "fake firm quote".to_owned(),
        ai_read: false,
        earliest_visit: Some(VISIT),
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

fn facts(trip_fee: TripFee, spanish: Spanish) -> QuoteFacts {
    QuoteFacts {
        trip_fee,
        parts: Parts::Extra,
        permit: Permit::NotNeeded,
        may_change: true,
        earliest_visit: VISIT,
        spanish,
    }
}

/// A job moved along `path` from Intake, its job type `job_type` when given.
async fn job(db: &mut Db, path: &[JobStatus], job_type: Option<&str>) -> Uuid {
    let when = at("02T21:00:00");
    let id = create(db, when, false, Actor::System).await.expect("job");
    let mut from = JobStatus::Intake;
    for to in path {
        let moved = move_to(db, id, (from, *to), when, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
        from = *to;
    }
    if let Some(t) = job_type {
        let types = [t.to_owned()];
        record_job_facts(db, id, "fake_plumbing", &types)
            .await
            .expect("job facts");
    }
    id
}

/// Records business `n`'s card for `job`, sent at `sent`.
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

async fn rows(db: &mut Db, table: &str, job: Uuid) -> i64 {
    let sql = match table {
        "quote" => "SELECT count(*) FROM quotes.quote WHERE job_id = $1",
        _ => "SELECT count(*) FROM quotes.quote_facts WHERE job_id = $1",
    };
    sqlx::query_scalar(sql)
        .bind(job)
        .fetch_one(&mut **db)
        .await
        .expect("count")
}

/// TD-PICK-63: a quote and its five facts are stored together and read back with the quote's
/// earliest visit; an edit replaces them; a quote with no facts row reads none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_63_facts_stored_with_the_quote_and_replaced_by_an_edit(mut db: Db) {
    let id = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let a = card(&mut db, id, 1, at("03T08:00:00")).await;
    let b = card(&mut db, id, 2, at("03T08:00:00")).await;
    let first = facts(TripFee::Counts, Spanish::Both);
    let mut other_visit = first;
    other_visit.earliest_visit = (date(2026, 12, 1), DayPart::Evening);
    let saved = submit_quote_with_facts(
        &mut db,
        (id, a),
        &quote(32_500),
        &other_visit,
        at("03T09:00:00"),
    )
    .await;
    assert_eq!(saved, Ok(Submitted::Saved));
    assert_eq!(
        stored_facts(&mut db, id, a).await,
        Ok(Some(first)),
        "the quote's own visit"
    );
    let edit = facts(TripFee::None, Spanish::No);
    let edited =
        submit_quote_with_facts(&mut db, (id, a), &quote(30_000), &edit, at("03T10:00:00")).await;
    assert_eq!(edited, Ok(Submitted::Saved));
    assert_eq!(stored_facts(&mut db, id, a).await, Ok(Some(edit)));
    assert_eq!(rows(&mut db, "quote_facts", id).await, 1);
    let plain = submit_quote(&mut db, (id, b), &quote(29_000), at("03T11:00:00")).await;
    assert_eq!(plain, Ok(Submitted::Saved));
    assert_eq!(stored_facts(&mut db, id, b).await, Ok(None), "no facts row");
    let stranger = Uuid::from_u128(9);
    assert_eq!(
        stored_facts(&mut db, id, stranger).await,
        Ok(None),
        "no quote"
    );
}

/// TD-PICK-63: a quote `submit_quote` refuses (window closed, job not taking quotes, an edit after
/// a pick) stores no facts, and a refused edit keeps the earlier ones.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_63_a_refused_quote_stores_no_facts(mut db: Db) {
    let f = facts(TripFee::DoesNotCount, Spanish::Phone);
    let open = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let late = card(&mut db, open, 1, at("01T08:00:00")).await;
    let closed =
        submit_quote_with_facts(&mut db, (open, late), &quote(32_500), &f, at("03T09:00:00")).await;
    assert_eq!(closed, Ok(Submitted::WindowClosed));
    assert_eq!(rows(&mut db, "quote_facts", open).await, 0);
    assert_eq!(rows(&mut db, "quote", open).await, 0);
    let paid = [
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
    ];
    let done = job(&mut db, &paid, Some("fake_leak")).await;
    let b = card(&mut db, done, 2, at("03T08:00:00")).await;
    let not_taking =
        submit_quote_with_facts(&mut db, (done, b), &quote(32_500), &f, at("03T09:00:00")).await;
    assert_eq!(not_taking, Ok(Submitted::NotTaking));
    assert_eq!(rows(&mut db, "quote_facts", done).await, 0);
    let picked = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let c = card(&mut db, picked, 3, at("03T08:00:00")).await;
    let saved =
        submit_quote_with_facts(&mut db, (picked, c), &quote(32_500), &f, at("03T09:00:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    let pick = (JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    let moved = move_to(&mut db, picked, pick, at("03T10:00:00"), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    let edit = facts(TripFee::None, Spanish::No);
    let refused = submit_quote_with_facts(
        &mut db,
        (picked, c),
        &quote(30_000),
        &edit,
        at("03T11:00:00"),
    )
    .await;
    assert_eq!(refused, Ok(Submitted::EditClosed));
    assert_eq!(
        stored_facts(&mut db, picked, c).await,
        Ok(Some(f)),
        "earlier facts kept"
    );
}

/// TD-PICK-63: `last_facts` gives the business's most recent answers on the job's type, none for
/// another type or another business, and a job with no type is never read for a pre-fill.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_63_last_facts_per_business_and_job_type(mut db: Db) {
    let older = facts(TripFee::Counts, Spanish::OnSite);
    let newer = facts(TripFee::None, Spanish::Both);
    let one = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let two = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let untyped = job(&mut db, &[JobStatus::Open], None).await;
    let a = card(&mut db, one, 1, at("03T08:00:00")).await;
    card(&mut db, two, 1, at("03T08:00:00")).await;
    card(&mut db, untyped, 1, at("03T08:00:00")).await;
    let send = |job_id, f, t| (job_id, f, at(t));
    for (job_id, f, now) in [
        send(two, newer, "03T12:00:00"),
        send(one, older, "03T09:00:00"),
        send(untyped, older, "03T13:00:00"),
    ] {
        let saved = submit_quote_with_facts(&mut db, (job_id, a), &quote(32_500), &f, now).await;
        assert_eq!(saved, Ok(Submitted::Saved));
    }
    assert_eq!(last_facts(&mut db, a, "fake_leak").await, Ok(Some(newer)));
    assert_eq!(
        last_facts(&mut db, a, "fake_drain").await,
        Ok(None),
        "another type"
    );
    let other = Uuid::from_u128(2);
    assert_eq!(
        last_facts(&mut db, other, "fake_leak").await,
        Ok(None),
        "another business"
    );
    assert_eq!(
        stored_facts(&mut db, untyped, a).await,
        Ok(Some(older)),
        "card still reads"
    );
}

/// TD-PICK-63, PICK-051: the app role may replace a quote's facts but never delete them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_63_facts_are_never_deleted(mut db: Db) {
    let id = job(&mut db, &[JobStatus::Open], Some("fake_leak")).await;
    let a = card(&mut db, id, 1, at("03T08:00:00")).await;
    let f = facts(TripFee::Counts, Spanish::No);
    let saved =
        submit_quote_with_facts(&mut db, (id, a), &quote(32_500), &f, at("03T09:00:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let edit = facts(TripFee::None, Spanish::Both);
    let again =
        submit_quote_with_facts(&mut db, (id, a), &quote(30_000), &edit, at("03T10:00:00")).await;
    assert_eq!(
        again,
        Ok(Submitted::Saved),
        "the app role replaces the facts"
    );
    let deleted = sqlx::query("DELETE FROM quotes.quote_facts WHERE job_id = $1")
        .bind(id)
        .execute(&mut *db)
        .await;
    assert!(deleted.is_err(), "the app role may not delete facts");
}

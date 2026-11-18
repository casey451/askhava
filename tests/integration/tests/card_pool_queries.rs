//! The card pool's service-area reads on a fresh database (T601; CARD-001, CARD-024; D-3360,
//! D-4196 finding 27). Fake IDs, ZIP codes from the reviewed Arizona centroid table and no
//! homeowner data.
//!
//! `quotes::api::card_pool` reads every stored service area in one statement
//! (`businesses::api::service_areas`), so a card send's reads of `businesses.service_area` do not
//! grow with the businesses that have one. The reads are counted from `pg_stat_xact_user_tables`
//! inside one transaction; the pool itself is the same as the per-business reads gave.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, ServiceArea, add_owner, apply, create, request_welcome,
    save_service_area, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_integration::set_app_role;
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardFacts, CardPool, card_pool};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const JOB_ZIP: &str = "86403";
const AC: (&str, &str) = ("hvac_repair", "AC not cooling");
/// Businesses whose service areas lie outside the job's town (each serves Kingman only).
const OUTSIDE: usize = 300;

fn at() -> Timestamp {
    "2026-11-02T10:00:00-07:00".parse().expect("time")
}

/// An active, welcomed business serving only `town`, ticking the AC job type, with base ZIP `zip`
/// and `radius` miles.
async fn stored_business(db: &mut PgConnection, town: &str, (zip, radius): (&str, &str)) -> Uuid {
    let b = create(db, false, at()).await.expect("business");
    let area = ServiceArea::parse(zip, radius).expect("area");
    save_service_area(db, b, &area, (Actor::Business(b), at()))
        .await
        .expect("saved");
    add_owner(db, b, Uuid::new_v4(), at()).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at())
        .await
        .expect("active");
    request_welcome(db, b, at()).await.expect("welcome");
    welcome_sent(db, b, at()).await.expect("sent");
    let actor = Actor::Business(b);
    set_towns(db, b, &[town], actor).await.expect("towns");
    let choice = JobTypeChoice::new(AC.0, AC.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
    b
}

/// The town businesses (one whose own radius reaches the job too, one with an out-of-table base)
/// and the Kingman businesses: even ones with 100 miles reach the job, odd ones with 20 do not.
/// Answers the town businesses and every business the pool must hold, each sorted.
async fn seed(db: &mut PgConnection) -> (Vec<Uuid>, Vec<Uuid>) {
    let mut town = vec![
        stored_business(db, TOWN, (JOB_ZIP, "50")).await,
        stored_business(db, TOWN, ("92363", "50")).await,
    ];
    town.sort();
    let mut want = town.clone();
    for i in 0..OUTSIDE {
        let radius = if i % 2 == 0 { "100" } else { "20" };
        let b = stored_business(db, "kingman", ("86401", radius)).await;
        if i % 2 == 0 {
            want.push(b);
        }
    }
    want.sort();
    (town, want)
}

/// Scans of `businesses.service_area` this backend has not yet flushed to the shared statistics.
/// Since PostgreSQL 15 that can include earlier transactions' scans, but no flush happens inside a
/// transaction, so two reads in one transaction give the scans between them.
async fn area_scans(tx: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(seq_scan + idx_scan), 0)::bigint FROM pg_stat_xact_user_tables \
         WHERE schemaname = 'businesses' AND relname = 'service_area'",
    )
    .fetch_one(tx)
    .await
    .expect("stats")
}

/// The pool for an AC job in the town at `zip`, and how many scans of `businesses.service_area`
/// building it took, counted inside one transaction.
async fn pool_and_reads(db: &mut PgConnection, zip: Option<&str>) -> (CardPool, i64) {
    let facts = CardFacts {
        category: AC.0.to_owned(),
        candidates: vec![AC.1.to_owned()],
        emergency: false,
    };
    let mut tx = db.begin().await.expect("begin");
    let before = area_scans(&mut tx).await;
    let pool = card_pool(&mut tx, ((TOWN, zip), &facts), String::new(), None)
        .await
        .expect("pool");
    let reads = area_scans(&mut tx).await - before;
    tx.rollback().await.expect("rollback");
    (pool, reads)
}

/// CARD-024, CARD-001, D-4196: with 300 businesses outside the job's town, the pool holds the
/// in-town ones and those whose radius reaches the job, each once, and one call scans
/// `businesses.service_area` once; a job with no location is towns only and never scans it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_pool_reads_every_service_area_in_one_statement(mut db: Db) {
    load_embedded(&mut db).await.expect("catalog");
    let (town, want) = seed(&mut db).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let (pool, reads) = pool_and_reads(&mut db, Some(JOB_ZIP)).await;
    let pooled: Vec<Uuid> = pool.businesses.iter().map(|p| p.card.id).collect();
    let mut sorted = pooled.clone();
    sorted.sort();
    assert_eq!(sorted, want);
    assert_eq!(pooled.len(), 2 + OUTSIDE / 2);
    assert!(pool.reach.job.is_some());
    assert_eq!(pool.reach.businesses.len(), OUTSIDE);
    assert_eq!(reads, 1);
    let (pool, reads) = pool_and_reads(&mut db, None).await;
    let pooled: Vec<Uuid> = pool.businesses.iter().map(|p| p.card.id).collect();
    assert_eq!(pooled, town);
    assert!(pool.reach.job.is_none() && pool.reach.businesses.is_empty());
    assert_eq!(reads, 0);
}

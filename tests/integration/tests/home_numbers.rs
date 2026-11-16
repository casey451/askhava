//! The home page's numbers on PostgreSQL (TD-JOB-04's WEB-028 cases; D-466, D-468, D-3144; T102).
//!
//! Proves that the home page shows no number while 49 requests have had a card sent and the three
//! computed numbers, with their date range and trade count, at 50; that the numbers come only
//! from the card and quote records (`quotes::api::home_numbers`, read as the app role): an
//! unconfirmed quote counts for nothing, a job's first card starts its wait and each job's trade is
//! its `jobs` facts' category; and that `ASKHAVA_HOME_NUMBERS_MIN_JOBS` moves the threshold.
//! Rows are seeded straight into each module's tables, with fake IDs and fake wording only.

use askhava_core::api::Settings;
use askhava_integration::set_app_role;
use askhava_quotes::api::home_numbers;
use askhava_site::api::{SitePages, home_html};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The first card: November 1, 2026, 10 a.m. in Arizona.
const FIRST_CARD: &str = "2026-11-01T17:00:00Z";
/// Three fake trade codes, one per job in turn.
const TRADES: [&str; 3] = ["pool_service", "hvac", "handyman"];

/// Settings with `extra` pairs on top of the two required keys (fake URL).
fn settings(extra: &[(&str, &str)]) -> SitePages {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV".to_owned(), "ci".to_owned()),
        (
            "ASKHAVA_DATABASE_URL".to_owned(),
            "postgres://fake@localhost/fake".to_owned(),
        ),
    ];
    pairs.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    SitePages::from_settings(&Settings::from_pairs(pairs).expect("settings"))
}

/// A quote by a fake business on `job`, received at `at`, confirmed or not.
async fn quote(db: &mut PgConnection, job: Uuid, at: Timestamp, confirmed: bool) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site,
           notes, original, ai_read, confirmed, received_at)
         VALUES ($1, gen_random_uuid(), 'firm', 10000, false, '', 'Fake quote', false, $2,
           $3::timestamptz)",
    )
    .bind(job)
    .bind(confirmed)
    .bind(at.to_string())
    .execute(db)
    .await
    .expect("quote");
}

/// Job `i`: its facts row (trade `TRADES[i % 3]`), a card at the first card plus `i` hours and a
/// second card an hour later, one confirmed quote 3 hours after the first card and, for an even
/// `i`, a second one 5 hours after it. Job 0 also has an unconfirmed quote after 1 hour.
async fn seed_job(db: &mut PgConnection, i: i64) {
    let job: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
         VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id",
    )
    .fetch_one(&mut *db)
    .await
    .expect("job");
    let trade = TRADES[usize::try_from(i).expect("index") % 3];
    sqlx::query(
        "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
         VALUES ($1, 1, $2, ARRAY['fake_type'], now())",
    )
    .bind(job)
    .bind(trade)
    .execute(&mut *db)
    .await
    .expect("facts");
    let sent = at(FIRST_CARD) + SignedDuration::from_hours(i);
    for later in [0, 1] {
        sqlx::query(
            "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
             VALUES ($1, gen_random_uuid(), false, $2::timestamptz)",
        )
        .bind(job)
        .bind((sent + SignedDuration::from_hours(later)).to_string())
        .execute(&mut *db)
        .await
        .expect("card");
    }
    quote(db, job, sent + SignedDuration::from_hours(3), true).await;
    if i % 2 == 0 {
        quote(db, job, sent + SignedDuration::from_hours(5), true).await;
    }
    if i == 0 {
        quote(db, job, sent + SignedDuration::from_hours(1), false).await;
    }
}

/// A fixed instant.
fn at(text: &str) -> Timestamp {
    text.parse().expect("timestamp")
}

/// TD-JOB-04 (WEB-028, D-466, D-468): no number at 49 requests sent; at 50 the three computed
/// numbers with their date range and trade count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_04_no_numbers_at_49_and_three_numbers_at_50(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for i in 0..49 {
        seed_job(&mut db, i).await;
    }
    let pages = settings(&[]);
    let html = home_html(&pool, &pages).await.expect("home");
    assert!(!html.contains("requests so far"), "no numbers at 49");
    assert!(!html.contains("Requests sent to businesses"));
    seed_job(&mut db, 49).await;
    let html = home_html(&pool, &pages).await.expect("home");
    assert!(html.contains("Requests sent to businesses: 50"), "{html}");
    assert!(html.contains("Typical wait for the first quote: 3 hours (median)"));
    assert!(html.contains("Average quotes per request: 1.5"));
    assert!(html.contains("These numbers cover Nov 1, 2026 to Nov 3, 2026 and 3 trades."));
}

/// TD-JOB-04 (WEB-028, D-466): the figures come from the card and quote records as the app role:
/// an unconfirmed quote counts for nothing and the wait runs from a job's first card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_04_numbers_come_from_the_card_and_quote_records(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for i in 0..50 {
        seed_job(&mut db, i).await;
    }
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    assert_eq!(home_numbers(&mut db, 51).await.expect("read"), None);
    let found = home_numbers(&mut db, 50)
        .await
        .expect("read")
        .expect("shown");
    assert_eq!(found.requests, 50);
    assert_eq!(
        found.median_first_quote,
        Some(SignedDuration::from_hours(3))
    );
    assert_eq!(found.quotes_per_request_tenths, 15);
    assert_eq!(found.cards_from, at(FIRST_CARD));
    assert_eq!(found.cards_to, at("2026-11-03T19:00:00Z"));
    assert_eq!(found.trades, 3);
}

/// WEB-028, D-3878: `ASKHAVA_HOME_NUMBERS_MIN_JOBS` moves the threshold; with no card at all the
/// page shows none whatever the setting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_threshold_is_a_setting(pool: PgPool) {
    let low = settings(&[("ASKHAVA_HOME_NUMBERS_MIN_JOBS", "2")]);
    let html = home_html(&pool, &low).await.expect("home");
    assert!(!html.contains("requests so far"));
    let mut db = pool.acquire().await.expect("db");
    for i in 0..2 {
        seed_job(&mut db, i).await;
    }
    let html = home_html(&pool, &low).await.expect("home");
    assert!(html.contains("Requests sent to businesses: 2"));
    let pairs = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_HOME_NUMBERS_MIN_JOBS", "0"),
    ];
    let refused = Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned())));
    assert!(refused.is_err(), "a threshold of 0 is refused");
}

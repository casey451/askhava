//! The home page's numbers read every job's trade in one statement and are kept for a window (WEB-028, INT-018, WEB-007; D-4098, T592).
//!
//! On PostgreSQL: `jobs::api::categories_of` answers each listed job's latest category (the
//! highest `seq` wins), skips a job with no facts row and answers an empty set for an empty list;
//! `quotes::api::home_numbers` over 60 fake jobs equals the figures computed the old way (one
//! `job_facts` read per job for the trade count); it makes exactly 2 statements for 1 job and for
//! 2,000 jobs, counted with T416's `DbMetrics` installed as `db_metrics.rs` installs it
//! (`app_pool`, `log_subscriber`); and through the page router a cold `GET /` makes at most 3
//! statements on one acquire, a second inside the window only the banner's, and one after the
//! (shortened) window reads the numbers again. Rows are seeded straight into each module's
//! tables, with fake IDs and fake codes only.

use std::collections::BTreeSet;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{DbMetrics, DbSnapshot, Settings, app_pool, log_subscriber};
use askhava_jobs::api::{categories_of, job_facts};
use askhava_quotes::api::{HomeNumbers, home_numbers, median_wait};
use askhava_site::api::{HomeNumbersCache, SitePages, page_routes_with};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;
use tracing::Level;
use tracing::subscriber::DefaultGuard;

/// Three fake trade codes.
const TRADES: [&str; 3] = ["pool_service", "hvac", "handyman"];
/// The first card: November 1, 2026, 10 a.m. in Arizona.
const FIRST_CARD: &str = "2026-11-01T17:00:00Z";
/// The shortened cache window of the page-router case.
const WINDOW: Duration = Duration::from_millis(1500);

/// A new job row; its ID.
async fn new_job(db: &mut PgConnection) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
         VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id",
    )
    .fetch_one(db)
    .await
    .expect("job")
}

/// A facts row `seq` for `job` with `category` and `types`.
async fn facts(db: &mut PgConnection, job: Uuid, seq: i32, category: &str, types: &[&str]) {
    sqlx::query(
        "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(job)
    .bind(seq)
    .bind(category)
    .bind(types)
    .execute(db)
    .await
    .expect("facts");
}

/// A card for `job` sent at `at`.
async fn card(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
         VALUES ($1, gen_random_uuid(), false, $2::timestamptz)",
    )
    .bind(job)
    .bind(at.to_string())
    .execute(db)
    .await
    .expect("card");
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

/// `n` jobs, each with one facts row (a trade in turn) and one card, in three statements.
async fn bulk_jobs(pool: &PgPool, n: i32) {
    sqlx::query(
        "WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
                    SELECT 'quotes_ready', false, now(), now(), now()
                      FROM generate_series(1, $1) RETURNING id),
              f AS (INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
                    SELECT id, 1, ($2::text[])[1 + abs(hashtext(id::text)) % 3],
                           ARRAY['fake_type'], now() FROM j)
         INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
         SELECT id, gen_random_uuid(), false, now() FROM j",
    )
    .bind(n)
    .bind(TRADES)
    .execute(pool)
    .await
    .expect("bulk jobs");
}

/// A fixed instant.
fn at(text: &str) -> Timestamp {
    text.parse().expect("timestamp")
}

/// Settings over this test's own database: a pool of 1 and WEB-028's threshold at 1.
async fn own_settings(test_pool: &PgPool) -> Settings {
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(test_pool)
        .await
        .expect("current database");
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL is set for database tests");
    let (path, query) = base.split_once('?').unwrap_or((&base, ""));
    let (server, _) = path
        .rsplit_once('/')
        .expect("DATABASE_URL names a database");
    let sep = if query.is_empty() { "" } else { "?" };
    let url = format!("{server}/{name}{sep}{query}");
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", url.as_str()),
        ("ASKHAVA_DB_POOL_MAX", "1"),
        ("ASKHAVA_HOME_NUMBERS_MIN_JOBS", "1"),
    ];
    Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned()))).expect("settings")
}

/// The app's pool with the metrics layer installed for this thread, as `db_metrics.rs` does: a
/// warm-up statement opens the one connection, then tracing's callsite interest is rebuilt.
async fn metered(settings: &Settings) -> (PgPool, Arc<DbMetrics>, DefaultGuard) {
    let pool = app_pool(settings).expect("app pool");
    let metrics = Arc::new(DbMetrics::new());
    let subscriber = log_subscriber(Level::INFO, io::sink, &metrics);
    let guard = tracing::subscriber::set_default(subscriber);
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("warm-up");
    tracing::callsite::rebuild_interest_cache();
    (pool, metrics, guard)
}

/// The statements and acquires between two snapshots.
fn moved(before: &DbSnapshot, after: &DbSnapshot) -> (u64, u64) {
    (
        after.statements - before.statements,
        after.acquires - before.acquires,
    )
}

/// INT-018, D-4098: `categories_of` reads each job's latest row, skips a job with none and
/// answers an empty set for an empty list.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn categories_of_reads_each_jobs_latest_category(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (two_rows, one_row, none) = (
        new_job(&mut db).await,
        new_job(&mut db).await,
        new_job(&mut db).await,
    );
    facts(&mut db, two_rows, 1, "hvac", &["fake_a", "fake_b"]).await;
    facts(&mut db, two_rows, 2, "pool_service", &["fake_a"]).await;
    facts(&mut db, one_row, 1, "handyman", &["fake_c"]).await;
    let found = categories_of(&mut db, &[two_rows, one_row, none])
        .await
        .expect("read");
    let want: BTreeSet<String> = ["pool_service", "handyman"].map(str::to_owned).into();
    assert_eq!(
        found, want,
        "the highest seq wins and a job with no row adds nothing"
    );
    let only = categories_of(&mut db, &[two_rows]).await.expect("read");
    assert_eq!(only, BTreeSet::from(["pool_service".to_owned()]));
    assert!(
        categories_of(&mut db, &[none])
            .await
            .expect("read")
            .is_empty()
    );
    assert!(categories_of(&mut db, &[]).await.expect("read").is_empty());
}

/// Job `i` of 60: a trade in turn (none for every tenth), a narrowed second row for every
/// fourth, a card at the first card plus `i` hours and mixed quotes. Its ID, its first
/// confirmed wait and its confirmed quote count.
async fn mixed_job(db: &mut PgConnection, i: i64) -> (Uuid, Option<SignedDuration>, i64) {
    let job = new_job(db).await;
    let trade = TRADES[usize::try_from(i % 3).expect("index")];
    if i % 10 != 9 {
        facts(db, job, 1, trade, &["fake_a", "fake_b"]).await;
    }
    if i % 4 == 0 {
        facts(db, job, 2, trade, &["fake_a"]).await;
    }
    let sent = at(FIRST_CARD) + SignedDuration::from_hours(i);
    card(db, job, sent).await;
    let (mut wait, mut count) = (None, 0);
    if i % 3 != 2 {
        let first = SignedDuration::from_mins(30 * (i % 5 + 1));
        quote(db, job, sent + first, true).await;
        (wait, count) = (Some(first), 1);
    }
    if i % 2 == 0 {
        quote(db, job, sent + SignedDuration::from_hours(6), true).await;
        wait = wait.or(Some(SignedDuration::from_hours(6)));
        count += 1;
    }
    if i % 7 == 0 {
        quote(db, job, sent + SignedDuration::from_mins(10), false).await;
    }
    (job, wait, count)
}

/// WEB-028, D-4098: over 60 fake jobs in 3 trades with mixed confirmed and unconfirmed quotes,
/// `home_numbers` equals the figures computed the old way.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn home_numbers_equal_the_old_per_job_reads(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (mut waits, mut quotes, mut trades) = (Vec::new(), 0, BTreeSet::new());
    let mut jobs = Vec::new();
    for i in 0..60 {
        let (job, wait, count) = mixed_job(&mut db, i).await;
        waits.extend(wait);
        quotes += count;
        jobs.push(job);
    }
    for job in jobs {
        let latest = job_facts(&mut db, job).await.expect("facts");
        trades.extend(latest.map(|f| f.category));
    }
    let want = HomeNumbers {
        requests: 60,
        median_first_quote: median_wait(waits),
        quotes_per_request_tenths: u32::try_from(quotes * 10 / 60).expect("tenths"),
        cards_from: at(FIRST_CARD),
        cards_to: at(FIRST_CARD) + SignedDuration::from_hours(59),
        trades: u32::try_from(trades.len()).expect("trades"),
    };
    assert_eq!(want.trades, 3);
    let found = home_numbers(&mut db, 50).await.expect("read");
    assert_eq!(found, Some(want));
}

/// The statements `home_numbers` makes on one held connection of `pool`.
async fn statements_of_one_read(pool: &PgPool, metrics: &DbMetrics) -> u64 {
    let mut db = pool.acquire().await.expect("db");
    let before = metrics.snapshot();
    let found = home_numbers(&mut db, 1).await.expect("read");
    assert!(found.is_some(), "above the threshold");
    metrics.snapshot().statements - before.statements
}

/// WEB-028, D-4098: with the threshold at 1, `home_numbers` makes exactly 2 statements for 1 job
/// and for 2,000 jobs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn home_numbers_make_two_statements_however_many_jobs(test_pool: PgPool) {
    let settings = own_settings(&test_pool).await;
    bulk_jobs(&test_pool, 1).await;
    let (pool, metrics, _guard) = metered(&settings).await;
    assert_eq!(statements_of_one_read(&pool, &metrics).await, 2, "1 job");
    bulk_jobs(&test_pool, 1999).await;
    assert_eq!(
        statements_of_one_read(&pool, &metrics).await,
        2,
        "2,000 jobs"
    );
}

/// `GET /` on `router`: the page's HTML.
async fn get_home(router: &Router) -> String {
    let request = Request::builder()
        .uri("/")
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("answer");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    String::from_utf8_lossy(&body).into_owned()
}

/// WEB-007, WEB-028, D-4098: through the page router a cold `GET /` makes at most 3 statements on
/// one acquire, a second inside the window only the banner's, and one after the window reads the
/// numbers again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_served_home_page_keeps_its_numbers_for_the_window(test_pool: PgPool) {
    let settings = own_settings(&test_pool).await;
    bulk_jobs(&test_pool, 3).await;
    let (pool, metrics, _guard) = metered(&settings).await;
    let router = page_routes_with(
        pool,
        SitePages::from_settings(&settings),
        HomeNumbersCache::new(WINDOW),
    );
    let before = metrics.snapshot();
    let cold = get_home(&router).await;
    let (statements, acquires) = moved(&before, &metrics.snapshot());
    assert!(statements <= 3, "cold GET / made {statements} statements");
    assert_eq!(acquires, 1, "banner and numbers on one connection");
    assert!(cold.contains("Requests sent to businesses: 3"), "{cold}");
    let before = metrics.snapshot();
    let warm = get_home(&router).await;
    assert_eq!(
        moved(&before, &metrics.snapshot()),
        (1, 1),
        "only the banner"
    );
    assert!(warm.contains("Requests sent to businesses: 3"));
    tokio::time::sleep(WINDOW + Duration::from_millis(100)).await;
    bulk_jobs(&test_pool, 1).await;
    let before = metrics.snapshot();
    let later = get_home(&router).await;
    let (statements, _) = moved(&before, &metrics.snapshot());
    assert_eq!(statements, 3, "the banner and the numbers again");
    assert!(later.contains("Requests sent to businesses: 4"));
}

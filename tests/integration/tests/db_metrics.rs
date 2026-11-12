//! The database metrics count sqlx's statements and acquire waits and log nothing but numbers (TD-OPS-41; SEC-001, PRIV-016; D-3292, D-3298).
//!
//! TD-OPS-41 (the counter, bucket and log cases; the route cases are in
//! `tests/security/tests/perf_db_route.rs`), on the CI Postgres: each test builds the app's own
//! pool with `app_pool` (pool of 1), installs the same JSON log and metrics layers `init` installs
//! (`log_subscriber`) with its own counters and a captured writer for the test's thread, and then
//! checks that a known number of statements moves the statement counter by exactly that number,
//! that a second acquire behind a held connection lands in the bucket that holds its wait and
//! raises the maximum, that the pool's maximum, acquire timeout and slow threshold are as set,
//! and that sqlx's slow-acquire WARN line reaches the log while no log line holds SQL text, the
//! database URL or the planted fake value (a slow statement, which sqlx logs at WARN with its
//! SQL, included). `#[sqlx::test]` runs each test on one thread, so the thread's subscriber sees
//! every event. Fake data only.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use askhava_core::api::{
    DbMetrics, DbSnapshot, Settings, WAIT_BUCKETS_US, app_pool, log_subscriber,
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use tracing::Level;

/// A fake value bound into every statement; it must never reach a log line.
const PLANTED: &str = "fake-planted-value-t416";
/// A comment in every statement's SQL text; it must never reach a log line.
const SQL_MARKER: &str = "t416_sql_marker";
/// How long the first acquire holds the pool's one connection.
const HOLD: Duration = Duration::from_millis(300);

/// Log lines written by the subscriber under test.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// `DATABASE_URL` pointed at this test's own database.
async fn own_database_url(pool: &PgPool) -> SecretString {
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await
        .expect("current database");
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL is set for database tests");
    let (path, query) = base.split_once('?').unwrap_or((&base, ""));
    let (server, _) = path
        .rsplit_once('/')
        .expect("DATABASE_URL names a database");
    let sep = if query.is_empty() { "" } else { "?" };
    SecretString::from(format!("{server}/{name}{sep}{query}"))
}

/// The app's pool over this test's database, one connection at most.
async fn one_connection_pool(test_pool: &PgPool) -> (PgPool, SecretString) {
    let url = own_database_url(test_pool).await;
    let settings = Settings::from_pairs(
        [
            ("ASKHAVA_APP_ENV", "dev"),
            ("ASKHAVA_DATABASE_URL", url.expose_secret()),
            ("ASKHAVA_DB_POOL_MAX", "1"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .expect("settings parse");
    (app_pool(&settings).expect("app pool"), url)
}

/// One statement with the planted value bound and the marker in its SQL text.
async fn marked_statement(pool: &PgPool) {
    let sql = format!("SELECT length($1::text) /* {SQL_MARKER} */");
    let length: i32 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(PLANTED)
        .fetch_one(pool)
        .await
        .expect("statement runs");
    assert_eq!(usize::try_from(length).ok(), Some(PLANTED.len()));
}

/// Every log line holds none of the SQL text, the URL, the database name or the planted value.
fn assert_logs_clean(logs: &str, url: &SecretString, database: &str) {
    for secret in [
        SQL_MARKER,
        PLANTED,
        "SELECT",
        "length(",
        url.expose_secret(),
        database,
    ] {
        assert!(!logs.contains(secret), "a log line holds {secret:?}");
    }
    assert!(!logs.contains("postgres://"), "a log line holds a URL");
}

/// The subtraction a reader makes between two snapshots, per bucket.
fn bucket_deltas(before: &DbSnapshot, after: &DbSnapshot) -> Vec<u64> {
    before
        .buckets
        .iter()
        .zip(&after.buckets)
        .map(|(b, a)| a.count - b.count)
        .collect()
}

/// TD-OPS-41: a known number of statements (a slow one among them) moves the counter by exactly
/// that number, and no `sqlx::query` event, even the slow statement's WARN, reaches the log.
#[sqlx::test(migrations = false)]
async fn td_ops_41_statements_move_the_counter_by_exactly_that_many(test_pool: PgPool) {
    let (pool, url) = one_connection_pool(&test_pool).await;
    let database = pool
        .connect_options()
        .get_database()
        .unwrap_or_default()
        .to_owned();
    let metrics = Arc::new(DbMetrics::new());
    let capture = Capture::default();
    let writer = capture.clone();
    let _guard = tracing::subscriber::set_default(log_subscriber(
        Level::INFO,
        move || writer.clone(),
        &metrics,
    ));
    marked_statement(&pool).await;
    let before = metrics.snapshot();
    for _ in 0..4 {
        marked_statement(&pool).await;
    }
    let slow = format!("SELECT pg_sleep(1.05), length($1::text) /* {SQL_MARKER} */");
    sqlx::query(sqlx::AssertSqlSafe(slow))
        .bind(PLANTED)
        .execute(&pool)
        .await
        .expect("slow statement runs");
    let after = metrics.snapshot();
    assert_eq!(after.statements - before.statements, 5);
    assert_eq!(
        after.acquires - before.acquires,
        5,
        "one acquire per statement"
    );
    pool.close().await;
    assert_logs_clean(&capture.text(), &url, &database);
}

/// TD-OPS-41: with the pool's one connection held, the second acquire's wait lands in the bucket
/// that holds it and raises the maximum; sqlx's slow-acquire WARN line is logged with numbers only.
#[sqlx::test(migrations = false)]
async fn td_ops_41_a_held_connection_makes_the_second_acquire_wait(test_pool: PgPool) {
    let (pool, url) = one_connection_pool(&test_pool).await;
    let database = pool
        .connect_options()
        .get_database()
        .unwrap_or_default()
        .to_owned();
    let metrics = Arc::new(DbMetrics::new());
    metrics.register_pool(&pool);
    let capture = Capture::default();
    let writer = capture.clone();
    let _guard = tracing::subscriber::set_default(log_subscriber(
        Level::INFO,
        move || writer.clone(),
        &metrics,
    ));
    marked_statement(&pool).await;
    let before = metrics.snapshot();
    let held = pool.acquire().await.expect("first acquire");
    let waiter = tokio::spawn({
        let pool = pool.clone();
        async move { pool.acquire().await.map(drop) }
    });
    tokio::time::sleep(HOLD).await;
    drop(held);
    waiter.await.expect("waiter joins").expect("second acquire");
    let after = metrics.snapshot();
    assert_eq!(after.acquires - before.acquires, 2);
    let hold_us = u64::try_from(HOLD.as_micros()).expect("fits");
    assert!(
        after.wait_max_us >= hold_us,
        "max {} < hold",
        after.wait_max_us
    );
    assert!(after.wait_sum_us - before.wait_sum_us >= hold_us);
    let slot = WAIT_BUCKETS_US
        .iter()
        .position(|bound| after.wait_max_us <= *bound)
        .unwrap_or(WAIT_BUCKETS_US.len());
    let deltas = bucket_deltas(&before, &after);
    assert_eq!(
        deltas[slot], 1,
        "the long wait is in its bucket: {deltas:?}"
    );
    assert_eq!(deltas.iter().sum::<u64>(), 2);
    let pool_now = after.pool.expect("the pool is registered");
    assert_eq!((pool_now.size, pool_now.max), (1, 1));
    pool.close().await;
    let logs = capture.text();
    let slow: Vec<&str> = logs
        .lines()
        .filter(|l| l.contains("acquired_after_secs"))
        .collect();
    assert_eq!(slow.len(), 1, "one slow-acquire line: {logs}");
    assert!(slow[0].contains(r#""level":"WARN""#), "{}", slow[0]);
    assert_logs_clean(&logs, &url, &database);
}

/// TD-OPS-41: turning the measurements on changes neither the pool's maximum nor its acquire
/// timeout (five seconds), and the slow-acquire threshold is 250 ms.
#[sqlx::test(migrations = false)]
async fn td_ops_41_the_pool_size_and_timeout_are_unchanged(test_pool: PgPool) {
    let (pool, _url) = one_connection_pool(&test_pool).await;
    let options = pool.options();
    assert_eq!(options.get_max_connections(), 1);
    assert_eq!(options.get_acquire_timeout(), Duration::from_secs(5));
    assert_eq!(
        options.get_acquire_slow_threshold(),
        Duration::from_millis(250)
    );
    pool.close().await;
}

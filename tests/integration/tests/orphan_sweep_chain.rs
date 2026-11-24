//! The daily photo orphan sweep keeps its chain (PRIV-009, PRIV-011, PRIV-006; T757, D-5399).
//!
//! On a fresh database, as `askhava_app` through the task queue on a manual clock: a sweep whose
//! storage call fails returns its error (the task is retried) and the next day's sweep is queued
//! all the same, so a sweep that fails all its attempts never ends the daily chain; a sweep that
//! succeeds queues exactly one next sweep. Every object name and key is fake.

use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, LocalStorage, StorageAdapter, StorageDelete,
};
use askhava_core::api::{Clock, Lease, ManualClock, Outcome, Registry, WorkerId, claim};
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    ORPHAN_MIN_AGE, ORPHAN_SWEEP_KIND, register_photo_deletes, schedule_orphan_sweep,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgConnection, PgPool};

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-orphan-sweep-chain-0123456789")
}

/// A photo store whose every call fails.
struct Broken;

impl StorageAdapter for Broken {
    async fn put_object(
        &self,
        _key: &IdempotencyKey,
        _name: &str,
        _bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn read_object(&self, _name: &str) -> Result<Vec<u8>, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl StorageDelete for Broken {
    async fn delete_object(&self, _key: &IdempotencyKey, _name: &str) -> Result<(), AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn list_objects(&self, _prefix: &str) -> Result<Vec<String>, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

/// Queues a sweep due at `start`, claims it and runs it through `registry`; the outcome.
async fn sweep_once(pool: &PgPool, registry: &Registry, clock: &ManualClock) -> Outcome {
    let start = clock.now();
    schedule_orphan_sweep(&mut app(pool).await, start - ORPHAN_MIN_AGE)
        .await
        .expect("schedule");
    let pool = app_pool(pool).await;
    let owner = WorkerId::new(&pool).await.expect("worker");
    let mut tasks = claim(&pool, owner, start, SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    assert_eq!(tasks.len(), 1, "the one sweep is due");
    let task = tasks.pop().expect("sweep");
    registry
        .run(&pool, clock, owner, task, Lease::WORKER)
        .await
        .expect("run")
}

/// How many sweeps are queued for the next day, after `start`, not yet run.
async fn next_sweeps(pool: &PgPool, start: Timestamp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND run_at = $2 AND done_at IS NULL \
         AND dead_at IS NULL AND attempts = 0",
    )
    .bind(ORPHAN_SWEEP_KIND)
    .bind((start + ORPHAN_MIN_AGE).to_sqlx())
    .fetch_one(pool)
    .await
    .expect("count")
}

/// PRIV-009, D-5399: a sweep whose storage list fails returns its error, so the task is retried,
/// and the next day's sweep is queued all the same.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_sweep_still_queues_the_next(pool: PgPool) {
    let start = Timestamp::now();
    let clock = Arc::new(ManualClock::new(start));
    let mut registry = Registry::new();
    register_photo_deletes(&mut registry, Arc::new(Broken), secret(), clock.clone());
    let outcome = sweep_once(&pool, &registry, &clock).await;
    assert!(matches!(outcome, Outcome::Retry(_)), "{outcome:?}");
    let failed: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM core.task WHERE kind = $1 AND attempts = 1")
            .bind(ORPHAN_SWEEP_KIND)
            .fetch_one(&pool)
            .await
            .expect("failed sweep");
    assert!(failed.is_some(), "the sweep's error is recorded");
    assert_eq!(
        next_sweeps(&pool, start).await,
        1,
        "the next sweep is queued"
    );
}

/// PRIV-009, D-5399: a sweep that succeeds queues exactly one next sweep.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_good_sweep_queues_exactly_one_next(pool: PgPool) {
    let root = std::env::temp_dir().join(format!("askhava-t757-sweep-{}", std::process::id()));
    std::fs::create_dir_all(root.join("photos")).expect("temporary directory");
    let local = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    let start = Timestamp::now();
    let clock = Arc::new(ManualClock::new(start));
    let mut registry = Registry::new();
    register_photo_deletes(&mut registry, Arc::new(local), secret(), clock.clone());
    let outcome = sweep_once(&pool, &registry, &clock).await;
    let _gone = std::fs::remove_dir_all(&root);
    assert_eq!(outcome, Outcome::Done);
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1 AND done_at IS NULL")
            .bind(ORPHAN_SWEEP_KIND)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(queued, 1, "exactly one next sweep");
    assert_eq!(next_sweeps(&pool, start).await, 1);
}

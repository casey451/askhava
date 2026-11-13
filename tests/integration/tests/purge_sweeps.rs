//! TD-PRIV-25 (T367, D-2641; PRIV-007, PRIV-008, PRIV-009, TXT-020): the daily purge sweep never
//! stops for good and reads only what is due.
//!
//! The registry is `askhava_server::worker_registry`, built from dev stand-in settings over a
//! `ManualClock`, so every case runs the composition's own registrations (the alert wrapper and
//! the worker's clock). The layer table lets a test crate depend on modules but not on
//! `askhava-server` (D-736), so the composition root is compiled in place, as
//! `task_registry.rs` and `emergency_notices.rs` do (D-2853's exception, logged for this file in
//! D-3447). Every job and task is fake: IDs only, no contact detail.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Clock, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, claim, enqueue,
};
use askhava_jobs::api::{
    PURGE_DUE_KIND, SWEEP_KIND, SWEEP_PAGE, final_jobs_page, sweep_closed, unqueued_finals,
};
use askhava_picks::api::ConfirmSettings;
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t367-not-a-real-key";
const DAY: SignedDuration = SignedDuration::from_hours(24);
/// Midnight UTC, so a few hours of retries stay on one day.
const START: i64 = 1_790_035_200;

/// The registry the `worker` binary runs, from dev stand-in settings, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t367-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    worker_registry(&settings, stream, (None, ConfirmSettings::default()), pairs).expect("registry")
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            registry
                .run(pool, clock, owner, task, Lease::WORKER)
                .await
                .expect("run");
        }
    }
}

async fn queue_sweep(pool: &PgPool, at: Timestamp) {
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &NewTask::new(SWEEP_KIND, json!({}), at))
        .await
        .expect("sweep");
}

/// `count` fake jobs closed at `closed`.
async fn closed_jobs(pool: &PgPool, count: i32, closed: Timestamp) {
    sqlx::query(
        "INSERT INTO jobs.job (status, created_at, status_at, timer_from)
         SELECT 'closed', $2, $2, $2 FROM generate_series(1, $1)",
    )
    .bind(count)
    .bind(jiff_sqlx::Timestamp::from(closed))
    .execute(pool)
    .await
    .expect("jobs");
}

async fn scalar(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// The last value drawn from `core.task`'s id sequence. An insert dropped by `ON CONFLICT DO
/// NOTHING` still draws one, so an unmoved value proves the sweep attempted no insert at all.
const TASK_IDS: &str = "SELECT last_value FROM pg_sequences \
     WHERE format('%I.%I', schemaname, sequencename) = pg_get_serial_sequence('core.task', 'id')";

/// TD-PRIV-25 (PRIV-007, D-2641): a sweep whose every attempt fails still leaves the next day's
/// sweep queued and live, and its last attempt raises exactly one `privacy.purge_failed` alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_25_a_sweep_failing_every_attempt_leaves_tomorrows_and_alerts_once(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(
        Timestamp::from_second(START).expect("time"),
    ));
    let registry = registry(clock.clone());
    queue_sweep(&pool, clock.now()).await;
    sqlx::query("ALTER TABLE jobs.job RENAME TO job_gone")
        .execute(&pool)
        .await
        .expect("break the read");
    for _ in 0..6 {
        run_due(&pool, &registry, &clock).await;
        clock.set(clock.now() + SignedDuration::from_hours(2));
    }
    let dead =
        "SELECT count(*) FROM core.task WHERE kind = 'jobs.purge_sweep' AND dead_at IS NOT NULL";
    assert_eq!(scalar(&pool, dead).await, 1);
    let next = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM core.task WHERE kind = 'jobs.purge_sweep' AND dead_at IS NULL
         AND done_at IS NULL AND run_at >= $1",
    )
    .bind(jiff_sqlx::Timestamp::from(
        Timestamp::from_second(START).expect("time") + DAY,
    ))
    .fetch_one(&pool)
    .await
    .expect("next");
    assert_eq!(next, 1, "the next day's sweep is queued once");
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = 'privacy.purge_failed' \
                  AND severity = 'high'";
    assert_eq!(scalar(&pool, alerts).await, 1);
}

/// TD-PRIV-25 (PRIV-007 to PRIV-009, D-2641): over 5,000 final jobs whose stages are queued, the
/// sweep hands on none again and queues nothing (the task id sequence does not move), in pages of at most `SWEEP_PAGE`; a newly closed
/// job is still found.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_25_a_sweep_over_5000_queued_jobs_reads_none_again(pool: PgPool) {
    let closed = Timestamp::from_second(START).expect("time");
    closed_jobs(&pool, 5_000, closed).await;
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(sweep_closed(&mut conn, closed + DAY).await, Ok(15_000));
    let tasks = "SELECT count(*) FROM core.task";
    let before = scalar(&pool, tasks).await;
    let ids_before = scalar(&pool, TASK_IDS).await;
    assert_eq!(unqueued_finals(&mut conn, closed + DAY).await, Ok(vec![]));
    let page = final_jobs_page(&mut conn, closed + DAY, Uuid::nil())
        .await
        .expect("page");
    assert_eq!(page.len(), usize::try_from(SWEEP_PAGE).expect("page size"));
    assert!(page.iter().all(|(_, _, open)| !open));
    assert_eq!(sweep_closed(&mut conn, closed + DAY * 2).await, Ok(0));
    assert_eq!(scalar(&pool, tasks).await, before);
    let ids_after = scalar(&pool, TASK_IDS).await;
    assert_eq!(
        ids_after, ids_before,
        "the sweep tried no insert, not even a deduped one"
    );
    closed_jobs(&pool, 1, closed + DAY).await;
    let found = unqueued_finals(&mut conn, closed + DAY * 2)
        .await
        .expect("read");
    assert_eq!(found.len(), 1);
    assert_eq!(sweep_closed(&mut conn, closed + DAY * 2).await, Ok(3));
}

/// TD-PRIV-25 (PRIV-007, SEC-035, D-2641): a worker on a `ManualClock` 31 days ahead queues the
/// stages of a job that closed 30 days ahead of the wall clock, so the sweep reads the worker's
/// clock, not the wall's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_25_a_worker_on_a_manual_clock_31_days_on_queues_the_stages(pool: PgPool) {
    let start = Timestamp::now();
    let clock = Arc::new(ManualClock::new(start));
    let registry = registry(clock.clone());
    closed_jobs(&pool, 1, start + DAY * 30).await;
    clock.set(start + DAY * 31);
    queue_sweep(&pool, clock.now()).await;
    run_due(&pool, &registry, &clock).await;
    let stages = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(PURGE_DUE_KIND)
        .fetch_one(&pool)
        .await
        .expect("stages");
    assert_eq!(stages, 3);
    let done = "SELECT count(*) FROM core.task WHERE kind = 'jobs.purge_sweep' \
                AND done_at IS NOT NULL";
    assert_eq!(scalar(&pool, done).await, 1);
}

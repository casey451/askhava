//! A task whose worker died on its last attempt is found on a busy queue, and `/healthz` tells a
//! failed queue read from a down database (T778, D-5081 F35, F36, D-5526; OPS-004, OPS-007).
//!
//! On a clock advanced by hand, with the real claim loop: beside a steady supply of due tasks, a
//! task at its last attempt whose lease expired is dead-lettered once the clock passes a minute,
//! the dead-letter check alerts on it, and the probe never reports the worker down for it; with
//! every lane held by a slow run the same happens. With `core.task` unreadable the probe answers
//! 503 with the database up and the worker unknown. The probe and the dead-letter check are
//! compiled in place from `app/server/src/lib.rs` (the layer table keeps `askhava-server` out of
//! test crates, D-736). All data is fake.

use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{
    Claimed, Clock, Environment, HandlerError, LEASE_EXPIRED, ManualClock, NewTask, REAP_EVERY,
    Registry, Worker, WorkerId, enqueue,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower::ServiceExt;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the health probe and the dead-letter check are called here"
)]
mod server;
use server::*;

use healthz::{HealthLimits, check, routes};

/// Monday, November 2, 2026, 10:00 in Arizona.
const START: &str = "2026-11-02T17:00:00Z";
/// A task that queues one more of itself when it runs: the queue never empties.
const STEADY: &str = "test.steady";
/// A task that holds its lane far longer than the test.
const SLOW: &str = "test.slow";
/// The task whose worker died on its last attempt.
const STUCK: &str = "test.stuck";
/// The longest the test waits, in real time, for the loop to act.
const PATIENCE: Duration = Duration::from_secs(30);

fn start() -> Timestamp {
    START.parse().expect("time")
}

async fn add(pool: &PgPool, kind: &'static str, run_at: Timestamp) {
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &NewTask::new(kind, json!({}), run_at))
        .await
        .expect("enqueue");
}

async fn steady(pool: PgPool, _task: Claimed) -> Result<(), HandlerError> {
    add(&pool, STEADY, start()).await;
    Ok(())
}

async fn slow(_pool: PgPool, _task: Claimed) -> Result<(), HandlerError> {
    tokio::time::sleep(Duration::from_secs(600)).await;
    Ok(())
}

/// Starts the real claim loop on `clock`; sending on the returned channel stops it.
async fn start_worker(
    pool: &PgPool,
    clock: Arc<ManualClock>,
) -> (oneshot::Sender<()>, JoinHandle<()>) {
    let mut registry = Registry::new();
    registry.register(STEADY, steady);
    registry.register(SLOW, slow);
    let worker = Worker {
        pool: pool.clone(),
        registry: Arc::new(registry),
        clock,
        owner: WorkerId::new(pool).await.expect("owner"),
    };
    let (stop, stopped) = oneshot::channel::<()>();
    let run = tokio::spawn(async move {
        let shutdown = async {
            let _either = stopped.await;
        };
        let _released = worker.run(shutdown, Duration::ZERO).await;
    });
    (stop, run)
}

async fn scalar(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(STUCK)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Waits, in real time up to [`PATIENCE`], until `sql` counts at least `want`.
async fn until(pool: &PgPool, sql: &'static str, want: i64) {
    let wait = async {
        while scalar(pool, sql).await < want {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(PATIENCE, wait).await.expect(sql);
}

/// A [`STUCK`] task due an hour ago, at its last attempt, whose other worker's lease ran out.
async fn stuck_task(pool: &PgPool) {
    add(pool, STUCK, start() - SignedDuration::from_hours(1)).await;
    sqlx::query(
        "UPDATE core.task SET attempts = max_attempts, lease_owner = gen_random_uuid(), \
         lease_until = $2 WHERE kind = $1",
    )
    .bind(STUCK)
    .bind((start() - SignedDuration::from_secs(1)).to_sqlx())
    .execute(pool)
    .await
    .expect("stuck");
}

const DEAD: &str = "SELECT count(*) FROM core.task WHERE kind = $1 AND dead_at IS NOT NULL";

/// The probe's state at the clock's time is healthy: the stuck task never counts as waiting.
async fn probe_healthy(pool: &PgPool, clock: &ManualClock) {
    let state = check(pool, clock.now(), &HealthLimits::default()).await;
    assert!(
        state.healthy(),
        "the worker is not reported down: {state:?}"
    );
}

/// Advances `clock` past a minute and waits for the stuck task's dead letter.
async fn reaped_within_a_minute(pool: &PgPool, clock: &ManualClock) {
    assert_eq!(scalar(pool, DEAD).await, 0, "not yet dead");
    clock.advance(REAP_EVERY + SignedDuration::from_secs(1));
    until(pool, DEAD, 1).await;
    let error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM core.task WHERE kind = $1")
            .bind(STUCK)
            .fetch_one(pool)
            .await
            .expect("error");
    assert_eq!(error.as_deref(), Some(LEASE_EXPIRED));
}

/// OPS-004, OPS-007, D-5081 F35: on a queue that never empties, the stuck task is dead-lettered
/// within a minute and alerted, and the probe stays healthy throughout.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_last_attempt_task_is_reaped_on_a_busy_queue(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(start()));
    for _ in 0..8 {
        add(&pool, STEADY, start()).await;
    }
    let (stop, run) = start_worker(&pool, Arc::clone(&clock)).await;
    let ran = "SELECT count(*) FROM core.task WHERE kind <> $1 AND done_at IS NOT NULL";
    until(&pool, ran, 20).await;
    stuck_task(&pool).await;
    probe_healthy(&pool, &clock).await;
    reaped_within_a_minute(&pool, &clock).await;
    until(&pool, ran, 40).await;
    probe_healthy(&pool, &clock).await;
    let _sent = stop.send(());
    run.await.expect("worker");
    no_handler_alert::run_check(&pool, Environment::Test, clock.now())
        .await
        .expect("check");
    let alerts: Vec<(String, sqlx::types::Uuid)> = sqlx::query_as(
        "SELECT severity, subject_id FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(no_handler_alert::DEAD_ALERT)
    .fetch_all(&pool)
    .await
    .expect("alerts");
    let day = clock.now().as_second().div_euclid(86_400);
    let one = vec![("high".to_owned(), no_handler_alert::day_id(STUCK, day))];
    assert_eq!(alerts, one, "the dead-letter alert counts it");
}

/// OPS-004, D-5526: with every lane held by a slow run, a minute on the clock still reaps.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_full_worker_still_reaps_once_a_minute(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(start()));
    for _ in 0..askhava_core::api::WORKER_CONCURRENCY {
        add(&pool, SLOW, start()).await;
    }
    let (stop, run) = start_worker(&pool, Arc::clone(&clock)).await;
    let held = "SELECT count(*) FROM core.task WHERE kind <> $1 AND lease_owner IS NOT NULL";
    let lanes = i64::try_from(askhava_core::api::WORKER_CONCURRENCY).expect("lanes");
    until(&pool, held, lanes).await;
    stuck_task(&pool).await;
    reaped_within_a_minute(&pool, &clock).await;
    let _sent = stop.send(());
    run.await.expect("worker");
}

/// OPS-007, D-5081 F36: a queue read that fails after a good round trip answers 503 with the
/// database up and the worker unknown, the answer's keys unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_queue_read_is_not_the_database_down(pool: PgPool) {
    sqlx::raw_sql("ALTER TABLE core.task RENAME TO task_hidden")
        .execute(&pool)
        .await
        .expect("hide the queue");
    let state = check(&pool, Timestamp::now(), &HealthLimits::default()).await;
    assert!(state.database_up && state.worker_unknown && !state.healthy());
    let request = Request::get("/healthz")
        .body(Body::empty())
        .expect("request");
    let response = routes(pool).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = to_bytes(response.into_body(), 4096).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    let expected = json!({"status": "down", "database": "up", "worker": "unknown", "last_activity_secs": null});
    assert_eq!(body, expected);
}

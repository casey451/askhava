//! `/healthz` and the liveness exits (TD-OPS-02's `/healthz` half, TD-OPS-24; OPS-007, OPS-049,
//! D-3144, D-3881, D-4044).
//!
//! The probe answers 200 on a fresh database and with an idle worker, 503 once due work has
//! waited past the worker-down setting or the database is unreachable, and its body names only
//! the two states and an age. The liveness watch ends (the binary then exits non-zero) only after
//! its check has failed for the whole limit, on Tokio's paused clock. All data is fake.

use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{ClaimTick, NewTask, enqueue};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the health probe and the liveness watch are called here"
)]
mod server;
use server::*;

use healthz::{HealthLimits, check, routes};
use liveness::{FailClock, LIMIT_KEY, LivenessLimit, watch_web, watch_worker};

/// A pool to a closed local port: every round trip fails.
fn unreachable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(1))
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("lazy pool")
}

async fn probe(pool: PgPool) -> (StatusCode, Value, Option<String>) {
    let request = Request::get("/healthz")
        .body(Body::empty())
        .expect("request");
    let response = routes(pool).oneshot(request).await.expect("response");
    let status = response.status();
    let cache = response
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|v| v.to_str().expect("ascii").to_owned());
    let bytes = to_bytes(response.into_body(), 4096).await.expect("body");
    (status, serde_json::from_slice(&bytes).expect("json"), cache)
}

async fn queue_task_due(pool: &PgPool, key: &str, ago: SignedDuration) {
    let run_at = Timestamp::now().checked_sub(ago).expect("time");
    let mut task = NewTask::new("test.health", json!({}), run_at);
    task.dedupe_key = Some(key.to_owned());
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &task).await.expect("enqueue");
}

/// TD-OPS-02: a fresh database with no worker answers 200, naming the database and worker up and
/// no activity yet, `no-store`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_02_a_fresh_database_answers_ok(pool: PgPool) {
    let (status, body, cache) = probe(pool).await;
    assert_eq!(status, StatusCode::OK);
    let expected =
        json!({"status": "ok", "database": "up", "worker": "up", "last_activity_secs": null});
    assert_eq!(body, expected);
    assert_eq!(cache.as_deref(), Some("no-store"));
}

/// TD-OPS-02, D-3144: work due for 5 minutes leaves the worker up; work due for 11 minutes (past
/// the 10-minute default) reports it down with 503.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_02_work_waiting_past_the_limit_reports_the_worker_down(pool: PgPool) {
    queue_task_due(&pool, "health-5", SignedDuration::from_mins(5)).await;
    assert_eq!(probe(pool.clone()).await.0, StatusCode::OK);
    queue_task_due(&pool, "health-11", SignedDuration::from_mins(11)).await;
    let (status, body, _) = probe(pool.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        (body["status"].clone(), body["worker"].clone()),
        (json!("down"), json!("down"))
    );
    let limits = HealthLimits::from_pairs([("ASKHAVA_WORKER_DOWN_SECS".into(), "900".into())])
        .expect("limits");
    assert!(check(&pool, Timestamp::now(), &limits).await.healthy());
}

/// TD-OPS-02: a finished task gives the newest activity's age in whole seconds.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_02_the_body_names_the_newest_finish_age(pool: PgPool) {
    queue_task_due(&pool, "health-done", SignedDuration::from_mins(1)).await;
    sqlx::query("UPDATE core.task SET done_at = now() - interval '30 seconds'")
        .execute(&pool)
        .await
        .expect("finish");
    let (status, body, _) = probe(pool).await;
    assert_eq!(status, StatusCode::OK);
    let age = body["last_activity_secs"].as_i64().expect("age");
    assert!((30..40).contains(&age), "age {age}");
}

/// TD-OPS-24, TD-OPS-02: with the database unreachable the probe answers 503, database down and
/// worker unknown, and the web watch has not ended at 2 minutes 59 seconds but has at 3 minutes.
#[tokio::test(start_paused = true)]
async fn td_ops_24_web_answers_503_then_exits_at_three_minutes() {
    let (status, body, _) = probe(unreachable_pool()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let expected = json!({"status": "down", "database": "down", "worker": "unknown", "last_activity_secs": null});
    assert_eq!(body, expected);
    let mut watch = Box::pin(watch_web(unreachable_pool(), LivenessLimit::default()));
    let early = tokio::time::timeout(Duration::from_secs(179), &mut watch).await;
    assert!(early.is_err(), "still running at 2:59");
    let ended = tokio::time::timeout(Duration::from_secs(2), &mut watch).await;
    assert!(ended.is_ok(), "ended at 3:00");
}

/// TD-OPS-24: a database back at 2 minutes resets the clock; 3 minutes of failure in a row expire.
#[test]
fn td_ops_24_a_good_check_resets_the_clock() {
    let start = tokio::time::Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    let mut clock = FailClock::new(LivenessLimit::default());
    assert!(!clock.observe(false, at(0)));
    assert!(!clock.observe(false, at(179)));
    assert!(!clock.observe(true, at(120 + 60)));
    assert!(!clock.observe(false, at(190)));
    assert!(!clock.observe(false, at(369)));
    assert!(clock.observe(false, at(370)));
}

/// TD-OPS-24: a worker whose claim loop never ticks ends its watch at 3 minutes even with the
/// database up.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_24_a_worker_that_stops_ticking_exits(pool: PgPool) {
    tokio::time::pause();
    let tick = Arc::new(ClaimTick::default());
    let watch = watch_worker(pool, tick, LivenessLimit::default());
    let ended = tokio::time::timeout(Duration::from_secs(200), watch).await;
    assert!(ended.is_ok(), "the stuck worker's watch ended");
}

/// TD-OPS-24: the 3 minutes is a setting; a bad value is refused naming the key, never the value.
/// The floor is 150 seconds (D-4224): a full worker waits up to 120 seconds on one handler run
/// without a new tick, plus the 10-second check, so a lower limit would kill a healthy worker.
#[tokio::test(start_paused = true)]
async fn td_ops_24_the_limit_is_a_setting() {
    let limit = LivenessLimit::from_pairs([(LIMIT_KEY.into(), "150".into())]).expect("limit");
    let watch = watch_web(unreachable_pool(), limit);
    assert!(
        tokio::time::timeout(Duration::from_secs(151), watch)
            .await
            .is_ok()
    );
    let refused = LivenessLimit::from_pairs([(LIMIT_KEY.into(), "fake-9".into())])
        .expect_err("refused")
        .to_string();
    assert!(
        refused.contains(LIMIT_KEY) && !refused.contains("fake-9"),
        "{refused}"
    );
    for low in ["30", "60", "149"] {
        assert!(LivenessLimit::from_pairs([(LIMIT_KEY.into(), low.into())]).is_err());
    }
}

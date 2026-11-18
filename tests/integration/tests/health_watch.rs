//! The 5-minute health check's alert (T624, D-4280; TD-OPS-44's health-check cases and TD-OPS-08's
//! health case; OPS-004, OPS-023, OPS-029, D-220).
//!
//! A failing check is planted by the test: one waiting task due an hour before the clock, so
//! `/healthz`'s worker-down test fails; deleting it makes the check pass. Each run takes the
//! oldest open `ops.health_check` task's payload from the queue, marks it done and runs it as the
//! handler does (`health_watch::run_payload`), so the chain's carried result is what decides; the
//! restart case runs the registered handler through the queue. All data is fake.

use std::sync::Arc;

use askhava_core::api::{
    Clock, Environment, Lease, ManualClock, NewTask, Outcome, Registry, WorkerId, claim, enqueue,
};
use askhava_ops::api::{HEALTH_ALERT, alerts_to_push};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::PgPool;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the health check task is called here")]
mod server;
use server::*;

use health_watch::{CHECK_KIND, HealthRun, SLOT_SECS, run_check, run_payload, schedule};
use healthz::HealthLimits;

/// A 5-minute slot start (fake time).
const START: &str = "2026-11-02T17:00:00Z";
const FIVE: SignedDuration = SignedDuration::from_mins(5);

fn clock() -> ManualClock {
    ManualClock::new(START.parse().expect("time"))
}

/// Plants the failing check: a task due an hour ago that no worker took.
async fn plant_stuck(pool: &PgPool, now: Timestamp) {
    let due = now - SignedDuration::from_hours(1);
    let mut task = NewTask::new("test.health_stuck", json!({}), due);
    task.dedupe_key = Some("test.health_stuck".to_owned());
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &task).await.expect("stuck task");
}

/// Makes the check pass again.
async fn unstick(pool: &PgPool) {
    sqlx::query("DELETE FROM core.task WHERE kind = 'test.health_stuck'")
        .execute(pool)
        .await
        .expect("unstick");
}

/// Runs the oldest open health check run at the clock's time in `environment`.
async fn run_next(pool: &PgPool, clock: &ManualClock, environment: Environment) {
    let (id, payload): (i64, Value) = sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = $1 AND done_at IS NULL ORDER BY run_at LIMIT 1",
    )
    .bind(CHECK_KIND)
    .fetch_one(pool)
    .await
    .expect("a queued run");
    sqlx::query("UPDATE core.task SET done_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("done");
    let limits = HealthLimits::default();
    run_payload(pool, environment, &limits, &payload, clock.now())
        .await
        .expect("run");
    clock.advance(FIVE);
}

/// Seeds the chain at the clock's time.
async fn seed(pool: &PgPool, clock: &ManualClock) {
    let mut conn = pool.acquire().await.expect("conn");
    schedule(&mut conn, clock.now()).await.expect("seed");
}

/// The severities of the open health alerts; a raised one is pinned to `raised` (the clock's
/// time, so T204's 10-minute push rule reads it as raised then).
async fn open_alerts(pool: &PgPool, raised: Timestamp) -> Vec<String> {
    sqlx::query("UPDATE ops.alert SET raised_at = $2::text::timestamptz WHERE kind = $1")
        .bind(HEALTH_ALERT)
        .bind(raised.to_string())
        .execute(pool)
        .await
        .expect("pin");
    sqlx::query_scalar("SELECT severity FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL")
        .bind(HEALTH_ALERT)
        .fetch_all(pool)
        .await
        .expect("alerts")
}

/// The dedupe keys of the queued health check runs, oldest first.
async fn keys(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT dedupe_key FROM core.task WHERE kind = $1 ORDER BY run_at")
        .bind(CHECK_KIND)
        .fetch_all(pool)
        .await
        .expect("keys")
}

/// TD-OPS-44 (OPS-004, OPS-023): one failure raises none, the second in a row one High alert, a
/// third nothing more, and a passing check clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_two_failed_checks_raise_one_high_alert_and_a_pass_clears_it(pool: PgPool) {
    let clock = clock();
    seed(&pool, &clock).await;
    plant_stuck(&pool, clock.now()).await;
    run_next(&pool, &clock, Environment::Test).await;
    assert!(
        open_alerts(&pool, clock.now()).await.is_empty(),
        "one failure raises none"
    );
    run_next(&pool, &clock, Environment::Test).await;
    assert_eq!(
        open_alerts(&pool, clock.now()).await,
        ["high"],
        "two raise one High alert"
    );
    run_next(&pool, &clock, Environment::Test).await;
    assert_eq!(open_alerts(&pool, clock.now()).await, ["high"], "still one");
    unstick(&pool).await;
    run_next(&pool, &clock, Environment::Test).await;
    assert!(
        open_alerts(&pool, clock.now()).await.is_empty(),
        "a pass clears it"
    );
}

/// TD-OPS-44: failures with a pass between them are never two in a row; dev and ci raise nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_failures_not_in_a_row_and_dev_or_ci_raise_none(pool: PgPool) {
    let clock = clock();
    seed(&pool, &clock).await;
    plant_stuck(&pool, clock.now()).await;
    run_next(&pool, &clock, Environment::Test).await;
    unstick(&pool).await;
    run_next(&pool, &clock, Environment::Test).await;
    plant_stuck(&pool, clock.now()).await;
    run_next(&pool, &clock, Environment::Test).await;
    for environment in [Environment::Dev, Environment::Ci] {
        run_next(&pool, &clock, environment).await;
    }
    assert!(open_alerts(&pool, clock.now()).await.is_empty());
    assert_eq!(keys(&pool).await.len(), 6, "every run queued the next slot");
}

/// D-4280: a second start-up in the same slot, before or after its run, queues no second run; the
/// registered handler (test environment) queues the next slot with the check's result.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_a_second_start_up_in_the_slot_queues_no_second_run(pool: PgPool) {
    let clock = Arc::new(clock());
    let slot = clock.now().as_second() / SLOT_SECS;
    seed(&pool, &clock).await;
    clock.advance(SignedDuration::from_mins(2));
    seed(&pool, &clock).await;
    assert_eq!(keys(&pool).await, [format!("{CHECK_KIND}.{slot}")]);
    let mut registry = Registry::new();
    let limits = HealthLimits::default();
    health_watch::register(&mut registry, Environment::Test, limits, clock.clone());
    let owner = WorkerId::new(&pool).await.expect("owner");
    let tasks = claim(&pool, owner, clock.now(), Lease::WORKER.length, 10)
        .await
        .expect("claim");
    assert_eq!(tasks.len(), 1);
    for task in tasks {
        let outcome = registry
            .run(&pool, clock.as_ref(), owner, task, Lease::WORKER)
            .await;
        assert!(matches!(outcome, Ok(Outcome::Done)), "{outcome:?}");
    }
    seed(&pool, &clock).await;
    let next = format!("{CHECK_KIND}.{}", slot + 1);
    assert_eq!(
        keys(&pool).await,
        [format!("{CHECK_KIND}.{slot}"), next.clone()]
    );
    let payload: Value = sqlx::query_scalar("SELECT payload FROM core.task WHERE dedupe_key = $1")
        .bind(&next)
        .fetch_one(&pool)
        .await
        .expect("next run");
    assert_eq!(payload, json!({"health_failed": false, "slot": slot + 1}));
}

/// D-4280: a run whose `now` is 1 second before its own slot's start still queues its slot plus 1.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_a_clock_behind_the_slot_still_moves_the_chain_on(pool: PgPool) {
    let start: Timestamp = START.parse().expect("time");
    let slot = start.as_second() / SLOT_SECS;
    let run = HealthRun {
        slot,
        health_failed: false,
    };
    let now = start - SignedDuration::from_secs(1);
    run_check(&pool, Environment::Test, &HealthLimits::default(), run, now)
        .await
        .expect("run");
    assert_eq!(keys(&pool).await, [format!("{CHECK_KIND}.{}", slot + 1)]);
}

/// Fails at 0 and 5 minutes, passes at 10 unless `still_failing`, runs at 15, and counts the
/// health alerts T204's push takes at 15 minutes.
async fn health_pushes_at_15(pool: &PgPool, still_failing: bool) -> usize {
    let clock = clock();
    let zero = clock.now();
    seed(pool, &clock).await;
    plant_stuck(pool, zero).await;
    run_next(pool, &clock, Environment::Test).await;
    run_next(pool, &clock, Environment::Test).await;
    assert_eq!(open_alerts(pool, zero + FIVE).await, ["high"]);
    if !still_failing {
        unstick(pool).await;
    }
    run_next(pool, &clock, Environment::Test).await;
    run_next(pool, &clock, Environment::Test).await;
    let mut conn = pool.acquire().await.expect("conn");
    let pushed = alerts_to_push(&mut conn, zero + FIVE * 3)
        .await
        .expect("push");
    pushed.iter().filter(|a| a.kind == HEALTH_ALERT).count()
}

/// TD-OPS-08's health case (OPS-023, OPS-029, D-218, D-220): failing at 0 and 5 minutes and passing
/// at 10 gives no push (the alert was acknowledged automatically).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_a_health_check_passing_at_10_minutes_never_pushes(pool: PgPool) {
    assert_eq!(health_pushes_at_15(&pool, false).await, 0);
}

/// TD-OPS-08's health case: still failing at 15 minutes gives the push.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_a_health_check_still_failing_at_15_minutes_pushes(pool: PgPool) {
    assert_eq!(health_pushes_at_15(&pool, true).await, 1);
}

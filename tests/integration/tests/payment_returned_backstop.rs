//! A dead Payment pending wait task is revived by the daily backstop (T778, D-5081 F32; JOB-006,
//! JOB-014, OPS-004, PRIV-007).
//!
//! On a clock advanced by hand: a job in Payment pending whose timer ran out has queued its
//! `picks.payment_returned` task; when that task is dead-lettered the backstop revives it once,
//! with one audit row, and with `picks`' handler the job leaves Payment pending; dead a third
//! time, it raises one High `jobs.timer_dead` alert and stays dead. A missing wait task (the
//! timer has not fired) queues nothing, and a dead one on a job that moved on is left alone. All
//! data is fake.

use std::sync::Arc;

use askhava_core::api::{Clock, Guarded, Lease, ManualClock, Registry, WorkerId, claim};
use askhava_jobs::api::{
    Backstop, JobStatus, PAYMENT_RETURNED_KIND, TIMER_DEAD_ALERT, TIMER_KIND, TIMER_REVIVED,
    create, move_to, revive_timers, run_timer,
};
use askhava_ops::api::Actor;
use askhava_picks::api::run_payment_returned;
use jiff::SignedDuration;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Monday, November 2, 2026, 10:00 in Arizona: inside business hours.
const START: &str = "2026-11-02T17:00:00Z";

/// One revival, none, and one left dead.
const ONE: Backstop = Backstop {
    revived: 1,
    left: 0,
};
const LEFT: Backstop = Backstop {
    revived: 0,
    left: 1,
};

/// A manual clock at [`START`] and a registry running the job timer and `picks`' wait handler.
fn setup() -> (Arc<ManualClock>, Registry) {
    let clock = Arc::new(ManualClock::new(START.parse().expect("time")));
    let mut registry = Registry::new();
    registry.register(TIMER_KIND, run_timer);
    let at = Arc::clone(&clock);
    registry.register(PAYMENT_RETURNED_KIND, move |pool, task| {
        run_payment_returned(pool, task, at.now())
    });
    (clock, registry)
}

/// A job moved to Payment pending (no pick) at the clock's time.
async fn payment_pending_job(pool: &PgPool, clock: &ManualClock) -> Uuid {
    let mut conn = pool.acquire().await.expect("conn");
    let id = create(&mut conn, clock.now(), false, Actor::System)
        .await
        .expect("create");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
        (JobStatus::AwaitingConfirm, JobStatus::PaymentPending),
    ] {
        let moved = move_to(&mut conn, id, step, clock.now(), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))));
    }
    id
}

/// Claims and runs every task due on `clock` with `registry`.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, clock.now(), Lease::WORKER.length, 50)
        .await
        .expect("claim");
    for task in tasks {
        let _either = registry.run(pool, clock, owner, task, Lease::WORKER).await;
    }
}

/// Lets the Payment pending timer run out, so it queues the wait task.
async fn timer_runs_out(pool: &PgPool, clock: &ManualClock) {
    let mut timer_only = Registry::new();
    timer_only.register(TIMER_KIND, run_timer);
    clock.advance(SignedDuration::from_hours(2));
    run_due(pool, &timer_only, clock).await;
}

/// Dead-letters job `id`'s wait task, as five failed tries would.
async fn kill_wait(pool: &PgPool, id: Uuid) {
    let done = sqlx::query(
        "UPDATE core.task SET dead_at = now(), attempts = max_attempts, lease_until = NULL, \
         lease_owner = NULL WHERE kind = $1 AND payload->>'job' = $2::text",
    )
    .bind(PAYMENT_RETURNED_KIND)
    .bind(id)
    .execute(pool)
    .await
    .expect("dead");
    assert_eq!(done.rows_affected(), 1, "one wait task");
}

/// Job `id`'s wait tasks: whether each is dead.
async fn waits(pool: &PgPool, id: Uuid) -> Vec<bool> {
    sqlx::query_scalar(
        "SELECT dead_at IS NOT NULL FROM core.task WHERE kind = $1 AND payload->>'job' = $2::text",
    )
    .bind(PAYMENT_RETURNED_KIND)
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("waits")
}

/// The backstop at the clock's time.
async fn backstop(pool: &PgPool, clock: &ManualClock) -> Backstop {
    let mut conn = pool.acquire().await.expect("conn");
    revive_timers(&mut conn, clock.now())
        .await
        .expect("backstop")
}

/// How many rows `sql` (one `$1` for the job, one `$2` text) counts.
async fn count(pool: &PgPool, sql: &'static str, id: Uuid, text: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .bind(text)
        .fetch_one(pool)
        .await
        .expect("count")
}

const STATUS: &str = "SELECT count(*) FROM jobs.job WHERE id = $1 AND status = $2";

/// JOB-014, PRIV-007, D-5081: a dead wait task is revived once with one audit row, and the job
/// then leaves Payment pending for Quotes ready.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_dead_wait_task_is_revived_and_the_job_leaves_payment_pending(pool: PgPool) {
    let (clock, registry) = setup();
    let id = payment_pending_job(&pool, &clock).await;
    timer_runs_out(&pool, &clock).await;
    kill_wait(&pool, id).await;
    assert_eq!(backstop(&pool, &clock).await, ONE);
    assert_eq!(waits(&pool, id).await, [false], "revived");
    let audit = "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND action = $2";
    assert_eq!(count(&pool, audit, id, TIMER_REVIVED).await, 1);
    assert_eq!(backstop(&pool, &clock).await, Backstop::default(), "once");
    let pending = JobStatus::PaymentPending.code();
    assert_eq!(count(&pool, STATUS, id, pending).await, 1, "still waiting");
    run_due(&pool, &registry, &clock).await;
    let ready = JobStatus::QuotesReady.code();
    assert_eq!(
        count(&pool, STATUS, id, ready).await,
        1,
        "left Payment pending"
    );
}

/// OPS-004, D-5081: a wait task dead after its last revival raises one High alert and is left.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_wait_task_dead_a_third_time_alerts_once_and_is_left(pool: PgPool) {
    let (clock, _) = setup();
    let id = payment_pending_job(&pool, &clock).await;
    timer_runs_out(&pool, &clock).await;
    for revival in 1..=2 {
        kill_wait(&pool, id).await;
        assert_eq!(backstop(&pool, &clock).await, ONE, "revival {revival}");
    }
    kill_wait(&pool, id).await;
    for _ in 0..2 {
        assert_eq!(backstop(&pool, &clock).await, LEFT);
    }
    let raised: Vec<(String, String, Uuid)> = sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(TIMER_DEAD_ALERT)
    .fetch_all(&pool)
    .await
    .expect("alerts");
    assert_eq!(raised, vec![("high".to_owned(), "jobs.job".to_owned(), id)]);
    assert_eq!(waits(&pool, id).await, [true], "left dead for Casey");
}

/// JOB-006, D-5081: before the timer fires the wait task is missing and nothing is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_missing_wait_task_queues_nothing(pool: PgPool) {
    let (clock, _) = setup();
    let id = payment_pending_job(&pool, &clock).await;
    assert_eq!(backstop(&pool, &clock).await, Backstop::default());
    assert!(waits(&pool, id).await.is_empty(), "nothing queued");
}

/// JOB-006, D-5081: a dead wait task of a job that moved on is left alone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_dead_wait_task_of_a_job_that_moved_on_is_left_alone(pool: PgPool) {
    let (clock, _) = setup();
    let id = payment_pending_job(&pool, &clock).await;
    timer_runs_out(&pool, &clock).await;
    kill_wait(&pool, id).await;
    let mut conn = pool.acquire().await.expect("conn");
    let step = (JobStatus::PaymentPending, JobStatus::QuotesReady);
    let moved = move_to(&mut conn, id, step, clock.now(), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "moved on");
    drop(conn);
    assert_eq!(backstop(&pool, &clock).await, Backstop::default());
    assert_eq!(waits(&pool, id).await, [true], "still dead");
}

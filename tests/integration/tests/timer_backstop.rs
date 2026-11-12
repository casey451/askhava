//! A job timer that runs out of attempts alerts and is revived by the daily backstop (T366,
//! D-2640; TD-OPS-38 for JOB-003, JOB-006, OPS-004, PRIV-007).
//!
//! On a clock advanced by hand: a Quotes ready job's timer fails 5 times (a test trigger makes
//! every status-history insert fail) and is dead-lettered; the hourly dead-letter check raises one
//! High `core.task_dead_letter` alert for its kind and day, and no second one; the backstop
//! revives it with one audit row; with the fault gone it closes the job, and the purge sweep
//! queues its three purge stages. A final job's dead timer is not revived; a timer that dies a
//! third time raises one High `jobs.timer_dead` alert and stays dead; a missing timer is queued
//! again. The dead-letter check is compiled in place from `app/server/src/no_handler_alert.rs`
//! (the layer table keeps `askhava-server` out of test crates, D-736). All data is fake.

use std::sync::Arc;

use askhava_core::api::{
    Clock, Environment, Guarded, Lease, ManualClock, Outcome, Registry, WorkerId, claim,
};
use askhava_jobs::api::{
    BACKSTOP_KIND, Backstop, JobStatus, PURGE_DUE_KIND, TIMER_DEAD_ALERT, TIMER_KIND,
    TIMER_REVIVED, create, current, move_to, revive_timers, run_timer, schedule_backstop,
    sweep_closed,
};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The hourly dead-letter check, compiled in place.
#[path = r"../../../app/server/src/no_handler_alert.rs"]
#[allow(
    dead_code,
    reason = "only run_check and the alert names are called here; the registration is the worker's"
)]
mod no_handler_alert;

/// Monday, November 2, 2026, 10:00 in Arizona: inside business hours.
const START: &str = "2026-11-02T17:00:00Z";

/// A manual clock at [`START`] and a registry running only the job timer.
fn setup() -> (Arc<ManualClock>, Registry) {
    let clock = Arc::new(ManualClock::new(START.parse().expect("time")));
    let mut registry = Registry::new();
    registry.register(TIMER_KIND, run_timer);
    (clock, registry)
}

/// A job created and moved to Quotes ready at the clock's time.
async fn quotes_ready_job(pool: &PgPool, at: Timestamp) -> Uuid {
    let mut conn = pool.acquire().await.expect("conn");
    let id = create(&mut conn, at, false, Actor::System)
        .await
        .expect("create");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(&mut conn, id, step, at, Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))));
    }
    id
}

/// Makes every job move fail (the fault) or work again.
async fn fault(pool: &PgPool, on: bool) {
    let sql = if on {
        "CREATE FUNCTION test_fault_fn() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'test fault'; END $$; \
         CREATE TRIGGER test_fault BEFORE INSERT ON jobs.status_history \
         FOR EACH ROW EXECUTE FUNCTION test_fault_fn()"
    } else {
        "DROP TRIGGER test_fault ON jobs.status_history; DROP FUNCTION test_fault_fn()"
    };
    sqlx::raw_sql(sql).execute(pool).await.expect("fault");
}

/// Claims and runs every task due on `clock`; an unregistered kind fails like any other.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, clock.now(), Lease::WORKER.length, 50)
        .await
        .expect("claim");
    for task in tasks {
        let outcome = registry.run(pool, clock, owner, task, Lease::WORKER).await;
        assert!(matches!(
            outcome,
            Ok(Outcome::Done | Outcome::Retry(_) | Outcome::Dead)
        ));
    }
}

/// Runs the due tasks 5 times, an hour apart, past every backoff.
async fn five_runs(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    for _ in 0..5 {
        run_due(pool, registry, clock).await;
        clock.advance(SignedDuration::from_hours(1));
    }
}

/// Whether job `id`'s timer for its current version is dead-lettered (none when it has no row).
async fn timer_dead(pool: &PgPool, id: Uuid) -> Option<bool> {
    let job = current(&mut pool.acquire().await.expect("conn"), id)
        .await
        .expect("job");
    let key = format!("{TIMER_KIND}.{id}.{}.{}", job.version, job.timer_version);
    sqlx::query_scalar("SELECT dead_at IS NOT NULL FROM core.task WHERE dedupe_key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .expect("timer")
}

/// The backstop at the clock's time.
async fn backstop(pool: &PgPool, clock: &ManualClock) -> Backstop {
    let mut conn = pool.acquire().await.expect("conn");
    revive_timers(&mut conn, clock.now())
        .await
        .expect("backstop")
}

/// Open alerts of `kind`: severity, subject kind and subject ID.
async fn alerts(pool: &PgPool, kind: &str) -> Vec<(String, String, Uuid)> {
    sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .expect("alerts")
}

/// How many rows `sql` (one `$1` for the job) counts.
async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// TD-OPS-38 (JOB-003, JOB-006, OPS-004, PRIV-007): dead after 5 failures with one alert, revived,
/// then closed and its purge stages queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_38_a_dead_timer_alerts_once_and_the_backstop_revives_it(pool: PgPool) {
    let (clock, registry) = setup();
    let id = quotes_ready_job(&pool, clock.now()).await;
    fault(&pool, true).await;
    clock.advance(SignedDuration::from_hours(4 * 24));
    five_runs(&pool, &registry, &clock).await;
    assert_eq!(
        timer_dead(&pool, id).await,
        Some(true),
        "dead after 5 tries"
    );
    for _ in 0..2 {
        no_handler_alert::run_check(&pool, Environment::Test, clock.now())
            .await
            .expect("check");
    }
    let day = clock.now().as_second().div_euclid(86_400);
    let one = vec![(
        "high".to_owned(),
        TIMER_KIND.to_owned(),
        no_handler_alert::day_id(TIMER_KIND, day),
    )];
    let raised = alerts(&pool, no_handler_alert::DEAD_ALERT).await;
    assert_eq!(raised, one, "one alert for the kind and the day it died");
    fault(&pool, false).await;
    clock.advance(SignedDuration::from_hours(24));
    let done = backstop(&pool, &clock).await;
    assert_eq!(
        done,
        Backstop {
            revived: 1,
            left: 0
        }
    );
    let revivals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND action = $2",
    )
    .bind(id)
    .bind(TIMER_REVIVED)
    .fetch_one(&pool)
    .await
    .expect("audit");
    assert_eq!(revivals, 1, "one audit row for the revival");
    run_due(&pool, &registry, &clock).await;
    let closed = "SELECT count(*) FROM jobs.job WHERE id = $1 AND status = 'closed'";
    assert_eq!(count(&pool, closed, id).await, 1, "the job closed");
    sweep_closed(&mut pool.acquire().await.expect("conn"), clock.now())
        .await
        .expect("sweep");
    let stages = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'job' = $1::text";
    let staged: i64 = sqlx::query_scalar(stages)
        .bind(id)
        .bind(PURGE_DUE_KIND)
        .fetch_one(&pool)
        .await
        .expect("stages");
    assert_eq!(
        staged, 3,
        "its photo, contact and text purge stages are queued"
    );
}

/// TD-OPS-38 (JOB-006): a final job's dead timer stays dead.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_38_a_final_jobs_dead_timer_is_not_revived(pool: PgPool) {
    let (clock, _) = setup();
    let mut conn = pool.acquire().await.expect("conn");
    let id = create(&mut conn, clock.now(), false, Actor::System)
        .await
        .expect("create");
    sqlx::query("UPDATE core.task SET dead_at = now() WHERE dedupe_key LIKE $1")
        .bind(format!("{TIMER_KIND}.{id}.%"))
        .execute(&mut *conn)
        .await
        .expect("dead");
    let step = (JobStatus::Intake, JobStatus::Cancelled);
    let moved = move_to(&mut conn, id, step, clock.now(), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "cancelled");
    drop(conn);
    assert_eq!(backstop(&pool, &clock).await, Backstop::default());
    let dead =
        "SELECT count(*) FROM core.task WHERE dead_at IS NOT NULL AND payload->>'job' = $1::text";
    assert_eq!(count(&pool, dead, id).await, 1, "still dead");
}

/// TD-OPS-38 (OPS-004): a timer that dies a third time raises one High alert and is left.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_38_a_timer_dead_three_times_alerts_and_is_left(pool: PgPool) {
    let (clock, registry) = setup();
    let id = quotes_ready_job(&pool, clock.now()).await;
    fault(&pool, true).await;
    clock.advance(SignedDuration::from_hours(4 * 24));
    for revival in 1..=2 {
        five_runs(&pool, &registry, &clock).await;
        let done = backstop(&pool, &clock).await;
        assert_eq!(
            done,
            Backstop {
                revived: 1,
                left: 0
            },
            "revival {revival}"
        );
    }
    five_runs(&pool, &registry, &clock).await;
    for _ in 0..2 {
        assert_eq!(
            backstop(&pool, &clock).await,
            Backstop {
                revived: 0,
                left: 1
            }
        );
    }
    let raised = alerts(&pool, TIMER_DEAD_ALERT).await;
    assert_eq!(raised, vec![("high".to_owned(), "jobs.job".to_owned(), id)]);
    assert_eq!(
        timer_dead(&pool, id).await,
        Some(true),
        "left dead for Casey"
    );
}

/// TD-OPS-38 (JOB-003): a waiting job whose timer row is gone gets it queued again, and the
/// backstop's own run is queued once a day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_38_a_missing_timer_is_queued_again(pool: PgPool) {
    let (clock, _) = setup();
    let id = quotes_ready_job(&pool, clock.now()).await;
    sqlx::query("DELETE FROM core.task WHERE kind = $1")
        .bind(TIMER_KIND)
        .execute(&pool)
        .await
        .expect("delete");
    assert_eq!(timer_dead(&pool, id).await, None);
    assert_eq!(
        backstop(&pool, &clock).await,
        Backstop {
            revived: 1,
            left: 0
        }
    );
    assert_eq!(timer_dead(&pool, id).await, Some(false), "queued again");
    let mut conn = pool.acquire().await.expect("conn");
    for _ in 0..2 {
        schedule_backstop(&mut conn, clock.now())
            .await
            .expect("schedule");
    }
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(BACKSTOP_KIND)
        .fetch_one(&pool)
        .await
        .expect("runs");
    assert_eq!(runs, 1);
}

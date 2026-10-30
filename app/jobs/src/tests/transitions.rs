//! Database cases of the job state machine (TD-JOB-01: JOB-001, JOB-002, JOB-003, JOB-005, JOB-006).
//!
//! Each move writes one history row and one audit row with its timer in the same transaction; a
//! move with no arrow or from a status the job has left changes nothing; a timer is a database
//! row that a fresh worker claims and runs, and a timer for a job that moved on, or whose wait was
//! restarted, does nothing (D-807); the app role cannot rewrite the history (postgres brain P5).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{Claimed, Guarded, WorkerId, claim};
use askhava_ops::api::{Actor, export_audit};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tracing_subscriber::layer::{Context, SubscriberExt};

use crate::api::{
    JobStatus, JobsError, TIMER_KIND, create, current, move_to, restart_timer, run_timer,
};

/// A fixed start, 2026-11-02 10:00 Arizona time (17:00 UTC).
fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

pub(super) async fn history(db: &mut PgConnection, id: Uuid) -> Vec<(String, String)> {
    sqlx::query!(
        "SELECT from_status, to_status FROM jobs.status_history WHERE job_id = $1 ORDER BY version",
        id
    )
    .fetch_all(db)
    .await
    .expect("history")
    .into_iter()
    .map(|r| (r.from_status, r.to_status))
    .collect()
}

pub(super) async fn audited_moves(db: &mut PgConnection, id: Uuid) -> Vec<(String, String)> {
    let rows = export_audit(db, 0, 1000).await.expect("audit");
    rows.into_iter()
        .filter(|r| r.subject_id == id && r.action == "status_changed")
        .filter_map(|r| r.from_status.zip(r.to_status))
        .collect()
}

/// Claims every due job timer as a fresh worker would after a restart (JOB-006).
async fn due_timers(pool: &PgPool, now: Timestamp) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let claimed = claim(pool, owner, now, lease, 100).await.expect("claim");
    claimed
        .into_iter()
        .filter(|t| t.kind == TIMER_KIND)
        .collect()
}

/// TD-JOB-01: a job walked along the arrows holds one status at a time, and every move writes
/// exactly one history row and one audit row (JOB-001, JOB-005).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_move_writes_one_history_and_one_audit_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let path = JobStatus::ALL[..8]
        .windows(2)
        .filter(|w| w[0].can_move_to(w[1]));
    let moves: Vec<_> = path.map(|w| (w[0], w[1])).collect();
    assert_eq!(moves.len(), 7, "Intake to Done is seven arrows");
    for &step in &moves {
        let done = move_to(&mut db, id, step, start(), Actor::System).await;
        assert_eq!(done, Ok(Guarded::Applied(())));
        assert_eq!(
            current(&mut db, id).await.expect("job").status.code(),
            step.1.code()
        );
    }
    let codes: Vec<_> = moves
        .iter()
        .map(|(f, t)| (f.code().to_owned(), t.code().to_owned()))
        .collect();
    assert_eq!(history(&mut db, id).await, codes);
    assert_eq!(audited_moves(&mut db, id).await, codes);
}

/// Counts the error-level events logged while it is the default subscriber.
struct Errors(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Errors {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// TD-JOB-01: a move with no arrow is refused, logged once as a bug and writes nothing; a move from a status the job has
/// left applies to nothing (JOB-002, JOB-004).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refused_or_stale_move_changes_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let errors = Arc::new(AtomicUsize::new(0));
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(Errors(Arc::clone(&errors))),
    );
    let skip = (JobStatus::Intake, JobStatus::Connected);
    let refused = move_to(&mut db, id, skip, start(), Actor::System).await;
    drop(guard);
    assert_eq!(refused, Err(JobsError::RefusedMove("intake", "connected")));
    assert_eq!(errors.load(Ordering::SeqCst), 1, "logged once as a bug");
    let stale = (JobStatus::Open, JobStatus::QuotesReady);
    let moved_on = move_to(&mut db, id, stale, start(), Actor::System).await;
    assert_eq!(moved_on, Ok(Guarded::MovedOn));
    assert_eq!(current(&mut db, id).await.expect("job").version, 0);
    assert!(history(&mut db, id).await.is_empty());
    assert!(audited_moves(&mut db, id).await.is_empty());
}

/// TD-JOB-01: a timer is a database row a fresh worker claims after a restart; at 6 p.m. Arizona
/// time the next day an Open job becomes No quotes yet, and the next status's timer is queued
/// (JOB-003, JOB-006, JOB-010).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_timer_survives_a_restart_and_moves_a_waiting_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    move_to(&mut db, id, open, start(), Actor::System)
        .await
        .expect("open")
        .applied()
        .expect("applied");
    let six_pm: Timestamp = "2026-11-04T01:00:00Z".parse().expect("instant");
    let early = due_timers(&pool, six_pm - SignedDuration::from_secs(1)).await;
    assert!(
        early.is_empty(),
        "nothing is due before 6 p.m. the next day"
    );
    let due = due_timers(&pool, six_pm).await;
    assert_eq!(
        due.len(),
        1,
        "only the Open timer; Intake's was for version 0"
    );
    run_timer(pool.clone(), due[0].clone()).await.expect("run");
    let job = current(&mut db, id).await.expect("job");
    assert_eq!(job.status.code(), "no_quotes_yet");
    assert_eq!(job.status_at, six_pm);
    let five_days = six_pm + SignedDuration::from_hours(5 * 24);
    let next = due_timers(&pool, five_days).await;
    let next = next
        .iter()
        .filter(|t| t.payload["status"] == "no_quotes_yet");
    assert_eq!(next.count(), 1);
}

/// TD-JOB-01: a timer that fires after the job moved on does nothing, and running a timer twice
/// moves the job once (JOB-006).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_timer_for_a_job_that_moved_on_does_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let three_days = start() + SignedDuration::from_hours(72);
    let intake_timer = due_timers(&pool, three_days).await;
    assert_eq!(intake_timer.len(), 1);
    let open = (JobStatus::Intake, JobStatus::Open);
    move_to(&mut db, id, open, start(), Actor::System)
        .await
        .expect("open")
        .applied()
        .expect("applied");
    run_timer(pool.clone(), intake_timer[0].clone())
        .await
        .expect("stale run");
    assert_eq!(
        current(&mut db, id).await.expect("job").status.code(),
        "open"
    );
    let quote = (JobStatus::Open, JobStatus::QuotesReady);
    move_to(&mut db, id, quote, start(), Actor::System)
        .await
        .expect("quote")
        .applied()
        .expect("applied");
    let timers = due_timers(&pool, three_days).await;
    assert_eq!(
        timers.len(),
        2,
        "the stale Open timer and the Quotes ready timer"
    );
    for task in timers.iter().chain(&timers) {
        run_timer(pool.clone(), task.clone()).await.expect("run");
    }
    assert_eq!(
        current(&mut db, id).await.expect("job").status.code(),
        "closed"
    );
    assert_eq!(history(&mut db, id).await.len(), 3);
}

/// TD-JOB-01: a restarted wait counts from the restart (INT-019's last reply, D-807): the timer
/// from entry does nothing, the new one closes the job 72 hours after the restart, and a restart
/// to an earlier instant changes nothing (JOB-003, JOB-006).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_restarted_wait_supersedes_the_pending_timer(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let reply = start() + SignedDuration::from_hours(48);
    let restart = restart_timer(&mut db, id, (JobStatus::Intake, reply)).await;
    assert_eq!(restart, Ok(Guarded::Applied(())));
    let earlier = restart_timer(&mut db, id, (JobStatus::Intake, start())).await;
    assert_eq!(
        earlier,
        Ok(Guarded::MovedOn),
        "a restart never shortens the wait"
    );
    let elsewhere = restart_timer(&mut db, id, (JobStatus::Open, reply)).await;
    assert_eq!(elsewhere, Ok(Guarded::MovedOn), "the job is not Open");
    let old = due_timers(&pool, start() + SignedDuration::from_hours(72)).await;
    assert_eq!(old.len(), 1, "only the timer from entry is due");
    run_timer(pool.clone(), old[0].clone())
        .await
        .expect("stale run");
    assert_eq!(
        current(&mut db, id).await.expect("job").status.code(),
        "intake"
    );
    let due = reply + SignedDuration::from_hours(72);
    let new: Vec<_> = due_timers(&pool, due)
        .await
        .into_iter()
        .filter(|t| t.payload["timer"] == 1)
        .collect();
    assert_eq!(
        new.len(),
        1,
        "the restarted timer is due 72 hours after the reply"
    );
    run_timer(pool.clone(), new[0].clone()).await.expect("run");
    let job = current(&mut db, id).await.expect("job");
    assert_eq!((job.status.code(), job.status_at), ("closed_early", due));
}

/// TD-JOB-01: the app role cannot update, delete or truncate `jobs.status_history`; it stays
/// append-only (JOB-005, postgres brain P5).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_app_role_cannot_rewrite_the_history(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, id, open, start(), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("set role");
    let update = sqlx::query!("UPDATE jobs.status_history SET to_status = 'done'")
        .execute(&mut *db)
        .await;
    assert!(update.is_err(), "the app role updated the history");
    let delete = sqlx::query!("DELETE FROM jobs.status_history")
        .execute(&mut *db)
        .await;
    assert!(delete.is_err(), "the app role deleted the history");
    let truncate = sqlx::query!("TRUNCATE jobs.status_history")
        .execute(&mut *db)
        .await;
    assert!(truncate.is_err(), "the app role truncated the history");
    assert_eq!(history(&mut db, id).await.len(), 1);
}

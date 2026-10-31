//! Same-moment collisions on one job, resolved by the row lock and the expected-status check
//! (TD-JOB-02: JOB-004; the twin's `races` scenario as oracle: one wins, the other applies to
//! nothing).
//!
//! Each staged race makes both orders certain: a third session holds the job's row lock, the
//! first event starts and waits on it, then the second, and only then is the lock let go, so the
//! first in line wins. The loser must change nothing: no second history or audit row, no timer or
//! text of its own, and the job left in the winner's status. The last case runs two moves with no
//! staging, many times over. All data is fake.

use std::time::Duration;

use askhava_core::api::{Claimed, Guarded, WorkerId, claim};
use askhava_jobs::api::rules::timeout;
use askhava_jobs::api::{JobStatus, TEXT_KIND, TIMER_KIND, create, current, move_to, run_timer};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// 2026-11-02 10:00 Arizona time (17:00 UTC).
fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

/// An hour after the start, inside every wait.
fn soon() -> Timestamp {
    start() + SignedDuration::from_hours(1)
}

/// When the wait in `status`, entered at the start, times out.
fn due(status: JobStatus) -> Timestamp {
    timeout(status, start(), false)
        .expect("a waiting status")
        .at
}

/// One of two colliding events: a move by a person or a vendor notice, or the job's live timer.
#[derive(Clone, Copy)]
enum Event {
    Move(JobStatus, JobStatus),
    Timer,
}

/// An event ready to run: a timer is claimed first, as a worker would.
enum Ready {
    Move(JobStatus, JobStatus),
    Timer(Claimed),
}

/// What a race leaves behind on one job, read before and after.
#[derive(Debug, PartialEq, Eq)]
struct Trace {
    status: &'static str,
    version: i32,
    history: i64,
    audits: i64,
    texts: i64,
}

/// Each order's winner: the status it moves to and the text events it queues.
type Wins = ((JobStatus, i64), (JobStatus, i64));

/// Creates a job at the start and walks it, at the start, along `steps` from Intake.
async fn job_in(pool: &PgPool, steps: &[JobStatus]) -> Uuid {
    let mut db = pool.acquire().await.expect("connection");
    let id = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let mut from = JobStatus::Intake;
    for &to in steps {
        let moved = move_to(&mut db, id, (from, to), start(), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
        from = to;
    }
    id
}

/// Runs a counting query over job `id` and kind `kind` (`$1`, `$2`).
async fn count(pool: &PgPool, sql: &'static str, id: Uuid, kind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Reads job `id`'s status and version and counts its history rows, `status_changed` audit rows
/// and queued text events, so a race's effects can be compared before and after.
async fn trace(pool: &PgPool, id: Uuid) -> Trace {
    let mut db = pool.acquire().await.expect("connection");
    let job = current(&mut db, id).await.expect("job");
    let history = "SELECT count(*) FROM jobs.status_history WHERE job_id = $1";
    let history: i64 = sqlx::query_scalar(history)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect(history);
    let audits = "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND action = $2";
    let texts = "SELECT count(*) FROM core.task WHERE payload->>'job' = $1::text AND kind = $2";
    Trace {
        status: job.status.code(),
        version: job.version,
        history,
        audits: count(pool, audits, id, "status_changed").await,
        texts: count(pool, texts, id, TEXT_KIND).await,
    }
}

/// The statuses of the timer tasks queued for job `id` at `version`.
async fn timers_at(pool: &PgPool, id: Uuid, version: i32) -> Vec<String> {
    let sql = "SELECT payload->>'status' FROM core.task WHERE kind = $3 \
               AND payload->>'job' = $1::text AND (payload->>'version')::int = $2";
    sqlx::query_scalar(sql)
        .bind(id)
        .bind(version)
        .bind(TIMER_KIND)
        .fetch_all(pool)
        .await
        .expect("timers")
}

/// Makes `event` ready on job `id` at `at`: the job's live timer (its current version; earlier
/// statuses' stale timers are due too) is claimed as a worker would claim it.
async fn ready(pool: &PgPool, id: Uuid, event: Event, at: Timestamp) -> Ready {
    let Event::Move(from, to) = event else {
        let version = trace(pool, id).await.version;
        let owner = WorkerId::new(pool).await.expect("worker");
        let lease = SignedDuration::from_mins(5);
        let claimed = claim(pool, owner, at, lease, 100).await.expect("claim");
        let live = |t: &Claimed| {
            t.kind == TIMER_KIND
                && t.payload["job"] == id.to_string()
                && t.payload["version"] == version
        };
        return Ready::Timer(claimed.into_iter().find(live).expect("the live timer"));
    };
    Ready::Move(from, to)
}

/// Runs one event on its own connection at `at`; a move answers whether it applied.
async fn run(pool: PgPool, id: Uuid, event: Ready, at: Timestamp) -> Option<bool> {
    match event {
        Ready::Move(from, to) => {
            let mut db = pool.acquire().await.expect("connection");
            let actor = Actor::Homeowner(Uuid::new_v4());
            let moved = move_to(&mut db, id, (from, to), at, actor).await;
            Some(moved.expect("a raced move is no error").applied().is_some())
        }
        Ready::Timer(task) => {
            run_timer(pool, task)
                .await
                .expect("a raced timer is no error");
            None
        }
    }
}

/// True once `n` sessions in this test's database wait on a lock, polling for up to 10 seconds.
async fn lock_waiters(pool: &PgPool, n: i64) -> bool {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("activity");
        if waiting >= n {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Stages `first` and `second` on job `id` at `at` so that `first` gets the row lock first.
async fn race(pool: &PgPool, id: Uuid, (first, second): (Ready, Ready), at: Timestamp) {
    let mut holder = pool.begin().await.expect("holder");
    sqlx::query("SELECT 1 FROM jobs.job WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .expect("row lock");
    let a = tokio::spawn(run(pool.clone(), id, first, at));
    assert!(lock_waiters(pool, 1).await, "the first event waits");
    let b = tokio::spawn(run(pool.clone(), id, second, at));
    assert!(lock_waiters(pool, 2).await, "the second event waits");
    holder.commit().await.expect("release");
    // A timer answers `None` (`run_timer` reports no outcome), so these two checks bind only
    // moves; a timer's win or loss is checked afterwards by `one_applied`'s full trace.
    assert_ne!(a.await.expect("first"), Some(false), "the first applies");
    assert_ne!(
        b.await.expect("second"),
        Some(true),
        "the second applies to nothing"
    );
}

/// Checks the race left exactly the winner's one move to `won`, its `texts` new text events and
/// its timer alone at the new version (JOB-004, JOB-005, JOB-006).
async fn one_applied(pool: &PgPool, id: Uuid, before: &Trace, (won, texts): (JobStatus, i64)) {
    let expected = Trace {
        status: won.code(),
        version: before.version + 1,
        history: before.history + 1,
        audits: before.audits + 1,
        texts: before.texts + texts,
    };
    assert_eq!(trace(pool, id).await, expected);
    let timer: Vec<String> = won
        .wait()
        .map(|_| won.code().to_owned())
        .into_iter()
        .collect();
    assert_eq!(timers_at(pool, id, expected.version).await, timer);
}

/// Races `a` and `b` at `at` in both orders, each on a new job walked along `steps`.
async fn both_orders(
    pool: &PgPool,
    steps: &[JobStatus],
    (a, b, at): (Event, Event, Timestamp),
    wins: Wins,
) {
    for (first, second, won) in [(a, b, wins.0), (b, a, wins.1)] {
        let id = job_in(pool, steps).await;
        let pair = (
            ready(pool, id, first, at).await,
            ready(pool, id, second, at).await,
        );
        let before = trace(pool, id).await;
        race(pool, id, pair, at).await;
        one_applied(pool, id, &before, won).await;
    }
}

const TO_AWAITING: [JobStatus; 3] = [
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
];
const TO_PAYMENT: [JobStatus; 4] = [
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
    JobStatus::PaymentPending,
];

/// TD-JOB-02: the homeowner's cancel and the business's Confirm land together, in both orders.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_02_a_cancel_and_a_confirm_in_both_orders(pool: PgPool) {
    let cancel = Event::Move(JobStatus::AwaitingConfirm, JobStatus::Cancelled);
    let confirm = Event::Move(JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let wins = ((JobStatus::Cancelled, 0), (JobStatus::PaymentPending, 0));
    both_orders(&pool, &TO_AWAITING, (cancel, confirm, soon()), wins).await;
}

/// TD-JOB-02: the Stripe payment notice and the payment deadline land together, in both orders;
/// a lost deadline returns nothing to Quotes ready, a lost notice connects nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_02_a_stripe_notice_and_the_payment_deadline_in_both_orders(pool: PgPool) {
    let paid = Event::Move(JobStatus::PaymentPending, JobStatus::Connected);
    let at = due(JobStatus::PaymentPending);
    let wins = ((JobStatus::Connected, 0), (JobStatus::QuotesReady, 0));
    both_orders(&pool, &TO_PAYMENT, (paid, Event::Timer, at), wins).await;
}

/// TD-JOB-02: a pick and the 3-day no-pick close land together, in both orders; only a winning
/// close queues its closing text (JOB-012).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_02_a_pick_and_the_no_pick_close_in_both_orders(pool: PgPool) {
    let pick = Event::Move(JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    let at = due(JobStatus::QuotesReady);
    let wins = ((JobStatus::AwaitingConfirm, 0), (JobStatus::Closed, 1));
    both_orders(&pool, &TO_AWAITING[..2], (pick, Event::Timer, at), wins).await;
}

/// TD-JOB-02: two picks land together; exactly one makes the move.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_02_two_picks_one_applies(pool: PgPool) {
    let pick = Event::Move(JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    let wins = (
        (JobStatus::AwaitingConfirm, 0),
        (JobStatus::AwaitingConfirm, 0),
    );
    both_orders(&pool, &TO_AWAITING[..2], (pick, pick, soon()), wins).await;
}

/// TD-JOB-02: a cancel and a Confirm with no staging, twenty times: each time exactly one
/// applies, the other answers "moved on", and the job holds the winner's status.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_02_unstaged_races_apply_exactly_once(pool: PgPool) {
    for _ in 0..20 {
        let id = job_in(&pool, &TO_AWAITING).await;
        let before = trace(&pool, id).await;
        let pool = &pool;
        let go = |to| async move {
            let mut db = pool.acquire().await.expect("connection");
            let moved = move_to(
                &mut db,
                id,
                (JobStatus::AwaitingConfirm, to),
                soon(),
                Actor::System,
            )
            .await;
            (to, moved.expect("a raced move is no error"))
        };
        let (a, b) = tokio::join!(go(JobStatus::Cancelled), go(JobStatus::PaymentPending));
        let won = match (a, b) {
            ((to, Guarded::Applied(())), (_, Guarded::MovedOn))
            | ((_, Guarded::MovedOn), (to, Guarded::Applied(()))) => to,
            _ => panic!("exactly one of two raced moves must apply"),
        };
        one_applied(pool, id, &before, (won, 0)).await;
    }
}

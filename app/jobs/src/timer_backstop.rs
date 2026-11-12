//! The daily job timer backstop (T366, D-2640; JOB-003, JOB-006, OPS-004, PRIV-007).
//!
//! A job timer (`jobs.timeout`) that fails on every attempt is dead-lettered by `core`, and its
//! dedupe key (`jobs.timeout.<job>.<version>.<timer>`) stays on the dead row, so it can never be
//! queued again: the job would wait in its status for good and, never reaching a final status,
//! never be swept for its purge stages (PRIV-007). Once a day [`BACKSTOP_KIND`] looks at every
//! non-final job's current timer (the key of its version and timer version) and:
//! - dead and revived fewer than [`MOST_REVIVALS`] times: revives it in place
//!   (`core::api::revive_dead`, one statement on that row, due now) with one
//!   [`TIMER_REVIVED`] audit row of IDs, in one transaction;
//! - missing (a waiting status with no timer row): queues it again, with the same audit row;
//! - dead after [`MOST_REVIVALS`] revivals (it died 3 times): raises one High
//!   [`TIMER_DEAD_ALERT`] about the job, unless one is open, and leaves it for Casey;
//! - waiting, running or done: nothing.
//!
//! A final job is never looked at, so its old timers stay dead. At most [`BATCH`] timers are
//! revived or re-queued per run; the rest wait for the next day. A revived timer re-checks the job
//! when it runs (`run_timer`), so a timer revived after the job moved on does nothing.

use askhava_core::api::{HandlerError, NewTask, Revival, TaskError, enqueue, revive_dead};
use askhava_ops::api::{Actor, AuditEntry, Change, Severity, audit, raise_alert_once};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{self, JobRow, JobsError};
use crate::rules::timeout;
use crate::service::{queue_timer, timer_key};
use crate::status::JobStatus;

/// The `core.task` kind of the daily backstop.
pub const BACKSTOP_KIND: &str = "jobs.timer_backstop";
/// The audit action of one revived or re-queued timer; its subject is the job (OPS-004).
pub const TIMER_REVIVED: &str = "jobs.timer_revived";
/// The High alert raised for a job whose timer died a third time; Casey decides (OPS-004).
pub const TIMER_DEAD_ALERT: &str = "jobs.timer_dead";
/// How many times one timer is revived; it is left dead and alerted when it dies again.
pub const MOST_REVIVALS: i64 = 2;
/// The most timers revived or re-queued in one run.
pub const BATCH: usize = 50;
/// The audit and alert subject kind of a job.
const SUBJECT: &str = "jobs.job";
/// How often the backstop runs.
const DAY: SignedDuration = SignedDuration::from_hours(24);
/// The handler's failure sentence (D-491).
const FAILED: HandlerError = HandlerError("the job timer backstop database call failed");

/// What one backstop run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backstop {
    /// Timers revived in place or queued again.
    pub revived: usize,
    /// Timers that died after their last revival, alerted and left dead.
    pub left: usize,
}

/// JOB-006: queues the backstop for the day after `now`, once per day (dedupe key by day number).
///
/// # Errors
/// [`TaskError::Db`].
pub async fn schedule_backstop(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now.checked_add(DAY).map_err(|_| TaskError::Db)?;
    let day = run_at.as_second().div_euclid(86_400);
    let mut task = NewTask::new(BACKSTOP_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{BACKSTOP_KIND}.{day}"));
    enqueue(db, &task).await.map(|_| ())
}

/// The [`BACKSTOP_KIND`] handler (JOB-003, JOB-006, D-2640): queues tomorrow's run, then
/// [`revive_timers`] at `now`.
///
/// # Errors
/// A fixed [`HandlerError`] when the database cannot be reached.
pub async fn run_backstop(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_backstop(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    revive_timers(&mut conn, now).await.map_err(|_| FAILED)?;
    Ok(())
}

/// JOB-003, JOB-006, OPS-004, PRIV-007 (D-2640): revives the dead or missing current timer of
/// each non-final job, at most [`BATCH`], and alerts on each that died after its last revival.
///
/// # Errors
/// [`JobsError::Database`].
pub async fn revive_timers(db: &mut PgConnection, now: Timestamp) -> Result<Backstop, JobsError> {
    let finals: Vec<String> = JobStatus::ALL
        .into_iter()
        .filter(|status| status.is_final())
        .map(|status| status.code().to_owned())
        .collect();
    let ids = sqlx::query_scalar!(
        "SELECT id FROM jobs.job WHERE status <> ALL($1) ORDER BY id",
        &finals
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| JobsError::Database)?;
    let mut done = Backstop::default();
    for id in ids {
        if done.revived >= BATCH {
            break;
        }
        let job = models::load(db, id).await?;
        match check_timer(db, id, &job, now).await? {
            Some(true) => done.revived += 1,
            Some(false) => done.left += 1,
            None => {}
        }
    }
    Ok(done)
}

/// One job's current timer: `Some(true)` revived or re-queued, `Some(false)` left dead and
/// alerted, `None` nothing to do (JOB-006, OPS-004).
async fn check_timer(
    db: &mut PgConnection,
    id: Uuid,
    job: &JobRow,
    now: Timestamp,
) -> Result<Option<bool>, JobsError> {
    if timeout(job.status, job.timer_from, job.emergency).is_none() {
        return Ok(None);
    }
    let key = timer_key(id, job.version, job.timer_version);
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let found = revive_dead(&mut tx, &key, now, MOST_REVIVALS)
        .await
        .map_err(|_| JobsError::Database)?;
    match found {
        Revival::Alive => return Ok(None),
        Revival::Exhausted => {
            let raised = raise_alert_once(&mut tx, TIMER_DEAD_ALERT, Severity::High, (SUBJECT, id))
                .await
                .map_err(|_| JobsError::Database)?;
            tx.commit().await.map_err(|_| JobsError::Database)?;
            if raised.is_some() {
                tracing::error!(job = %id, "a job timer died after its last revival");
            }
            return Ok(Some(false));
        }
        Revival::Missing => {
            let fenced = (job.version, job.timer_version, job.emergency);
            queue_timer(&mut tx, id, job.status, fenced, job.timer_from).await?;
        }
        Revival::Revived(_) => {}
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: TIMER_REVIVED,
        subject_kind: SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(Some(true))
}

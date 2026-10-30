//! Job lifecycle orchestration: create a job, move it along an arrow, and run its timers
//! (JOB-002, JOB-003, JOB-005, JOB-006; rust brain R11, R12).
//!
//! A move, its `jobs.status_history` row, its audit row and the next status's timer commit in one
//! transaction (R11). A timer is a `core.task` of kind [`TIMER_KIND`] carrying the job ID, status and
//! version; when it fires it re-checks all three and does nothing if the job moved on (JOB-006).

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{self, JobsError};
use crate::rules::{check_move, timeout};
use crate::status::JobStatus;

/// The `core.task` kind of a job status timer (JOB-006).
pub const TIMER_KIND: &str = "jobs.timeout";
/// The audit subject kind of a job (PRIV-028).
const SUBJECT: &str = "jobs.job";
/// The fixed handler error sentences (D-491).
const BAD_PAYLOAD: &str = "bad jobs timer payload";
const DB: &str = "the jobs database statement failed";

/// Creates a job in Intake at `at`, audited, with its Intake timer queued (JOB-001, JOB-003,
/// JOB-005). Returns the job's ID.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn create(
    db: &mut PgConnection,
    at: Timestamp,
    emergency: bool,
    actor: Actor,
) -> Result<Uuid, JobsError> {
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let id = models::insert_job(&mut tx, at, emergency).await?;
    let entry = AuditEntry {
        actor,
        action: "created",
        subject_kind: SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    queue_timer(&mut tx, id, (JobStatus::Intake, 0), at, emergency).await?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(id)
}

/// Moves job `id` from `from` to `to` at `at` (JOB-002, JOB-004, JOB-005, JOB-006). A move with no
/// arrow is refused and logged as a bug before anything is read or written. If the job is no
/// longer in `from`, nothing changes and the result is [`Guarded::MovedOn`]. Otherwise the status,
/// the history row, the audit row and the new status's timer commit together.
///
/// # Errors
///
/// [`JobsError::RefusedMove`] or [`JobsError::Database`].
pub async fn move_to(
    db: &mut PgConnection,
    id: Uuid,
    (from, to): (JobStatus, JobStatus),
    at: Timestamp,
    actor: Actor,
) -> Result<Guarded<()>, JobsError> {
    apply(db, id, (from, None, to), at, actor).await
}

/// The move itself, optionally fenced on the job's version (a timer's, JOB-006).
async fn apply(
    db: &mut PgConnection,
    id: Uuid,
    (from, version, to): (JobStatus, Option<i32>, JobStatus),
    at: Timestamp,
    actor: Actor,
) -> Result<Guarded<()>, JobsError> {
    if let Err(refused) = check_move(from, to) {
        let (from, to) = (refused.from.code(), refused.to.code());
        tracing::error!(job = %id, from, to, "bug: a job move with no arrow was refused");
        return Err(JobsError::RefusedMove(from, to));
    }
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let Some((version, emergency)) =
        models::guarded_move(&mut tx, id, (from, version), to, at).await?
    else {
        return Ok(Guarded::MovedOn);
    };
    models::insert_history(&mut tx, id, version, (from, to), at).await?;
    let entry = AuditEntry::status_change(actor, (SUBJECT, id), from.code(), to.code());
    audit(&mut tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    queue_timer(&mut tx, id, (to, version), at, emergency).await?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(Guarded::Applied(()))
}

/// Queues the timeout of `status` entered at `at`, if it waits for anything (JOB-003, JOB-006).
/// The dedupe key names the job and version, so a retry never queues it twice.
async fn queue_timer(
    db: &mut PgConnection,
    id: Uuid,
    (status, version): (JobStatus, i32),
    at: Timestamp,
    emergency: bool,
) -> Result<(), JobsError> {
    let Some(due) = timeout(status, at, emergency) else {
        return Ok(());
    };
    let payload = json!({"job": id, "status": status.code(), "version": version});
    let mut task = NewTask::new(TIMER_KIND, payload, due.at);
    task.dedupe_key = Some(format!("{TIMER_KIND}.{id}.{version}"));
    enqueue(db, &task).await.map_err(|_| JobsError::Database)?;
    Ok(())
}

/// Runs one job timer (JOB-003, JOB-006): re-checks the job's status and version first and does
/// nothing if it moved on; otherwise moves it to its timeout's target at the timer's due time,
/// as the system. Safe to run twice: the second run finds the version moved on.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a database failure.
pub async fn run_timer(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let p = &task.payload;
    let id = p["job"].as_str().and_then(|s| Uuid::parse_str(s).ok());
    let status = p["status"].as_str().and_then(JobStatus::from_code);
    let version = p["version"].as_i64().and_then(|v| i32::try_from(v).ok());
    let (Some(id), Some(status), Some(version)) = (id, status, version) else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut db = pool.acquire().await.map_err(|_| HandlerError(DB))?;
    let job = models::load(&mut db, id)
        .await
        .map_err(|_| HandlerError(DB))?;
    // A move bumps the version, so an equal version means the job never left `status`; the
    // guarded update re-checks both under the row lock.
    let due = timeout(status, job.status_at, job.emergency);
    let (true, Some(due)) = (job.version == version, due) else {
        return Ok(());
    };
    let step = (status, Some(version), due.then);
    match apply(&mut db, id, step, due.at, Actor::System).await {
        Ok(_) => Ok(()),
        Err(_) => Err(HandlerError(DB)),
    }
}

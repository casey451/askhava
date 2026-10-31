//! Job lifecycle orchestration: create a job, move it along an arrow, and run its timers
//! (JOB-002, JOB-003, JOB-005, JOB-006; rust brain R11, R12).
//!
//! A move, its `jobs.status_history` row, its audit row and the next status's timer commit in one
//! transaction (R11). A timer is a `core.task` of kind [`TIMER_KIND`] carrying the job ID, status and
//! version; when it fires it re-checks all three and does nothing if the job moved on (JOB-006).
//! A later task that must count a wait from a later event (a reply, a decline, a visit date, the end
//! of a HOLD or a pause) calls [`restart_timer`], which supersedes the pending timer (D-807).
//! Every job gets its number in the transaction that creates it and keeps it for ever; after a
//! restore the replay raises the counter with [`raise_number_floor`] (JOB-024, D-851).
//!
//! Same-moment collisions (JOB-004, TD-JOB-02): a move's first statement is the guarded
//! `UPDATE … WHERE status = from`, which takes the job's row lock. Two events that try to move one
//! job at once (a pick and a close, two picks, a timer and a reply, a cancel and a Confirm) queue
//! on that lock; when the first commits, Postgres re-checks the second's `WHERE` against the
//! committed row, finds the expected status gone and updates nothing, so the second writes no
//! history, audit row, timer or text and answers [`Guarded::MovedOn`]. A move that lost never
//! retries from the job's new status; the caller decides what "moved on" means for it.
//! `tests/integration/tests/job_races.rs` stages every pair in both orders.

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::handlers;
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

/// Creates a job in Intake at `at`, numbered, audited, with its Intake timer queued (JOB-001,
/// JOB-003, JOB-005, JOB-024). Returns the job's ID; [`job_number`] reads its number.
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
    issue_number(&mut tx, id, at).await?;
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
    queue_timer(&mut tx, id, JobStatus::Intake, (0, 0, emergency), at).await?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(id)
}

/// Takes the next job number and gives it to job `id` for good, in the caller's transaction
/// (JOB-024). The counter row's lock orders concurrent creates, and `jobs.job_number` is
/// append-only with the number as its primary key, so a number already issued fails the create
/// rather than being given to anyone else. If the counter row is missing, nothing is inserted and
/// the create fails with [`JobsError::Database`] instead of leaving the job unnumbered.
async fn issue_number(db: &mut PgConnection, id: Uuid, at: Timestamp) -> Result<(), JobsError> {
    let done = sqlx::query!(
        "WITH taken AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n) \
         INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n, $1, $2 FROM taken",
        id,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| JobsError::Database)?;
    if done.rows_affected() == 1 {
        Ok(())
    } else {
        Err(JobsError::Database)
    }
}

/// Job `id`'s number, the one its texts carry (JOB-024).
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn job_number(db: &mut PgConnection, id: Uuid) -> Result<i64, JobsError> {
    sqlx::query_scalar!("SELECT number FROM jobs.job_number WHERE job_id = $1", id)
        .fetch_optional(db)
        .await
        .map_err(|_| JobsError::Database)?
        .ok_or(JobsError::NotFound)
}

/// After a restore, makes every later job number higher than `highest_texted`, the highest job
/// number the replay found in the vendors' sent-message records (JOB-024, OPS-015, D-851). The
/// restored counter only moves up, so a number texted after the backup is never issued again.
/// The replay (T110) calls it before the app takes new jobs.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn raise_number_floor(
    db: &mut PgConnection,
    highest_texted: i64,
) -> Result<(), JobsError> {
    sqlx::query!(
        "UPDATE jobs.job_number_next SET next = GREATEST(next, $1::bigint + 1)",
        highest_texted
    )
    .execute(db)
    .await
    .map_err(|_| JobsError::Database)?;
    Ok(())
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
    apply(db, id, (from, None, to), (at, actor), None).await
}

/// The move itself, optionally fenced on the job's version and timer version (a timer's, JOB-006).
/// `text` is the homeowner text the move sends, by template ID; its event commits with the move,
/// so a move that does not apply sends nothing (JOB-010 to JOB-012, JOB-025, R11). The guarded
/// update is the transaction's first statement, so the row lock is the first lock a move takes and
/// two racing moves never wait on each other in opposite orders; the loser rolls back having
/// written nothing (JOB-004).
pub(crate) async fn apply(
    db: &mut PgConnection,
    id: Uuid,
    (from, fence, to): (JobStatus, Option<(i32, i32)>, JobStatus),
    (at, actor): (Timestamp, Actor),
    text: Option<&'static str>,
) -> Result<Guarded<()>, JobsError> {
    if let Err(refused) = check_move(from, to) {
        let (from, to) = (refused.from.code(), refused.to.code());
        tracing::error!(job = %id, from, to, "bug: a job move with no arrow was refused");
        return Err(JobsError::RefusedMove(from, to));
    }
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let Some(fenced) = models::guarded_move(&mut tx, id, (from, fence), to, at).await? else {
        return Ok(Guarded::MovedOn);
    };
    models::insert_history(&mut tx, id, fenced.0, (from, to), at).await?;
    let entry = AuditEntry::status_change(actor, (SUBJECT, id), from.code(), to.code());
    audit(&mut tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    queue_timer(&mut tx, id, to, fenced, at).await?;
    if let Some(template) = text {
        let number = job_number(&mut tx, id).await?;
        let values = json!({ "job": number });
        handlers::queue_text(&mut tx, id, (template, fenced.0), values, at).await?;
    }
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(Guarded::Applied(()))
}

/// Restarts job `id`'s wait in `status` so it counts from `from` (D-807): INT-019's last reply,
/// JOB-014's decline, JOB-015's later of release and visit, the end of a HOLD or away time
/// (PICK-043, BIZ-034) or a pause (FEE-018, FEE-038, as `timer_from` plus the paused time). The
/// pending timer is superseded and does nothing; a new one is queued in the same transaction. A
/// `from` before the current start never shortens the wait.
///
/// # Errors
///
/// [`JobsError::Database`]. [`Guarded::MovedOn`] when the job left `status` or `from` is earlier.
pub async fn restart_timer(
    db: &mut PgConnection,
    id: Uuid,
    (status, from): (JobStatus, Timestamp),
) -> Result<Guarded<()>, JobsError> {
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let Some(fenced) = models::restart_wait(&mut tx, id, status, from).await? else {
        return Ok(Guarded::MovedOn);
    };
    queue_timer(&mut tx, id, status, fenced, from).await?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(Guarded::Applied(()))
}

/// Queues the timeout of `status` counted from `from`, if it waits for anything (JOB-003,
/// JOB-006). The dedupe key names the job, version and timer version, so a retry never queues it
/// twice.
async fn queue_timer(
    db: &mut PgConnection,
    id: Uuid,
    status: JobStatus,
    (version, timer, emergency): models::Fenced,
    from: Timestamp,
) -> Result<(), JobsError> {
    let Some(due) = timeout(status, from, emergency) else {
        return Ok(());
    };
    let payload = json!({"job": id, "status": status.code(), "version": version, "timer": timer});
    let mut task = NewTask::new(TIMER_KIND, payload, due.at);
    task.dedupe_key = Some(format!("{TIMER_KIND}.{id}.{version}.{timer}"));
    enqueue(db, &task).await.map_err(|_| JobsError::Database)?;
    Ok(())
}

/// Runs one job timer (JOB-003, JOB-004, JOB-006): re-checks the job's status, version and timer
/// version first and does nothing if it moved on or its wait was restarted (D-807); otherwise
/// moves it to its timeout's target at the timer's due time, as the system, with the status's
/// timeout text (`JobStatus::timeout_text`: JOB-010 at 6 p.m., JOB-011, JOB-012; D-030, D-461).
/// Safe to run twice: the second run finds the version moved on.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a database failure.
pub async fn run_timer(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let p = &task.payload;
    let id = p["job"].as_str().and_then(|s| Uuid::parse_str(s).ok());
    let status = p["status"].as_str().and_then(JobStatus::from_code);
    let int = |key: &str| p[key].as_i64().and_then(|v| i32::try_from(v).ok());
    let (Some(id), Some(status), Some(version), Some(timer)) =
        (id, status, int("version"), int("timer"))
    else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut db = pool.acquire().await.map_err(|_| HandlerError(DB))?;
    let job = models::load(&mut db, id)
        .await
        .map_err(|_| HandlerError(DB))?;
    // A move bumps the version and a restart the timer version, so equal versions mean this is
    // the job's live timer; the guarded update re-checks all of it under the row lock.
    let due = timeout(status, job.timer_from, job.emergency);
    let live = job.version == version && job.timer_version == timer;
    let (true, Some(due)) = (live, due) else {
        return Ok(());
    };
    let step = (status, Some((version, timer)), due.then);
    let text = status.timeout_text();
    match apply(&mut db, id, step, (due.at, Actor::System), text).await {
        Ok(_) => Ok(()),
        Err(_) => Err(HandlerError(DB)),
    }
}

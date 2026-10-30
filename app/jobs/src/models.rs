//! The `jobs` rows and their queries, all in the `jobs` schema (JOB-001, JOB-002, JOB-005).
//!
//! A status is read and written only as [`JobStatus::code`]; a stored code that names no status is
//! a database fault, never guessed at.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::status::JobStatus;

/// Why a `jobs` call failed. No variant carries a value or the database's message (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum JobsError {
    /// The move has no arrow; it was refused, logged as a bug and changed nothing (JOB-002).
    #[error("a job cannot move from {0} to {1}")]
    RefusedMove(&'static str, &'static str),
    /// No job has this ID.
    #[error("no such job")]
    NotFound,
    /// The database refused or dropped the statement, or the audit row could not be written.
    #[error("the jobs database statement failed")]
    Database,
}

/// A job's current state, as the timer handler re-checks it (JOB-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRow {
    /// The job's one status (JOB-001).
    pub status: JobStatus,
    /// Bumped on every move; a timer for another version is stale (JOB-006).
    pub version: i32,
    /// Whether the job was chosen as an emergency (PICK-044).
    pub emergency: bool,
    /// When the job entered its status.
    pub status_at: Timestamp,
}

/// Inserts a job in Intake at `at` and returns its ID (JOB-001).
pub(crate) async fn insert_job(
    db: &mut PgConnection,
    at: Timestamp,
    emergency: bool,
) -> Result<Uuid, JobsError> {
    sqlx::query_scalar!(
        "INSERT INTO jobs.job (status, emergency, created_at, status_at) \
         VALUES ($1, $2, $3, $3) RETURNING id",
        JobStatus::Intake.code(),
        emergency,
        at.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| JobsError::Database)
}

/// Reads job `id`'s current status, version and entry time (JOB-006).
pub(crate) async fn load(db: &mut PgConnection, id: Uuid) -> Result<JobRow, JobsError> {
    let row = sqlx::query!(
        "SELECT status, version, emergency, status_at FROM jobs.job WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| JobsError::Database)?
    .ok_or(JobsError::NotFound)?;
    Ok(JobRow {
        status: JobStatus::from_code(&row.status).ok_or(JobsError::Database)?,
        version: row.version,
        emergency: row.emergency,
        status_at: row.status_at.to_jiff(),
    })
}

/// Moves job `id` to `to` only if it is still in `from` (and at `version`, when given), returning
/// the new version and the emergency flag; none when it moved on (JOB-004, JOB-006).
pub(crate) async fn guarded_move(
    db: &mut PgConnection,
    id: Uuid,
    (from, version): (JobStatus, Option<i32>),
    to: JobStatus,
    at: Timestamp,
) -> Result<Option<(i32, bool)>, JobsError> {
    let row = sqlx::query!(
        "UPDATE jobs.job SET status = $3, version = version + 1, status_at = $4 \
         WHERE id = $1 AND status = $2 AND ($5::integer IS NULL OR version = $5) \
         RETURNING version, emergency",
        id,
        from.code(),
        to.code(),
        at.to_sqlx() as _,
        version,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| JobsError::Database)?;
    Ok(row.map(|r| (r.version, r.emergency)))
}

/// Appends the move to `jobs.status_history` (JOB-005).
pub(crate) async fn insert_history(
    db: &mut PgConnection,
    id: Uuid,
    version: i32,
    (from, to): (JobStatus, JobStatus),
    at: Timestamp,
) -> Result<(), JobsError> {
    sqlx::query!(
        "INSERT INTO jobs.status_history (job_id, version, from_status, to_status, at) \
         VALUES ($1, $2, $3, $4, $5)",
        id,
        version,
        from.code(),
        to.code(),
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| JobsError::Database)?;
    Ok(())
}

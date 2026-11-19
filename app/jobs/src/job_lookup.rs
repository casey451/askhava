//! A job by the number its texts carry (JOB-024; T540, D-3892).
//!
//! [`job_by_number`] is the reverse of `job_number`: the owner types a job number on the release
//! page and `portals` resolves it here, since neither `portals` nor `picks` may read the `jobs`
//! schema. An unknown number is `None`, never an error.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;

/// The job whose number is `number`, or `None` when no job has it (JOB-024, D-3892).
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn job_by_number(db: &mut PgConnection, number: i64) -> Result<Option<Uuid>, JobsError> {
    sqlx::query_scalar!(
        "SELECT job_id FROM jobs.job_number WHERE number = $1",
        number
    )
    .fetch_optional(db)
    .await
    .map_err(|_| JobsError::Database)
}

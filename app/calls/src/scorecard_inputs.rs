//! The pilot scorecard's masked-call input (CALL-024, OPS-039; D-413, D-3753, D-3883; T478).
//!
//! Read-only, counts only. [`calls_answered`] counts the logged calls on masked numbers that
//! ended in a half-open window (`from` counted, `to` not) and those a person answered
//! (`answered_by = 'person'`; a machine, nobody or unknown counts as not answered). `calls`
//! stores no trade, so it takes the job IDs of one trade or of all (from
//! `jobs::api::jobs_by_trade`) and joins `call_log.line_id` to `number_pool.job_id` (D-3883).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::CallsError;

/// The masked calls of some jobs in a window and those a person answered (CALL-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallsAnswered {
    /// The logged calls.
    pub calls: i64,
    /// Those a person answered.
    pub answered: i64,
}

/// CALL-024, OPS-039: of jobs `jobs`, the masked calls that ended at or after `from` and before
/// `to`, and those a person answered.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn calls_answered(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<CallsAnswered, CallsError> {
    sqlx::query_as!(
        CallsAnswered,
        r#"SELECT count(*) AS "calls!",
                  count(*) FILTER (WHERE l.answered_by = 'person') AS "answered!"
           FROM calls.call_log l JOIN calls.number_pool p ON p.id = l.line_id
           WHERE p.job_id = ANY($1) AND l.ended_at >= $2 AND l.ended_at < $3"#,
        jobs,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| CallsError::Database)
}

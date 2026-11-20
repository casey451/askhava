//! The read of one pick's own masked line (CALL-001, CALL-009; T667, D-4349).
//!
//! `calls.number_pool` has no pick column, so a pick's line is the line of its release row's job,
//! business and bound homeowner contact given at or after that release. `picks`' line retry
//! (`picks.line_retry`) asks [`pick_has_line`] before it opens a line: a job connected, returned
//! and connected again gets its second line, while a line closed for an earlier connection, of
//! the same business too, is never taken for this pick's and never reopened.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::CallsError;

/// CALL-001, CALL-009, D-4349: whether job `job` has a masked line, open or closed, between
/// business `business` and homeowner contact `homeowner` given at or after `released` (the
/// pick's `released_at`): the pick's own line. A line of an earlier connection, given before this
/// release, does not count, so the retry opens the reconnected job's line; once this pick's line
/// exists, closed or not, the retry opens nothing more.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn pick_has_line(
    db: &mut PgConnection,
    (job, business, homeowner): (Uuid, Uuid, Uuid),
    released: Timestamp,
) -> Result<bool, CallsError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM calls.number_pool WHERE job_id = $1 AND business_id = $2
             AND homeowner_id = $3 AND assigned_at >= $4) AS "has!""#,
        job,
        business,
        homeowner,
        released.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| CallsError::Database)
}

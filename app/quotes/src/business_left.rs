//! A business leaving withdraws its open quotes (BIZ-051, PICK-051; D-446, D-491; T228).
//!
//! [`withdraw_for_leaving`] takes, job by job, the job's row lock (`jobs::api::lock_for_quote_change`,
//! the lock every quote change takes) and stamps `withdrawn_at` on the business's unwithdrawn quote
//! while the job still `JobStatus::takes_quotes` (Open to Awaiting confirm), so the quote page and
//! the LIST text show "Withdrawn by the business" in its place and no pick path can take it. Unlike
//! the business's own [`crate::edit_withdraw::withdraw_quote`], a pick on the job does not stop it:
//! the caller ends the business's waiting picks first. A quote on a job past Confirm stays as it
//! was; no row is deleted.

use askhava_jobs::api::lock_for_quote_change;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::QuotesError;

/// BIZ-051, PICK-051: inside the caller's transaction, withdraws every unwithdrawn quote of
/// `business` on a job that still takes quotes, at `now`; returns how many it withdrew.
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`] (roll back).
pub async fn withdraw_for_leaving(
    tx: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<u64, QuotesError> {
    let jobs = sqlx::query_scalar!(
        "SELECT job_id FROM quotes.quote WHERE business_id = $1 AND withdrawn_at IS NULL \
         ORDER BY job_id",
        business
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| QuotesError::Database)?;
    let mut withdrawn = 0;
    for job in jobs {
        let status = lock_for_quote_change(tx, job)
            .await
            .map_err(|_| QuotesError::Job)?;
        if !status.takes_quotes() {
            continue;
        }
        let done = sqlx::query!(
            "UPDATE quotes.quote SET withdrawn_at = $3 \
             WHERE job_id = $1 AND business_id = $2 AND withdrawn_at IS NULL",
            job,
            business,
            now.to_sqlx()
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| QuotesError::Database)?;
        withdrawn += done.rows_affected();
    }
    Ok(withdrawn)
}

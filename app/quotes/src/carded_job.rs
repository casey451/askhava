//! The business job page's lookups (CARD-023, CARD-019, PICK-051, SEC-019; T666, D-4349,
//! D-5526).
//!
//! [`carded_job`] turns the number in `/b/jobs/{j}` into the job only when its card went to the
//! signed-in business: `jobs::api::job_by_number` first (`jobs` owns `jobs.job_number` and may not
//! call `quotes`), then `page_counts`' card test on `quotes.job_card`, so an unknown number and
//! another business's job are the same `None` before anything else is read. [`carded_job_id`] is
//! the same card test for a job ID (the sign-up done screen links each open job by its UUID).
//! [`quote_standing`] says where the business's own quote on the job stands, for the page's
//! PICK-051 controls; `server` adds a job's accepted pick, which `quotes` cannot read.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, QuotesError};

/// PICK-051: where a business's own quote on a job stands, as the job page's controls show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStanding {
    /// No quote, or one still waiting for its read-back confirm (PICK-006), so nothing is shown.
    NotSent,
    /// Confirmed and not withdrawn: it can be changed or withdrawn.
    Standing,
    /// Withdrawn by the business, whether or not it was ever confirmed (D-5619).
    Withdrawn,
    /// The job has had a pick or left the statuses that take quote edits, so it can't change.
    Closed,
}

/// CARD-023, SEC-019, D-4349: the job numbered `number` when its card went to `business`, else
/// `None` (an unknown number and another business's job alike, so nothing tells them apart).
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`].
pub async fn carded_job(
    db: &mut PgConnection,
    (number, business): (i64, Uuid),
) -> Result<Option<Uuid>, QuotesError> {
    let job = askhava_jobs::api::job_by_number(db, number).await;
    let Some(job) = job.map_err(|_| QuotesError::Job)? else {
        return Ok(None);
    };
    carded_job_id(db, (job, business)).await
}

/// CARD-023, SEC-019, D-5526: `job` when its card went to `business`, else `None` (an unknown ID
/// too): the same card test as [`carded_job`], for a job named by its ID.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn carded_job_id(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
) -> Result<Option<Uuid>, QuotesError> {
    let counts = models::page_counts(db, job, business).await?;
    Ok(counts.holds_card.then_some(job))
}

/// PICK-051, D-2912, D-5619: where `business`'s own quote on `job` stands: closed once any quote
/// on the job is unavailable (a pick) or the job has left the statuses that take quote edits, as
/// `withdraw_quote` refuses; else withdrawn when the business withdrew it (checked before the
/// confirm, so a withdrawn read-back still reads as withdrawn), standing when confirmed, and not
/// sent otherwise.
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`].
pub async fn quote_standing(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
) -> Result<QuoteStanding, QuotesError> {
    let status = askhava_jobs::api::current(db, job).await;
    let status = status.map_err(|_| QuotesError::Job)?.status;
    let (_, picked) = models::edit_facts(db, job, business).await?;
    if picked || !status.takes_quote_edits() {
        return Ok(QuoteStanding::Closed);
    }
    let own = sqlx::query!(
        "SELECT confirmed, withdrawn_at IS NOT NULL AS \"withdrawn!\" FROM quotes.quote \
         WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(match own {
        Some(row) if row.withdrawn => QuoteStanding::Withdrawn,
        Some(row) if row.confirmed => QuoteStanding::Standing,
        _ => QuoteStanding::NotSent,
    })
}

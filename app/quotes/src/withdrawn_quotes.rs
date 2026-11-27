//! The withdrawn quotes still held on a homeowner's jobs, for her "Download my data" file (T820,
//! D-5399, D-5526, D-5709; PRIV-031).
//!
//! A withdrawal only sets `withdrawn_at` (`edit_withdraw`), so the business's words stay stored
//! while `page_quotes` blanks them on her page. [`withdrawn_quotes`] reads those words back for the
//! jobs the caller names (her own, from `intake::api::jobs_of_contact`); `page_quotes` and her page
//! are unchanged.

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::QuotesError;

/// One confirmed quote that its business withdrew, with the words it still holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnQuote {
    /// The job it was given on.
    pub job: Uuid,
    /// The business that gave and withdrew it.
    pub business: Uuid,
    /// When it was received.
    pub received_at: Timestamp,
    /// The business's own wording.
    pub original: String,
}

/// PRIV-031, D-5526: every confirmed, withdrawn quote on `jobs`, in arrival order (time received,
/// then business ID); only those jobs' rows are read.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn withdrawn_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
) -> Result<Vec<WithdrawnQuote>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT job_id, business_id, received_at, original FROM quotes.quote \
         WHERE job_id = ANY($1) AND confirmed AND withdrawn_at IS NOT NULL \
         ORDER BY received_at, business_id",
        jobs
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| WithdrawnQuote {
            job: r.job_id,
            business: r.business_id,
            received_at: r.received_at.to_jiff(),
            original: r.original,
        })
        .collect())
}

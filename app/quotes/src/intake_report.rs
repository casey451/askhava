//! The quotes half of the monthly intake accuracy report (OPS-040; T434, D-3155, D-3753).
//!
//! [`intake_report_ids`] gives, for a window `[from, to)` (the page passes an Arizona month), the
//! job of each job card sent in it whose own 24-hour window ([`QUOTE_WINDOW`]) is over by `now`
//! and `to` with no quote of its business on the job that is not withdrawn (a withdrawn quote
//! counts as none, D-3155), and each quote received in it, not withdrawn, that said the business
//! needed more information: `confirm_on_site` ticked or the `diagnostic_visit` format. That is
//! derived from the two stored fields only; the notes are never read or parsed (D-3155). IDs only,
//! never a business's or homeowner's name, contact or words (PRIV-005); `portals` groups them by
//! the job's category and question set through `jobs::api::report_facts` and `catalog::api`.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::QUOTE_WINDOW;
use crate::models::QuotesError;

/// One "need more info" quote: its ID and its job's (OPS-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoreInfoQuote {
    /// The quote's ID.
    pub quote: Uuid,
    /// The job it answers.
    pub job: Uuid,
}

/// The month's report IDs (OPS-040, D-3155).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntakeReportIds {
    /// The job of each card that got no quote (not withdrawn), once per card, oldest card first.
    pub no_quote_jobs: Vec<Uuid>,
    /// The quotes not withdrawn that ticked confirm-on-site or chose a diagnostic visit.
    pub more_info: Vec<MoreInfoQuote>,
}

/// OPS-040, D-3155: the [`IntakeReportIds`] of `[from, to)` as of `now`.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn intake_report_ids(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
    now: Timestamp,
) -> Result<IntakeReportIds, QuotesError> {
    let end = to.min(now);
    let ended_by = end.saturating_sub(QUOTE_WINDOW).unwrap_or(end);
    let no_quote_jobs = sqlx::query_scalar!(
        "SELECT c.job_id FROM quotes.job_card c
          WHERE c.sent_at >= $1 AND c.sent_at < $2 AND c.sent_at <= $3 AND NOT EXISTS (
                SELECT 1 FROM quotes.quote q WHERE q.job_id = c.job_id
                   AND q.business_id = c.business_id AND q.withdrawn_at IS NULL)
          ORDER BY c.sent_at, c.id",
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        ended_by.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let more_info = sqlx::query!(
        "SELECT id, job_id FROM quotes.quote
          WHERE received_at >= $1 AND received_at < $2 AND withdrawn_at IS NULL
            AND (confirm_on_site OR format = 'diagnostic_visit')
          ORDER BY received_at, id",
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?
    .into_iter()
    .map(|r| MoreInfoQuote {
        quote: r.id,
        job: r.job_id,
    })
    .collect();
    Ok(IntakeReportIds {
        no_quote_jobs,
        more_info,
    })
}

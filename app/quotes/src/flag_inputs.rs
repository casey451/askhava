//! The crowded-trade flag's quote inputs (OPS-027; D-464, D-4280, D-4311), read-only.
//!
//! The caller passes one trade's job IDs (`jobs_by_trade` in `scorecard_inputs.rs`, keyed on the
//! first sent card's `quotes.shown_card.category`) and a calendar month as `[from, to)`; the
//! month's jobs are those whose first card was sent in it. Each counts its confirmed quotes
//! received before `to` (an unconfirmed quote is never shown, so it is not a quote she got), and a
//! carded job with none counts 0 (D-4311). Counts only; no contact field (PRIV-028).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_numbers::median_tenths;
use crate::models::QuotesError;

/// One trade's quotes in a calendar month (OPS-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthQuotes {
    /// The jobs first carded in the month.
    pub jobs: i64,
    /// The median confirmed quotes per job, in tenths (D-331's rule: the mean of the two middle
    /// values for an even count, rounded down).
    pub median_tenths: u32,
    /// The confirmed quotes on those jobs, for T524's quotes per win.
    pub confirmed_quotes: i64,
}

/// OPS-027, D-464, D-4311: the [`MonthQuotes`] of `jobs` whose first card was sent in
/// `[from, to)`, counting confirmed quotes received before `to`; `None` with no such job.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn month_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<Option<MonthQuotes>, QuotesError> {
    let counts = sqlx::query_scalar!(
        "SELECT (SELECT count(*) FROM quotes.quote q WHERE q.job_id = c.job_id \
           AND q.confirmed AND q.received_at < $3) AS \"quotes!\" \
         FROM quotes.shown_card c WHERE c.job_id = ANY($1) AND c.sent_at < $3 \
         GROUP BY c.job_id HAVING min(c.sent_at) >= $2",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let jobs = i64::try_from(counts.len()).map_err(|_| QuotesError::Database)?;
    let confirmed_quotes = counts.iter().sum();
    let per_job = counts
        .into_iter()
        .map(|n| u32::try_from(n).map_err(|_| QuotesError::Database))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(median_tenths(per_job).map(|median_tenths| MonthQuotes {
        jobs,
        median_tenths,
        confirmed_quotes,
    }))
}

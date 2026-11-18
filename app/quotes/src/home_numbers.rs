//! The home page's three numbers (WEB-028; D-466, D-468, D-3144; T102).
//!
//! [`home_numbers`] reads the append-only card and quote records, `quotes.job_card` (`sent_at`)
//! and `quotes.quote` (`received_at`, confirmed quotes only), never a typed figure: the number of
//! jobs with at least one card sent (one job is one request), the median time from a job's first
//! card to its first quote, the average number of quotes per such job, the date range of those
//! cards and the number of trades they cover (every job's trade read at once through
//! `jobs::api::categories_of`, so it makes 2 statements however many jobs exist, D-4098). Below
//! the caller's threshold (`ASKHAVA_HOME_NUMBERS_MIN_JOBS`, default 50) it answers `None` after
//! 1 statement and reads no trade. Counts and times only; no business or homeowner detail.

use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;

use crate::models::QuotesError;

/// WEB-028's figures, each over the same cards (D-468).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeNumbers {
    /// Jobs (requests) with at least one card sent.
    pub requests: u32,
    /// The median time from a job's first card to its first confirmed quote, over the jobs that
    /// got one; `None` while no job has a quote.
    pub median_first_quote: Option<SignedDuration>,
    /// Confirmed quotes per request, in tenths, rounded down.
    pub quotes_per_request_tenths: u32,
    /// The first and the last card's send time.
    pub cards_from: Timestamp,
    /// The last card's send time.
    pub cards_to: Timestamp,
    /// The distinct trades (catalog categories) of those jobs.
    pub trades: u32,
}

/// WEB-028, D-466, D-468: the home page's numbers, or `None` while fewer than `min_jobs` jobs have
/// had a card sent. Two statements at most, however many jobs exist (D-4098).
///
/// # Errors
///
/// [`QuotesError::Database`]; [`QuotesError::Job`] when a job's trade cannot be read.
pub async fn home_numbers(
    db: &mut PgConnection,
    min_jobs: u32,
) -> Result<Option<HomeNumbers>, QuotesError> {
    let rows = sqlx::query!(
        r#"WITH c AS (SELECT job_id, min(sent_at) AS first_card, max(sent_at) AS last_card
                        FROM quotes.job_card GROUP BY job_id),
                q AS (SELECT job_id, count(*) AS n, min(received_at) AS first_quote
                        FROM quotes.quote WHERE confirmed GROUP BY job_id)
           SELECT c.job_id AS "job!", c.first_card AS "first_card!",
                  c.last_card AS "last_card!", coalesce(q.n, 0) AS "quotes!",
                  q.first_quote AS "first_quote"
             FROM c LEFT JOIN q ON q.job_id = c.job_id ORDER BY c.first_card, c.job_id"#
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let requests = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    if requests == 0 || requests < min_jobs {
        return Ok(None);
    }
    let jobs: Vec<_> = rows.iter().map(|r| r.job).collect();
    let trades = askhava_jobs::api::categories_of(db, &jobs)
        .await
        .map_err(|_| QuotesError::Job)?;
    let waits = rows.iter().filter_map(|r| {
        let quote = r.first_quote.as_ref()?.to_jiff();
        Some(quote.duration_since(r.first_card.to_jiff()))
    });
    let quotes: i64 = rows.iter().map(|r| r.quotes).sum();
    Ok(Some(HomeNumbers {
        requests,
        median_first_quote: median_wait(waits.collect()),
        quotes_per_request_tenths: u32::try_from(quotes.saturating_mul(10) / i64::from(requests))
            .unwrap_or(u32::MAX),
        cards_from: rows
            .iter()
            .map(|r| r.first_card.to_jiff())
            .min()
            .unwrap_or_default(),
        cards_to: rows
            .iter()
            .map(|r| r.last_card.to_jiff())
            .max()
            .unwrap_or_default(),
        trades: u32::try_from(trades.len()).unwrap_or(u32::MAX),
    }))
}

/// WEB-028: the median of `waits` (the mean of the two middle ones for an even count); `None`
/// for none.
#[must_use]
pub fn median_wait(mut waits: Vec<SignedDuration>) -> Option<SignedDuration> {
    waits.sort_unstable();
    let mid = waits.len() / 2;
    match waits.len() {
        0 => None,
        n if n % 2 == 1 => waits.get(mid).copied(),
        _ => {
            let (a, b) = (waits.get(mid - 1)?, waits.get(mid)?);
            Some(*a + (*b - *a) / 2)
        }
    }
}

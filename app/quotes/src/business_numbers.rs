//! One business's card and quote counts for the portal's "Your numbers" panel (BIZ-025; D-193,
//! D-331; T451).
//!
//! [`quote_numbers`] counts, for one business and one Arizona month `[from, to)` the caller gives
//! (`to` is now): the job cards sent to it in the month; of those, the cards that ended without a
//! quote (its own 24-hour window, [`QUOTE_WINDOW`], over by `to` with no quote of this business on
//! the job, withdrawn or not, D-331); the quotes it sent in the month; and, for each trade (the
//! sent card's category in `quotes.shown_card`) of the jobs it got a card for in the month, the
//! typical (median) number of quotes per job over every job of that trade whose card went out in
//! the month, in tenths ([`median_tenths`], D-331). IDs and counts only.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::QUOTE_WINDOW;
use crate::models::QuotesError;

/// One business's card and quote counts for one month (BIZ-025).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuoteNumbers {
    /// Job cards sent to the business in the month.
    pub cards: u32,
    /// Of those, the cards whose window ended without a quote from it (D-331).
    pub cards_without_quote: u32,
    /// Quotes the business sent in the month.
    pub quotes: u32,
    /// `(trade, median quotes per job in tenths)`, by trade code (D-331).
    pub typical_quotes: Vec<(String, u32)>,
}

/// D-331: the median of `counts` in tenths (the mean of the two middle values for an even count,
/// rounded down); `None` for no counts.
#[must_use]
pub fn median_tenths(mut counts: Vec<u32>) -> Option<u32> {
    counts.sort_unstable();
    let n = counts.len();
    let mid = n / 2;
    match n {
        0 => None,
        _ if n % 2 == 1 => counts.get(mid).map(|c| c.saturating_mul(10)),
        _ => {
            let (a, b) = (counts.get(mid - 1)?, counts.get(mid)?);
            Some(a.saturating_add(*b).saturating_mul(5))
        }
    }
}

/// BIZ-025, D-331: `business`'s cards, cards ended without a quote, quotes and the typical
/// quotes per job in each of its trades, for `[from, to)`.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn quote_numbers(
    db: &mut PgConnection,
    business: Uuid,
    (from, to): (Timestamp, Timestamp),
) -> Result<QuoteNumbers, QuotesError> {
    let ended_by = to.saturating_sub(QUOTE_WINDOW).unwrap_or(to);
    let row = sqlx::query!(
        r#"SELECT
             (SELECT count(*) FROM quotes.job_card c
               WHERE c.business_id = $1 AND c.sent_at >= $2 AND c.sent_at < $3) AS "cards!",
             (SELECT count(*) FROM quotes.job_card c
               WHERE c.business_id = $1 AND c.sent_at >= $2 AND c.sent_at < $3
                 AND c.sent_at <= $4
                 AND NOT EXISTS (SELECT 1 FROM quotes.quote q
                                  WHERE q.job_id = c.job_id AND q.business_id = $1)) AS "ended!",
             (SELECT count(*) FROM quotes.quote q
               WHERE q.business_id = $1 AND q.received_at >= $2 AND q.received_at < $3) AS "quotes!""#,
        business,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        ended_by.to_sqlx() as _,
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(QuoteNumbers {
        cards: count(row.cards),
        cards_without_quote: count(row.ended),
        quotes: count(row.quotes),
        typical_quotes: typical_quotes(db, business, (from, to)).await?,
    })
}

/// D-331: per trade of `business`'s cards in `[from, to)`, the median quotes per job of that
/// trade whose card went out in the window, in tenths, by trade code.
async fn typical_quotes(
    db: &mut PgConnection,
    business: Uuid,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<(String, u32)>, QuotesError> {
    let rows = sqlx::query!(
        r#"WITH trades AS (
             SELECT DISTINCT s.category FROM quotes.job_card c
               JOIN quotes.shown_card s ON s.job_id = c.job_id AND s.sent_at IS NOT NULL
              WHERE c.business_id = $1 AND c.sent_at >= $2 AND c.sent_at < $3)
           SELECT s.category AS "trade!",
                  (SELECT count(*) FROM quotes.quote q WHERE q.job_id = s.job_id) AS "quotes!"
             FROM quotes.shown_card s
            WHERE s.sent_at >= $2 AND s.sent_at < $3
              AND s.category IN (SELECT category FROM trades)"#,
        business,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let mut by_trade: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for r in rows {
        by_trade.entry(r.trade).or_default().push(count(r.quotes));
    }
    Ok(by_trade
        .into_iter()
        .filter_map(|(trade, counts)| median_tenths(counts).map(|m| (trade, m)))
        .collect())
}

/// A database count as a panel number, saturating.
fn count(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

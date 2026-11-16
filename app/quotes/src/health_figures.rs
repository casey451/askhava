//! The quotes figures of the owner health page (OPS-008, OPS-009; T475, D-3753, D-3844, D-3877).
//!
//! [`card_figures`] counts, in one query, the job cards sent in a window `[from, to)` (the page
//! passes this Arizona month) and the cards that ended without a quote: the card's own 24-hour
//! window ([`QUOTE_WINDOW`]) over by `to` with no unwithdrawn quote of its business on the job (a
//! withdrawn quote counts as none, D-3155; BIZ-044, D-331), and that share in thousandths, `None`
//! for a window with no card. [`spot_check_cards`] draws up to `n` distinct random cards of the
//! window for OPS-009's monthly spot-check, with only the owner card view's fields: card ID, job
//! ID, trade (the sent card's `quotes.shown_card.category`) and send time. Counts and IDs only,
//! never a business's or homeowner's name, contact or words (PRIV-005).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::QUOTE_WINDOW;
use crate::models::QuotesError;

/// This window's job cards and those that ended without a quote (OPS-008, BIZ-044).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardFigures {
    /// Job cards sent in the window.
    pub cards: i64,
    /// Of those, the cards whose window ended with no unwithdrawn quote (D-3155).
    pub ended_without_quote: i64,
    /// `ended_without_quote` per thousand cards, rounded down; `None` with no card.
    pub without_quote_per_mille: Option<i64>,
}

/// One card of the OPS-009 spot-check, as the owner's card view shows it (D-3877).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotCard {
    /// The job card's ID.
    pub card: Uuid,
    /// The job it is for.
    pub job: Uuid,
    /// The trade code of the sent card; `None` when no sent card text is stored.
    pub trade: Option<String>,
    /// When the card went to the business.
    pub sent_at: Timestamp,
}

/// OPS-008, BIZ-044, D-3155: the [`CardFigures`] of `[from, to)`.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn card_figures(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<CardFigures, QuotesError> {
    let ended_by = to.saturating_sub(QUOTE_WINDOW).unwrap_or(to);
    let r = sqlx::query!(
        r#"WITH c AS (
             SELECT c.sent_at <= $3 AND NOT EXISTS (
                      SELECT 1 FROM quotes.quote q WHERE q.job_id = c.job_id
                         AND q.business_id = c.business_id AND q.withdrawn_at IS NULL) AS ended
               FROM quotes.job_card c WHERE c.sent_at >= $1 AND c.sent_at < $2)
           SELECT count(*) AS "cards!", count(*) FILTER (WHERE ended) AS "ended!",
                  1000 * count(*) FILTER (WHERE ended) / NULLIF(count(*), 0) AS "share?"
             FROM c"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        ended_by.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(CardFigures {
        cards: r.cards,
        ended_without_quote: r.ended,
        without_quote_per_mille: r.share,
    })
}

/// OPS-009, D-3844, D-3877: up to `n` distinct job cards sent in `[from, to)`, drawn at random,
/// each with only the owner card view's fields; fewer when fewer exist.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn spot_check_cards(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
    n: u32,
) -> Result<Vec<SpotCard>, QuotesError> {
    let rows = sqlx::query!(
        r#"SELECT c.id, c.job_id, c.sent_at,
                  (SELECT s.category FROM quotes.shown_card s
                    WHERE s.job_id = c.job_id AND s.sent_at IS NOT NULL
                    ORDER BY s.sent_at DESC, s.id LIMIT 1) AS "trade?"
             FROM quotes.job_card c WHERE c.sent_at >= $1 AND c.sent_at < $2
            ORDER BY random() LIMIT $3"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        i64::from(n),
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| SpotCard {
            card: r.id,
            job: r.job_id,
            trade: r.trade,
            sent_at: r.sent_at.to_jiff(),
        })
        .collect())
}

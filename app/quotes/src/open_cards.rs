//! The cards a business still has open, for the open-cards email (CARD-022, BIZ-038, PICK-001;
//! T684, D-5017).
//!
//! `picks` composes and queues that email but may not read `quotes.*`, so it asks here.
//! [`open_cards`] lists, for one business, each card it holds and has not quoted (no unwithdrawn
//! quote of its own) whose own quote window ([`QUOTE_WINDOW`] from the card's send, unchanged) is
//! still open, on a job that still takes quotes (`JobStatus::takes_quotes`): the job, the trade
//! and town as the sent card names them, and the window's end. [`card_businesses`] names the
//! businesses holding any card whose window is open, the run's worklist. Nothing here reads or
//! returns a homeowner detail, and nothing is changed.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::QUOTE_WINDOW;
use crate::models::QuotesError;

/// One unquoted card of a business (CARD-022): no homeowner detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCardFacts {
    /// The job the card is for; its Quote link is made from it (BIZ-038).
    pub job: Uuid,
    /// The trade, as on the sent card.
    pub trade: String,
    /// The town, as on the sent card.
    pub town: String,
    /// When this business's quote window ends: the card's send plus [`QUOTE_WINDOW`] (PICK-001).
    pub ends: Timestamp,
}

/// The start of the newest quote window already over at `now`: a card sent after it is open.
fn window_start(now: Timestamp) -> Timestamp {
    now.saturating_sub(QUOTE_WINDOW).unwrap_or(now)
}

/// CARD-022, D-5017: the businesses holding a card whose quote window is still open at `now`,
/// by ID.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn card_businesses(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT DISTINCT business_id FROM quotes.job_card WHERE sent_at > $1 ORDER BY business_id",
        window_start(now).to_sqlx() as _
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)
}

/// CARD-022, BIZ-038, PICK-001, D-5017: each card `business` holds and has not quoted whose
/// window is open at `now`, on a job still taking quotes, oldest card first.
///
/// # Errors
///
/// [`QuotesError::Database`], or [`QuotesError::Job`] when a job cannot be read.
pub async fn open_cards(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Vec<OpenCardFacts>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT c.job_id, c.sent_at, s.category, s.town FROM quotes.job_card c \
         JOIN LATERAL (SELECT category, town FROM quotes.shown_card s WHERE s.job_id = c.job_id \
           AND s.sent_at IS NOT NULL ORDER BY s.sent_at, s.id LIMIT 1) s ON true \
         WHERE c.business_id = $1 AND c.sent_at > $2 AND NOT EXISTS ( \
           SELECT 1 FROM quotes.quote q WHERE q.job_id = c.job_id \
             AND q.business_id = c.business_id AND q.withdrawn_at IS NULL) \
         ORDER BY c.sent_at, c.job_id",
        business,
        window_start(now).to_sqlx() as _
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let mut open = Vec::new();
    for r in rows {
        let job = askhava_jobs::api::current(db, r.job_id).await;
        if job.map_err(|_| QuotesError::Job)?.status.takes_quotes() {
            let ends = r.sent_at.to_jiff().saturating_add(QUOTE_WINDOW);
            open.push(OpenCardFacts {
                job: r.job_id,
                trade: r.category,
                town: r.town,
                ends: ends.map_err(|_| QuotesError::Database)?,
            });
        }
    }
    Ok(open)
}

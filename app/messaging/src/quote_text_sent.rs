//! The last quote text a recipient got about a job, and when it reached her (TXT-017, D-1190,
//! D-4052, D-4064).
//!
//! [`last_quote_text`] reads the newest `first_quote` or `quote_update` outbox row to a recipient
//! whose `subject_ids` hold the job, for the quote-count update sender (T583) to measure TXT-017's
//! 3 hours from. Its time is the row's first Twilio `delivered` event, else its `sent_at`, else its
//! queue time (`created_at`), so a text still queued or sending is never invisible. A `blocked`,
//! `failed` or `expired` row never reached her and is never sent, so it is skipped and the row
//! before it counts; an `unknown` row (the carrier may have taken it, never resent) counts as
//! sent. Nothing here reads or returns a body or a phone (PRIV-002).

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;

/// The templates whose texts carry a job's quote count: the first-quote text (TXT-016) and the
/// quote-count update (TXT-017).
pub const QUOTE_TEXT_TEMPLATES: [&str; 2] = ["first_quote", "quote_update"];

/// The last quote text to a recipient about a job (TXT-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTextSent {
    /// The outbox row's ID.
    pub id: Uuid,
    /// `first_quote` or `quote_update`.
    pub template_id: String,
    /// Its delivery time, else its send time, else its queue time.
    pub at: Timestamp,
}

/// TXT-017, D-1190, D-4064: the newest `first_quote` or `quote_update` text to `recipient` about
/// `job` that was not blocked, failed or expired, with the time TXT-017 measures from; none when
/// there is no such text.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn last_quote_text(
    db: &mut PgConnection,
    recipient: Uuid,
    job: Uuid,
) -> Result<Option<QuoteTextSent>, OutboxError> {
    let templates = QUOTE_TEXT_TEMPLATES.map(String::from);
    let row = sqlx::query!(
        "SELECT o.id, o.template_id, coalesce((SELECT min(d.received_at) \
         FROM messaging.delivery_event d WHERE d.provider_sid = o.provider_sid \
         AND d.status = 'delivered'), o.sent_at, o.created_at) AS \"at!\" \
         FROM messaging.outbox o WHERE o.recipient_id = $1 AND $2 = ANY(o.subject_ids) \
         AND o.template_id = ANY($3::text[]) \
         AND o.status NOT IN ('blocked', 'failed', 'expired') \
         ORDER BY o.created_at DESC, o.id DESC LIMIT 1",
        recipient,
        job,
        &templates
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| QuoteTextSent {
        id: r.id,
        template_id: r.template_id,
        at: r.at.to_jiff(),
    }))
}

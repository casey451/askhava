//! The last quote text a recipient got about a job, and when it reached her (TXT-017, D-1190,
//! D-4052, D-4064).
//!
//! [`last_quote_text`] reads the newest `first_quote` or `quote_update` outbox row to a recipient
//! whose `subject_ids` hold the job, for the quote-count update sender (T583) to measure TXT-017's
//! 3 hours from. Its time is the row's first Twilio `delivered` event, else its `sent_at`, else its
//! queue time (`created_at`), so a text still queued or sending is never invisible. A `blocked`,
//! `failed` or `expired` row never reached her and is never sent, so it is skipped and the row
//! before it counts; an `unknown` row (the carrier may have taken it, never resent) counts as
//! sent. A homeowner who chose updates by email (TXT-018, D-5617) is told by an
//! `update.first_quote` or `update.quote_update` email row for the same recipient and job, which
//! counts too unless `dropped` (never sent; a bounced one was still told): the newer of the text
//! and the email by queue time (a text's `created_at`, an email's `queued_at`) wins, the text on
//! a tie (`Both` queues them in one transaction), returned under the text template ID with the
//! email's `sent_at`, else its `queued_at`. Nothing here reads or returns a body or a phone
//! (PRIV-002).

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;
use crate::update_emails::UPDATE_EMAIL_PREFIX;

/// The templates whose texts carry a job's quote count: the first-quote text (TXT-016) and the
/// quote-count update (TXT-017).
pub const QUOTE_TEXT_TEMPLATES: [&str; 2] = ["first_quote", "quote_update"];

/// The last quote text to a recipient about a job (TXT-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTextSent {
    /// The outbox row's ID, or the email outbox row's when an update email told her (D-5617).
    pub id: Uuid,
    /// `first_quote` or `quote_update`.
    pub template_id: String,
    /// Its delivery time, else its send time, else its queue time.
    pub at: Timestamp,
}

/// TXT-017, D-1190, D-4064: the newest `first_quote` or `quote_update` text to `recipient` about
/// `job` that was not blocked, failed or expired, with the time TXT-017 measures from; none when
/// there is no such text. An update email of either template that was not dropped counts too,
/// the newer by queue time winning and the text on a tie (TXT-018, D-5617).
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
        "SELECT id AS \"id!\", template_id AS \"template_id!\", at AS \"at!\" FROM ( \
         (SELECT o.id, o.template_id, coalesce((SELECT min(d.received_at) \
         FROM messaging.delivery_event d WHERE d.provider_sid = o.provider_sid \
         AND d.status = 'delivered'), o.sent_at, o.created_at) AS at, o.created_at AS queued, \
         0 AS rank FROM messaging.outbox o WHERE o.recipient_id = $1 AND $2 = ANY(o.subject_ids) \
         AND o.template_id = ANY($3::text[]) \
         AND o.status NOT IN ('blocked', 'failed', 'expired') \
         ORDER BY o.created_at DESC, o.id DESC LIMIT 1) \
         UNION ALL \
         (SELECT e.id, substr(e.template_id, length($4) + 1), coalesce(e.sent_at, e.queued_at), \
         e.queued_at, 1 FROM messaging.email_outbox e WHERE e.recipient_id = $1 \
         AND $2 = ANY(e.subject_ids) AND substr(e.template_id, length($4) + 1) = ANY($3::text[]) \
         AND starts_with(e.template_id, $4) AND e.status <> 'dropped' \
         ORDER BY e.queued_at DESC, e.id DESC LIMIT 1) \
         ) told ORDER BY queued DESC, rank ASC LIMIT 1",
        recipient,
        job,
        &templates,
        UPDATE_EMAIL_PREFIX
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

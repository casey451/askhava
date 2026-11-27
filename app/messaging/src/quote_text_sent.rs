//! The last quote update a recipient was told about a job, when it reached her and the count it
//! told (TXT-017, TXT-018, D-1190, D-4052, D-4064, D-5617, D-5639, D-5640).
//!
//! [`last_quote_text`] reads the newest `first_quote` or `quote_update` outbox row to a recipient
//! whose `subject_ids` hold the job, and the newest `update.first_quote` or `update.quote_update`
//! email row (T730's update emails) to her about it, for the quote-count update sender (T583) to
//! measure TXT-017's 3 hours from. Newer is by queue time (a text's `created_at`, an email's
//! `queued_at`); on a tie (`Both`: one transaction queued the pair) the text wins. A text's time
//! is its first Twilio `delivered` event, else its `sent_at`, else `created_at`; an email's its
//! first `email.delivered` event (`messaging.email_event` by `provider_id`), else `sent_at`, else
//! `queued_at`, so an update still queued or sending is never invisible. A `blocked`, `failed` or
//! `expired` text, and a `dropped` email or one with an `email.bounced`, `email.failed` or
//! `email.suppressed` event, never reached her: it is skipped and the row before it counts; an
//! `unknown` text (the carrier may have taken it, never resent) counts as sent. The count told is
//! read from the row's `subject_ids` (`[job, count]`, an email's with her address row appended).
//! [`untold_first_quote_email`] says whether such a skipped `update.first_quote` email exists, so
//! the first-quote text goes after the reset to text. Nothing here reads or returns a body, an
//! address or a phone (PRIV-002).

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;
use crate::update_emails::UPDATE_EMAIL_PREFIX;

/// The templates whose texts carry a job's quote count: the first-quote text (TXT-016) and the
/// quote-count update (TXT-017).
pub const QUOTE_TEXT_TEMPLATES: [&str; 2] = ["first_quote", "quote_update"];

/// The email events that mean an update email never reached her (D-5639, D-5640).
const UNTOLD_EVENTS: [&str; 3] = ["email.bounced", "email.failed", "email.suppressed"];

/// The last quote update to a recipient about a job, by text or by email (TXT-017, TXT-018).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTextSent {
    /// The outbox row's ID, or the `messaging.email_outbox` row's when an update email is newer.
    pub id: Uuid,
    /// `first_quote` or `quote_update` (an email's text template, without `update.`).
    pub template_id: String,
    /// Its delivery time, else its send time, else its queue time.
    pub at: Timestamp,
    /// The count it told: 1 for `first_quote`, else the count in its `subject_ids`; `None` for a
    /// row that carries none (the caller falls back to its own record).
    pub quoted: Option<u32>,
}

/// TXT-017, TXT-018, D-1190, D-4064, D-5639: the newest `first_quote` or `quote_update` update to
/// `recipient` about `job`, text or email, that was not blocked, failed, expired, dropped,
/// bounced or suppressed, with the time TXT-017 measures from and the count it told; none when
/// there is no such update.
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
    let emails = QUOTE_TEXT_TEMPLATES.map(|t| format!("{UPDATE_EMAIL_PREFIX}{t}"));
    let untold = UNTOLD_EVENTS.map(String::from);
    let row = sqlx::query!(
        "SELECT id AS \"id!\", template_id AS \"template_id!\", at AS \"at!\", \
         subject_ids AS \"subject_ids!\", email AS \"email!\" FROM ( \
         SELECT o.id, o.template_id, coalesce((SELECT min(d.received_at) \
         FROM messaging.delivery_event d WHERE d.provider_sid = o.provider_sid \
         AND d.status = 'delivered'), o.sent_at, o.created_at) AS at, o.subject_ids, \
         o.created_at AS queued, false AS email \
         FROM messaging.outbox o WHERE o.recipient_id = $1 AND $2 = ANY(o.subject_ids) \
         AND o.template_id = ANY($3::text[]) \
         AND o.status NOT IN ('blocked', 'failed', 'expired') \
         UNION ALL \
         SELECT e.id, substr(e.template_id, length($6) + 1), coalesce((SELECT min(v.event_at) \
         FROM messaging.email_event v WHERE v.provider_id = e.provider_id \
         AND v.kind = 'email.delivered'), e.sent_at, e.queued_at), e.subject_ids, \
         e.queued_at, true FROM messaging.email_outbox e \
         WHERE e.recipient_id = $1 AND $2 = ANY(e.subject_ids) \
         AND e.template_id = ANY($4::text[]) AND e.status <> 'dropped' \
         AND NOT EXISTS (SELECT 1 FROM messaging.email_event v \
         WHERE v.provider_id = e.provider_id AND v.kind = ANY($5::text[]))) u \
         ORDER BY queued DESC, email ASC, id DESC LIMIT 1",
        recipient,
        job,
        &templates,
        &emails,
        &untold,
        UPDATE_EMAIL_PREFIX
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| QuoteTextSent {
        quoted: told(&r.template_id, &r.subject_ids, job, r.email),
        id: r.id,
        template_id: r.template_id,
        at: r.at.to_jiff(),
    }))
}

/// D-5640: the count a row told: 1 for `first_quote`, else its subject entry other than `job`
/// (and, on an email row, other than her address row T730 appends last) that is a count
/// (`Uuid::from_u128`, 1 to `u32::MAX`); `None` when it carries none.
fn told(template: &str, subject: &[Uuid], job: Uuid, email: bool) -> Option<u32> {
    if template == QUOTE_TEXT_TEMPLATES[0] {
        return Some(1);
    }
    let ids = match subject.split_last() {
        Some((_, rest)) if email => rest,
        _ => subject,
    };
    ids.iter()
        .filter(|id| **id != job)
        .find_map(|id| u32::try_from(id.as_u128()).ok().filter(|n| *n > 0))
}

/// TXT-016, TXT-018, D-5639: whether an `update.first_quote` email to `recipient` about `job` was
/// dropped or bounced, failed or was suppressed, so the first quote was never told and the
/// first-quote text is due once texting is possible.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn untold_first_quote_email(
    db: &mut PgConnection,
    recipient: Uuid,
    job: Uuid,
) -> Result<bool, OutboxError> {
    let template = format!("{UPDATE_EMAIL_PREFIX}{}", QUOTE_TEXT_TEMPLATES[0]);
    let untold = UNTOLD_EVENTS.map(String::from);
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM messaging.email_outbox e \
         WHERE e.recipient_id = $1 AND $2 = ANY(e.subject_ids) AND e.template_id = $3 \
         AND (e.status = 'dropped' OR EXISTS (SELECT 1 FROM messaging.email_event v \
         WHERE v.provider_id = e.provider_id AND v.kind = ANY($4::text[])))) AS \"untold!\"",
        recipient,
        job,
        template,
        &untold
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_is_read_past_the_job_and_an_email_address_row() {
        let job = Uuid::from_u128(0xabc_0000_0000_0000_0001);
        let row = Uuid::from_u128(0xdef_0000_0000_0000_0002);
        let three = Uuid::from_u128(3);
        assert_eq!(told("first_quote", &[job], job, false), Some(1));
        assert_eq!(told("quote_update", &[job, three], job, false), Some(3));
        assert_eq!(told("quote_update", &[job, three, row], job, true), Some(3));
        assert_eq!(told("quote_update", &[job, row], job, true), None);
        assert_eq!(told("quote_update", &[job, row], job, false), None);
        assert_eq!(told("quote_update", &[job], job, false), None);
    }
}

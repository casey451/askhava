//! The message log and its 30-day body cut (TXT-019, TXT-020, CON-022, CON-036; D-093, D-1851 to
//! D-1853).
//!
//! `messaging.outbox` is the outbound message log: each row holds the recipient's vault reference
//! (never the number), the template ID and recipient kind, the language its body was rendered in
//! ([`language_of`], set when the row is queued), the request among its subject IDs, the sent time,
//! the attempts and, through its provider SID, Twilio's status; [`message_log`] reads one entry.
//! A sent row keeps no body from the send on (SEC-037); [`cut_bodies`] cuts the body of any row
//! still unsent 30 days after it was queued, which also ends it as `failed`, so no body outlives
//! [`BODY_KEPT_FOR`]. No row is deleted: the entry keeps its template ID for the consent-log
//! period (CON-022). An inbound text is logged in `messaging.inbox` by its message ID and time
//! only; its raw and scrubbed words are never stored in `messaging` (D-093, D-801). A texted
//! opt-out keeps how it was detected and its inbox row in `messaging.optout_event`, which nothing
//! here touches (CON-036). [`CUT_KIND`] is the daily task that runs the cut.

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::OutboxError;
use crate::service;
use crate::status::DeliveryStatus;
use crate::templates::{Language, Registry};

/// How long a text's body may be kept after it was queued (TXT-020).
pub const BODY_KEPT_FOR: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The `core.task` kind of the daily body cut (TXT-020).
pub const CUT_KIND: &str = "messaging.cut_bodies";

const DAY: SignedDuration = SignedDuration::from_hours(24);

/// One outbound message log entry (TXT-019). No field holds a phone number or a body word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The recipient's vault contact or business address ID, never the number.
    pub recipient: Uuid,
    /// `homeowner` or `business`.
    pub recipient_kind: String,
    /// The template ID, which names the kind of text and is all that is kept of the body.
    pub template_id: String,
    /// The body's language, if it matched one language's template (D-1851).
    pub language: Option<Language>,
    /// The record IDs the text is about; the request's ID is among them (D-1852).
    pub subject: Vec<Uuid>,
    /// When the carrier took it, if it was sent.
    pub sent_at: Option<Timestamp>,
    /// Twilio's status as recorded (TXT-013).
    pub status: DeliveryStatus,
    /// How many sends were tried.
    pub attempts: i32,
    /// Whether a body is still stored (only for an unsent text younger than 30 days).
    pub body_kept: bool,
}

/// The language whose template `template_id` the rendered `body` fits, or `None` when it fits
/// neither or both (TXT-019, D-1851). Each placeholder may stand for any text; the fixed parts must
/// appear in order, the first at the start and the last at the end.
#[must_use]
pub(crate) fn language_of(template_id: &str, body: &str) -> Option<&'static str> {
    let registry = Registry::embedded().ok()?;
    let fits = |language| {
        registry
            .template(template_id, language)
            .is_some_and(|(text, _)| fits_template(text, body))
    };
    match (fits(Language::En), fits(Language::Es)) {
        (true, false) => Some("en"),
        (false, true) => Some("es"),
        _ => None,
    }
}

/// Whether `body` is `template` with each `{name}` replaced by some text.
fn fits_template(template: &str, body: &str) -> bool {
    let mut pieces = template.split('{');
    let Some(mut rest) = body.strip_prefix(pieces.next().unwrap_or_default()) else {
        return false;
    };
    let tails: Vec<&str> = pieces
        .map(|piece| piece.split_once('}').map_or(piece, |(_, tail)| tail))
        .collect();
    let Some((last, middle)) = tails.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        let Some(at) = rest.find(part) else {
            return false;
        };
        rest = &rest[at + part.len()..];
    }
    rest.ends_with(last)
}

/// Cuts the body of every text still unsent [`BODY_KEPT_FOR`] after it was queued, as of `now`,
/// ending it as `failed`; the row and its template ID stay (TXT-020, CON-022). Returns how many
/// rows it cut. Safe to run twice: a cut row has no body left.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn cut_bodies(db: &mut PgConnection, now: Timestamp) -> Result<u64, OutboxError> {
    let before = now - BODY_KEPT_FOR;
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'failed', body = NULL \
         WHERE body IS NOT NULL AND status IN ('queued', 'sending') AND created_at <= $1",
        before.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|done| done.rows_affected())
    .map_err(|_| OutboxError::Database)
}

/// The message log entry of outbox row `id` (TXT-019).
///
/// # Errors
///
/// [`OutboxError::NotFound`] or [`OutboxError::Database`].
pub async fn message_log(db: &mut PgConnection, id: Uuid) -> Result<LogEntry, OutboxError> {
    let row = sqlx::query!(
        "SELECT recipient_id, recipient_kind, template_id, language, subject_ids, sent_at, \
         attempts, body IS NOT NULL AS \"body_kept!\" FROM messaging.outbox WHERE id = $1",
        id
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?
    .ok_or(OutboxError::NotFound)?;
    Ok(LogEntry {
        recipient: row.recipient_id,
        recipient_kind: row.recipient_kind,
        template_id: row.template_id,
        language: row.language.map(|code| {
            if code == "es" {
                Language::Es
            } else {
                Language::En
            }
        }),
        subject: row.subject_ids,
        sent_at: row.sent_at.map(|t| t.to_jiff()),
        status: service::delivery_status(db, id).await?,
        attempts: row.attempts,
        body_kept: row.body_kept,
    })
}

/// Queues the next daily [`CUT_KIND`] task, one per day (TXT-020).
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_cut(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let mut task = NewTask::new(CUT_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{CUT_KIND}:{}", run_at.as_second() / 86_400));
    enqueue(db, &task).await.map(|_| ())
}

/// The [`CUT_KIND`] handler: [`cut_bodies`] at the wall clock, then the next day's cut (TXT-020).
///
/// # Errors
///
/// A fixed [`HandlerError`] when the database call fails.
pub async fn run_cut(pool: PgPool, _task: Claimed) -> Result<(), HandlerError> {
    let failed = HandlerError("message body cut database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    let now = Timestamp::now();
    cut_bodies(&mut conn, now).await.map_err(|_| failed)?;
    schedule_cut(&mut conn, now).await.map_err(|_| failed)
}

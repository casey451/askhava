//! The sender of the homeowner texts a job event asks for: the `jobs.homeowner_text` handler
//! (JOB-010, JOB-011, JOB-012, JOB-025, TXT-042, TXT-040, QH-001, QH-006, QH-007, SEC-037, CON-021,
//! CON-028; D-871, D-872, D-1030 (A), D-1215, D-1301 to D-1303).
//!
//! `jobs` holds no homeowner contact, so a move that texts the homeowner queues a `core.task` of
//! kind `jobs::api::TEXT_KIND` with the job ID, the job's version, the template ID and the numbers
//! the template needs (D-871). [`job_text_step`] sends it: it skips a job whose version moved on
//! since the event (`jobs::api::text_due`, so a held text of a job that closed meanwhile is
//! dropped while a close text, whose version is a final status's, still goes: QH-007, D-461),
//! finds the request and homeowner from the job ID (the `job.<trade>.<property>` intake answer
//! whose choice is the job ID, or the request's first job, D-993), skips a contact with no consent
//! record (CON-021) and asks `messaging::api::send_window` when it may go: an opted-out number
//! gets nothing (her job carries on by email and the quote page, CON-028), a number outside the
//! area-code table nothing (QH-003), and outside 8 a.m. to 8 p.m. in Arizona or her area-code
//! zone the same event is queued again for the next instant both are open (QH-001, QH-006). Intake
//! never sees her number (D-1196). Inside the window it renders the template in her language with
//! the request number and, for TXT-042, `quoted`, `count` and her quote-page link, whose token
//! `jobs::api::quote_token::issue` makes at send time (SEC-037; never in a payload or a log), and
//! queues one outbox row keyed on template, contact and job, in one transaction with the token:
//! a second run finds the row and rolls its new token back, so the queued link stays live.

use std::sync::Arc;

use askhava_core::api::{Claimed, Clock, HandlerError, IdSecret, NewTask, SystemClock, enqueue};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{TEXT_KIND, text_due};
use askhava_messaging::api::{Language, NewText, Relation, SendWindow, render, send_window};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::IntakeError;
use crate::rules::language_from_code;
use crate::service::{Queued, may_text};

/// The TXT-042 template, the one that carries the quote-page link and the counts.
pub const ALL_QUOTES_IN_TEMPLATE: &str = "all_quotes_in";
/// Every template a `jobs.homeowner_text` event may name: JOB-010, JOB-011, JOB-012, JOB-025's
/// closing note and TXT-042 (`jobs`' `timeout_text` and `handlers`).
pub const JOB_TEXT_TEMPLATES: [&str; 5] = [
    "no_quotes_yet",
    "closed_no_quotes",
    "closed_no_pick",
    "closed_no_quote_left",
    ALL_QUOTES_IN_TEMPLATE,
];
/// Her private quote page (PICK-010, `/q/{token}`, served by `portals`).
const QUOTE_PAGE: &str = "https://askhava.com/q/";
const BAD_PAYLOAD: &str = "bad jobs homeowner-text payload";
const FAILED: &str = "the jobs homeowner-text step failed";

/// What the sender needs and never logs: the `ID_SECRET` as the link-token key and as the outbox
/// key, and the vault `messaging` reads her number from (SEC-037, SEC-018, PRIV-004).
pub struct JobTextKeys {
    link: IdSecret,
    outbox: SecretString,
    vault: Vault,
}

impl JobTextKeys {
    /// The keys: `link` signs quote-page tokens, `outbox` derives text keys, `vault` holds her
    /// number for `messaging::api::send_window`.
    #[must_use]
    pub fn new(link: IdSecret, outbox: SecretString, vault: Vault) -> Self {
        Self {
            link,
            outbox,
            vault,
        }
    }
}

/// One `jobs.homeowner_text` event, parsed once (D-871).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobText {
    /// The job.
    pub job: Uuid,
    /// The job's version when the event was queued; a newer one means it moved on.
    pub version: i32,
    /// One of [`JOB_TEXT_TEMPLATES`].
    pub template: &'static str,
    /// The numbers the template needs: `job`, and for TXT-042 `quoted` and `count`.
    pub values: Value,
}

/// Parses a `jobs.homeowner_text` payload; `None` for a template not in [`JOB_TEXT_TEMPLATES`]
/// or a missing field.
#[must_use]
pub fn parse_job_text(payload: &Value) -> Option<JobText> {
    let name = payload["template"].as_str()?;
    Some(JobText {
        job: Uuid::parse_str(payload["job"].as_str()?).ok()?,
        version: i32::try_from(payload["version"].as_i64()?).ok()?,
        template: JOB_TEXT_TEMPLATES.into_iter().find(|t| *t == name)?,
        values: payload["values"].clone(),
    })
}

/// Why a job text was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobTextSkip {
    /// The job's version moved on since the event (a pick, a close, a return; QH-007, TXT-042).
    MovedOn,
    /// No request names the job.
    NoRequest,
    /// The homeowner has no consent record (CON-021).
    NoConsent,
    /// Her number is on the suppression list (CON-028).
    OptedOut,
    /// Her number is not one quiet hours can place (QH-003).
    NotTextable,
    /// The same text is already in the outbox (TXT-011).
    AlreadyQueued,
}

/// What [`job_text_step`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobTextStep {
    /// The text was queued.
    Queued(Queued),
    /// Quiet hours: the event was queued again for this instant (QH-006).
    Held(Timestamp),
    /// Nothing was sent.
    Skipped(JobTextSkip),
}

/// The `jobs.homeowner_text` handler `server` registers (D-1030 (A)): [`job_text_step`] on the
/// task's payload at the system clock's now.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed step.
pub async fn run_job_text(
    pool: PgPool,
    keys: Arc<JobTextKeys>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let text = parse_job_text(&task.payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    job_text_step(&mut db, &keys, &text, SystemClock.now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// Sends one job text at `now` (JOB-010 to JOB-012, TXT-042, CON-021, CON-028, QH-001, QH-006,
/// QH-007): the checks in the module order, then the text. Safe to run twice: the outbox keeps
/// one row per template, contact and job. Call it outside a transaction (the vault read).
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Outbox`], [`IntakeError::Template`] or
/// [`IntakeError::Database`].
pub async fn job_text_step(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    now: Timestamp,
) -> Result<JobTextStep, IntakeError> {
    if !due(db, text).await? {
        return Ok(JobTextStep::Skipped(JobTextSkip::MovedOn));
    }
    let Some((contact, language)) = homeowner_of(db, text.job).await? else {
        return Ok(JobTextStep::Skipped(JobTextSkip::NoRequest));
    };
    if !may_text(db, contact).await? {
        return Ok(JobTextStep::Skipped(JobTextSkip::NoConsent));
    }
    let window = send_window(db, &keys.vault, contact, now)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    let skip = match window {
        SendWindow::Now => return queue(db, keys, text, (contact, &language), now).await,
        SendWindow::Hold { until } => return hold(db, text, until).await,
        SendWindow::OptedOut => JobTextSkip::OptedOut,
        SendWindow::NotTextable => JobTextSkip::NotTextable,
    };
    Ok(JobTextStep::Skipped(skip))
}

/// True while the job is still at the event's version (`jobs::api::text_due`).
async fn due(db: &mut PgConnection, text: &JobText) -> Result<bool, IntakeError> {
    text_due(db, text.job, text.version)
        .await
        .map_err(|_| IntakeError::Job)
}

/// The homeowner's vault contact ID and the language code of the newest request that opened job
/// `job` (D-993), open or closed.
async fn homeowner_of(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Option<(Uuid, String)>, IntakeError> {
    sqlx::query!(
        "SELECT h.contact_id, c.language FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE c.job_id = $1 OR EXISTS (SELECT 1 FROM intake.intake_answer a \
         WHERE a.conversation_id = c.id AND a.question_id LIKE 'job.%' AND a.choice = $2) \
         ORDER BY c.opened_at DESC LIMIT 1",
        job,
        job.simple().to_string()
    )
    .fetch_optional(db)
    .await
    .map(|r| r.map(|r| (r.contact_id, r.language)))
    .map_err(|_| IntakeError::Database)
}

/// Queues the same event again for `until`, the next instant both zones are open (QH-006); the
/// dedupe key names that instant, so a second run queues it once.
async fn hold(
    db: &mut PgConnection,
    text: &JobText,
    until: Timestamp,
) -> Result<JobTextStep, IntakeError> {
    let payload = json!({"job": text.job, "version": text.version,
        "template": text.template, "values": text.values});
    let mut task = NewTask::new(TEXT_KIND, payload, until);
    let run = until.as_microsecond();
    task.dedupe_key = Some(format!("{TEXT_KIND}.{}.{}.{run}", text.job, text.template));
    enqueue(db, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    Ok(JobTextStep::Held(until))
}

/// Renders and queues the text in one transaction with its quote-page token (TXT-042, SEC-037),
/// re-checking the version inside it; an outbox row already there rolls the new token back.
async fn queue(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    (contact, language): (Uuid, &str),
    now: Timestamp,
) -> Result<JobTextStep, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if !due(&mut tx, text).await? {
        return Ok(JobTextStep::Skipped(JobTextSkip::MovedOn));
    }
    let link = if text.template == ALL_QUOTES_IN_TEMPLATE {
        let token = issue(&mut tx, &keys.link, text.job, now)
            .await
            .map_err(|_| IntakeError::Job)?;
        Some(format!("{QUOTE_PAGE}{}", token.as_str()))
    } else {
        None
    };
    let body = job_text_body(text, language_from_code(language), link.as_deref())?;
    let new = NewText {
        template_id: text.template,
        subject: &[text.job],
        recipient: contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox_id, created) = askhava_messaging::api::enqueue(&mut tx, &keys.outbox, &new)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    if !created {
        return Ok(JobTextStep::Skipped(JobTextSkip::AlreadyQueued));
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    let template = text.template;
    Ok(JobTextStep::Queued(Queued {
        outbox_id,
        template,
    }))
}

/// The text of `text` in `language` (Spanish only for a Spanish request): `{job}` from the
/// request number, `{quoted}` and `{count}` from the event, `{link}` from `link` (TXT-040,
/// TXT-042).
///
/// # Errors
///
/// [`IntakeError::Template`] for a value the template needs and the event lacks.
pub fn job_text_body(
    text: &JobText,
    language: Language,
    link: Option<&str>,
) -> Result<String, IntakeError> {
    let number = |key: &str| text.values[key].as_i64().map(|n| n.to_string());
    let owned: Vec<(&str, String)> = ["job", "quoted", "count"]
        .into_iter()
        .filter_map(|key| number(key).map(|n| (key, n)))
        .chain(link.map(|l| ("link", l.to_owned())))
        .collect();
    let values: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    render(text.template, language, language == Language::Es, &values)
        .map(|t| t.body)
        .map_err(|_| IntakeError::Template)
}

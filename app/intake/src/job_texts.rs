//! The sender of the homeowner texts a job event asks for: the `jobs.homeowner_text` handler
//! (JOB-009, JOB-010, JOB-011, JOB-012, JOB-015, JOB-016, JOB-017, JOB-025, JOB-027, TXT-016,
//! TXT-029, TXT-042, TXT-040, QH-001, QH-006, QH-007, SEC-037, CON-021, CON-028; D-871, D-872,
//! D-1030 (A), D-1215, D-1301 to D-1303, D-1842).
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
//! the request number and, for TXT-042, `quoted` and `count`, and for TXT-042 and TXT-016
//! (`first_quote`, sent with the first-quote move, D-1517) her quote-page link, and for JOB-017's
//! `feedback_request` the feedback page's `/f/` link (D-1842), whose token
//! `jobs::api::quote_token::issue` makes at send time (SEC-037; never in a payload or a log), and
//! queues one outbox row keyed on template, contact and job, in one transaction with the token:
//! a second run finds the row and rolls its new token back, so the queued link stays live.
//!
//! T328 (PICK-046, PICK-035, QH-004, D-3579): `picks`' `pick_ended` queues `pick_lapsed` with the
//! IDs `{job, pick, business}` when a pick `payments` ended has no backup. Its body is
//! `picks::api::lapse_text` from the business's public name, the TXT-031 lines
//! (`quotes::api::list_lines`) of the quotes `picks::api::lapse_plan` leaves her and her `/q/`
//! link; its outbox row is keyed on the job and the pick, so each ended pick gets one. Consent,
//! opt-out and the area-code check apply as above; outside the window a regular job's event is
//! held, and an emergency job's (read from the job now, never from the event) goes at once through
//! `messaging::api::enqueue_exempt` with `Exemption::EmergencyJob`. `server` registers
//! [`run_job_text_at`] on the worker's clock.

use std::sync::Arc;

use askhava_businesses::api::public_names;
use askhava_core::api::{
    Claimed, Clock, HandlerError, IdSecret, NewTask, SystemClock, brand, enqueue,
};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{
    CHECK_IN_DAY3, CHECK_IN_DAY14, FEEDBACK_REQUEST, PROBLEM_REPLY, TEXT_KIND, current, text_due,
};
use askhava_messaging::api::{
    Exemption, Language, NewText, Relation, SendWindow, enqueue_exempt, render, send_window,
};
use askhava_picks::api::{PICK_LAPSED_TEMPLATE, lapse_plan, lapse_text};
use askhava_privacy::api::Vault;
use askhava_quotes::api::{list_lines, page_quotes};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{self, IntakeError};
use crate::rules::language_from_code;
use crate::service::{Queued, may_text};

/// The TXT-042 template, the one that carries the quote-page link and the counts.
pub const ALL_QUOTES_IN_TEMPLATE: &str = "all_quotes_in";
/// The TXT-016 template, sent with the first-quote move to Quotes ready; it carries the link too.
const FIRST_QUOTE_TEMPLATE: &str = "first_quote";
/// JOB-027's close text after the unreachable refund (`picks`' `close_unreachable`, D-1842).
pub const UNREACHABLE_CLOSED_TEMPLATE: &str = "unreachable_closed";
/// JOB-027's 72-hour question to an email-contact homeowner (sent by T281's step, D-1817).
pub const EMAILED_QUESTION_TEMPLATE: &str = "emailed_question";
/// The reply to MORE or MÁS at a check-in: the full list of answers (JOB-015, JOB-028, D-1842).
pub const CHECK_IN_MORE_TEMPLATE: &str = "check_in_more";
/// Every template a `jobs.homeowner_text` event may name: JOB-010, JOB-011, JOB-012, JOB-025's
/// closing note, TXT-042 and TXT-016 (`jobs`' `timeout_text`, `handlers` and `first_quote`), the
/// day-3 and day-14 check-ins and the MORE list (JOB-015), the feedback request (JOB-017), the
/// TXT-029 PROBLEM reply (JOB-016), JOB-027's close text and 72-hour question (D-1842) and
/// PICK-046's text after a pick ended with no backup (`picks`' `pick_ended`, D-3579).
pub const JOB_TEXT_TEMPLATES: [&str; 14] = [
    "no_quotes_yet",
    "closed_no_quotes",
    "closed_no_pick",
    "closed_no_quote_left",
    ALL_QUOTES_IN_TEMPLATE,
    FIRST_QUOTE_TEMPLATE,
    CHECK_IN_DAY3,
    CHECK_IN_DAY14,
    CHECK_IN_MORE_TEMPLATE,
    FEEDBACK_REQUEST,
    PROBLEM_REPLY,
    UNREACHABLE_CLOSED_TEMPLATE,
    EMAILED_QUESTION_TEMPLATE,
    PICK_LAPSED_TEMPLATE,
];
/// The page a template's `{link}` opens, by path prefix on the brand's domain: her quote page
/// `/q/` for TXT-016, TXT-042 and PICK-046 (PICK-010), the private feedback page `/f/` for JOB-017 (T230,
/// D-1821); both take the job's quote-page link token (SEC-037). Other templates carry no link.
const LINKED_TEMPLATES: [(&str, &str); 4] = [
    (ALL_QUOTES_IN_TEMPLATE, "q"),
    (FIRST_QUOTE_TEMPLATE, "q"),
    (PICK_LAPSED_TEMPLATE, "q"),
    (FEEDBACK_REQUEST, "f"),
];
/// The link `https://<domain>/<page>/<token>` (PICK-010, TXT-016, JOB-017, D-1932; the `/q/` and
/// `/f/` routes are `portals`').
fn page_link(page: &str, token: &str) -> String {
    format!("https://{}/{page}/{token}", brand().domain())
}

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
    run_job_text_at(pool, keys, task, SystemClock.now()).await
}

/// [`run_job_text`] at `now`, the worker clock's time: what `server` registers, so a manual clock
/// drives quiet hours in the registry tests (T328, D-3579).
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed step.
pub async fn run_job_text_at(
    pool: PgPool,
    keys: Arc<JobTextKeys>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let text = parse_job_text(&task.payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    job_text_step(&mut db, &keys, &text, now)
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
        SendWindow::Now | SendWindow::Hold { .. } if exempt(db, text).await? => {
            let exemption = Some(Exemption::EmergencyJob);
            return queue(db, keys, text, (contact, &language), (now, exemption)).await;
        }
        SendWindow::Now => return queue(db, keys, text, (contact, &language), (now, None)).await,
        SendWindow::Hold { until } => return hold(db, text, until).await,
        SendWindow::OptedOut => JobTextSkip::OptedOut,
        SendWindow::NotTextable => JobTextSkip::NotTextable,
    };
    Ok(JobTextStep::Skipped(skip))
}

/// QH-004, D-3579: PICK-046's text of a job the homeowner chose as an emergency may go outside
/// the window; the choice is read from the job now, never from the event.
async fn exempt(db: &mut PgConnection, text: &JobText) -> Result<bool, IntakeError> {
    if text.template != PICK_LAPSED_TEMPLATE {
        return Ok(false);
    }
    let row = current(db, text.job).await.map_err(|_| IntakeError::Job)?;
    Ok(row.emergency)
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

/// The homeowner of a job as the page pick needs her: the request, her vault contact ID (never
/// her number) and the request's language (D-993, D-1516).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobHomeowner {
    /// The open request (conversation) that opened the job.
    pub conversation: Uuid,
    /// Her vault contact ID.
    pub contact: Uuid,
    /// The request's language.
    pub language: Language,
}

/// The homeowner of live job `job`: the open request that opened it (her newest, since a
/// homeowner has one open request), her vault contact ID and the request's language, found as
/// [`homeowner_of`] finds a job's request; `None` when no open request opened it (PICK-013,
/// PICK-016, TERMS-016, D-1516). The page pick sends its code to this contact and stores the
/// terms acceptance on this request.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn job_homeowner(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Option<JobHomeowner>, IntakeError> {
    let Some((conversation, _)) = models::request_of_job(db, job).await? else {
        return Ok(None);
    };
    Ok(models::recipient(db, conversation)
        .await?
        .map(|(contact, code)| JobHomeowner {
            conversation,
            contact,
            language: language_from_code(&code),
        }))
}

/// Takes the homeowner lock of request `conversation` in the caller's transaction (the lock
/// [`crate::api::on_opt_out`] and [`crate::api::answer_text_pick`] take), then says whether job
/// `job` is paused by her (CON-029, D-1605): a CANCEL committing meanwhile is waited for and seen,
/// so a page pick stored in the same transaction never lands on a paused job.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`] or [`IntakeError::Database`].
pub async fn paused_under_lock(
    tx: &mut PgConnection,
    conversation: Uuid,
    job: Uuid,
) -> Result<bool, IntakeError> {
    let (homeowner, _) = models::conversation_state(tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(tx, homeowner).await?;
    models::is_paused(tx, job).await
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

/// Renders and queues the text in one transaction with its link token (TXT-016, TXT-042,
/// JOB-017, PICK-046, SEC-037), re-checking the version inside it; an outbox row already there
/// rolls the new token back. PICK-046's row is keyed on the job and the pick, so each ended pick
/// gets its own; `exemption` lets an emergency job's go at any hour (QH-004).
async fn queue(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    (contact, language): (Uuid, &str),
    (now, exemption): (Timestamp, Option<Exemption>),
) -> Result<JobTextStep, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if !due(&mut tx, text).await? {
        return Ok(JobTextStep::Skipped(JobTextSkip::MovedOn));
    }
    let body = body_of(&mut tx, keys, text, language_from_code(language), now).await?;
    let subject: Vec<Uuid> = std::iter::once(text.job)
        .chain(id_value(&text.values, "pick").filter(|_| text.template == PICK_LAPSED_TEMPLATE))
        .collect();
    let new = NewText {
        template_id: text.template,
        subject: &subject,
        recipient: contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let queued = match exemption {
        Some(e) => enqueue_exempt(&mut tx, &keys.outbox, &new, e).await,
        None => askhava_messaging::api::enqueue(&mut tx, &keys.outbox, &new).await,
    };
    let (outbox_id, created) = queued.map_err(|_| IntakeError::Outbox)?;
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

/// The body of `text` in `language`: the template's link, with a token issued in `tx` (SEC-037),
/// then PICK-046's [`lapsed_body`] or the template's own [`job_text_body`].
async fn body_of(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    language: Language,
    now: Timestamp,
) -> Result<String, IntakeError> {
    let page = LINKED_TEMPLATES
        .iter()
        .find_map(|(id, page)| (*id == text.template).then_some(*page));
    let link = match page {
        Some(page) => {
            let token = issue(&mut *tx, &keys.link, text.job, now)
                .await
                .map_err(|_| IntakeError::Job)?;
            Some(page_link(page, token.as_str()))
        }
        None => None,
    };
    match (text.template == PICK_LAPSED_TEMPLATE, link) {
        (true, Some(link)) => lapsed_body(tx, text, language, (&link, now)).await,
        (_, link) => job_text_body(text, language, link.as_deref()),
    }
}

/// An ID the event carries in `values` under `key` (PICK-046's `pick` and `business`).
fn id_value(values: &Value, key: &str) -> Option<Uuid> {
    values[key].as_str().and_then(|v| Uuid::parse_str(v).ok())
}

/// PICK-046, PICK-035, PICK-016, D-3579: `picks::api::lapse_text` naming the ended business by
/// its public name, with the TXT-031 lines (`quotes::api::list_lines`) of the quotes
/// `picks::api::lapse_plan` leaves her, never the ended business's own, and `link` her quote
/// page; with none left it is the "we'll text you" text.
async fn lapsed_body(
    tx: &mut PgConnection,
    text: &JobText,
    language: Language,
    (link, now): (&str, Timestamp),
) -> Result<String, IntakeError> {
    let business = id_value(&text.values, "business").ok_or(IntakeError::Template)?;
    let page = page_quotes(&mut *tx, text.job)
        .await
        .map_err(|_| IntakeError::Quotes)?;
    let quotes: Vec<(u32, Uuid)> = (1..)
        .zip(page)
        .filter(|(_, q)| !q.unavailable)
        .map(|(n, q)| (n, q.business))
        .collect();
    let others = lapse_plan((now, false), business, &quotes).others;
    let ids: Vec<Uuid> = std::iter::once(business)
        .chain(quotes.iter().map(|(_, b)| *b))
        .collect();
    let names = public_names(&mut *tx, &ids)
        .await
        .map_err(|_| IntakeError::Database)?;
    let name = names.iter().find(|(id, _)| *id == business);
    let name = name.ok_or(IntakeError::Template)?.1.clone();
    let lines: Vec<String> = list_lines(&mut *tx, text.job, language, &names)
        .await
        .map_err(|_| IntakeError::Quotes)?
        .into_iter()
        .filter(|(n, _)| others.contains(n))
        .map(|(_, line)| line)
        .collect();
    Ok(lapse_text(&name, &lines, link, language))
}

/// The text of `text` in `language` (Spanish only for a Spanish request): `{job}` from the
/// request number, `{quoted}` and `{count}` from the event, `{business}` (the business's public
/// name, JOB-027's texts) from the event, `{link}` from `link` (TXT-040, TXT-042, JOB-027).
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
        .chain(
            text.values["business"]
                .as_str()
                .map(|b| ("business", b.to_owned())),
        )
        .chain(link.map(|l| ("link", l.to_owned())))
        .collect();
    let values: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    render(text.template, language, language == Language::Es, &values)
        .map(|t| t.body)
        .map_err(|_| IntakeError::Template)
}

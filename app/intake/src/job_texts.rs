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
//!
//! T446 (PICK-046, CON-028, CON-021, PRIV-004; D-3579, D-3880): when that text cannot go (no
//! consent record, opted out, not textable) and she has an email on file
//! (`messaging::api::homeowner_has_email`, read outside any transaction: the vault refuses one),
//! [`lapsed_by_email`] queues one [`PICK_LAPSED_EMAIL`] (`messaging`'s `Notice::PickLapsed`: the
//! business's public name and, while other quotes remain, her `/q/` link) keyed on the job and the
//! pick, so a rerun stores nothing more. The footer postal line reaches it through
//! [`JobTextKeys::with_postal`]; without it the step sends nothing and logs why. A text that can
//! go goes as above, never both.
//!
//! T471 (PICK-047, QH-004, D-3750): `picks` queues `pick_moved` with `{job, pick, moved, business}`
//! when an ended pick moved to her backup. Its body is `picks::api::backup_text` with the ended
//! business's and the backup's public names (the backup's business read through
//! `picks::api::pick_business` from the `moved` pick); it carries no link and has no email. Like
//! `pick_lapsed` it is exempt on an emergency job and keyed and held on the ended pick.

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
    Audience, EmailClass, Exemption, Language, NewEmail, NewText, Notice, OnDomain, PostalAddress,
    Recipient, Relation, SendWindow, enqueue_exempt, homeowner_has_email, queue_email, render,
    send_window,
};
use askhava_picks::api::{
    PICK_LAPSED_TEMPLATE, PICK_MOVED_TEMPLATE, backup_text, ended_businesses, lapse_plan,
    lapse_text, pick_business,
};
use askhava_privacy::api::Vault;
use askhava_quotes::api::{list_lines, page_quotes, shown_numbers};
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
/// TXT-029 PROBLEM reply (JOB-016), JOB-027's close text and 72-hour question (D-1842),
/// PICK-046's text after a pick ended with no backup (`picks`' `pick_ended`, D-3579) and PICK-047's
/// after it moved to her backup (D-3750).
pub const JOB_TEXT_TEMPLATES: [&str; 15] = [
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
    PICK_MOVED_TEMPLATE,
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

/// PICK-046's email when she can't be texted (T446, D-3579); with the contact and the subject IDs
/// (the job and the pick) it makes the email's key.
pub const PICK_LAPSED_EMAIL: &str = "pick_lapsed_email";
/// Her portal page, the email's "Manage email notices" link.
const MANAGE: OnDomain = OnDomain("/h");

const BAD_PAYLOAD: &str = "bad jobs homeowner-text payload";
const FAILED: &str = "the jobs homeowner-text step failed";

/// What the sender needs and never logs: the `ID_SECRET` as the link-token key and as the outbox
/// and email key, the vault `messaging` reads her number and email from (SEC-037, SEC-018,
/// PRIV-004), and the footer postal line PICK-046's email needs (SEC-028; `None` sends no email).
pub struct JobTextKeys {
    link: IdSecret,
    pub(crate) outbox: SecretString,
    pub(crate) vault: Vault,
    postal: Option<PostalAddress>,
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
            postal: None,
        }
    }

    /// The same keys with the footer's checked postal line, so PICK-046's email can go when she
    /// can't be texted (T446, SEC-028).
    #[must_use]
    pub fn with_postal(self, postal: PostalAddress) -> Self {
        Self {
            postal: Some(postal),
            ..self
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

/// [`run_job_text_at`] at the system clock's now (D-1030 (A)). `server` registers
/// [`run_job_text_at`]; this stays for `src/tests/job_texts.rs`, a file T328 may not change
/// (D-3573).
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

/// The `jobs.homeowner_text` handler `server` registers (D-1030 (A)): [`job_text_step`] on the
/// task's payload at `now`, the worker clock's time, so a manual clock drives quiet hours in the
/// registry tests (T328, D-3579).
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
        lapsed_by_email(db, keys, text, (contact, &language), now).await?;
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
    lapsed_by_email(db, keys, text, (contact, &language), now).await?;
    Ok(JobTextStep::Skipped(skip))
}

/// PICK-046, CON-028, CON-021, PRIV-004, D-3579, D-3880: PICK-046's text could not go, so with
/// the postal line set and an email on file (read before the transaction: the vault refuses one)
/// one [`PICK_LAPSED_EMAIL`] keyed on the job and the pick, in one transaction with its link
/// token; an email already stored rolls the new token back. Any other template sends nothing.
async fn lapsed_by_email(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    (contact, language): (Uuid, &str),
    now: Timestamp,
) -> Result<(), IntakeError> {
    let Some(postal) = email_postal(db, keys, text, contact).await? else {
        return Ok(());
    };
    let pick = id_value(&text.values, "pick").ok_or(IntakeError::Template)?;
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if !due(&mut tx, text).await? {
        return Ok(());
    }
    let language = language_from_code(language);
    let (name, lines) = lapsed_parts(&mut tx, text, language, now).await?;
    let link = if lines.is_empty() {
        None
    } else {
        Some(quote_link(&mut tx, keys, text.job, now).await?)
    };
    let manage = MANAGE.to_string();
    let to = Recipient {
        id: contact,
        audience: Audience::Homeowner,
        language,
        manage_url: &manage,
    };
    let notice = Notice::PickLapsed {
        business: &name,
        quotes_link: link.as_deref(),
    };
    let subject = [text.job, pick];
    let email = NewEmail {
        template_id: PICK_LAPSED_EMAIL,
        subject: &subject,
        class: EmailClass::Notice,
        notice,
    };
    let (_, created) = queue_email(&mut tx, (&keys.outbox, postal), &to, &email, now)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    if created {
        tx.commit().await.map_err(|_| IntakeError::Database)?;
    }
    Ok(())
}

/// The footer postal line when PICK-046's email may go: the event is `pick_lapsed`, the line is
/// set (else a warn log naming only the job) and she has an email on file (PRIV-004; the vault
/// read, outside any transaction).
async fn email_postal<'k>(
    db: &mut PgConnection,
    keys: &'k JobTextKeys,
    text: &JobText,
    contact: Uuid,
) -> Result<Option<&'k PostalAddress>, IntakeError> {
    if text.template != PICK_LAPSED_TEMPLATE {
        return Ok(None);
    }
    let Some(postal) = keys.postal.as_ref() else {
        tracing::warn!(job = %text.job, "intake: PICK-046's email not sent, no footer postal line set");
        return Ok(None);
    };
    let has = homeowner_has_email(db, &keys.vault, contact)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(has.then_some(postal))
}

/// Her quote page `/q/<token>` with a token issued in `tx` (PICK-010, SEC-037).
pub(crate) async fn quote_link(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    job: Uuid,
    now: Timestamp,
) -> Result<String, IntakeError> {
    let token = issue(&mut *tx, &keys.link, job, now)
        .await
        .map_err(|_| IntakeError::Job)?;
    Ok(page_link("q", token.as_str()))
}

/// QH-004, D-3579, D-3750: PICK-046's and PICK-047's texts of a job the homeowner chose as an
/// emergency may go outside the window; the choice is read from the job now, never from the event.
async fn exempt(db: &mut PgConnection, text: &JobText) -> Result<bool, IntakeError> {
    if !per_pick(text) {
        return Ok(false);
    }
    let row = current(db, text.job).await.map_err(|_| IntakeError::Job)?;
    Ok(row.emergency)
}

/// PICK-046, PICK-047, D-3572, D-3750: a text sent once per ended pick (`pick_lapsed` and
/// `pick_moved`): its outbox key and hold key carry the ended pick.
fn per_pick(text: &JobText) -> bool {
    text.template == PICK_LAPSED_TEMPLATE || text.template == PICK_MOVED_TEMPLATE
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
/// dedupe key names that instant (and, for PICK-046's text, the pick, D-3572), so a second run
/// queues it once and two ended picks held to one instant stay two.
async fn hold(
    db: &mut PgConnection,
    text: &JobText,
    until: Timestamp,
) -> Result<JobTextStep, IntakeError> {
    let payload = json!({"job": text.job, "version": text.version,
        "template": text.template, "values": text.values});
    let mut task = NewTask::new(TEXT_KIND, payload, until);
    let run = until.as_microsecond();
    let pick = id_value(&text.values, "pick")
        .filter(|_| per_pick(text))
        .map_or_else(String::new, |p| format!(".{p}"));
    task.dedupe_key = Some(format!(
        "{TEXT_KIND}.{}.{}{pick}.{run}",
        text.job, text.template
    ));
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
        .chain(id_value(&text.values, "pick").filter(|_| per_pick(text)))
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
/// then PICK-046's [`lapsed_body`] or the template's own [`job_text_body`]; PICK-047's
/// [`moved_body`] has no link.
async fn body_of(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    text: &JobText,
    language: Language,
    now: Timestamp,
) -> Result<String, IntakeError> {
    if text.template == PICK_MOVED_TEMPLATE {
        return moved_body(tx, text, language).await;
    }
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

/// PICK-047, D-3750: `picks::api::backup_text` in `language` naming the ended pick's business
/// (`business`) and her backup's (the `moved` pick's) by their public names.
async fn moved_body(
    tx: &mut PgConnection,
    text: &JobText,
    language: Language,
) -> Result<String, IntakeError> {
    let business = id_value(&text.values, "business").ok_or(IntakeError::Template)?;
    let moved = id_value(&text.values, "moved").ok_or(IntakeError::Template)?;
    let backup = pick_business(&mut *tx, moved)
        .await
        .map_err(|_| IntakeError::Database)?;
    let names = public_names(&mut *tx, &[business, backup])
        .await
        .map_err(|_| IntakeError::Database)?;
    let name = |id: Uuid| {
        names
            .iter()
            .find(|(b, _)| *b == id)
            .map(|(_, n)| n.as_str())
            .ok_or(IntakeError::Template)
    };
    Ok(backup_text(name(business)?, name(backup)?, language))
}

/// An ID the event carries in `values` under `key` (PICK-046's and PICK-047's `pick` and
/// `business`, PICK-047's `moved`).
fn id_value(values: &Value, key: &str) -> Option<Uuid> {
    values[key].as_str().and_then(|v| Uuid::parse_str(v).ok())
}

/// PICK-046, PICK-035, PICK-016, D-3579, D-3571: `picks::api::lapse_text` naming the ended
/// business by its public name, with the TXT-031 lines (`quotes::api::list_lines`) of the quotes
/// `picks::api::lapse_plan` leaves her, never the ended business's own nor any business whose
/// pick on the job already ended (`picks::api::ended_businesses`), and `link` her quote page;
/// with none left it is the "we'll text you" text. PICK-008, D-2632, D-3999: each pickable page
/// quote is numbered by its business's shown number before `lapse_plan`, so `others` and the
/// lines use the one numbering a "<number> YES" picks by.
async fn lapsed_body(
    tx: &mut PgConnection,
    text: &JobText,
    language: Language,
    (link, now): (&str, Timestamp),
) -> Result<String, IntakeError> {
    let (name, lines) = lapsed_parts(tx, text, language, now).await?;
    Ok(lapse_text(&name, &lines, link, language))
}

/// PICK-046, PICK-035, PICK-008, D-3999: the ended business's public name and the TXT-031 lines
/// of the quotes left to her, as [`lapsed_body`] describes; the email uses only whether any is left.
async fn lapsed_parts(
    tx: &mut PgConnection,
    text: &JobText,
    language: Language,
    now: Timestamp,
) -> Result<(String, Vec<String>), IntakeError> {
    let business = id_value(&text.values, "business").ok_or(IntakeError::Template)?;
    let page = page_quotes(&mut *tx, text.job)
        .await
        .map_err(|_| IntakeError::Quotes)?;
    let ended = ended_businesses(&mut *tx, text.job)
        .await
        .map_err(|_| IntakeError::Database)?;
    let shown = shown_numbers(&mut *tx, text.job)
        .await
        .map_err(|_| IntakeError::Quotes)?;
    let pickable: Vec<(u32, Uuid)> = shown
        .into_iter()
        .filter(|(_, b)| page.iter().any(|q| q.business == *b && !q.unavailable))
        .collect();
    let ids: Vec<Uuid> = std::iter::once(business)
        .chain(pickable.iter().map(|(_, b)| *b))
        .collect();
    let quotes: Vec<(u32, Uuid)> = pickable
        .into_iter()
        .filter(|(_, b)| !ended.contains(b))
        .collect();
    let others = lapse_plan((now, false), business, &quotes).others;
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
    Ok((name, lines))
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

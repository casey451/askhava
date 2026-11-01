//! A request's timers and replies outside the question flow: the no-reply reminder and close, the
//! LIST reply and the call-back clock (INT-019, INT-021, INT-023, TXT-031; D-095, D-419, D-492,
//! D-493, D-1195 to D-1197).
//!
//! INT-019: every job opened in Intake gets two [`NO_REPLY_KIND`] tasks counted from its wait's
//! start (`jobs`' `timer_from`): the reminder at 24 hours, moved to 8 a.m. Arizona when it falls in
//! quiet hours, and the close at 72 hours. A homeowner message restarts the wait (`jobs`'
//! `restart_timer`, D-807) and queues a fresh pair, so the old pair finds `timer_from` moved and does
//! nothing. The close moves the job to Closed early at the 72-hour mark (or finds `jobs`' own
//! Intake timer did), closes the request once all its jobs are final, and texts the INT-019 line,
//! held to 8 a.m. at night. Consent is the first text (D-419), so every request with a consent
//! record gets both texts; the reminder goes once per job (the outbox key over template, contact
//! and job).
//!
//! INT-021: intake never calls the AI (no `askhava-ai` dependency, D-1030), so every step already
//! runs at once on the AI-off path: keyword rules and the fixed questions, no hold, no "we have
//! your message" text, no retry. TXT-031: [`answer_list`] answers LIST or LISTA with `quotes`' LIST
//! lines. INT-023: [`call_back_due`] and [`counts_urgent`] give the call-back deadline.

use askhava_core::api::{
    Claimed, HandlerError, NewTask, arizona, arizona_time, enqueue as enqueue_task,
};
use askhava_jobs::api::{JobRow, JobStatus, current, job_number, move_to, restart_timer};
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::Actor;
use askhava_quotes::api::{Reader, list_texts, shown_quotes, which_job_text};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::{self, IntakeError};
use crate::rules::language_from_code;
use crate::service::{Queued, TextIn, card_allowed};

/// The `core.task` kind of a job's no-reply step (INT-019).
pub const NO_REPLY_KIND: &str = "intake.no_reply";
/// The one reminder goes this long after the homeowner's last reply (INT-019).
pub const REMIND_AFTER: SignedDuration = SignedDuration::from_hours(24);
/// The job closes this long after the homeowner's last reply (INT-019, D-095).
pub const CLOSE_AFTER: SignedDuration = SignedDuration::from_hours(72);
/// The 24-hour reminder.
pub const REMINDER_TEMPLATE: &str = "intake_reminder";
/// The INT-019 closing text.
pub const CLOSED_TEMPLATE: &str = "intake_closed_no_reply";
/// INT-023's default wait for a call-back on a job that is not urgent (a setting), in days.
pub const CALL_BACK_DAYS: i64 = 4;
/// Every new request hears back within this, and an urgent call-back is due within it (INT-023).
pub const HEAR_BACK_WITHIN: SignedDuration = SignedDuration::from_hours(24);
const BAD_PAYLOAD: &str = "bad intake no-reply payload";
const FAILED: &str = "the intake no-reply step failed";

/// One step of a job's no-reply wait (INT-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoReply {
    /// The 24-hour reminder.
    Remind,
    /// The 72-hour close.
    Close,
    /// The closing text held from a close at night.
    Tell,
}

impl NoReply {
    const fn code(self) -> &'static str {
        match self {
            Self::Remind => "remind",
            Self::Close => "close",
            Self::Tell => "tell",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Self::Remind, Self::Close, Self::Tell]
            .into_iter()
            .find(|s| s.code() == code)
    }
}

/// One no-reply step of job `job` in request `conversation`, for the wait counted from `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait {
    /// The job.
    pub job: Uuid,
    /// The request it belongs to.
    pub conversation: Uuid,
    /// Which step.
    pub step: NoReply,
    /// The start of the silence: the job's `timer_from` when the step was queued.
    pub from: Timestamp,
}

/// `at` when it is inside 8:00:00 to 19:59:59 Arizona time, else the next 8 a.m. Arizona (QH-001,
/// INT-019's reminder and closing text; the area-code zone is the send path's, D-1196).
#[must_use]
pub fn texting_window(at: Timestamp) -> Timestamp {
    let local = arizona_time(at);
    if (8..20).contains(&local.hour()) {
        return at;
    }
    let day = if local.hour() >= 20 {
        local.date().tomorrow().ok()
    } else {
        Some(local.date())
    };
    day.and_then(|d| d.at(8, 0, 0, 0).to_zoned(arizona()).ok())
        .map_or(at, |z| z.timestamp())
}

/// Queues job `job`'s reminder and close, counted from its current `timer_from`, if it is still in
/// Intake (INT-019). Safe to run twice: the dedupe key names job, step and start.
pub(crate) async fn schedule_no_reply(
    db: &mut PgConnection,
    (job, conversation): (Uuid, Uuid),
) -> Result<(), IntakeError> {
    let row = current(db, job).await.map_err(|_| IntakeError::Job)?;
    if row.status.code() != "intake" {
        return Ok(());
    }
    let remind = Wait {
        job,
        conversation,
        step: NoReply::Remind,
        from: row.timer_from,
    };
    queue_step(db, &remind, texting_window(row.timer_from + REMIND_AFTER)).await?;
    let close = Wait {
        step: NoReply::Close,
        ..remind
    };
    queue_step(db, &close, row.timer_from + CLOSE_AFTER).await
}

/// Restarts job `job`'s Intake wait at the homeowner's reply `at` and queues its new pair
/// (INT-019, D-807); a job no longer in Intake is left alone.
pub(crate) async fn restart_no_reply(
    db: &mut PgConnection,
    (job, conversation): (Uuid, Uuid),
    at: Timestamp,
) -> Result<(), IntakeError> {
    let moved = restart_timer(db, job, (JobStatus::Intake, at))
        .await
        .map_err(|_| IntakeError::Job)?;
    if moved.applied().is_some() {
        schedule_no_reply(db, (job, conversation)).await?;
    }
    Ok(())
}

async fn queue_step(db: &mut PgConnection, wait: &Wait, at: Timestamp) -> Result<(), IntakeError> {
    let from = wait.from.as_microsecond();
    let payload = json!({"job": wait.job, "conversation": wait.conversation,
        "step": wait.step.code(), "from": from});
    let mut task = NewTask::new(NO_REPLY_KIND, payload, at);
    let key = format!("{NO_REPLY_KIND}.{}.{}.{from}", wait.job, wait.step.code());
    task.dedupe_key = Some(key);
    enqueue_task(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Database)
}

fn parse(p: &Value) -> Option<Wait> {
    let id = |key: &str| p[key].as_str().and_then(|s| Uuid::parse_str(s).ok());
    Some(Wait {
        job: id("job")?,
        conversation: id("conversation")?,
        step: NoReply::from_code(p["step"].as_str()?)?,
        from: Timestamp::from_microsecond(p["from"].as_i64()?).ok()?,
    })
}

/// The [`NO_REPLY_KIND`] handler `server` registers, `secret` being the `ID_SECRET` (JOB-006):
/// [`no_reply_step`] on the task's payload.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed step.
pub async fn run_no_reply(
    pool: PgPool,
    secret: SecretString,
    task: Claimed,
) -> Result<(), HandlerError> {
    let wait = parse(&task.payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    no_reply_step(&mut db, &secret, &wait)
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// Runs one no-reply step (INT-019, JOB-006): re-checks the job first, so a step whose wait was
/// restarted by a reply, or whose job left Intake another way, sends nothing. Returns the text it
/// queued, if any. Safe to run twice: the outbox keeps one row per text.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Template`], [`IntakeError::Outbox`] or
/// [`IntakeError::Database`].
pub async fn no_reply_step(
    db: &mut PgConnection,
    secret: &SecretString,
    wait: &Wait,
) -> Result<Option<Queued>, IntakeError> {
    let job = current(db, wait.job).await.map_err(|_| IntakeError::Job)?;
    match wait.step {
        NoReply::Remind if waiting(&job, wait.from) => {
            tell(db, secret, wait, REMINDER_TEMPLATE).await
        }
        NoReply::Close => close(db, secret, wait, &job).await,
        NoReply::Tell if closed_by_wait(&job, wait.from) => {
            tell(db, secret, wait, CLOSED_TEMPLATE).await
        }
        NoReply::Remind | NoReply::Tell => Ok(None),
    }
}

/// Still in Intake, waiting since `from`.
fn waiting(job: &JobRow, from: Timestamp) -> bool {
    job.status.code() == "intake" && job.timer_from == from
}

/// Closed early at the 72-hour mark of the wait from `from`, by this step or `jobs`' own timer.
fn closed_by_wait(job: &JobRow, from: Timestamp) -> bool {
    job.status.code() == "closed_early" && job.status_at == from + CLOSE_AFTER
}

async fn close(
    db: &mut PgConnection,
    secret: &SecretString,
    wait: &Wait,
    job: &JobRow,
) -> Result<Option<Queued>, IntakeError> {
    let due = wait.from + CLOSE_AFTER;
    if waiting(job, wait.from) {
        let step = (JobStatus::Intake, JobStatus::ClosedEarly);
        // A move that lost to another event is settled by the reload below.
        let _moved = move_to(db, wait.job, step, due, Actor::System)
            .await
            .map_err(|_| IntakeError::Job)?;
    }
    let job = current(db, wait.job).await.map_err(|_| IntakeError::Job)?;
    if !closed_by_wait(&job, wait.from) {
        return Ok(None);
    }
    close_if_done(db, wait.conversation, due).await?;
    let open = texting_window(due);
    if open == due {
        return tell(db, secret, wait, CLOSED_TEMPLATE).await;
    }
    let held = Wait {
        step: NoReply::Tell,
        ..*wait
    };
    queue_step(db, &held, open).await?;
    Ok(None)
}

/// Closes request `conversation` at `at` once every job it opened is final, so the next text
/// starts a new request ("Text us any time and we'll start again.").
async fn close_if_done(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<(), IntakeError> {
    for job in models::request_jobs(db, conversation).await? {
        let row = current(db, job).await.map_err(|_| IntakeError::Job)?;
        if !row.status.is_final() {
            return Ok(());
        }
    }
    models::close_request(db, conversation, at)
        .await
        .map(|_| ())
}

/// Queues `template` about the job to the request's homeowner, only with a consent record
/// (CON-007, D-419), in the request's language.
async fn tell(
    db: &mut PgConnection,
    secret: &SecretString,
    wait: &Wait,
    template: &'static str,
) -> Result<Option<Queued>, IntakeError> {
    if !card_allowed(db, wait.conversation).await? {
        return Ok(None);
    }
    let Some((contact, code)) = models::recipient(db, wait.conversation).await? else {
        return Ok(None);
    };
    let number = job_number(db, wait.job)
        .await
        .map_err(|_| IntakeError::Job)?
        .to_string();
    let language = language_from_code(&code);
    let spanish = language == Language::Es;
    let body = render(template, language, spanish, &[("job", &number)])
        .map_err(|_| IntakeError::Template)?
        .body;
    let to = (contact, template);
    queue_body(db, secret, to, &[wait.job], &body)
        .await
        .map(Some)
}

async fn queue_body(
    db: &mut PgConnection,
    secret: &SecretString,
    (recipient, template): (Uuid, &'static str),
    subject: &[Uuid],
    body: &str,
) -> Result<Queued, IntakeError> {
    let text = NewText {
        template_id: template,
        subject,
        recipient,
        relation: Relation::OpenRequest,
        body,
    };
    let (outbox_id, _) = enqueue(db, secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(Queued {
        outbox_id,
        template,
    })
}

/// A LIST text: LIST or LISTA alone, or with one request number (TXT-031, D-239).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListAsk {
    /// LISTA: the Spanish word.
    pub spanish: bool,
    /// The request number after the word, if any.
    pub job: Option<i64>,
}

/// Reads `words` as a LIST text (TXT-031): LIST or LISTA in any case, optionally followed by one
/// request number; anything else is `None`.
#[must_use]
pub fn list_ask(words: &str) -> Option<ListAsk> {
    let mut parts = words.split_whitespace();
    let word = parts.next()?.to_lowercase();
    let spanish = match word.as_str() {
        "list" => false,
        "lista" => true,
        _ => return None,
    };
    let job = match parts.next() {
        Some(n) => Some(n.parse().ok()?),
        None => None,
    };
    parts.next().is_none().then_some(ListAsk { spanish, job })
}

/// Answers a LIST or LISTA text in request `conversation` (TXT-031, D-239, D-491): with one job
/// holding quotes (or the one named), `quotes`' LIST texts, one queued row per page; with more,
/// the which-request text. `names` are the businesses' public names, which `quotes` needs and the
/// composition supplies. Not a LIST text, or no job with quotes: nothing.
///
/// # Errors
///
/// [`IntakeError::Quotes`], [`IntakeError::Job`], [`IntakeError::Template`],
/// [`IntakeError::Outbox`], [`IntakeError::UnknownRequest`] or [`IntakeError::Database`].
pub async fn answer_list(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (conversation, spanish_on): (Uuid, bool),
    names: &[(Uuid, String)],
) -> Result<Vec<Queued>, IntakeError> {
    let Some(ask) = list_ask(msg.inbound.words) else {
        return Ok(Vec::new());
    };
    let jobs = quoted_jobs(db, conversation, ask.job).await?;
    let Some(&(job, number)) = jobs.first() else {
        return Ok(Vec::new());
    };
    let row = models::conversation(db, conversation).await?;
    let code = row.ok_or(IntakeError::UnknownRequest)?.language;
    let wanted = if ask.spanish {
        Language::Es
    } else {
        language_from_code(&code)
    };
    let reader = Reader { wanted, spanish_on };
    let number = number.to_string();
    let (template, bodies) = if jobs.len() > 1 {
        let which = which_job_text(reader, &number).map_err(|_| IntakeError::Template)?;
        ("quote_list_which", vec![which])
    } else {
        let pages = list_texts(db, (job, &number), reader, names).await;
        ("quote_list", pages.map_err(|_| IntakeError::Quotes)?)
    };
    let mut queued = Vec::new();
    for (page, body) in (0_u128..).zip(&bodies) {
        let subject = [conversation, msg.inbox_id, Uuid::from_u128(page)];
        queued.push(queue_body(db, secret, (msg.contact, template), &subject, body).await?);
    }
    Ok(queued)
}

/// The request's jobs with a shown quote, with their numbers; only number `wanted` when given.
async fn quoted_jobs(
    db: &mut PgConnection,
    conversation: Uuid,
    wanted: Option<i64>,
) -> Result<Vec<(Uuid, i64)>, IntakeError> {
    let mut found = Vec::new();
    for job in models::request_jobs(db, conversation).await? {
        let number = job_number(db, job).await.map_err(|_| IntakeError::Job)?;
        let shown = shown_quotes(db, job)
            .await
            .map_err(|_| IntakeError::Quotes)?;
        if !shown.is_empty() && wanted.is_none_or(|n| n == number) {
            found.push((job, number));
        }
    }
    Ok(found)
}

/// True when a request counts as urgent for INT-023: the job was marked emergency, "How soon?"
/// was answered Today, or it is not answered yet and the request had a 911-list keyword (D-277).
#[must_use]
pub fn counts_urgent(how_soon: Option<&str>, emergency_keyword: bool, emergency_job: bool) -> bool {
    emergency_job || how_soon == Some("today") || (how_soon.is_none() && emergency_keyword)
}

/// When a call-back to a homeowner reachable by neither text nor email is due (INT-023, D-226):
/// within [`HEAR_BACK_WITHIN`] of the request when urgent, else within `days` days (the setting,
/// default [`CALL_BACK_DAYS`]).
#[must_use]
pub fn call_back_due(requested_at: Timestamp, urgent: bool, days: i64) -> Timestamp {
    if urgent {
        requested_at + HEAR_BACK_WITHIN
    } else {
        requested_at + SignedDuration::from_hours(24 * days)
    }
}

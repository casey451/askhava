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
//! lines. INT-023: [`queue_call_back`] raises the call-back alert to Casey with its deadline
//! ([`call_back_due`], [`counts_urgent`]); no app path calls it yet (only
//! `tests/integration/tests/intake_timeouts.rs`): the failed-text path (T094) and web chat (T103)
//! will. [`answer_list`]'s caller is `replies.rs`'s `receive_reply` (T241, D-2206).
//!
//! OPS-036, CON-034 (T151, D-3155, D-3753, D-3881, D-4044): a HELP keyword only records its time
//! ([`note_help`]); [`help_answered`] tells T482's `receive_reply` whether the next message follows
//! a HELP within 24 hours, and [`help_follow_up`] opens or joins her help request for a follow-up
//! that is not a plain-word question. A business text that is not a command ([`business_help`]),
//! a business's "Ask about a charge" event ([`run_help_request_opened`]) and a help email
//! `privacy` read ([`receive_help_email`]) open one at once. Each keeps the scrubbed words in
//! `privacy.message_body` (her job, else the request's own ID), opens or joins the request in
//! `ops`, and for a request notified now queues `messaging`'s notification with the job fields
//! ([`job_fields`]) and the words passed through `quotes`' CARD-008 gate (PRIV-030).

use askhava_core::api::{
    Claimed, Clock, HandlerError, NewTask, SystemClock, arizona, arizona_time,
    enqueue as enqueue_task,
};
use askhava_jobs::api::{JobRow, JobStatus, current, job_number, move_to, restart_timer};
use askhava_messaging::api::{
    HelpNotice, Language, NewText, Relation, enqueue, last_text, queue_help_notice, render,
};
use askhava_ops::api::{
    Actor, Asker, HelpLimits, HelpOpened, NewAlert, Severity, help_keyword_recent,
    open_help_request, raise_alert, record_help_keyword,
};
use askhava_privacy::api::{
    CommonWords, InboundEmail, Known, NameMemory, ScrubbedText, Standing, keep_problem_words, scrub,
};
use askhava_quotes::api::{Reader, final_gate, list_texts, shown_quotes, which_job_text};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

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
/// Intake (INT-019). Safe to run twice: the dedupe key names job, step, start and run time.
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
    let run = at.as_microsecond();
    let key = format!(
        "{NO_REPLY_KIND}.{}.{}.{from}.{run}",
        wait.job,
        wait.step.code()
    );
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
    no_reply_step(&mut db, &secret, &wait, SystemClock.now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// Runs one no-reply step at `now` (INT-019, JOB-006, QH-001): re-checks the job first, so a step
/// whose wait was restarted by a reply, or whose job left Intake another way, sends nothing. A step
/// run before its due time does nothing; a text step run in quiet hours queues itself again for the
/// next 8 a.m. Arizona and sends nothing now. Returns the text it queued, if any. Safe to run
/// twice: the outbox keeps one row per text.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Template`], [`IntakeError::Outbox`] or
/// [`IntakeError::Database`].
pub async fn no_reply_step(
    db: &mut PgConnection,
    secret: &SecretString,
    wait: &Wait,
    now: Timestamp,
) -> Result<Option<Queued>, IntakeError> {
    if now < earliest(wait) {
        return Ok(None);
    }
    let job = current(db, wait.job).await.map_err(|_| IntakeError::Job)?;
    match wait.step {
        NoReply::Remind if waiting(&job, wait.from) => tell_in_window(db, secret, wait, now).await,
        NoReply::Close => close(db, secret, wait, (&job, now)).await,
        NoReply::Tell if closed_by_wait(&job, wait.from) => {
            tell_in_window(db, secret, wait, now).await
        }
        NoReply::Remind | NoReply::Tell => Ok(None),
    }
}

/// The first instant step `wait` may act: the reminder at 24 hours (in the window), the close
/// and its held text at 72 hours (INT-019).
fn earliest(wait: &Wait) -> Timestamp {
    match wait.step {
        NoReply::Remind => texting_window(wait.from + REMIND_AFTER),
        NoReply::Close | NoReply::Tell => wait.from + CLOSE_AFTER,
    }
}

/// Sends the step's text now when `now` is inside the texting window, else queues the same step
/// for the window's next opening (QH-001, QH-008: checked when the step runs, not only when it
/// was queued).
async fn tell_in_window(
    db: &mut PgConnection,
    secret: &SecretString,
    wait: &Wait,
    now: Timestamp,
) -> Result<Option<Queued>, IntakeError> {
    let open = texting_window(now);
    if open != now {
        queue_step(db, wait, open).await?;
        return Ok(None);
    }
    let template = match wait.step {
        NoReply::Remind => REMINDER_TEMPLATE,
        NoReply::Close | NoReply::Tell => CLOSED_TEMPLATE,
    };
    tell(db, secret, wait, template).await
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
    (job, now): (&JobRow, Timestamp),
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
    let held = Wait {
        step: NoReply::Tell,
        ..*wait
    };
    tell_in_window(db, secret, &held, now).await
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

/// Queues `body` from `template` to the request's homeowner `recipient` about `subject`
/// (TXT-011: the outbox key is over template, recipient and subject).
pub(crate) async fn queue_body(
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
/// request number written in digits only (no sign); anything else is `None`.
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
        Some(n) if n.bytes().all(|b| b.is_ascii_digit()) => Some(n.parse().ok()?),
        Some(_) => return None,
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

/// The `ops` alert kind of a call-back due within [`HEAR_BACK_WITHIN`]: an item of Casey's OPS-032
/// daytime push (INT-023, D-277).
pub const CALL_BACK_URGENT_ALERT: &str = "intake.call_back_urgent";
/// The `ops` alert kind of a call-back due within the setting's days (INT-023, D-226).
pub const CALL_BACK_ALERT: &str = "intake.call_back";
/// The `intake_answer` question that records a request's call-back, choice `urgent` or `regular`.
const CALL_BACK_QUESTION: &str = "call_back";

/// A call-back alert queued for Casey (INT-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallBack {
    /// Due within 24 hours ([`counts_urgent`]).
    pub urgent: bool,
    /// When the call-back is due ([`call_back_due`]).
    pub due: Timestamp,
    /// False when an earlier call queued it already and nothing new was raised.
    pub raised: bool,
}

/// Queues the call-back alert to Casey for request `conversation`, asked at `requested_at`, whose
/// homeowner can be reached by neither text nor email (INT-023, D-226, D-277, D-1199): urgency read
/// from the request (emergency choice, "How soon?" Today, or a 911-list match before "How soon?"),
/// one `ops` alert ([`CALL_BACK_URGENT_ALERT`] or [`CALL_BACK_ALERT`], severity normal, the
/// request by ID only) and one `call_back` answer row, in one transaction. Safe to call twice: the
/// homeowner row is locked and a second call raises nothing. `days` is the setting (default
/// [`CALL_BACK_DAYS`]). The callers are the failed-text path (TD-TXT-05, T094) and web chat (T103).
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Alert`] or [`IntakeError::Database`].
pub async fn queue_call_back(
    db: &mut PgConnection,
    conversation: Uuid,
    requested_at: Timestamp,
    days: i64,
) -> Result<CallBack, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let earlier = models::latest_answer(&mut tx, conversation, CALL_BACK_QUESTION).await?;
    let urgent = match earlier.as_deref() {
        Some(code) => code == "urgent",
        None => request_urgent(&mut tx, conversation).await?,
    };
    let due = call_back_due(requested_at, urgent, days);
    let raised = earlier.is_none();
    if raised {
        let kind = if urgent {
            CALL_BACK_URGENT_ALERT
        } else {
            CALL_BACK_ALERT
        };
        let alert = NewAlert {
            kind,
            severity: Severity::Normal,
            subject: Some(("intake.conversation", conversation)),
        };
        raise_alert(&mut tx, &alert)
            .await
            .map_err(|_| IntakeError::Alert)?;
        let code = if urgent { "urgent" } else { "regular" };
        models::insert_plain(
            &mut tx,
            conversation,
            (CALL_BACK_QUESTION, code),
            requested_at,
        )
        .await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(CallBack {
        urgent,
        due,
        raised,
    })
}

/// [`counts_urgent`] on what request `conversation` stored: its "How soon?" answer, a 911-list
/// match (any emergency event but an INT-037 beep question) and the emergency choice (D-277).
pub(crate) async fn request_urgent(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<bool, IntakeError> {
    let how_soon = models::latest_answer(db, conversation, "how_soon").await?;
    let choice = models::latest_answer(db, conversation, "emergency_choice").await?;
    let keyword = models::events(db, conversation)
        .await?
        .iter()
        .any(|e| e.matched != "beep_question");
    Ok(counts_urgent(
        how_soon.as_deref(),
        keyword,
        choice.as_deref() == Some("emergency"),
    ))
}

/// OPS-036, D-887: a help request is due this long after it opens, or [`HEAR_BACK_WITHIN`] when
/// her job is urgent or an emergency.
pub const HELP_DUE: SignedDuration = SignedDuration::from_hours(48);
/// The line kept and shown for a business's "Ask about a charge", whose own words are not carried
/// (BIZ-032, D-492, D-1958).
pub const CHARGE_QUESTION: &str = "The business tapped Ask about a charge for this job.";
const HELP_FAILED: HandlerError = HandlerError("the help request could not be opened");

/// D-3882: the facts of a job a help notification shows (OPS-036).
pub(crate) struct JobFields {
    job: Uuid,
    number: i64,
    status: JobStatus,
    trade: Option<String>,
    town: Option<String>,
    urgent: bool,
    list_line: Option<String>,
}

/// OPS-036, D-3882: her newest job in her open request with its status, number, trade, town,
/// urgency and first LIST line (no business names); `None` with no open request or job.
pub(crate) async fn job_fields(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<JobFields>, IntakeError> {
    let row = sqlx::query!(
        "SELECT a.conversation_id, a.choice, split_part(a.question_id, '.', 2) AS \"trade!\", \
         (SELECT p.town FROM intake.property p WHERE p.homeowner_id = h.id \
          AND left(replace(p.id::text, '-', ''), 12) = split_part(a.question_id, '.', 3) \
          LIMIT 1) AS town \
         FROM intake.intake_answer a JOIN intake.conversation c ON c.id = a.conversation_id \
         JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 AND c.closed_at IS NULL AND a.question_id LIKE 'job.%' \
         ORDER BY a.answered_at DESC LIMIT 1",
        contact
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let Some(row) = row else { return Ok(None) };
    let Some(job) = row.choice.and_then(|c| Uuid::parse_str(&c).ok()) else {
        return Ok(None);
    };
    let mut fields = charge_job(db, job).await?;
    fields.urgent = fields.urgent || request_urgent(db, row.conversation_id).await?;
    let reader = Reader {
        wanted: Language::En,
        spanish_on: false,
    };
    let list = list_texts(db, (job, &fields.number.to_string()), reader, &[]).await;
    fields.list_line = list.ok().and_then(|lines| lines.into_iter().next());
    (fields.trade, fields.town) = (Some(row.trade), row.town);
    Ok(Some(fields))
}

/// A job's number, status and emergency flag, with no trade, town or LIST line yet.
async fn charge_job(db: &mut PgConnection, job: Uuid) -> Result<JobFields, IntakeError> {
    let row = current(db, job).await.map_err(|_| IntakeError::Job)?;
    let number = job_number(db, job).await.map_err(|_| IntakeError::Job)?;
    Ok(JobFields {
        job,
        number,
        status: row.status,
        trade: None,
        town: None,
        urgent: row.emergency,
        list_line: None,
    })
}

/// One help message on its way to `ops`: who, its inbox ID (the `privacy.message_body` key), its
/// scrubbed words, its job's fields and the number the notification shows.
struct HelpIn<'a> {
    asker: Asker,
    inbox: Uuid,
    words: &'a ScrubbedText,
    fields: Option<JobFields>,
    number: String,
}

/// OPS-036, PRIV-030, D-4044: opens or joins the request in `ops`, keeps the words in
/// `privacy.message_body` under the job or the request's own ID, and for a request notified now
/// queues the one notification with the words through the CARD-008 gate.
async fn open_help(
    db: &mut PgConnection,
    help: HelpIn<'_>,
    at: Timestamp,
) -> Result<HelpOpened, IntakeError> {
    let urgent = help.fields.as_ref().is_some_and(|f| f.urgent);
    let due = at + if urgent { HEAR_BACK_WITHIN } else { HELP_DUE };
    let key = (help.inbox, at, due);
    let opened = open_help_request(db, help.asker, key, HelpLimits::default())
        .await
        .map_err(|_| IntakeError::Alert)?;
    let job = help.fields.as_ref().map_or(opened.request(), |f| f.job);
    keep_problem_words(db, help.inbox, job, help.words, false)
        .await
        .map_err(|_| IntakeError::Vault)?;
    let HelpOpened::Opened {
        request,
        notify: true,
    } = opened
    else {
        return Ok(opened);
    };
    let last = match help.asker {
        Asker::Homeowner(id) | Asker::Business(id) => last_text(db, id).await,
        Asker::Email => Ok(None),
    };
    let f = help.fields;
    let notice = HelpNotice {
        request,
        asker: help.asker.code().to_owned(),
        number: help.number,
        status: f.as_ref().map(|f| f.status.code().to_owned()),
        trade: f.as_ref().and_then(|f| f.trade.clone()),
        town: f.as_ref().and_then(|f| f.town.clone()),
        urgent,
        list_line: f.and_then(|f| f.list_line),
        message: final_gate(help.words.as_str()),
        last_text: last.map_err(|_| IntakeError::Outbox)?,
        due: due.as_second(),
    };
    queue_help_notice(db, &notice, at)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(opened)
}

/// CON-034, D-3881: records her HELP keyword at `at`; no request and no notification.
///
/// # Errors
///
/// [`IntakeError::Alert`] when `ops` cannot record it.
pub async fn note_help(
    db: &mut PgConnection,
    contact: Uuid,
    at: Timestamp,
) -> Result<(), IntakeError> {
    record_help_keyword(db, contact, at)
        .await
        .map_err(|_| IntakeError::Alert)
}

/// CON-034, D-3753: whether her number's HELP was answered within the 24 hours before `at`
/// (T482's `receive_reply` checks this before a follow-up).
///
/// # Errors
///
/// [`IntakeError::Alert`] when `ops` cannot be read.
pub async fn help_answered(
    db: &mut PgConnection,
    contact: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    help_keyword_recent(db, contact, at)
        .await
        .map_err(|_| IntakeError::Alert)
}

/// OPS-036, D-3881: her follow-up after HELP that is not a plain-word question (inbox row `inbox`,
/// scrubbed `words`) opens or joins her help request, with her newest open job's fields.
///
/// # Errors
///
/// [`IntakeError::Database`], `Job`, `Alert`, `Vault` or `Outbox`.
pub async fn help_follow_up(
    db: &mut PgConnection,
    (inbox, contact): (Uuid, Uuid),
    words: &ScrubbedText,
    at: Timestamp,
) -> Result<HelpOpened, IntakeError> {
    let fields = job_fields(db, contact).await?;
    let number = fields
        .as_ref()
        .map_or_else(|| "with no open job".to_owned(), |f| f.number.to_string());
    let asker = Asker::Homeowner(contact);
    let help = HelpIn {
        asker,
        inbox,
        words,
        fields,
        number,
    };
    open_help(db, help, at).await
}

/// The short reference a business's notification shows in place of a job number.
fn business_number(business: Uuid) -> String {
    let hex = business.simple().to_string();
    format!("B-{}", &hex[..8])
}

/// OPS-036, D-3881: a business text that is not a command ("why was I charged?", "cargo
/// incorrecto") opens or joins the business's help request at once.
///
/// # Errors
///
/// [`IntakeError::Alert`], `Vault` or `Outbox`.
pub async fn business_help(
    db: &mut PgConnection,
    (inbox, business): (Uuid, Uuid),
    words: &ScrubbedText,
    at: Timestamp,
) -> Result<HelpOpened, IntakeError> {
    let asker = Asker::Business(business);
    let number = business_number(business);
    let help = HelpIn {
        asker,
        inbox,
        words,
        fields: None,
        number,
    };
    open_help(db, help, at).await
}

/// OPS-036, PRIV-026, D-3155: a help email `privacy::api::read_inbound_email` read (T133's route
/// hands it here) opens a request; a sender that failed SPF, DKIM or DMARC may only opt out, so
/// it opens nothing (`None`).
///
/// # Errors
///
/// [`IntakeError::Database`], `Alert`, `Vault` or `Outbox`.
pub async fn receive_help_email(
    db: &mut PgConnection,
    email: &InboundEmail,
    at: Timestamp,
) -> Result<Option<HelpOpened>, IntakeError> {
    if email.standing != Standing::Authenticated {
        return Ok(None);
    }
    let inbox = sqlx::query_scalar!("SELECT gen_random_uuid() AS \"id!\"")
        .fetch_one(&mut *db)
        .await
        .map_err(|_| IntakeError::Database)?;
    let help = HelpIn {
        asker: Asker::Email,
        inbox,
        words: &email.text,
        fields: None,
        number: "help email".to_owned(),
    };
    open_help(db, help, at).await.map(Some)
}

/// BIZ-032, OPS-036, D-492: the `portals::api::HELP_REQUEST_KIND` handler: a business's "Ask
/// about a charge" opens one help request with [`CHARGE_QUESTION`], its job's number and status,
/// keyed by the charge so a rerun joins rather than opens a second.
///
/// # Errors
///
/// [`HandlerError`] for a bad payload or any failed step (the transaction rolls back).
pub async fn run_help_request_opened(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let id = |k: &str| {
        task.payload
            .get(k)
            .and_then(Value::as_str)
            .and_then(|v| Uuid::parse_str(v).ok())
    };
    let (Some(business), Some(charge), Some(job)) = (id("business"), id("charge"), id("job"))
    else {
        return Err(HandlerError("bad help request payload"));
    };
    let mut tx = pool.begin().await.map_err(|_| HELP_FAILED)?;
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let words = scrub(CHARGE_QUESTION, &known, &mut NameMemory::default());
    let fields = charge_job(&mut tx, job).await.map_err(|_| HELP_FAILED)?;
    let number = fields.number.to_string();
    let asker = Asker::Business(business);
    let help = HelpIn {
        asker,
        inbox: charge,
        words: &words,
        fields: Some(fields),
        number,
    };
    open_help(&mut tx, help, now)
        .await
        .map_err(|_| HELP_FAILED)?;
    tx.commit().await.map_err(|_| HELP_FAILED)
}

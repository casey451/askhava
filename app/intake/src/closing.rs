//! Closing a job at the homeowner's word (JOB-018 to JOB-023, D-005, D-1200 to D-1202).
//!
//! [`answer_close`] reads one homeowner text with [`close_reading`]: "CLOSE 1042" (or CERRAR)
//! closes that job, "CLOSE ALL" every open job; a cancel-intent message closes the only open job
//! and says so, or, with several open jobs, asks "Close which?" with each number and an ALL
//! option and closes nothing until answered (a lone number or ALL then answers it); a message
//! that only mentions cancelling gets the JOB-023 how-to-close line and closes nothing. An
//! opt-out (bare "cancel" included) changes no job (D-005): `messaging` already answered it.
//! [`close_from_page`] is the quote page's close button. A close moves the job to Cancelled from
//! any status before Done (JOB-018), with its audit row, and closes the request once every job is
//! final. Like `answer_list`, [`answer_close`] waits for the conversation dispatcher (D-999).

use askhava_jobs::api::{JobStatus, current, job_number, move_to};
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, IntakeError};
use crate::rules::{CloseReading, CloseTarget, close_reading, language_from_code};
use crate::service::{Queued, TextIn};

/// "Close which?" with the open request numbers and the ALL option (JOB-021).
pub const CLOSE_WHICH_TEMPLATE: &str = "close_which";
/// The JOB-023 line: "Ask Hava: To close request <number>, reply CLOSE <number>."
pub const CLOSE_MENTION_TEMPLATE: &str = "close_mention";
/// One job closed at the homeowner's word (JOB-022).
pub const JOB_CLOSED_TEMPLATE: &str = "job_closed";
/// Several jobs closed by CLOSE ALL (JOB-021).
pub const JOBS_CLOSED_TEMPLATE: &str = "jobs_closed";
/// The `intake_answer` question that marks a "Close which?" waiting (`asked`) or done (`answered`).
const WHICH_QUESTION: &str = "close_which";

/// What [`answer_close`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    /// These request numbers moved to Cancelled.
    Closed(Vec<i64>),
    /// "Close which?" went, listing these numbers; nothing closed (JOB-021).
    Which(Vec<i64>),
    /// The JOB-023 how-to-close line went, naming this number; nothing closed.
    Explained(i64),
    /// Not about closing, an opt-out, or no open job: nothing sent, nothing closed.
    Nothing,
}

/// One open job of a request: ID, number and status.
type Open = (Uuid, i64, JobStatus);

/// Answers one homeowner text about closing (JOB-018 to JOB-023, D-005, D-1200, D-1201), in one
/// transaction under the homeowner lock. Returns what it did and the text it queued, if any.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Template`], [`IntakeError::Outbox`] or
/// [`IntakeError::Database`].
pub async fn answer_close(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
) -> Result<(Closing, Option<Queued>), IntakeError> {
    let in_scope = !msg.inbound.opt_out && !msg.inbound.help;
    let found = models::open_conversation(db, msg.contact).await?;
    let Some(conversation) = found.filter(|_| in_scope) else {
        return Ok((Closing::Nothing, None));
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let open = open_jobs(&mut tx, conversation).await?;
    let waiting = models::latest_answer(&mut tx, conversation, WHICH_QUESTION).await?;
    let reading = close_reading(msg.inbound.words, waiting.as_deref() == Some("asked"));
    let closing = decide(reading, &open);
    let at = msg.received_at;
    let mark = match &closing {
        Closing::Closed(_) if waiting.as_deref() == Some("asked") => Some("answered"),
        Closing::Which(_) => Some("asked"),
        _ => None,
    };
    if let Some(choice) = mark {
        models::insert_plain(&mut tx, conversation, (WHICH_QUESTION, choice), at).await?;
    }
    if let Closing::Closed(numbers) = &closing {
        let actor = Actor::Homeowner(homeowner);
        for (job, _, status) in open.iter().filter(|j| numbers.contains(&j.1)) {
            cancel(&mut tx, *job, *status, (at, actor)).await?;
        }
        close_when_final(&mut tx, conversation, at).await?;
    }
    let text = say(&mut tx, secret, (msg, conversation), &closing, &open).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok((closing, text))
}

/// The rule (JOB-019 to JOB-023, D-1201): a number not open gets the how-to line with one open
/// job and "Close which?" with several; intent closes the only job or asks which.
fn decide(reading: CloseReading, open: &[Open]) -> Closing {
    let numbers: Vec<i64> = open.iter().map(|j| j.1).collect();
    let (Some(&newest), several) = (numbers.last(), numbers.len() > 1) else {
        return Closing::Nothing;
    };
    match reading {
        CloseReading::Close(CloseTarget::All) => Closing::Closed(numbers),
        CloseReading::Close(CloseTarget::Number(n)) if numbers.contains(&n) => {
            Closing::Closed(vec![n])
        }
        CloseReading::Close(_) | CloseReading::Intent if several => Closing::Which(numbers),
        CloseReading::Intent => Closing::Closed(numbers),
        CloseReading::Close(_) | CloseReading::Mention => Closing::Explained(newest),
        CloseReading::Other => Closing::Nothing,
    }
}

/// The request's jobs before a final status, by number (JOB-018).
async fn open_jobs(db: &mut PgConnection, conversation: Uuid) -> Result<Vec<Open>, IntakeError> {
    let mut open = Vec::new();
    for job in models::request_jobs(db, conversation).await? {
        let status = current(db, job).await.map_err(|_| IntakeError::Job)?.status;
        if !status.is_final() {
            let number = job_number(db, job).await.map_err(|_| IntakeError::Job)?;
            open.push((job, number, status));
        }
    }
    open.sort_by_key(|j| j.1);
    Ok(open)
}

/// Moves job `job` from `status` to Cancelled (JOB-002, JOB-004, JOB-005, JOB-018); a job that
/// moved on meanwhile is left as it is.
async fn cancel(
    db: &mut PgConnection,
    job: Uuid,
    status: JobStatus,
    (at, actor): (Timestamp, Actor),
) -> Result<(), IntakeError> {
    move_to(db, job, (status, JobStatus::Cancelled), at, actor)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Job)
}

/// Closes request `conversation` at `at` once every job it opened is final, so the next text
/// starts a new request.
async fn close_when_final(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<(), IntakeError> {
    if open_jobs(db, conversation).await?.is_empty() {
        models::close_request(db, conversation, at).await?;
    }
    Ok(())
}

/// Queues the text for `closing` in the request's language (JOB-021 to JOB-023). Keys: one
/// confirmation per job; the others per inbound text.
async fn say(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    closing: &Closing,
    open: &[Open],
) -> Result<Option<Queued>, IntakeError> {
    let code = models::recipient(db, conversation).await?.map(|r| r.1);
    let language = language_from_code(code.as_deref().unwrap_or("en"));
    let list = |numbers: &[i64]| joined(numbers, language);
    let (template, number) = match closing {
        Closing::Closed(n) if n.len() == 1 => (JOB_CLOSED_TEMPLATE, list(n)),
        Closing::Closed(n) => (JOBS_CLOSED_TEMPLATE, list(n)),
        Closing::Which(n) => (CLOSE_WHICH_TEMPLATE, list(n)),
        Closing::Explained(n) => (CLOSE_MENTION_TEMPLATE, n.to_string()),
        Closing::Nothing => return Ok(None),
    };
    let one = match closing {
        Closing::Closed(n) if n.len() == 1 => open.iter().find(|j| j.1 == n[0]).map(|j| j.0),
        _ => None,
    };
    let subject = one.map_or_else(|| vec![conversation, msg.inbox_id], |job| vec![job]);
    let body = render(
        template,
        language,
        language == Language::Es,
        &[("job", &number)],
    )
    .map_err(|_| IntakeError::Template)?
    .body;
    let text = NewText {
        template_id: template,
        subject: &subject,
        recipient: msg.contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox_id, _) = enqueue(db, secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(Some(Queued {
        outbox_id,
        template,
    }))
}

/// Request numbers as a person writes them: "1042", "1042 and 1043", "1042, 1043 and 1044"
/// ("y" in Spanish).
fn joined(numbers: &[i64], language: Language) -> String {
    let words: Vec<String> = numbers.iter().map(i64::to_string).collect();
    let and = if language == Language::Es {
        " y "
    } else {
        " and "
    };
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}{and}{last}", rest.join(", ")),
        _ => words.concat(),
    }
}

/// The quote page's close button (JOB-018, JOB-019): moves job `job` to Cancelled at `at` from any
/// status before Done, as its homeowner, and closes the request once every job is final. The page
/// shows the result, so no text goes. `false` when the job is already final or has no open
/// request. Safe to press twice.
///
/// # Errors
///
/// [`IntakeError::Job`] or [`IntakeError::Database`].
pub async fn close_from_page(
    db: &mut PgConnection,
    job: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let Some((conversation, homeowner)) = models::request_of_job(&mut tx, job).await? else {
        return Ok(false);
    };
    models::lock_homeowner(&mut tx, homeowner).await?;
    let status = current(&mut tx, job)
        .await
        .map_err(|_| IntakeError::Job)?
        .status;
    if status.is_final() {
        return Ok(false);
    }
    cancel(&mut tx, job, status, (at, Actor::Homeowner(homeowner))).await?;
    close_when_final(&mut tx, conversation, at).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(true)
}

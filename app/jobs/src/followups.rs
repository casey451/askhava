//! A Connected job's follow-ups (T097, D-1809 to D-1811): the day-3 and day-14 check-ins, the
//! homeowner's answer to them (DONE or LISTO, NO, PROBLEM, LATER or MÁS TARDE, ONGOING or
//! CONTINUO, OTHER or OTRO), the PROBLEM listing for Casey and the one private feedback request
//! after Done (JOB-015, JOB-016, JOB-017's request, JOB-028, TXT-029; D-071, D-083, D-222, D-240).
//!
//! [`schedule_check_ins`] queues one [`CHECK_IN_KIND`] task per check-in when the job connects;
//! [`run_check_in`] queues the check-in's homeowner text only while the job is still Connected, so
//! ONGOING, OTHER, DONE or a close stops every later check-in. [`answer_check_in`] applies an
//! answer: a move sends its text in the move's transaction (the feedback request with Done, the
//! TXT-029 reply with a PROBLEM close), and a PROBLEM also raises one `ops.alert` naming the
//! inbound message, never its words: [`PROBLEM_ALERT`] for the weekly digest, or
//! [`PROBLEM_SAFETY_ALERT`] for the OPS-032 push when it held a safety word (OPS-005). Nothing here
//! touches a fee or the ledger (D-071). The feedback page and its storage are T230's (D-493).

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::handlers::queue_text;
use crate::models::{self, JobsError};
use crate::rules::{CheckIn, Contact, Reply, connected_move};
use crate::service::{apply, job_number};
use crate::status::JobStatus;

/// The `core.task` kind of one due check-in; `server` registers [`run_check_in`] for it.
pub const CHECK_IN_KIND: &str = "jobs.check_in";
/// The day-3 check-in text's template ID (JOB-015).
pub const CHECK_IN_DAY3: &str = "check_in_day3";
/// The day-14 check-in text's template ID (JOB-015).
pub const CHECK_IN_DAY14: &str = "check_in_day14";
/// The one private feedback request, sent with the move to Done (JOB-017, D-083, D-466).
pub const FEEDBACK_REQUEST: &str = "feedback_request";
/// The TXT-029 reply to a PROBLEM (D-222).
pub const PROBLEM_REPLY: &str = "problem_reply";
/// The `ops.alert` kind listing a PROBLEM reply in the weekly digest (JOB-016, OPS-005).
pub const PROBLEM_ALERT: &str = "jobs.problem_report";
/// The `ops.alert` kind of a PROBLEM reply holding a safety word, for the OPS-032 push (D-277).
pub const PROBLEM_SAFETY_ALERT: &str = "jobs.problem_report_safety";
/// The alert's subject: the inbound text row, whose scrubbed words the digest shows.
const INBOX: &str = "messaging.inbox";
const BAD_PAYLOAD: &str = "bad jobs check-in payload";
const DB: &str = "jobs check-in database call failed";

/// A check-in answer, read from the homeowner's text by [`parse_answer`] (JOB-015, JOB-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// DONE or LISTO.
    Done,
    /// NO: the business never reached her.
    No,
    /// PROBLEM or PROBLEMA, with or without words after it.
    Problem,
    /// LATER or MÁS TARDE: not done yet.
    Later,
    /// ONGOING or CONTINUO: a recurring service has started.
    Ongoing,
    /// OTHER or OTRO: she hired someone else.
    Other,
}

/// What an answer does to a Connected job (JOB-015, JOB-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// The job moves to this status, sending this homeowner text with the move.
    Move(JobStatus, Option<&'static str>),
    /// LATER at day 3: nothing moves; the day-14 check-in asks again.
    AskAtDay14,
    /// LATER at day 14: nothing moves; the JOB-026 backstop closes it 7 days on.
    Nothing,
}

/// The inbound text an answer came in: its `messaging.inbox` row and whether its words hold a
/// safety word (`businesses::api::has_safety_word`, OPS-005; the caller reads it, D-1810).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inbound {
    /// The `messaging.inbox` row ID.
    pub inbox: Uuid,
    /// True when the words hold a safety word (D-222).
    pub safety: bool,
}

/// JOB-015, JOB-028: the answer a check-in reply gives, in any case, with end punctuation and
/// extra spaces ignored; a PROBLEM may carry words after it. `None` for anything else.
#[must_use]
pub fn parse_answer(words: &str) -> Option<Answer> {
    let upper = words.to_uppercase();
    let words: Vec<&str> = upper
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect();
    if matches!(words.first(), Some(&("PROBLEM" | "PROBLEMA"))) {
        return Some(Answer::Problem);
    }
    match words.as_slice() {
        ["DONE" | "LISTO"] => Some(Answer::Done),
        ["NO"] => Some(Answer::No),
        ["LATER"] | ["MÁS" | "MAS", "TARDE"] => Some(Answer::Later),
        ["ONGOING" | "CONTINUO"] => Some(Answer::Ongoing),
        ["OTHER" | "OTRO"] => Some(Answer::Other),
        _ => None,
    }
}

/// JOB-015, JOB-016, JOB-017, JOB-028: where `answer` to `check` takes a Connected job. DONE and
/// ONGOING go to Done with the feedback request; OTHER closes with no no-show, also under email
/// contact; NO follows `connected_move`; PROBLEM closes with the TXT-029 reply; LATER moves nothing.
#[must_use]
pub fn after_answer(answer: Answer, check: CheckIn, contact: Contact) -> Next {
    match (answer, check) {
        (Answer::Done | Answer::Ongoing, _) => Next::Move(JobStatus::Done, Some(FEEDBACK_REQUEST)),
        (Answer::Other, _) => Next::Move(JobStatus::Closed, None),
        (Answer::No, _) => Next::Move(connected_move(Reply::No, check, contact), None),
        (Answer::Problem, _) => Next::Move(JobStatus::Closed, Some(PROBLEM_REPLY)),
        (Answer::Later, CheckIn::Day3) => Next::AskAtDay14,
        (Answer::Later, CheckIn::Day14) => Next::Nothing,
    }
}

/// JOB-015: the day-3 and day-14 check-in times of a job released at `released` whose picked
/// quote's earliest visit is `visit`. Day 3 counts from the later of the two, day 14 from the
/// release (JOB-026's backstop counts from it too); a day 3 not before day 14 is dropped.
#[must_use]
pub fn check_in_times(
    released: Timestamp,
    visit: Option<Timestamp>,
) -> (Option<Timestamp>, Timestamp) {
    let days = |from: Timestamp, d: i64| from.saturating_add(SignedDuration::from_hours(d * 24));
    let day14 = days(released, 14).unwrap_or(Timestamp::MAX);
    let from = visit.map_or(released, |v| v.max(released));
    let day3 = days(from, 3).ok().filter(|t| *t < day14);
    (day3, day14)
}

/// JOB-015: queues job `id`'s check-ins ([`check_in_times`]) in the caller's transaction, once
/// each (dedupe key `jobs.check_in.<job>.<day>`). The release calls it when the job connects.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn schedule_check_ins(
    db: &mut PgConnection,
    id: Uuid,
    released: Timestamp,
    visit: Option<Timestamp>,
) -> Result<(), JobsError> {
    let (day3, day14) = check_in_times(released, visit);
    let checks = [(CHECK_IN_DAY3, day3), (CHECK_IN_DAY14, Some(day14))];
    for (template, at) in checks {
        let Some(at) = at else { continue };
        let payload = json!({"job": id, "template": template, "at": at.to_string()});
        let mut task = NewTask::new(CHECK_IN_KIND, payload, at);
        task.dedupe_key = Some(format!("{CHECK_IN_KIND}.{id}.{template}"));
        enqueue(db, &task).await.map_err(|_| JobsError::Database)?;
    }
    Ok(())
}

/// The [`CHECK_IN_KIND`] handler (JOB-015, JOB-028): queues the check-in text, at the check-in's
/// time, only while the job is still Connected; after ONGOING, OTHER, any move or a close it
/// does nothing. Safe to run twice: the text is queued once per job and template.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a database failure.
pub async fn run_check_in(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let p = &task.payload;
    let id = p["job"].as_str().and_then(|s| Uuid::parse_str(s).ok());
    let at = p["at"].as_str().and_then(|s| s.parse::<Timestamp>().ok());
    let template = [CHECK_IN_DAY3, CHECK_IN_DAY14]
        .into_iter()
        .find(|t| p["template"].as_str() == Some(*t));
    let (Some(id), Some(at), Some(template)) = (id, at, template) else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut db = pool.acquire().await.map_err(|_| HandlerError(DB))?;
    let job = models::load(&mut db, id)
        .await
        .map_err(|_| HandlerError(DB))?;
    if !job.status.takes_check_ins() {
        return Ok(());
    }
    let number = job_number(&mut db, id)
        .await
        .map_err(|_| HandlerError(DB))?;
    let values = json!({ "job": number });
    queue_text(&mut db, id, (template, job.version), values, at)
        .await
        .map_err(|_| HandlerError(DB))
}

/// JOB-015, JOB-016, JOB-028, TXT-029, D-071: applies `answer` to check-in `check` on job `id`,
/// which came in as `inbound`. A move ([`after_answer`]) is guarded on Connected and sends its
/// text in its transaction; a PROBLEM close raises its alert in the same transaction, and no fee
/// or ledger row changes. LATER changes nothing. A job no longer Connected changes nothing and
/// raises nothing ([`Guarded::MovedOn`]), so a repeated answer is never listed twice.
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn answer_check_in(
    db: &mut PgConnection,
    id: Uuid,
    (answer, check, contact): (Answer, CheckIn, Contact),
    inbound: Inbound,
    (at, actor): (Timestamp, Actor),
) -> Result<Guarded<Next>, JobsError> {
    let next = after_answer(answer, check, contact);
    let Next::Move(to, text) = next else {
        let connected = models::load(db, id).await?.status.takes_check_ins();
        return Ok(if connected {
            Guarded::Applied(next)
        } else {
            Guarded::MovedOn
        });
    };
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let step = (JobStatus::Connected, None, to);
    if apply(&mut tx, id, step, (at, actor), text)
        .await?
        .applied()
        .is_none()
    {
        return Ok(Guarded::MovedOn);
    }
    if answer == Answer::Problem {
        flag_problem(&mut tx, inbound).await?;
    }
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(Guarded::Applied(next))
}

/// JOB-016, OPS-005, D-222: one alert naming the PROBLEM text's inbox row: the digest kind, or
/// the safety kind for the OPS-032 push. Severity Normal either way, as for complaints (D-1810).
async fn flag_problem(tx: &mut PgConnection, inbound: Inbound) -> Result<(), JobsError> {
    let kind = if inbound.safety {
        PROBLEM_SAFETY_ALERT
    } else {
        PROBLEM_ALERT
    };
    let alert = NewAlert {
        kind,
        severity: Severity::Normal,
        subject: Some((INBOX, inbound.inbox)),
    };
    raise_alert(tx, &alert)
        .await
        .map_err(|_| JobsError::Database)?;
    Ok(())
}

//! A homeowner's answer to a Connected job's check-in, dispatched from `receive_reply` inside the
//! delivery's transaction (JOB-015, JOB-016, TXT-029, TXT-020, PRIV-006, FEE-035; D-1810,
//! D-1811 (3), D-1840, D-3118, T277).
//!
//! A reply is a check-in answer when `jobs::api::parse_answer` reads it and one of her jobs is
//! Connected with a check-in due: day 14 once 14 days passed since the job became Connected, else
//! day 3 once its time passed (`jobs::api::check_in_times`, from the connection and the picked
//! quote's earliest visit). With two such jobs, the latest connection answers. A PROBLEM's words
//! are scrubbed again with `privacy::api::scrub` (the common words, no vault name, a fresh
//! `NameMemory`; the route already scrubbed them with her names, so this only removes more) and
//! kept with `privacy::api::keep_problem_words` before `jobs::api::answer_check_in` closes the job
//! and raises the alert naming the inbox row; a failed store fails the delivery, so Twilio resends.
//! An email-contact day-3 NO is not judged here (`intake` may not call `calls`, whose call log the
//! no-show needs): it queues T281's `picks.unreachable_step` (`day3_no`) for the pick, due now
//! and once per pick and answer, and that step records the no-show and returns the job.

use askhava_businesses::api::has_safety_word;
use askhava_core::api::enqueue;
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{Answer, Inbound, JobRow, answer_check_in, check_in_times, current};
use askhava_ops::api::Actor;
use askhava_picks::api::{Step, UNREACHABLE_STEP_KIND, accepted_pick, contact_choice, step_task};
use askhava_privacy::api::{CommonWords, Known, NameMemory, keep_problem_words, scrub};
use askhava_quotes::api::earliest_visit;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, IntakeError};
use crate::service::TextIn;

/// One of her Connected jobs whose check-in is due, with its pick and the contact she chose.
struct Due {
    job: Uuid,
    connected_at: Timestamp,
    check: CheckIn,
    pick: Option<Uuid>,
    contact: Contact,
}

/// JOB-015, JOB-016, FEE-035, D-1840: answers `msg` as a check-in reply when it reads as an answer
/// and one of her jobs is Connected with a check-in due; the job answered, or `None` (the text
/// goes to intake as before). Runs in the caller's transaction.
///
/// # Errors
///
/// [`IntakeError::Database`] for any failed read or write (the words' store included), so the
/// delivery rolls back; [`IntakeError::Job`] when the job is gone.
pub(crate) async fn check_in_reply(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
) -> Result<Option<Uuid>, IntakeError> {
    let Some(answer) = askhava_jobs::api::parse_answer(msg.inbound.words) else {
        return Ok(None);
    };
    let Some(due) = due_check_in(db, msg.contact, msg.received_at).await? else {
        return Ok(None);
    };
    if let (Answer::No, CheckIn::Day3, Contact::Email, Some(pick)) =
        (answer, due.check, due.contact, due.pick)
    {
        queue_day_three_no(db, pick, msg.received_at).await?;
        return Ok(Some(due.job));
    }
    let safety = has_safety_word(msg.inbound.words);
    if answer == Answer::Problem {
        keep_words(db, msg, due.job, safety).await?;
    }
    let inbound = Inbound {
        inbox: msg.inbox_id,
        safety,
    };
    let how = (answer, due.check, due.contact);
    let actor = Actor::Homeowner(msg.contact);
    // A job that moved on meanwhile changes nothing and raises nothing; the reply was still a
    // check-in answer, so it does not go on to intake.
    let answered = answer_check_in(db, due.job, how, inbound, (msg.received_at, actor)).await;
    drop(answered.map_err(job_err)?);
    Ok(Some(due.job))
}

/// PRIV-006, TXT-020, D-3118: scrubs the words again (only ever removing more) and keeps them
/// for the job and inbox row.
async fn keep_words(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
    job: Uuid,
    safety: bool,
) -> Result<(), IntakeError> {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let words = scrub(msg.inbound.words, &known, &mut NameMemory::default());
    keep_problem_words(db, msg.inbox_id, job, &words, safety)
        .await
        .map(drop)
        .map_err(|_| IntakeError::Database)
}

/// FEE-035, D-3488: T281's `day3_no` step for `pick`, due `now`, deduped per pick and answer so a
/// repeated NO queues nothing more.
async fn queue_day_three_no(
    db: &mut PgConnection,
    pick: Uuid,
    now: Timestamp,
) -> Result<(), IntakeError> {
    let mut task = step_task(pick, Step::DayThreeNo, now);
    task.dedupe_key = Some(format!(
        "{UNREACHABLE_STEP_KIND}.{pick}.{}",
        Step::DayThreeNo.code()
    ));
    enqueue(db, &task)
        .await
        .map(drop)
        .map_err(|_| IntakeError::Database)
}

/// JOB-015: of her jobs, the Connected one with a check-in due at `now` and the latest connection.
async fn due_check_in(
    db: &mut PgConnection,
    contact: Uuid,
    now: Timestamp,
) -> Result<Option<Due>, IntakeError> {
    let mut best: Option<Due> = None;
    for job in her_jobs(db, contact).await? {
        let row = current(db, job).await.map_err(job_err)?;
        if !row.status.takes_check_ins() {
            continue;
        }
        let Some(due) = check_in_of(db, job, &row, now).await? else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|b| due.connected_at > b.connected_at)
        {
            best = Some(due);
        }
    }
    Ok(best)
}

/// JOB-015, D-143: the check-in due at `now` on Connected job `job` (`row`), with its pick (the
/// `paid` or `free` one) and her contact choice (`Call` with no pick or no stored choice).
async fn check_in_of(
    db: &mut PgConnection,
    job: Uuid,
    row: &JobRow,
    now: Timestamp,
) -> Result<Option<Due>, IntakeError> {
    let picked = accepted_pick(db, job)
        .await
        .map_err(|_| IntakeError::Database)?;
    let mut visit = None;
    let mut contact = Contact::Call;
    if let Some(p) = &picked {
        visit = earliest_visit(db, (job, p.business))
            .await
            .map_err(|_| IntakeError::Database)?;
        let choice = contact_choice(db, p.pick)
            .await
            .map_err(|_| IntakeError::Database)?;
        if choice.email_chosen() {
            contact = Contact::Email;
        }
    }
    let (day3, day14) = check_in_times(row.status_at, visit);
    let check = if day14 <= now {
        CheckIn::Day14
    } else if day3.is_some_and(|d| d <= now) {
        CheckIn::Day3
    } else {
        return Ok(None);
    };
    Ok(Some(Due {
        job,
        connected_at: row.status_at,
        check,
        pick: picked.map(|p| p.pick),
        contact,
    }))
}

/// Every job any of her requests opened, open or closed (her job links and first jobs).
async fn her_jobs(db: &mut PgConnection, contact: Uuid) -> Result<Vec<Uuid>, IntakeError> {
    let requests = sqlx::query_scalar!(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1",
        contact
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let mut jobs = Vec::new();
    for request in requests {
        jobs.extend(models::request_jobs(db, request).await?);
    }
    jobs.sort_unstable();
    jobs.dedup();
    Ok(jobs)
}

/// A `jobs` error as `intake`'s: a failed statement is [`IntakeError::Database`] (the delivery
/// rolls back and Twilio resends), anything else [`IntakeError::Job`].
fn job_err(e: askhava_jobs::api::JobsError) -> IntakeError {
    if matches!(e, askhava_jobs::api::JobsError::Database) {
        IntakeError::Database
    } else {
        IntakeError::Job
    }
}

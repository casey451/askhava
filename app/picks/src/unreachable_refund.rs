//! T191: what happens after a release when the business cannot reach the homeowner (FEE-022,
//! JOB-027, TXT-026, TXT-030, FEE-050, FEE-052, JOB-013, JOB-014; D-049, D-141, D-142, D-236,
//! D-243, D-286, D-336, D-440, D-491).
//!
//! `payments` keeps the pure refund rule (`decide_unreachable`, `no_show`); this module gathers
//! the call log, sends the texts, makes the decision at 96 hours and moves the job, calling
//! `payments::api` for every refund and `jobs::api` for every move (D-491). Texts carry the public
//! business name, the job number and the masked pool number only, never a homeowner contact.

use askhava_adapters::api::PaymentsAdapter;
use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_calls::api::{AnsweredBy, CloseReason, LoggedCall, close_line};
use askhava_core::api::{Guarded, NewTask, enqueue as enqueue_task};
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::{
    AreaCodes, Channel, Decision, Kind, Language, NewText, Outgoing, QuietHoursError, Relation,
    decide, enqueue, render,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    Answer, CallRecord, Caller, Contact, EmailCheck, NoShow, RefundDecision, RefundTrigger, Unmet,
    UnreachableFacts, decide_unreachable, early_fraud_warning, mark_auto_submit, no_show,
    request_refund,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::PicksError;
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::status::PickStatus;

/// TXT-030: the missed-call text to the homeowner (an important text, TXT-014).
pub const MISSED_CALL_TEMPLATE: &str = "caller_missed";
/// TXT-026: the no-call reminder to the business.
pub const REMINDER_TEMPLATE: &str = "business_no_call_reminder";
/// TXT-026: the reminder when the homeowner chose email, with the email line.
pub const REMINDER_EMAIL_TEMPLATE: &str = "business_no_call_reminder_email";
/// TXT-026: the reminders go this long after the release while no call attempt was made.
pub const REMINDERS_AFTER: [SignedDuration; 2] = [
    SignedDuration::from_hours(24),
    SignedDuration::from_hours(48),
];

/// A released pick as these steps see it (IDs, the job number and the public business name).
#[derive(Debug, Clone, Copy)]
pub struct ReleasedPick<'a> {
    /// The pick.
    pub pick: Uuid,
    /// Its job.
    pub job: Uuid,
    /// The job number the texts name.
    pub job_number: i64,
    /// The business.
    pub business: Uuid,
    /// The business's public name (BIZ-022).
    pub business_name: &'a str,
    /// The homeowner's vault contact ID, never her number (PRIV-002).
    pub homeowner: Uuid,
    /// Her language, and whether Spanish texts are switched on (TXT-003).
    pub language: (Language, bool),
}

/// CALL-007: a logged call as the refund rule reads it; only a person answering is talking.
#[must_use]
pub fn call_record(call: &LoggedCall) -> CallRecord {
    let caller = match call.caller {
        askhava_calls::api::Caller::Business => Caller::Business,
        askhava_calls::api::Caller::Homeowner => Caller::Customer,
    };
    let answer = match call.answered {
        AnsweredBy::Person => Answer::Person,
        AnsweredBy::Machine => Answer::Machine,
        AnsweredBy::Nobody | AnsweredBy::Unknown => Answer::NoAnswer,
    };
    CallRecord {
        at: call.ended_at,
        caller,
        answer,
    }
}

/// TXT-026: the reminder times still owed at `released_at + 24h` and `+ 48h`; a business call
/// attempt before a reminder's time cancels it.
#[must_use]
pub fn reminder_times(released_at: Timestamp, calls: &[CallRecord]) -> Vec<Timestamp> {
    REMINDERS_AFTER
        .iter()
        .map(|after| released_at.saturating_add(*after).unwrap_or(Timestamp::MAX))
        .filter(|due| {
            !calls
                .iter()
                .any(|c| c.caller == Caller::Business && c.at < *due)
        })
        .collect()
}

/// TXT-026, D-049: the reminder template for the homeowner's contact choice.
#[must_use]
pub const fn reminder_template(contact: Contact) -> &'static str {
    match contact {
        Contact::Call => REMINDER_TEMPLATE,
        Contact::Email => REMINDER_EMAIL_TEMPLATE,
    }
}

/// TXT-030, D-236: when the missed-call text is owed: the first business attempt after the
/// release, when no person answered it; `None` otherwise (one text per pick).
#[must_use]
pub fn missed_call_at(released_at: Timestamp, calls: &[CallRecord]) -> Option<Timestamp> {
    calls
        .iter()
        .filter(|c| c.caller == Caller::Business && c.at >= released_at)
        .min_by_key(|c| c.at)
        .filter(|c| c.answer != Answer::Person)
        .map(|c| c.at)
}

/// QH-001, QH-004, TXT-026, TXT-030: when a text due at `at` to `phone` may go: the missed-call
/// text is a pick step (any hour on an emergency job), a reminder an update held to 8 a.m.
///
/// # Errors
/// [`QuietHoursError`] for a number that is never texted (QH-003).
pub fn send_at(
    table: &AreaCodes,
    (phone, kind, emergency): (&str, Kind, bool),
    at: Timestamp,
) -> Result<Timestamp, QuietHoursError> {
    let msg = Outgoing {
        phone,
        kind,
        channel: Channel::Text,
        emergency_job: emergency,
        after_hours_business: false,
        emergency_emails: false,
    };
    Ok(match decide(table, &msg, at)? {
        Decision::Send => at,
        Decision::Hold { until } => until,
    })
}

/// FEE-022, D-336: the decision email's template for `decision`, which names the condition met
/// or missed and points to "Ask about a charge" (BIZ-032). `None` when there is nothing to decide
/// yet, nothing was charged or the fee was already refunded.
#[must_use]
pub const fn decision_template(decision: RefundDecision) -> Option<&'static str> {
    match decision {
        RefundDecision::Refund(_) => Some("unreachable_refunded"),
        RefundDecision::NotYet
        | RefundDecision::NoRefund(Unmet::AlreadyRefunded | Unmet::NothingCharged) => None,
        RefundDecision::NoRefund(Unmet::Talked) => Some("unreachable_no_talked"),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(0)) => Some("unreachable_no_attempts"),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(_)) => Some("unreachable_no_one_attempt"),
        RefundDecision::NoRefund(Unmet::AttemptsTooClose) => Some("unreachable_no_too_close"),
        RefundDecision::NoRefund(Unmet::NoMissedCallNotice) => Some("unreachable_no_notice"),
        RefundDecision::NoRefund(Unmet::DoneOrProblemReply) => Some("unreachable_no_reply"),
        RefundDecision::NoRefund(Unmet::EmailedAndAgreed) => Some("unreachable_no_emailed"),
    }
}

/// FEE-050, D-490: evidence is auto-submitted only with a recorded release and at least one
/// recorded call attempt or a 3DS-authenticated payment.
#[must_use]
pub const fn auto_submits(released: bool, call_attempts: usize, three_ds: bool) -> bool {
    released && (call_attempts > 0 || three_ds)
}

fn render_text(
    id: &str,
    (lang, es): (Language, bool),
    values: &[(&str, &str)],
) -> Result<String, PicksError> {
    render(id, lang, es, values)
        .map(|t| t.body)
        .map_err(|_| PicksError::Template)
}

/// Queues homeowner text `template` about `case` once (keyed by template, job and pick).
async fn tell_homeowner(
    conn: &mut PgConnection,
    secret: &SecretString,
    case: &ReleasedPick<'_>,
    (template, number): (&str, &str),
) -> Result<(), PicksError> {
    let job = case.job_number.to_string();
    let values = [
        ("business", case.business_name),
        ("job", job.as_str()),
        ("number", number),
    ];
    let body = render_text(template, case.language, &values)?;
    let subject = [case.job, case.pick];
    let text = NewText {
        template_id: template,
        subject: &subject,
        recipient: case.homeowner,
        relation: Relation::OpenRequest,
        body: &body,
    };
    enqueue(conn, secret, &text)
        .await
        .map_err(|_| PicksError::Outbox)
        .map(drop)
}

/// TXT-030, D-236, D-286: queues the one missed-call text, naming the masked pool `number`, when
/// the first business attempt went unanswered. `true` when the text is owed (queued now or
/// before), the fact FEE-022's `notice_attempted` needs; a failed delivery still counts.
///
/// # Errors
/// `Template`, `Outbox`.
pub async fn missed_call_text(
    conn: &mut PgConnection,
    secret: &SecretString,
    (case, number): (&ReleasedPick<'_>, &str),
    (released_at, calls): (Timestamp, &[CallRecord]),
) -> Result<bool, PicksError> {
    if missed_call_at(released_at, calls).is_none() {
        return Ok(false);
    }
    tell_homeowner(conn, secret, case, (MISSED_CALL_TEMPLATE, number)).await?;
    Ok(true)
}

/// BIZ-019, D-308: queues business notice `template` about `case` at `at` once under `key`.
async fn tell_business(
    conn: &mut PgConnection,
    case: &ReleasedPick<'_>,
    (template, key): (&str, String),
    at: Timestamp,
) -> Result<bool, PicksError> {
    let to = notice_addresses(conn, case.business, NoticePurpose::Money)
        .await
        .map_err(|_| PicksError::Database)?;
    let payload = json!({
        "template": template, "business": case.business, "pick": case.pick, "job": case.job,
        "to": to, "values": {"job": case.job_number},
    });
    let mut task = NewTask::new(BUSINESS_NOTICE_KIND, payload, at);
    task.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{key}"));
    let queued = enqueue_task(conn, &task)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(queued.is_some())
}

/// TXT-026: queues each reminder still owed at its time (the business notice sender applies the
/// quiet hours); a call attempt before a reminder's time cancels it. Returns how many were new.
///
/// # Errors
/// `Database`.
pub async fn queue_reminders(
    conn: &mut PgConnection,
    case: &ReleasedPick<'_>,
    (released_at, contact, calls): (Timestamp, Contact, &[CallRecord]),
) -> Result<usize, PicksError> {
    let mut queued = 0;
    for due in reminder_times(released_at, calls) {
        let key = format!("no_call_reminder.{}.{}", case.pick, due.as_second());
        let what = (reminder_template(contact), key);
        queued += usize::from(tell_business(conn, case, what, due).await?);
    }
    Ok(queued)
}

/// FEE-022, JOB-027, D-142, D-336: the 96-hour decision. A refund goes through
/// `payments::api::request_refund`, then the job moves Connected to Closed (stopping the
/// check-ins) and the call line closes, once; every decision emails the business the condition
/// met or missed. `NotYet` does nothing. JOB-027's homeowner text and 72-hour question wait for
/// their templates (D-1818).
///
/// # Errors
/// `Database` (a refund or move failure too), `Template`, `Outbox`.
pub async fn decide_at_96_hours(
    pool: &PgPool,
    (stripe, secret): (&impl PaymentsAdapter, &SecretString),
    case: &ReleasedPick<'_>,
    facts: &UnreachableFacts<'_>,
    now: Timestamp,
) -> Result<RefundDecision, PicksError> {
    let decision = decide_unreachable(facts, now);
    if decision == RefundDecision::NotYet {
        return Ok(decision);
    }
    if let RefundDecision::Refund(reason) = decision {
        request_refund(
            pool,
            stripe,
            secret,
            (case.pick, RefundTrigger::Rule(reason)),
            now,
        )
        .await
        .map_err(|_| PicksError::Database)?;
        close_unreachable(pool, case, now).await?;
    }
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    if let Some(template) = decision_template(decision) {
        let key = format!("{template}.{}", case.pick);
        tell_business(&mut conn, case, (template, key), now).await?;
    }
    Ok(decision)
}

/// JOB-027, D-142: Connected to Closed and the line closed, in one transaction; a job already
/// moved on changes nothing. The homeowner's close text waits for its template (D-1818).
async fn close_unreachable(
    pool: &PgPool,
    case: &ReleasedPick<'_>,
    now: Timestamp,
) -> Result<(), PicksError> {
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    let arrow = (JobStatus::Connected, JobStatus::Closed);
    let moved = move_to(&mut tx, case.job, arrow, now, Actor::System).await;
    if moved.map_err(|_| PicksError::Job)? == Guarded::MovedOn {
        return Ok(());
    }
    close_line(
        &mut tx,
        (case.job, case.job_number),
        CloseReason::NoShow,
        now,
    )
    .await
    .map_err(|_| PicksError::Database)?;
    tx.commit().await.map_err(|_| PicksError::Database)
}

/// JOB-013, D-143: 72 hours after the release a call-contact job whose business placed no call
/// returns to Quotes ready with its line closed and its pick a no-show (never refunded, FEE-035);
/// an email-contact job stays Connected until the day-3 reply (`jobs`' check-in answer).
///
/// # Errors
/// `Database`, `Job`.
pub async fn no_call_at_72_hours(
    conn: &mut PgConnection,
    case: &ReleasedPick<'_>,
    (contact, released_at, calls): (Contact, Timestamp, &[CallRecord]),
    now: Timestamp,
) -> Result<bool, PicksError> {
    if contact == Contact::Email
        || no_show(contact, released_at, calls, EmailCheck::NoReply, now) != NoShow::Recorded
    {
        return Ok(false);
    }
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    let arrow = (JobStatus::Connected, JobStatus::QuotesReady);
    let moved = move_to(&mut tx, case.job, arrow, now, Actor::System).await;
    if moved.map_err(|_| PicksError::Job)? == Guarded::MovedOn {
        return Ok(false);
    }
    close_line(
        &mut tx,
        (case.job, case.job_number),
        CloseReason::NoShow,
        now,
    )
    .await
    .map_err(|_| PicksError::Database)?;
    end_pick(&mut tx, case.pick, PickStatus::NoShow, now).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(true)
}

/// Ends paid pick `pick` as `to` at `at`; a pick no longer paid is left alone.
async fn end_pick(
    tx: &mut PgConnection,
    pick: Uuid,
    to: PickStatus,
    at: Timestamp,
) -> Result<(), PicksError> {
    sqlx::query!(
        "UPDATE picks.pick SET status = $2, ended_at = $3 WHERE id = $1 AND status = $4",
        pick,
        to.code(),
        at.to_sqlx() as _,
        PickStatus::Paid.code(),
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)
    .map(drop)
}

/// FEE-050: marks `dispute` for auto-submit through `payments::api::mark_auto_submit` only when
/// [`auto_submits`]; every other dispute waits for Casey. `true` when newly marked.
///
/// # Errors
/// `Database`.
pub async fn dispute_auto_submit(
    conn: &mut PgConnection,
    dispute: &str,
    (released, call_attempts, three_ds): (bool, usize, bool),
    now: Timestamp,
) -> Result<bool, PicksError> {
    if !auto_submits(released, call_attempts, three_ds) {
        return Ok(false);
    }
    mark_auto_submit(conn, dispute, now)
        .await
        .map_err(|_| PicksError::Database)
}

/// FEE-052, JOB-014, D-491: an early fraud warning goes to `payments::api::early_fraud_warning`
/// (the alert, and the refund of an unreleased charge); an unreleased pick then ends as
/// fraud-refunded and its job returns from Payment pending to Quotes ready. `true` when moved.
///
/// # Errors
/// `Database`, `Job`.
pub async fn fraud_warning(
    pool: &PgPool,
    (stripe, secret): (&impl PaymentsAdapter, &SecretString),
    (pick, job, released): (Uuid, Uuid, bool),
    now: Timestamp,
) -> Result<bool, PicksError> {
    early_fraud_warning(pool, stripe, secret, (pick, released), now)
        .await
        .map_err(|_| PicksError::Database)?;
    if released {
        return Ok(false);
    }
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    end_pick(&mut tx, pick, PickStatus::FraudRefunded, now).await?;
    let arrow = (JobStatus::PaymentPending, JobStatus::QuotesReady);
    let moved = move_to(&mut tx, job, arrow, now, Actor::System).await;
    let moved = moved.map_err(|_| PicksError::Job)? == Guarded::Applied(());
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(moved)
}

//! T191: what happens after a release when the business cannot reach the homeowner (FEE-022,
//! JOB-027, TXT-026, TXT-030, FEE-050, FEE-052, JOB-013, JOB-014; D-049, D-141, D-142, D-236,
//! D-243, D-286, D-336, D-440, D-491).
//!
//! `payments` keeps the pure refund rule (`decide_unreachable`, `no_show`); this module gathers
//! the call log, sends the texts, makes the decision at 96 hours and moves the job, calling
//! `payments::api` for every refund and `jobs::api` for every move (D-491). Texts carry the public
//! business name, the job number and the masked pool number only, never a homeowner contact.
//!
//! T270 (D-1836, D-2145 to D-2147): the 96-hour close and the fraud-warning end act only when
//! `payments::api::refund_state` reads the pick's refund as `Succeeded`, whatever the refund call
//! answered; the no-show ends a `paid` or `free` pick, and a pick in neither rolls the move back.

use askhava_adapters::api::PaymentsAdapter;
use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_calls::api::{AnsweredBy, CloseReason, LoggedCall, close_line};
use askhava_core::api::{Guarded, NewTask, enqueue as enqueue_task};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to};
use askhava_messaging::api::{
    AreaCodes, Channel, Decision, Kind, Language, NewText, Outgoing, QuietHoursError, Relation,
    SendWindow, decide, enqueue, render,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    Answer, CallRecord, Caller, Contact, EmailCheck, NoShow, PickRefundState, RefundDecision,
    RefundTrigger, Unmet, UnreachableFacts, decide_unreachable, early_fraud_warning,
    mark_auto_submit, no_show, refund_state, request_refund,
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
/// JOB-027: the homeowner's text when the unreachable refund closes her job (D-1818, D-1842).
pub const UNREACHABLE_CLOSED_TEMPLATE: &str = "unreachable_closed";
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

/// CALL-007, FEE-022, D-1836: a logged call as the refund rule reads it; only a person answering
/// is talking. `at` is the call's start; a call whose start was never recorded (a lost callback,
/// Ask Hava's fault) is judged by its end time for every FEE-022 check and still counts as an
/// attempt, so the business never loses its refund for it.
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
        at: call.started_at.unwrap_or(call.ended_at),
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

/// TXT-030, D-236, D-286, D-1819: queues the one missed-call text, naming the masked pool
/// `number`, when the first business attempt went unanswered and her number may be texted.
/// `reach` is `messaging::api::send_window`'s answer for her vault contact (`picks` never reads
/// her phone): an opted-out or untextable number gets no text and the answer is `false`, so
/// FEE-022's `notice_attempted` is not met (the email copy is not built, D-1819). `true` when the
/// text is owed and queued (now or before); a failed delivery still counts, and the outbox's
/// TXT-014 retries email it as an important text. The outbox row carries no due time: the sender
/// holds it by [`send_at`] (D-1819).
///
/// # Errors
/// `Template`, `Outbox`.
pub async fn missed_call_text(
    conn: &mut PgConnection,
    secret: &SecretString,
    (case, number, reach): (&ReleasedPick<'_>, &str, SendWindow),
    (released_at, calls): (Timestamp, &[CallRecord]),
) -> Result<bool, PicksError> {
    let textable = match reach {
        SendWindow::Now | SendWindow::Hold { .. } => true,
        SendWindow::OptedOut | SendWindow::NotTextable => false,
    };
    if !textable || missed_call_at(released_at, calls).is_none() {
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

/// What the 96-hour step did (FEE-022, JOB-027, D-1819, D-2146).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtNinetySix {
    /// The decision was carried out (`NotYet`: nothing to do yet).
    Decided(RefundDecision),
    /// The refund rule was met but `payments` blocked the refund (`blocked`, a dispute, Casey is
    /// alerted) or Stripe failed it (its hourly retry is queued): nothing was closed or emailed,
    /// so the step runs again later.
    RefundWaiting {
        /// `true` for a disputed charge, `false` for a failed send.
        blocked: bool,
    },
    /// The refund rule was met and a try is still waiting for Stripe (`sending`, or held
    /// `pending`): nothing was closed or emailed, so the step runs again once Stripe answers
    /// (FEE-026, D-1836, D-2146).
    RefundInFlight,
}

/// FEE-022, FEE-026, FEE-052, D-1836: `None` only when `payments` reads the pick's refund as
/// `Succeeded`; otherwise what the 96-hour step answers instead of closing anything.
const fn refund_waiting(state: PickRefundState) -> Option<AtNinetySix> {
    match state {
        PickRefundState::Succeeded => None,
        PickRefundState::InFlight => Some(AtNinetySix::RefundInFlight),
        PickRefundState::Blocked => Some(AtNinetySix::RefundWaiting { blocked: true }),
        PickRefundState::Failed | PickRefundState::NoRefund => {
            Some(AtNinetySix::RefundWaiting { blocked: false })
        }
    }
}

/// D-1836: where `pick`'s refund stands, from `payments::api::refund_state`.
async fn read_refund(pool: &PgPool, pick: Uuid) -> Result<PickRefundState, PicksError> {
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    refund_state(&mut conn, pick)
        .await
        .map_err(|_| PicksError::Database)
}

/// FEE-022, FEE-026, JOB-027, D-142, D-336, D-1819, D-1836: the 96-hour decision. A refund goes
/// through `payments::api::request_refund`; only when `payments::api::refund_state` then reads it
/// `Succeeded` (whatever `request_refund` answered) the job moves Connected to Closed (stopping
/// the check-ins), the call line closes and the business gets the decision email, each once. A
/// blocked or failed refund changes nothing ([`AtNinetySix::RefundWaiting`]), nor does a try still
/// waiting for Stripe ([`AtNinetySix::RefundInFlight`]).
/// A pick refunded before is judged as if it were not, so a run that refunded but stopped before
/// the close finishes on the next run (`request_refund` sends nothing twice). `NotYet` does
/// nothing. The close queues JOB-027's homeowner text (D-1842); the 72-hour question's sending is
/// T281's (D-1818).
///
/// # Errors
/// `Database` (a refund or move failure too), `Template`, `Outbox`.
pub async fn decide_at_96_hours(
    pool: &PgPool,
    (stripe, secret): (&impl PaymentsAdapter, &SecretString),
    case: &ReleasedPick<'_>,
    facts: &UnreachableFacts<'_>,
    now: Timestamp,
) -> Result<AtNinetySix, PicksError> {
    let fresh = UnreachableFacts {
        refunded_before: false,
        ..*facts
    };
    let decision = match decide_unreachable(&fresh, now) {
        rule @ RefundDecision::Refund(_) => rule,
        _ => decide_unreachable(facts, now),
    };
    if decision == RefundDecision::NotYet {
        return Ok(AtNinetySix::Decided(decision));
    }
    if let RefundDecision::Refund(reason) = decision {
        let trigger = (case.pick, RefundTrigger::Rule(reason));
        request_refund(pool, stripe, secret, trigger, now)
            .await
            .map_err(|_| PicksError::Database)?;
        if let Some(waiting) = refund_waiting(read_refund(pool, case.pick).await?) {
            return Ok(waiting);
        }
        close_unreachable(pool, case, now).await?;
    }
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    if let Some(template) = decision_template(decision) {
        let key = format!("{template}.{}", case.pick);
        tell_business(&mut conn, case, (template, key), now).await?;
    }
    Ok(AtNinetySix::Decided(decision))
}

/// JOB-027, D-142, D-1842: Connected to Closed, the line closed and the homeowner's
/// [`UNREACHABLE_CLOSED_TEMPLATE`] text queued, in one transaction; a job already moved on
/// changes nothing and queues nothing.
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
    queue_closed_text(&mut tx, case, now).await?;
    tx.commit().await.map_err(|_| PicksError::Database)
}

/// JOB-027, D-1818, D-1842: one `jobs.homeowner_text` event for [`UNREACHABLE_CLOSED_TEMPLATE`]
/// at the closed job's version (a final status's, so the sender still sends it, QH-007), with the
/// job number and the business's public name only; `intake`'s sender applies consent, opt-out and
/// quiet hours. Deduped per job, so a rerun queues nothing more.
async fn queue_closed_text(
    tx: &mut PgConnection,
    case: &ReleasedPick<'_>,
    now: Timestamp,
) -> Result<(), PicksError> {
    let version = current(tx, case.job)
        .await
        .map_err(|_| PicksError::Job)?
        .version;
    let payload = json!({"job": case.job, "version": version,
        "template": UNREACHABLE_CLOSED_TEMPLATE,
        "values": {"job": case.job_number, "business": case.business_name}});
    let mut task = NewTask::new(TEXT_KIND, payload, now);
    task.dedupe_key = Some(format!(
        "{TEXT_KIND}.{}.{UNREACHABLE_CLOSED_TEMPLATE}",
        case.job
    ));
    enqueue_task(tx, &task)
        .await
        .map_err(|_| PicksError::Database)
        .map(drop)
}

/// JOB-013, JOB-027, D-143, D-1819: returns a no-show job to Quotes ready with its line closed and
/// its pick a no-show (never refunded, FEE-035): a call-contact job whose business placed no call
/// within 72 hours of the release, or an email-contact job whose homeowner answered NO to the
/// day-3 question (`day3`, from `jobs`' check-in answer). `true` when moved. FEE-035, D-1836,
/// D-2147: the pick ends as `no_show` with its ended time whether it was `paid` or `free` (a free
/// first win, a repeat customer or a fee-free job type), in the transaction of the move and the
/// line close; a pick in neither status rolls all three back.
///
/// # Errors
/// `Database`, `Job`; `MovedOn` when the pick is no longer `paid` or `free` (nothing kept).
pub async fn no_call_at_72_hours(
    conn: &mut PgConnection,
    case: &ReleasedPick<'_>,
    (contact, day3, released_at, calls): (Contact, EmailCheck, Timestamp, &[CallRecord]),
    now: Timestamp,
) -> Result<bool, PicksError> {
    if no_show(contact, released_at, calls, day3, now) != NoShow::Recorded {
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
    let connected = [PickStatus::Paid, PickStatus::Free];
    end_pick(&mut tx, (case.pick, &connected), PickStatus::NoShow, now).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(true)
}

/// D-1836, D-2147: ends pick `pick` as `to` at `at` only while its status is one of `from`;
/// `MovedOn` when it is in none (the caller's transaction is then dropped, so it rolls back).
async fn end_pick(
    tx: &mut PgConnection,
    (pick, from): (Uuid, &[PickStatus]),
    to: PickStatus,
    at: Timestamp,
) -> Result<(), PicksError> {
    let codes: Vec<&str> = from.iter().map(|s| s.code()).collect();
    let ended = sqlx::query!(
        "UPDATE picks.pick SET status = $2, ended_at = $3 WHERE id = $1 AND status = ANY($4)",
        pick,
        to.code(),
        at.to_sqlx() as _,
        &codes as &[&str],
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    if ended.rows_affected() == 0 {
        return Err(PicksError::MovedOn);
    }
    Ok(())
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

/// FEE-052, JOB-014, D-491, D-1819, D-1836: an early fraud warning goes to
/// `payments::api::early_fraud_warning` (the alert, and the refund of an unreleased charge); only
/// when `payments::api::refund_state` then reads an unreleased pick's refund as `Succeeded` its
/// job returns from Payment pending to Quotes ready and the pick ends as fraud-refunded, together;
/// a refund in flight, blocked or failed, or a job no longer in Payment pending, changes nothing.
/// `true` when moved. Only a `paid` pick ends (FEE-052 never applies to a fee-free pick).
///
/// # Errors
/// `Database`, `Job`; `MovedOn` when the pick is no longer `paid` (the move is rolled back).
pub async fn fraud_warning(
    pool: &PgPool,
    (stripe, secret): (&impl PaymentsAdapter, &SecretString),
    (pick, job, released): (Uuid, Uuid, bool),
    now: Timestamp,
) -> Result<bool, PicksError> {
    early_fraud_warning(pool, stripe, secret, (pick, released), now)
        .await
        .map_err(|_| PicksError::Database)?;
    if released || refund_waiting(read_refund(pool, pick).await?).is_some() {
        return Ok(false);
    }
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    let arrow = (JobStatus::PaymentPending, JobStatus::QuotesReady);
    let moved = move_to(&mut tx, job, arrow, now, Actor::System).await;
    if moved.map_err(|_| PicksError::Job)? == Guarded::MovedOn {
        return Ok(false);
    }
    let paid = (pick, &[PickStatus::Paid][..]);
    end_pick(&mut tx, paid, PickStatus::FraudRefunded, now).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(true)
}

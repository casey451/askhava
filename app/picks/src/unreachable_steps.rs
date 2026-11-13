//! T281: the worker steps that run T191's unreachable-homeowner rules (FEE-022, FEE-035, FEE-052,
//! JOB-013, JOB-027, TXT-026, TXT-030; D-1814, D-1818, D-1819, D-1845, D-2204, D-3486 to D-3490).
//!
//! One `core.task` kind, [`UNREACHABLE_STEP_KIND`], carries `{"pick", "step"}` ([`Step`]). The
//! release queues four in its claim's transaction ([`schedule_steps`]: reminders 24 and 48
//! hours after it, the 72-hour step, the 96-hour step); T271's route queues [`Step::BusinessCall`]
//! after each logged business call and T277 queues [`Step::EmailedAnswer`] for the homeowner's
//! YES or NO to the emailed question, each through [`step_task`]. Each run gathers the facts
//! (the release row, the contact choice, the job, the masked line's call log through
//! `calls::api::job_line` and `call_log`, the refund state) and calls the T191 functions that are
//! due. A refund still waiting runs the 96-hour step again an hour later ([`RERUN_AFTER`]); the
//! missed-call text is queued only at its send time; a number that can't be texted gets the
//! TXT-030 copy by email. [`run_fraud_warning`] runs `payments`' `picks.fraud_warning` task.
//! Payloads and keys hold IDs only (PRIV-002).

use askhava_adapters::api::PaymentsAdapter;
use askhava_businesses::api::public_names;
use askhava_calls::api::{call_log, job_line};
use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current};
use askhava_messaging::api::{
    Audience, EmailClass, Language, NewEmail, Notice, OnDomain, PostalAddress, Recipient,
    SendWindow, homeowner_has_email, queue_email, render, send_window,
};
use askhava_ops::api::{Actor, AuditEntry, Change, audit_once, audited};
use askhava_payments::api::{
    CallRecord, Contact, EmailCheck, FRAUD_WARNING_KIND, PickRefundState, UnreachableFacts,
    refund_state,
};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::contact_choice::contact_choice;
use crate::status::steps_run;
use crate::unreachable_refund::{
    AtNinetySix, MISSED_CALL_TEMPLATE, ReleasedPick, call_record, decide_at_96_hours,
    fraud_warning, missed_call_at, missed_call_text, no_call_at_72_hours, queue_reminders,
};

/// The unreachable-homeowner step task kind (D-1845).
pub const UNREACHABLE_STEP_KIND: &str = "picks.unreachable_step";
/// JOB-027: the 72-hour question to an email-contact homeowner (D-1817, D-1818).
pub const EMAILED_QUESTION_TEMPLATE: &str = "emailed_question";
/// The audit mark of the homeowner's answer to the emailed question (JOB-027, D-3488).
pub const EMAILED_YES_ACTION: &str = "picks.emailed_yes";
/// The audit mark of a NO to the emailed question (FEE-035, D-3488).
pub const EMAILED_NO_ACTION: &str = "picks.emailed_no";
/// A waiting refund's step, or a fraud warning's refund in flight, runs again this much later.
pub const RERUN_AFTER: SignedDuration = SignedDuration::from_hours(1);

/// One unreachable-homeowner step (D-1845).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// TXT-026: the no-call reminders (+ 24 and + 48 hours).
    Reminders,
    /// FEE-035, JOB-013, JOB-027: + 72 hours: the no-call return, or the emailed question.
    SeventyTwo,
    /// FEE-022, JOB-027: + 96 hours: the refund decision.
    NinetySix,
    /// TXT-030: after a logged business call: the missed-call text at its send time.
    BusinessCall,
    /// JOB-027, FEE-035: her answer to the emailed question, `true` for YES.
    EmailedAnswer(bool),
}

impl Step {
    /// The step's payload code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Reminders => "reminders",
            Self::SeventyTwo => "at_72",
            Self::NinetySix => "at_96",
            Self::BusinessCall => "business_call",
            Self::EmailedAnswer(true) => "emailed_yes",
            Self::EmailedAnswer(false) => "emailed_no",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        [
            Self::Reminders,
            Self::SeventyTwo,
            Self::NinetySix,
            Self::BusinessCall,
            Self::EmailedAnswer(true),
            Self::EmailedAnswer(false),
        ]
        .into_iter()
        .find(|s| s.code() == code)
    }
}

/// The steps the release queues and their hours after it (TXT-026, FEE-035, FEE-022).
pub const RELEASE_STEPS: [(Step, i64); 4] = [
    (Step::Reminders, 24),
    (Step::Reminders, 48),
    (Step::SeventyTwo, 72),
    (Step::NinetySix, 96),
];

/// The task for `step` of `pick` due `at`, deduped per pick, step and due second, so each due
/// time queues once (D-1845).
#[must_use]
pub fn step_task(pick: Uuid, step: Step, at: Timestamp) -> NewTask {
    let payload = json!({"pick": pick, "step": step.code()});
    let mut task = NewTask::new(UNREACHABLE_STEP_KIND, payload, at);
    task.dedupe_key = Some(format!(
        "{UNREACHABLE_STEP_KIND}.{pick}.{}.{}",
        step.code(),
        at.as_second()
    ));
    task
}

/// D-1845: queues [`RELEASE_STEPS`] for `pick` released at `at`, in the caller's transaction
/// (the release's claim), each once.
///
/// # Errors
/// `Database`.
pub async fn schedule_steps(
    tx: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<(), crate::models::PicksError> {
    for (step, hours) in RELEASE_STEPS {
        let due = at
            .saturating_add(SignedDuration::from_hours(hours))
            .unwrap_or(Timestamp::MAX);
        enqueue(tx, &step_task(pick, step, due))
            .await
            .map_err(|_| crate::models::PicksError::Database)?;
    }
    Ok(())
}

/// What the steps need besides the database: Stripe for the refund calls, the ID secret for the
/// outbox and email keys, the vault for her send window and email, and the email footer's postal
/// line. A step that needs a missing part fails, so it is
/// retried and, at the end, dead-lettered with an alert (OPS-004).
pub struct StepParts<P> {
    /// The Stripe client.
    pub stripe: Option<P>,
    /// The `ID_SECRET` key.
    pub secret: SecretString,
    /// Both vault keys.
    pub vault: Option<Vault>,
    /// The checked footer postal line (SEC-028).
    pub postal: Option<PostalAddress>,
}

/// Her portal page, the email's "Manage email notices" link (SEC-028).
const MANAGE: OnDomain = OnDomain("/h");
const FAILED: HandlerError = HandlerError("the unreachable step failed");
const MISSING: HandlerError = HandlerError("the unreachable step lacks a setting");
const MALFORMED: HandlerError = HandlerError("the unreachable step payload is malformed");

/// The facts one run reads (IDs, times and the call records only).
struct Gathered {
    pick: Uuid,
    job: Uuid,
    job_number: i64,
    business: Uuid,
    name: String,
    homeowner: Uuid,
    released_at: Timestamp,
    charged: bool,
    contact: Contact,
    status: JobStatus,
    emergency: bool,
    line: Option<String>,
    calls: Vec<CallRecord>,
}

impl Gathered {
    /// Spanish is not chosen here: `picks` cannot read her request's language (D-3489).
    const fn case(&self) -> ReleasedPick<'_> {
        ReleasedPick {
            pick: self.pick,
            job: self.job,
            job_number: self.job_number,
            business: self.business,
            business_name: self.name.as_str(),
            homeowner: self.homeowner,
            language: (Language::En, false),
        }
    }
}

/// The release row, the pick's status and her contact choice; `None` before a release.
async fn released(db: &mut PgConnection, pick: Uuid) -> Result<Option<Gathered>, HandlerError> {
    let row = sqlx::query!(
        "SELECT r.job_id, r.business_id, r.contact_id, r.released_at, p.status FROM picks.release r JOIN picks.pick p ON p.id = r.pick_id WHERE r.pick_id = $1",
        pick
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| FAILED)?;
    let Some(row) = row else { return Ok(None) };
    let choice = contact_choice(db, pick).await.map_err(|_| FAILED)?;
    let job = current(db, row.job_id).await.map_err(|_| FAILED)?;
    let number = askhava_jobs::api::job_number(db, row.job_id)
        .await
        .map_err(|_| FAILED)?;
    let names = public_names(db, &[row.business_id])
        .await
        .map_err(|_| FAILED)?;
    let name = names.into_iter().next().ok_or(FAILED)?.1;
    Ok(Some(Gathered {
        pick,
        job: row.job_id,
        job_number: number,
        business: row.business_id,
        name,
        homeowner: row.contact_id,
        released_at: row.released_at.to_jiff(),
        charged: row.status == "paid",
        contact: if choice.email_chosen() {
            Contact::Email
        } else {
            Contact::Call
        },
        status: job.status,
        emergency: job.emergency,
        line: None,
        calls: Vec::new(),
    }))
}

/// [`released`] plus the job's masked line and its call log (CALL-007, D-1836).
async fn gather(db: &mut PgConnection, pick: Uuid) -> Result<Option<Gathered>, HandlerError> {
    let Some(mut facts) = released(db, pick).await? else {
        return Ok(None);
    };
    if let Some((line, number)) = job_line(db, facts.job).await.map_err(|_| FAILED)? {
        let log = call_log(db, line).await.map_err(|_| FAILED)?;
        facts.calls = log.iter().map(call_record).collect();
        facts.line = Some(number);
    }
    Ok(Some(facts))
}

fn read_payload(payload: &Value) -> Option<(Uuid, Step)> {
    let pick = payload["pick"].as_str()?.parse().ok()?;
    Some((pick, Step::parse(payload["step"].as_str()?)?))
}

/// The `picks.unreachable_step` handler (D-1845): reads the step, gathers the facts and runs it
/// at `now`. A job no longer Connected gets no step, except the 96-hour step of a pick whose
/// refund already succeeded, so a run that refunded and closed but failed before the decision
/// email is finished by the queue's retry (the email's key keeps it once, D-2204).
///
/// # Errors
/// A fixed sentence for a malformed payload, a missing setting or a failed step (retried).
pub async fn run_unreachable_step<P: PaymentsAdapter>(
    pool: PgPool,
    parts: std::sync::Arc<StepParts<P>>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let (pick, step) = read_payload(&task.payload).ok_or(MALFORMED)?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let Some(facts) = gather(&mut db, pick).await? else {
        return Ok(());
    };
    let refunded =
        refund_state(&mut db, pick).await.map_err(|_| FAILED)? == PickRefundState::Succeeded;
    let finishing = step == Step::NinetySix && refunded;
    if !steps_run(facts.status) && !finishing {
        return Ok(());
    }
    match step {
        Step::Reminders => queue_reminders(&mut db, &facts.case(), facts_calls(&facts))
            .await
            .map(drop)
            .map_err(|_| FAILED),
        Step::SeventyTwo => at_72(&mut db, &facts, now).await,
        Step::BusinessCall => missed_call(&mut db, &parts, &facts, now).await,
        Step::EmailedAnswer(yes) => answered(&mut db, &facts, yes, now).await,
        Step::NinetySix => at_96(&pool, &mut db, (&parts, &facts), refunded, now).await,
    }
}

const fn facts_calls(facts: &Gathered) -> (Timestamp, Contact, &[CallRecord]) {
    (facts.released_at, facts.contact, facts.calls.as_slice())
}

/// FEE-035, JOB-013, JOB-027, D-1818: + 72 hours: a call-contact job with no business call
/// returns to Quotes ready as a no-show; an email-contact job gets the emailed question.
async fn at_72(
    db: &mut PgConnection,
    facts: &Gathered,
    now: Timestamp,
) -> Result<(), HandlerError> {
    match facts.contact {
        Contact::Call => {
            let what = (
                Contact::Call,
                EmailCheck::NoReply,
                facts.released_at,
                &facts.calls[..],
            );
            no_call(db, facts, what, now).await
        }
        Contact::Email => emailed_question(db, facts, now).await,
    }
}

/// `no_call_at_72_hours`; a pick that moved on (`MovedOn`) is done, not a failure.
async fn no_call(
    db: &mut PgConnection,
    facts: &Gathered,
    what: (Contact, EmailCheck, Timestamp, &[CallRecord]),
    now: Timestamp,
) -> Result<(), HandlerError> {
    match no_call_at_72_hours(db, &facts.case(), what, now).await {
        Ok(_) | Err(crate::models::PicksError::MovedOn) => Ok(()),
        Err(_) => Err(FAILED),
    }
}

/// JOB-027, D-1818: one `jobs.homeowner_text` event for [`EMAILED_QUESTION_TEMPLATE`] at the
/// job's version with the business's public name, deduped per job; `intake`'s sender applies
/// consent, opt-out and quiet hours.
async fn emailed_question(
    db: &mut PgConnection,
    facts: &Gathered,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let version = current(db, facts.job).await.map_err(|_| FAILED)?.version;
    let payload = json!({"job": facts.job, "version": version,
        "template": EMAILED_QUESTION_TEMPLATE,
        "values": {"job": facts.job_number, "business": facts.name}});
    let mut task = NewTask::new(TEXT_KIND, payload, now);
    task.dedupe_key = Some(format!(
        "{TEXT_KIND}.{}.{EMAILED_QUESTION_TEMPLATE}",
        facts.job
    ));
    enqueue(db, &task).await.map(drop).map_err(|_| FAILED)
}

/// JOB-027, FEE-035, D-3488: stores her answer once as an audit mark on the pick; a NO on an
/// email-contact job records the no-show (`no_call_at_72_hours` with the day-3 NO).
async fn answered(
    db: &mut PgConnection,
    facts: &Gathered,
    yes: bool,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: if yes {
            EMAILED_YES_ACTION
        } else {
            EMAILED_NO_ACTION
        },
        subject_kind: "picks.pick",
        subject_id: facts.pick,
        change: Change::Action,
    };
    audit_once(db, &entry).await.map_err(|_| FAILED)?;
    if yes || facts.contact != Contact::Email {
        return Ok(());
    }
    let what = (
        Contact::Email,
        EmailCheck::No,
        facts.released_at,
        &facts.calls[..],
    );
    no_call(db, facts, what, now).await
}

/// TXT-030, QH-001, QH-004, D-1819, D-3487: the missed-call notice when the first business
/// attempt went unanswered: texted now inside her window or at any hour on an emergency job;
/// outside it the step is queued again for the window's opening, so the text is never queued
/// early; a number that can't be texted gets the same words by email when one is on file.
async fn missed_call<P>(
    db: &mut PgConnection,
    parts: &StepParts<P>,
    facts: &Gathered,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let (Some(number), true) = (
        &facts.line,
        missed_call_at(facts.released_at, &facts.calls).is_some(),
    ) else {
        return Ok(());
    };
    let vault = parts.vault.as_ref().ok_or(MISSING)?;
    let reach = send_window(db, vault, facts.homeowner, now)
        .await
        .map_err(|_| FAILED)?;
    let reach = match reach {
        SendWindow::Hold { .. } if facts.emergency => SendWindow::Now,
        SendWindow::Hold { until } => {
            let again = step_task(facts.pick, Step::BusinessCall, until);
            return enqueue(db, &again).await.map(drop).map_err(|_| FAILED);
        }
        other => other,
    };
    let calls = (facts.released_at, &facts.calls[..]);
    let what = (&facts.case(), number.as_str(), reach);
    if missed_call_text(db, &parts.secret, what, calls)
        .await
        .map_err(|_| FAILED)?
    {
        return Ok(());
    }
    email_copy(db, parts, facts, number, now).await.map(drop)
}

/// TXT-030, SEC-028: the missed-call words as an email to her (the outbox's `TextFailed` notice
/// in the layout with the postal line and her manage link), once per job and pick; `false` when
/// no email is on file.
async fn email_copy<P>(
    db: &mut PgConnection,
    parts: &StepParts<P>,
    facts: &Gathered,
    number: &str,
    now: Timestamp,
) -> Result<bool, HandlerError> {
    let vault = parts.vault.as_ref().ok_or(MISSING)?;
    if !homeowner_has_email(db, vault, facts.homeowner)
        .await
        .map_err(|_| FAILED)?
    {
        return Ok(false);
    }
    let postal = parts.postal.as_ref().ok_or(MISSING)?;
    let manage_url = &MANAGE.to_string();
    let job = facts.job_number.to_string();
    let values = [
        ("business", facts.name.as_str()),
        ("job", job.as_str()),
        ("number", number),
    ];
    let body = render(MISSED_CALL_TEMPLATE, Language::En, false, &values).map_err(|_| FAILED)?;
    let to = Recipient {
        id: facts.homeowner,
        audience: Audience::Homeowner,
        language: Language::En,
        manage_url,
    };
    let email = NewEmail {
        template_id: MISSED_CALL_TEMPLATE,
        subject: &[facts.job, facts.pick],
        class: EmailClass::Notice,
        notice: Notice::TextFailed(&body.body),
    };
    queue_email(db, (&parts.secret, postal), &to, &email, now)
        .await
        .map_err(|_| FAILED)?;
    Ok(true)
}

/// Whether the missed-call notice was sent or attempted: owed, and a textable number or an email
/// on file (FEE-022, D-1819); a delivery failure still counts.
async fn notice_attempted<P>(
    db: &mut PgConnection,
    parts: &StepParts<P>,
    facts: &Gathered,
    now: Timestamp,
) -> Result<bool, HandlerError> {
    if missed_call_at(facts.released_at, &facts.calls).is_none() {
        return Ok(false);
    }
    let vault = parts.vault.as_ref().ok_or(MISSING)?;
    let reach = send_window(db, vault, facts.homeowner, now)
        .await
        .map_err(|_| FAILED)?;
    if matches!(reach, SendWindow::Now | SendWindow::Hold { .. }) {
        return Ok(true);
    }
    homeowner_has_email(db, vault, facts.homeowner)
        .await
        .map_err(|_| FAILED)
}

/// Her answer to the emailed question, from the marks [`answered`] stores (JOB-027).
async fn email_check(db: &mut PgConnection, pick: Uuid) -> Result<EmailCheck, HandlerError> {
    let subject = ("picks.pick", pick);
    if audited(db, EMAILED_YES_ACTION, subject)
        .await
        .map_err(|_| FAILED)?
    {
        return Ok(EmailCheck::Yes);
    }
    let no = audited(db, EMAILED_NO_ACTION, subject)
        .await
        .map_err(|_| FAILED)?;
    Ok(if no {
        EmailCheck::No
    } else {
        EmailCheck::NoReply
    })
}

/// FEE-022, JOB-027, D-1819, D-2204: the 96-hour decision; a refund waiting or in flight queues
/// this step again [`RERUN_AFTER`] later. A job still Connected has had no DONE or PROBLEM
/// reply (either moves it on); `refunded` is the pick's succeeded refund.
async fn at_96<P: PaymentsAdapter>(
    pool: &PgPool,
    db: &mut PgConnection,
    (parts, facts): (&StepParts<P>, &Gathered),
    refunded: bool,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let stripe = parts.stripe.as_ref().ok_or(MISSING)?;
    let rule = UnreachableFacts {
        released_at: facts.released_at,
        emergency: facts.emergency,
        calls: &facts.calls,
        notice_attempted: notice_attempted(db, parts, facts, now).await?,
        done_or_problem: false,
        contact: facts.contact,
        email_check: email_check(db, facts.pick).await?,
        charged: facts.charged,
        refunded_before: refunded,
    };
    let case = facts.case();
    let done = decide_at_96_hours(pool, (stripe, &parts.secret), &case, &rule, now)
        .await
        .map_err(|_| FAILED)?;
    if matches!(done, AtNinetySix::Decided(_)) {
        return Ok(());
    }
    let at = now.saturating_add(RERUN_AFTER).map_err(|_| FAILED)?;
    let again = step_task(facts.pick, Step::NinetySix, at);
    enqueue(db, &again).await.map(drop).map_err(|_| FAILED)
}

/// The `picks.fraud_warning` handler (FEE-052, JOB-014, D-1845, D-2071): runs
/// `fraud_warning` for the payload's pick (its job, and whether it was released); while an
/// unreleased pick's refund is still in flight it queues itself again [`RERUN_AFTER`] later. A
/// released pick changes nothing here (`payments` lists it for Casey).
///
/// # Errors
/// A fixed sentence for a malformed payload, no Stripe client or a failed step (retried).
pub async fn run_fraud_warning<P: PaymentsAdapter>(
    pool: PgPool,
    parts: std::sync::Arc<StepParts<P>>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick: Uuid = task.payload["pick"]
        .as_str()
        .and_then(|p| p.parse().ok())
        .ok_or(MALFORMED)?;
    let stripe = parts.stripe.as_ref().ok_or(MISSING)?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let row = sqlx::query!(
        r#"SELECT p.job_id, EXISTS (SELECT 1 FROM picks.release r WHERE r.pick_id = p.id) AS "released!" FROM picks.pick p WHERE p.id = $1"#,
        pick
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| FAILED)?
    .ok_or(MALFORMED)?;
    let what = (pick, row.job_id, row.released);
    match fraud_warning(&pool, (stripe, &parts.secret), what, now).await {
        Ok(_) | Err(crate::models::PicksError::MovedOn) => {}
        Err(_) => return Err(FAILED),
    }
    let state = refund_state(&mut db, pick).await.map_err(|_| FAILED)?;
    if row.released || state != PickRefundState::InFlight {
        return Ok(());
    }
    let at = now.saturating_add(RERUN_AFTER).map_err(|_| FAILED)?;
    let mut again = NewTask::new(FRAUD_WARNING_KIND, json!({"pick": pick}), at);
    again.dedupe_key = Some(format!("{FRAUD_WARNING_KIND}.{pick}.{}", at.as_second()));
    enqueue(&mut db, &again).await.map(drop).map_err(|_| FAILED)
}

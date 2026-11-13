//! The who's-calling text (TXT-027, QH-004, TXT-014; T318, D-1687, D-2206, D-3118).
//!
//! After a release opens the job's masked line, `picks` queues one [`WHOS_CALLING_KIND`] task per
//! pick with [`queue_whos_calling`] (IDs, the request number and the emergency flag only, never a
//! contact value, PRIV-002). Its handler, [`run_whos_calling`], registered in `server`, calls
//! [`send_whos_calling`]: it reads the line opened for that pick's job, business and homeowner
//! contact (none yet: the task fails and runs again after its backoff, so a line that opens later
//! through `picks.line_retry` still gets the text once), then asks `messaging::api::send_window`
//! when the text may go. An emergency job's text goes at once at any hour (QH-004, stored exempt);
//! a regular job's goes now inside her window, else one more task is queued at the window's start
//! and the text waits for it (QH-001, QH-006); an opted-out or untextable number gets nothing
//! (CON-013, QH-003). The text names the business by its public name
//! (`businesses::api::public_names`, [`generic_name`] when it has none) and the masked number;
//! an email-contact pick (PICK-018) gets [`WHOS_CALLING_EMAIL_TEMPLATE`]'s words instead, still by
//! text (TD-TXT-29). Both templates are important texts (`messaging`'s `IMPORTANT_TEMPLATES`),
//! so failed sends are retried, then emailed (TXT-014, TXT-015). The outbox keys the text by its
//! template, job, pick and recipient, so a repeated task never sends it twice.

use askhava_businesses::api::public_names;
use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue as enqueue_task};
use askhava_messaging::api::{
    Exemption, Language, NewText, Relation, SendWindow, enqueue, enqueue_exempt, render,
    send_window,
};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::job_line;
use crate::models::CallsError;

/// The task kind that sends the who's-calling text (TXT-027, D-1687).
pub const WHOS_CALLING_KIND: &str = "calls.whos_calling";
/// The who's-calling text for a homeowner who takes the business's call (TXT-027).
pub const WHOS_CALLING_TEMPLATE: &str = "whos_calling";
/// The who's-calling text for a homeowner who chose email contact (TXT-027, PICK-018).
pub const WHOS_CALLING_EMAIL_TEMPLATE: &str = "whos_calling_email";

/// Claims the first task gets while it waits for a line `picks.line_retry` opens later: with the
/// worker's backoff (30 seconds doubling to an hour) about 3 hours (D-3118).
pub const LINE_WAIT_ATTEMPTS: i32 = 10;

/// One pick's who's-calling text: IDs, the request number and the emergency flag only (PRIV-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhosCalling {
    /// The released pick; the task and the text are kept once per pick (D-3118).
    pub pick: Uuid,
    /// The pick's job.
    pub job: Uuid,
    /// The job's request number, for `{job}`.
    pub job_number: i64,
    /// The picked business.
    pub business: Uuid,
    /// The homeowner's vault contact bound to the release.
    pub homeowner: Uuid,
    /// She chose email contact at the pick (PICK-018).
    pub email_contact: bool,
    /// She chose emergency for the job (QH-004).
    pub emergency: bool,
}

impl WhosCalling {
    /// TXT-027, PICK-018: the template for her contact choice.
    #[must_use]
    pub const fn template(&self) -> &'static str {
        if self.email_contact {
            WHOS_CALLING_EMAIL_TEMPLATE
        } else {
            WHOS_CALLING_TEMPLATE
        }
    }

    fn payload(&self) -> Value {
        json!({
            "pick": self.pick, "job": self.job, "job_number": self.job_number,
            "business": self.business, "homeowner": self.homeowner,
            "email_contact": self.email_contact, "emergency": self.emergency,
        })
    }

    fn from_payload(payload: &Value) -> Option<Self> {
        let id = |key: &str| payload[key].as_str().and_then(|v| Uuid::parse_str(v).ok());
        Some(Self {
            pick: id("pick")?,
            job: id("job")?,
            job_number: payload["job_number"].as_i64()?,
            business: id("business")?,
            homeowner: id("homeowner")?,
            email_contact: payload["email_contact"].as_bool()?,
            emergency: payload["emergency"].as_bool()?,
        })
    }
}

/// What [`send_whos_calling`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhosCallingSent {
    /// The text is queued in the outbox (now, or before by an earlier run).
    Queued,
    /// Outside her window on a regular job: one task waits for `until` (QH-001, QH-006).
    Held {
        /// The window's start.
        until: Timestamp,
    },
    /// Her number is opted out or cannot be texted: no text (CON-013, QH-003).
    NotTextable,
    /// The pick's masked line is not open yet: nothing sent.
    NoLine,
}

/// TXT-027: the business's name when it has no public name, in `language` (D-2206).
#[must_use]
pub const fn generic_name(language: Language) -> &'static str {
    match language {
        Language::En => "The business you picked",
        Language::Es => "La empresa que eligió",
    }
}

/// TXT-027, D-3118: queues the one [`WHOS_CALLING_KIND`] task of `call`'s pick, due at `at`; a
/// repeated release or retry queues none (one dedupe key per pick). `true` when this call queued
/// it.
///
/// # Errors
/// [`CallsError::Database`].
pub async fn queue_whos_calling(
    db: &mut PgConnection,
    call: &WhosCalling,
    at: Timestamp,
) -> Result<bool, CallsError> {
    let mut task = NewTask::new(WHOS_CALLING_KIND, call.payload(), at);
    task.max_attempts = LINE_WAIT_ATTEMPTS;
    task.dedupe_key = Some(format!("{WHOS_CALLING_KIND}.{}", call.pick));
    let queued = enqueue_task(db, &task)
        .await
        .map_err(|_| CallsError::Database)?;
    Ok(queued.is_some())
}

/// TXT-027, QH-001, QH-004, QH-006, PICK-018, D-1687: sends `call`'s who's-calling text (see the
/// module docs): at once on an emergency job, else inside her window, else one task at the
/// window's start. Reads her phone through `send_window`, so call it outside a transaction.
///
/// # Errors
/// [`CallsError::Database`] or [`CallsError::Messaging`] (the window, the template or the outbox).
pub async fn send_whos_calling(
    db: &mut PgConnection,
    (vault, secret): (&Vault, &SecretString),
    call: &WhosCalling,
    now: Timestamp,
) -> Result<WhosCallingSent, CallsError> {
    let whose = (call.job, call.business, call.homeowner);
    let Some((_, number)) = job_line(db, whose).await? else {
        return Ok(WhosCallingSent::NoLine);
    };
    let reach = send_window(db, vault, call.homeowner, now)
        .await
        .map_err(|_| CallsError::Messaging)?;
    match reach {
        SendWindow::OptedOut | SendWindow::NotTextable => return Ok(WhosCallingSent::NotTextable),
        SendWindow::Hold { until } if !call.emergency => {
            hold(db, call, until).await?;
            return Ok(WhosCallingSent::Held { until });
        }
        SendWindow::Hold { .. } | SendWindow::Now => {}
    }
    let name = public_names(db, &[call.business])
        .await
        .map_err(|_| CallsError::Database)?
        .into_iter()
        .next()
        .map(|(_, name)| name);
    let language = (Language::En, false);
    let name = name.as_deref().unwrap_or(generic_name(language.0));
    let job = call.job_number.to_string();
    let values = [
        ("business", name),
        ("job", job.as_str()),
        ("number", &number),
    ];
    let body = render(call.template(), language.0, language.1, &values)
        .map_err(|_| CallsError::Messaging)?
        .body;
    let subject = [call.job, call.pick];
    let text = NewText {
        template_id: call.template(),
        subject: &subject,
        recipient: call.homeowner,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let queued = if call.emergency {
        enqueue_exempt(db, secret, &text, Exemption::EmergencyJob).await
    } else {
        enqueue(db, secret, &text).await
    };
    queued.map_err(|_| CallsError::Messaging)?;
    Ok(WhosCallingSent::Queued)
}

/// QH-006: one more [`WHOS_CALLING_KIND`] task at `until`, the window's start, kept once per pick
/// and start time.
async fn hold(
    db: &mut PgConnection,
    call: &WhosCalling,
    until: Timestamp,
) -> Result<(), CallsError> {
    let mut task = NewTask::new(WHOS_CALLING_KIND, call.payload(), until);
    task.dedupe_key = Some(format!("{WHOS_CALLING_KIND}.{}.{until}", call.pick));
    enqueue_task(db, &task)
        .await
        .map_err(|_| CallsError::Database)
        .map(drop)
}

/// The [`WHOS_CALLING_KIND`] handler (TXT-027): [`send_whos_calling`] for the task's pick; a line
/// not open yet fails the task so the worker runs it again after its backoff (D-3118).
///
/// # Errors
/// [`HandlerError`] for a bad payload, a failed send or a line not open yet.
pub async fn run_whos_calling(
    pool: PgPool,
    (vault, secret): (&Vault, &SecretString),
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let call = WhosCalling::from_payload(&task.payload)
        .ok_or(HandlerError("who's-calling task has a bad payload"))?;
    let mut db = pool
        .acquire()
        .await
        .map_err(|_| HandlerError("who's-calling text failed"))?;
    match send_whos_calling(&mut db, (vault, secret), &call, now).await {
        Ok(WhosCallingSent::NoLine) => Err(HandlerError("who's-calling line is not open yet")),
        Ok(_) => Ok(()),
        Err(_) => Err(HandlerError("who's-calling text failed")),
    }
}

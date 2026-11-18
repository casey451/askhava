//! The owner's HELP notification email (OPS-036, OPS-035, PRIV-030, WORD-021; T151, D-348, D-349,
//! D-3881).
//!
//! `intake` opens or joins a help request (`ops::api::open_help_request`) and, for a request
//! notified now, queues one [`HELP_NOTICE_KIND`] task holding a [`HelpNotice`]: the job facts it
//! gathered and the message after `privacy`'s scrubber and `quotes`' CARD-008 gate, so no email
//! address, street address or phone number reaches the event or the alert mailbox. This module
//! composes the email from the event only ([`help_lines`]) and sends it to the OPS-035 alert
//! address with [`HelpSender`], as an email only (never a text or call, so it never breaks Do Not
//! Disturb). The email carries no personal name, only "homeowner" or "business" and a number, and
//! one link to the owner's page for the request (`/owner/help/<id>`, T200's, behind the owner's
//! sign-in). With no sender (no provider or alert address, dev and Ci) the task sends nothing.

use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{
    Claimed, Clock, HandlerError, NewTask, Registry, TaskError, arizona_time, brand, enqueue,
};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::{
    ALERT_ADDRESS, DailyLimit, EmailError, EmailSent, EmailTo, MailboxKind, PostalAddress,
    queue_account_email, send_email,
};

/// The `core.task` kind of one HELP notification (OPS-036).
pub const HELP_NOTICE_KIND: &str = "messaging.help_notice";
/// The email key template of a HELP notification.
pub const HELP_EMAIL_TEMPLATE: &str = "owner_help_notice";
/// The owner's page for one request (T200's route), followed by the request ID.
pub const HELP_PAGE_PATH: &str = "/owner/help/";
const FAILED: HandlerError = HandlerError("the help notification failed");

/// The facts one HELP notification shows (OPS-036), gathered by `intake`. `message` is the
/// scrubbed and gated text only (PRIV-030); no field holds a name or contact detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelpNotice {
    /// The help request.
    pub request: Uuid,
    /// `homeowner` or `business`.
    pub asker: String,
    /// The job number, or for a business with no job a short reference.
    pub number: String,
    /// The job status code, if there is a job.
    pub status: Option<String>,
    /// The trade code, if known.
    pub trade: Option<String>,
    /// The town code, if known.
    pub town: Option<String>,
    /// Whether the job is urgent or an emergency.
    pub urgent: bool,
    /// The quote list's first line, if any quote was shown.
    pub list_line: Option<String>,
    /// The message after the scrubber and the CARD-008 gate.
    pub message: String,
    /// The template ID of the last automated message to the asker.
    pub last_text: Option<String>,
    /// The reply-by time, in Unix seconds.
    pub due: i64,
}

/// OPS-036: queues the one notification of `notice`'s request (one per request, by dedupe key).
///
/// # Errors
///
/// `TaskError` from the enqueue.
pub async fn queue_help_notice(
    db: &mut PgConnection,
    notice: &HelpNotice,
    now: Timestamp,
) -> Result<(), TaskError> {
    let payload = serde_json::to_value(notice).map_err(|_| TaskError::Db)?;
    let mut task = NewTask::new(HELP_NOTICE_KIND, payload, now);
    task.dedupe_key = Some(format!("{HELP_NOTICE_KIND}:{}", notice.request));
    enqueue(db, &task).await.map(drop)
}

/// OPS-036: the template ID of the newest text or email queued to `recipient`, if any.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn last_text(
    db: &mut PgConnection,
    recipient: Uuid,
) -> Result<Option<String>, EmailError> {
    sqlx::query_scalar!(
        "SELECT template_id FROM messaging.outbox WHERE recipient_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
        recipient
    )
    .fetch_optional(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// OPS-036, WORD-021: the subject ("<brand> HELP: <homeowner or business> <number>, reply by
/// <Arizona time>") and lines of `notice`'s email; no personal name.
#[must_use]
pub fn help_lines(notice: &HelpNotice) -> (String, Vec<String>) {
    let due = Timestamp::from_second(notice.due).unwrap_or(Timestamp::UNIX_EPOCH);
    let due = arizona_time(due).strftime("%b %-d, %-I:%M %p Arizona time");
    let subject = format!(
        "{} HELP: {} {}, reply by {due}",
        brand().short_name(),
        notice.asker,
        notice.number
    );
    let plain = |v: &Option<String>| {
        v.as_deref()
            .map_or_else(|| "none".to_owned(), |c| c.replace('_', " "))
    };
    let lines = vec![
        format!("A {} needs a reply from you.", notice.asker),
        format!("Job or business: {} {}", notice.asker, notice.number),
        format!("Job status: {}", plain(&notice.status)),
        format!("Trade: {}", plain(&notice.trade)),
        format!("Town: {}", plain(&notice.town)),
        format!(
            "Urgent or emergency: {}",
            if notice.urgent { "yes" } else { "no" }
        ),
        format!(
            "Quotes: {}",
            notice.list_line.as_deref().unwrap_or("none yet")
        ),
        format!("Message: {}", notice.message),
        format!(
            "Last text or email we sent (template): {}",
            plain(&notice.last_text)
        ),
        format!("Reply by: {due}"),
        format!(
            "See the request: https://{}{HELP_PAGE_PATH}{}",
            brand().domain(),
            notice.request
        ),
    ];
    (subject, lines)
}

/// The HELP notification sender: the email key, the provider, the footer's postal line, the
/// shared daily limit and the OPS-035 alert address (never logged).
pub struct HelpSender<E> {
    secret: SecretString,
    provider: E,
    postal: (String, MailboxKind),
    limit: DailyLimit,
    address: SecretString,
}

impl<E: EmailAdapter + Send + Sync> HelpSender<E> {
    /// A sender to `address` through `provider`, keyed with `secret`.
    ///
    /// # Errors
    ///
    /// [`EmailError::PostalAddress`] for a postal line that is not that kind of mailbox.
    pub fn new(
        (secret, provider): (SecretString, E),
        (postal, kind): (&str, MailboxKind),
        limit: DailyLimit,
        address: SecretString,
    ) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        Ok(Self {
            secret,
            provider,
            postal: (postal.trim().to_owned(), kind),
            limit,
            address,
        })
    }
}

/// OPS-036, OPS-035: emails `notice` to the alert address at once, keyed by its request, so a
/// rerun sends it once.
///
/// # Errors
///
/// An [`EmailError`] from queueing or sending; a held send is [`EmailError::Provider`].
pub async fn send_help_notice<E: EmailAdapter + Send + Sync>(
    db: &mut PgConnection,
    sender: &HelpSender<E>,
    notice: &HelpNotice,
    now: Timestamp,
) -> Result<(), EmailError> {
    let (subject, lines) = help_lines(notice);
    let keys = (&sender.secret, (sender.postal.0.as_str(), sender.postal.1));
    let ids = [notice.request];
    let (row, _) = queue_account_email(
        db,
        keys,
        (HELP_EMAIL_TEMPLATE, ALERT_ADDRESS, &ids),
        (&subject, &lines),
        now,
    )
    .await?;
    let to = EmailTo::Business(sender.address.expose_secret());
    let provider = (&sender.secret, &sender.provider, sender.limit);
    match send_email(db, provider, row, to, now).await? {
        EmailSent::Sent(_) | EmailSent::AlreadySent => Ok(()),
        EmailSent::Waiting(_) => Err(EmailError::Provider),
    }
}

/// Registers the [`HELP_NOTICE_KIND`] handler; with no sender it takes the task and sends nothing.
pub fn register_help_notices<E>(
    registry: &mut Registry,
    sender: Option<Arc<HelpSender<E>>>,
    clock: &Arc<dyn Clock>,
) where
    E: EmailAdapter + Send + Sync + 'static,
{
    let clock = Arc::clone(clock);
    registry.register(HELP_NOTICE_KIND, move |pool, task| {
        let (sender, now) = (sender.clone(), clock.now());
        async move { run_help_notice(&pool, sender.as_deref(), &task, now).await }
    });
}

/// The [`HELP_NOTICE_KIND`] handler: parses the event and sends its one email.
///
/// # Errors
///
/// [`HandlerError`] for a bad payload, a database error or a failed send (the task retries).
pub async fn run_help_notice<E: EmailAdapter + Send + Sync>(
    pool: &PgPool,
    sender: Option<&HelpSender<E>>,
    task: &Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let notice: HelpNotice = serde_json::from_value(task.payload.clone())
        .map_err(|_| HandlerError("bad help notice payload"))?;
    let Some(sender) = sender else {
        return Ok(());
    };
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    send_help_notice(&mut conn, sender, &notice, now)
        .await
        .map_err(|_| FAILED)
}

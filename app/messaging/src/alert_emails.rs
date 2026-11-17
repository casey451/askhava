//! The alert emails to Casey's alert address, the email-limit check and the hourly webhook-silence
//! check (OPS-004, OPS-029, OPS-032, OPS-035, SEC-051; T204, D-218, D-277, D-314, D-381, D-2207,
//! D-3880, D-4044).
//!
//! `ops` decides what pushes (`ops::api::alerts_to_push`, `daytime_push_due`); this module sends.
//! Every push is an email to the OPS-035 alert address, which the owner guide sets to notify on
//! Casey's phone; nothing here sends a text, and an email can never break Do Not Disturb. A
//! critical safety alert also queues one `core.task` of [`OWNER_ALERT_CALL_KIND`] holding the alert
//! ID only, for T484's automated call. [`ALERT_PUSH_KIND`] runs every minute: it emails each alert
//! that pushes now and records its push (T487's safe-mode clock), then the 9 a.m. or 3 p.m. push
//! when one is due. [`HOURLY_CHECKS_KIND`] runs `check_webhook_silence` (SEC-051) and
//! [`check_email_use`] (OPS-004). With no sender (no provider or no alert address, dev and Ci only)
//! both still run their checks and send nothing. The emails hold kind code names, record IDs and
//! times only, never a homeowner detail (PRIV-006); the alert address is never logged.

use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{
    DaytimePush, EMAIL_LIMIT_ALERT, HEALTH_ALERT, PushAlert, Severity, alerts_to_push,
    daytime_push_due, raise_alert_once_a_day, record_daytime_push, record_push,
};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::{
    DailyLimit, EmailError, EmailSent, EmailTo, MailboxKind, PostalAddress, check_webhook_silence,
    emails_sent, next_reset, queue_account_email, send_email, short_name,
};
use crate::templates::Language;

/// The every-minute task that emails due pushes (OPS-029, OPS-032, OPS-035).
pub const ALERT_PUSH_KIND: &str = "messaging.alert_push";
/// The hourly task that runs the webhook-silence and email-use checks (SEC-051, OPS-004).
pub const HOURLY_CHECKS_KIND: &str = "messaging.hourly_checks";
/// OPS-035, D-3880: the task kind of a critical safety alert's automated call (T484 places it).
pub const OWNER_ALERT_CALL_KIND: &str = "calls.owner_alert_call";
/// The email key template of one alert's push email.
pub const ALERT_EMAIL_TEMPLATE: &str = "owner_alert_push";
/// The email key template of one 9 a.m. or 3 p.m. push email.
pub const DAYTIME_EMAIL_TEMPLATE: &str = "owner_daytime_push";
/// The alert address's recipient ID in the email outbox; its address is given at send time only.
pub const ALERT_ADDRESS: Uuid = Uuid::from_u128(0x0a5c_4a7a_0000_4000_8000_0000_0000_0035);

const MINUTE: SignedDuration = SignedDuration::from_mins(1);
const HOUR: SignedDuration = SignedDuration::from_hours(1);
const FAILED: HandlerError = HandlerError("the alert email task failed");

/// OPS-004, D-381: the provider's daily and monthly email limits (T469's settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailLimits {
    daily: u32,
    monthly: u32,
}

impl EmailLimits {
    /// The limits, or `None` when either is 0.
    #[must_use]
    pub fn new(daily: u32, monthly: u32) -> Option<Self> {
        (daily > 0 && monthly > 0).then_some(Self { daily, monthly })
    }

    /// OPS-004, D-381: the share of the plan limit in use, in whole percent, when the day's count
    /// is at 70% of the daily limit or more, or the month's at 80% of the monthly limit or more;
    /// `None` below both (69 of 100 a day is none, 70 is 70).
    #[must_use]
    pub fn alert_percent(self, today: i64, month: i64) -> Option<i64> {
        let day = today * 100 / i64::from(self.daily);
        let month = month * 100 / i64::from(self.monthly);
        (day >= 70 || month >= 80).then_some(day.max(month))
    }
}

/// The alert email sender: the email key, the provider, the footer's postal line, the shared
/// daily limit, the OPS-035 alert address and the email limits (SEC-025, OPS-035).
pub struct AlertEmailSender<E> {
    secret: SecretString,
    provider: E,
    postal: (String, MailboxKind),
    limit: DailyLimit,
    address: SecretString,
    limits: EmailLimits,
}

impl<E: EmailAdapter + Send + Sync> AlertEmailSender<E> {
    /// A sender to `address` through `provider`, keyed with `secret`.
    ///
    /// # Errors
    ///
    /// [`EmailError::PostalAddress`] for a postal line that is not that kind of mailbox.
    pub fn new(
        (secret, provider): (SecretString, E),
        (postal, kind): (&str, MailboxKind),
        limit: DailyLimit,
        (address, limits): (SecretString, EmailLimits),
    ) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        Ok(Self {
            secret,
            provider,
            postal: (postal.trim().to_owned(), kind),
            limit,
            address,
            limits,
        })
    }

    /// Queues and sends one email to the alert address, keyed by `template` and `ids`.
    async fn send(
        &self,
        db: &mut PgConnection,
        (template, ids): (&str, &[Uuid]),
        (subject, lines): (&str, &[String]),
        now: Timestamp,
    ) -> Result<(), EmailError> {
        let postal = (self.postal.0.as_str(), self.postal.1);
        let keys = (&self.secret, postal);
        let (row, _) = queue_account_email(
            db,
            keys,
            (template, ALERT_ADDRESS, ids),
            (subject, lines),
            now,
        )
        .await?;
        let to = EmailTo::Business(self.address.expose_secret());
        match send_email(db, (&self.secret, &self.provider, self.limit), row, to, now).await? {
            EmailSent::Sent(_) | EmailSent::AlreadySent => Ok(()),
            EmailSent::Waiting(_) => Err(EmailError::Provider),
        }
    }
}

/// OPS-004: emails sent this provider day (from midnight UTC) and this UTC month, up to `now`.
async fn email_use(db: &mut PgConnection, now: Timestamp) -> Result<(i64, i64), EmailError> {
    let end = now + SignedDuration::from_secs(1);
    let day = next_reset(now) - SignedDuration::from_hours(24);
    let month = now
        .to_zoned(TimeZone::UTC)
        .first_of_month()
        .and_then(|d| d.start_of_day())
        .map_or(day, |d| d.timestamp());
    let today = emails_sent(db, day, end).await?;
    Ok((today, emails_sent(db, month, end).await?))
}

/// OPS-004, OPS-029, D-381, D-4000: raises the [`EMAIL_LIMIT_ALERT`] (Normal, so it never starts
/// safe mode) when email use at `now` is at 70% of the daily or 80% of the monthly limit, at most
/// once an Arizona day; the push then emails it once. Returns the new alert's ID.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn check_email_use(
    db: &mut PgConnection,
    limits: EmailLimits,
    now: Timestamp,
) -> Result<Option<Uuid>, EmailError> {
    let (today, month) = email_use(db, now).await?;
    if limits.alert_percent(today, month).is_none() {
        return Ok(None);
    }
    raise_alert_once_a_day(db, EMAIL_LIMIT_ALERT, Severity::Normal)
        .await
        .map_err(|_| EmailError::Database)
}

/// OPS-035: the subject and lines of one alert's email: its kind, record ID and what to do;
/// `percent` is the email-limit alert's share in use (D-381). No homeowner detail (PRIV-006).
#[must_use]
pub fn alert_lines(alert: &PushAlert, percent: Option<i64>) -> (String, Vec<String>) {
    let brand = short_name(Language::En);
    let mut lines = vec![format!("Alert: {}", alert.kind)];
    lines.extend(alert.subject_id.map(|id| format!("Record: {id}")));
    if alert.kind == EMAIL_LIMIT_ALERT {
        let p = percent.unwrap_or(70);
        lines.push(format!("{brand} email use is at {p}% of the plan limit."));
        lines.push("Upgrade the email plan in the provider dashboard.".to_owned());
    }
    if alert.kind == HEALTH_ALERT {
        lines.push("The site hasn't answered its health check for 15 minutes.".to_owned());
        lines.push("Try pressing Restart in the host dashboard.".to_owned());
    }
    lines.push("Open the owner portal to see it and mark it done.".to_owned());
    (format!("{brand} alert: {}", alert.kind), lines)
}

/// OPS-032: the subject and lines of one 9 a.m. or 3 p.m. push: each open item by kind and ID.
#[must_use]
pub fn daytime_lines(push: &DaytimePush) -> (String, Vec<String>) {
    let brand = short_name(Language::En);
    let mut lines = vec!["Still open, oldest first:".to_owned()];
    lines.extend(push.alerts.iter().map(|a| match a.subject_id {
        Some(id) => format!("{} (record {id})", a.kind),
        None => a.kind.clone(),
    }));
    lines.extend(
        push.items
            .iter()
            .map(|i| format!("{} (record {})", i.kind, i.subject_id)),
    );
    lines.push(
        "Open the owner portal to mark each one done. Until you do, it stays on this list."
            .to_owned(),
    );
    let count = push.alerts.len() + push.items.len();
    (format!("{brand}: {count} items still open"), lines)
}

/// OPS-029, OPS-035: for each alert that pushes at `now`, queues a critical one's call first, so
/// the call never waits on the email, then emails it and records the push. A failed send does not
/// stop the rest; returns how many were emailed.
///
/// # Errors
///
/// The first [`EmailError`] met, after every alert was tried (the next run tries the failed ones
/// again with the same key).
pub async fn push_alerts<E: EmailAdapter + Send + Sync>(
    db: &mut PgConnection,
    sender: &AlertEmailSender<E>,
    now: Timestamp,
) -> Result<usize, EmailError> {
    let due = alerts_to_push(db, now)
        .await
        .map_err(|_| EmailError::Database)?;
    let (mut sent, mut failed) = (0, None);
    for alert in &due {
        match push_one(db, sender, alert, now).await {
            Ok(()) => sent += 1,
            Err(e) => failed = failed.or(Some(e)),
        }
    }
    failed.map_or(Ok(sent), Err)
}

/// OPS-035, D-3880: queues a critical alert's [`OWNER_ALERT_CALL_KIND`] task (alert ID only, once
/// per alert by its dedupe key), then emails the alert and records its push (T487's clock).
async fn push_one<E: EmailAdapter + Send + Sync>(
    db: &mut PgConnection,
    sender: &AlertEmailSender<E>,
    alert: &PushAlert,
    now: Timestamp,
) -> Result<(), EmailError> {
    if alert.severity == Severity::Critical.as_str() {
        let mut task = NewTask::new(OWNER_ALERT_CALL_KIND, json!({ "alert": alert.id }), now);
        task.dedupe_key = Some(format!("{OWNER_ALERT_CALL_KIND}:{}", alert.id));
        enqueue(db, &task).await.map_err(|_| EmailError::Database)?;
    }
    let percent = if alert.kind == EMAIL_LIMIT_ALERT {
        let (today, month) = email_use(db, now).await?;
        sender.limits.alert_percent(today, month)
    } else {
        None
    };
    let (subject, lines) = alert_lines(alert, percent);
    sender
        .send(
            db,
            (ALERT_EMAIL_TEMPLATE, &[alert.id]),
            (&subject, &lines),
            now,
        )
        .await?;
    record_push(db, alert.id, now)
        .await
        .map_err(|_| EmailError::Database)
}

/// OPS-032, D-277: emails the 9 a.m. or 3 p.m. push when one is due at `now` (none when nothing
/// is open) and records the slot; returns whether one was sent.
///
/// # Errors
///
/// [`EmailError::Database`], or a send error (the slot stays due and the next run tries again).
pub async fn push_daytime<E: EmailAdapter + Send + Sync>(
    db: &mut PgConnection,
    sender: &AlertEmailSender<E>,
    now: Timestamp,
) -> Result<bool, EmailError> {
    let Some(push) = daytime_push_due(db, now)
        .await
        .map_err(|_| EmailError::Database)?
    else {
        return Ok(false);
    };
    let (subject, lines) = daytime_lines(&push);
    let slot = u64::try_from(push.slot.as_second()).unwrap_or_default();
    let ids = [Uuid::from_u64_pair(0, slot)];
    sender
        .send(db, (DAYTIME_EMAIL_TEMPLATE, &ids), (&subject, &lines), now)
        .await?;
    record_daytime_push(db, &push, now)
        .await
        .map_err(|_| EmailError::Database)?;
    Ok(true)
}

/// Queues `kind` one `every` after `now`, once per period number (its dedupe key), so a start and
/// a running task queue one next run (JOB-006).
async fn schedule_every(
    db: &mut PgConnection,
    (kind, every): (&'static str, SignedDuration),
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now + every;
    let mut task = NewTask::new(kind, json!({}), run_at);
    let period = run_at.as_second().div_euclid(every.as_secs());
    task.dedupe_key = Some(format!("{kind}.{period}"));
    enqueue(db, &task).await.map(drop)
}

/// Queues the next [`ALERT_PUSH_KIND`] run, one minute after `now` (the worker's start queues the first).
///
/// # Errors
///
/// The queue's [`TaskError`].
pub async fn schedule_alert_push(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    schedule_every(db, (ALERT_PUSH_KIND, MINUTE), now).await
}

/// Queues the next [`HOURLY_CHECKS_KIND`] run, one hour after `now` (SEC-051).
///
/// # Errors
///
/// The queue's [`TaskError`].
pub async fn schedule_hourly_checks(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    schedule_every(db, (HOURLY_CHECKS_KIND, HOUR), now).await
}

/// OPS-029, OPS-032, OPS-035, SEC-051, D-2207: registers [`ALERT_PUSH_KIND`] and
/// [`HOURLY_CHECKS_KIND`] on `clock`; each queues its next run first. With no `sender` the push
/// sends nothing and the hourly task runs only the webhook-silence check.
pub fn register_alert_emails<E>(
    registry: &mut Registry,
    sender: Option<Arc<AlertEmailSender<E>>>,
    clock: &Arc<dyn Clock>,
) where
    E: EmailAdapter + Send + Sync + 'static,
{
    let (push_sender, push_clock) = (sender.clone(), Arc::clone(clock));
    registry.register(ALERT_PUSH_KIND, move |pool, _task| {
        let (sender, now) = (push_sender.clone(), push_clock.now());
        async move { run_alert_push(&pool, sender.as_deref(), now).await }
    });
    let check_clock = Arc::clone(clock);
    registry.register(HOURLY_CHECKS_KIND, move |pool, _task| {
        let (limits, now) = (sender.as_ref().map(|s| s.limits), check_clock.now());
        async move { run_hourly_checks(&pool, limits, now).await }
    });
}

/// The [`ALERT_PUSH_KIND`] handler: the next run first, then each due alert and the daytime push;
/// a failed alert email never holds back the daytime push.
async fn run_alert_push<E: EmailAdapter + Send + Sync>(
    pool: &PgPool,
    sender: Option<&AlertEmailSender<E>>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_alert_push(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    let Some(sender) = sender else {
        return Ok(());
    };
    let alerts = push_alerts(&mut conn, sender, now).await.map(drop);
    let daytime = push_daytime(&mut conn, sender, now).await.map(drop);
    alerts.and(daytime).map_err(|_| FAILED)
}

/// The [`HOURLY_CHECKS_KIND`] handler: the next run first, then SEC-051's webhook-silence check
/// and, with the email limits, OPS-004's email-use check.
async fn run_hourly_checks(
    pool: &PgPool,
    limits: Option<EmailLimits>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_hourly_checks(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    check_webhook_silence(&mut conn, now)
        .await
        .map_err(|_| FAILED)?;
    if let Some(limits) = limits {
        check_email_use(&mut conn, limits, now)
            .await
            .map_err(|_| FAILED)?;
    }
    Ok(())
}

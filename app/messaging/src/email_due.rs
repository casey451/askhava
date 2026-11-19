//! The queued-email drain: the `messaging.email_due` worker task that sends every queued email no
//! handler sends itself (SEC-025, OPS-038, SEC-018, PRIV-016, BIZ-019, BIZ-051, SEC-024; D-3750,
//! D-3889, D-4196, D-4327).
//!
//! Each run, at the worker clock's now, queues its next run, then leases at most [`BATCH`] due
//! rows, oldest first, in one committed statement (`models::lease_emails`, `FOR UPDATE SKIP
//! LOCKED`, `status = 'sending'` and `claimed_at` set together): `queued` rows past `not_before`
//! and `sending` rows whose lease is older than [`STALE_AFTER`], never a template of
//! [`OWN_RETRY`], and one of [`SELF_SENDING`] only once `send_email` held it (`waiting_for` set).
//! A fresh lease is never taken again, so each row gets one provider call per lease, under its
//! stored key (SEC-018). Each leased row, at the clock's now, in order: a row whose first unsure
//! call began [`GIVE_UP_AFTER`] ago is `dropped` with one [`GIVEN_UP_ALERT`]; a row with no unsure
//! call queued [`EXPIRE_AFTER`] ago is `dropped` with [`EXPIRED_ALERT`] (one while open); a row
//! whose subject or body holds `LINK_MARK` or `CODE_MARK` is `dropped`, with one
//! [`MARKED_ALERT`] unless it is a held sign-in link or email code (expired by now); a business
//! row's address comes from the composition's [`BusinessAddresses`] (`Gone`: `dropped`, it holds
//! that person's sign-in link), a homeowner's from the vault; then `send_email` with the handed
//! provider, key and `DailyLimit` (OPS-038's bulk hold and the quota). A missing homeowner
//! address is `dropped` (D-3889); a 400 or 422 is dropped by `send_email`; any other failure puts
//! the row back with a doubling back-off (`models::back_off_email`). A run stops leasing work after
//! [`RUN_BUDGET`] and gives back the leases it did not reach, inside the 2-minute handler timeout.
//! With no drain handed in, a run does nothing and queues no next run. No transaction is open
//! during the provider call, and no address, body or key is logged (PRIV-016).

use std::future::Future;
use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert, raise_kind_once};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::alert_emails::{ALERT_EMAIL_TEMPLATE, DAYTIME_EMAIL_TEMPLATE};
use crate::coverage_email::COVERAGE_EMAIL_TEMPLATE;
use crate::email::{DailyLimit, EMAIL_ROW, EmailSent, EmailTo, send_email};
use crate::help_requests::HELP_EMAIL_TEMPLATE;
use crate::mailbox_code_mail::{CODE_MARK, MAILBOX_CODE_TEMPLATE};
use crate::models::{self, EmailError, LeasedEmail};
use crate::signin_mail::{LINK_MARK, SIGNIN_LINK_TEMPLATE};

/// The `core.task` kind of the drain; each run queues the next.
pub const EMAIL_DUE_KIND: &str = "messaging.email_due";
/// How often the drain runs.
pub const DUE_EVERY: SignedDuration = SignedDuration::from_mins(1);
/// How long a lease holds before another run may take the row again (as `outbox_sender`'s).
pub const STALE_AFTER: SignedDuration = SignedDuration::from_mins(5);
/// A row whose first unsure provider call began this long ago is never handed over again: inside
/// the provider's 24-hour idempotency window (SEC-018, D-4327).
pub const GIVE_UP_AFTER: SignedDuration = SignedDuration::from_hours(23);
/// A row with no unsure call queued this long ago is dropped: its links and facts are stale.
pub const EXPIRE_AFTER: SignedDuration = SignedDuration::from_hours(72);
/// How long a run keeps starting sends: six 15-second provider timeouts fit, with room, in the
/// worker's 2-minute handler timeout.
pub const RUN_BUDGET: SignedDuration = SignedDuration::from_secs(90);
/// The most rows one run leases.
pub const BATCH: i64 = 6;
/// The High alert raised once per row dropped for holding a credential mark.
pub const MARKED_ALERT: &str = "messaging.email_marked";
/// The High alert raised once per row given up 23 hours after its first unsure call.
pub const GIVEN_UP_ALERT: &str = "messaging.email_given_up";
/// The High alert opened once (while open) for rows dropped 72 hours after queueing.
pub const EXPIRED_ALERT: &str = "messaging.email_expired";

/// Every template whose own handler calls `send_email` (D-3750, D-4196): the sign-in link, the
/// email code, the welcome and phone-stopped emails, the coverage notice, the business event
/// emails (T329) and the owner alert and HELP emails. The drain takes one only once held.
pub const SELF_SENDING: [&str; 17] = [
    SIGNIN_LINK_TEMPLATE,
    MAILBOX_CODE_TEMPLATE,
    "business_welcome",
    "business_phone_stopped",
    COVERAGE_EMAIL_TEMPLATE,
    "business_backup_changed",
    "business_phone_linked",
    "business_sample_card",
    "business_sample_pick",
    "business_address_changed",
    "business_address_returned",
    "business_emergency_test",
    "business_after_hours_off",
    "business_license_change",
    ALERT_EMAIL_TEMPLATE,
    DAYTIME_EMAIL_TEMPLATE,
    HELP_EMAIL_TEMPLATE,
];
/// The self-sending templates never taken, held or not: they go to the owner alert address, not a
/// member's mailbox, and their handlers try again themselves (D-4196).
pub const OWN_RETRY: [&str; 3] = [
    ALERT_EMAIL_TEMPLATE,
    DAYTIME_EMAIL_TEMPLATE,
    HELP_EMAIL_TEMPLATE,
];
/// The templates whose held, marked row is dropped with no alert: the link or code has expired.
const EXPIRED_MARKS: [&str; 2] = [SIGNIN_LINK_TEMPLATE, MAILBOX_CODE_TEMPLATE];
const FAILED: HandlerError = HandlerError("email drain database call failed");

/// A business row's address as the composition answers it (BIZ-051, SEC-024).
pub enum DueAddress {
    /// Send to this address; never logged.
    To(SecretString),
    /// The row's business left or the mailbox is no longer a current member's or backup's.
    Gone,
}

/// The business row the composition resolves: recipient mailbox, template and subject IDs.
#[derive(Debug, Clone, Copy)]
pub struct DueBusiness<'a> {
    /// The recipient ID the row was queued for.
    pub recipient: Uuid,
    /// Its template ID.
    pub template: &'a str,
    /// Its subject IDs (a business notice names its business there).
    pub subjects: &'a [Uuid],
}

/// The composition's answer for a business row's address (D-3750); read outside a transaction.
pub trait BusinessAddresses {
    /// The address `row` goes to, or [`DueAddress::Gone`].
    fn address(
        &self,
        db: &mut PgConnection,
        row: DueBusiness<'_>,
    ) -> impl Future<Output = Result<DueAddress, HandlerError>> + Send;
}

/// What the drain needs: the email key, the provider, the shared daily limit, the vault, the
/// business address resolver and the worker clock. No `Debug`.
pub struct EmailDrain<E, R> {
    secret: SecretString,
    provider: E,
    limit: DailyLimit,
    vault: Arc<Vault>,
    resolver: R,
    clock: Arc<dyn Clock>,
}

impl<E, R> EmailDrain<E, R> {
    /// The drain over `provider` with `secret` (the email key) and `limit` (SEC-025, OPS-038).
    #[must_use]
    pub fn new(
        (secret, provider, limit): (SecretString, E, DailyLimit),
        (vault, resolver): (Arc<Vault>, R),
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            secret,
            provider,
            limit,
            vault,
            resolver,
            clock,
        }
    }
}

/// Registers [`EMAIL_DUE_KIND`] with `drain`, or with none (a run does nothing).
pub fn register_email_due<E, R>(registry: &mut Registry, drain: Option<Arc<EmailDrain<E, R>>>)
where
    E: EmailAdapter + Send + Sync + 'static,
    R: BusinessAddresses + Send + Sync + 'static,
{
    registry.register(EMAIL_DUE_KIND, move |pool, task| {
        run_email_due(pool, drain.clone(), task)
    });
}

/// Queues the [`EMAIL_DUE_KIND`] run due at `run_at`, once per minute.
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_email_due(db: &mut PgConnection, run_at: Timestamp) -> Result<(), TaskError> {
    let mut task = NewTask::new(EMAIL_DUE_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{EMAIL_DUE_KIND}:{}", run_at.as_second() / 60));
    enqueue(db, &task).await.map(|_| ())
}

/// The [`EMAIL_DUE_KIND`] handler (SEC-018, OPS-038, PRIV-016, D-3750, D-4327): with no drain,
/// nothing; else the next run, the lease and each leased row at the clock's now, a failed row
/// logged by ID only and backed off, never aborting the batch. Safe on two workers at once.
///
/// # Errors
///
/// A fixed [`HandlerError`] when the connection, the next run's insert or the lease fails.
pub async fn run_email_due<E, R>(
    pool: PgPool,
    drain: Option<Arc<EmailDrain<E, R>>>,
    _task: Claimed,
) -> Result<(), HandlerError>
where
    E: EmailAdapter + Sync,
    R: BusinessAddresses + Sync,
{
    let Some(drain) = drain else {
        return Ok(());
    };
    let start = drain.clock.now();
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_email_due(&mut conn, start + DUE_EVERY)
        .await
        .map_err(|_| FAILED)?;
    let lists: (Vec<String>, Vec<String>) = (
        OWN_RETRY.map(str::to_owned).to_vec(),
        SELF_SENDING.map(str::to_owned).to_vec(),
    );
    let stale = start - STALE_AFTER;
    let rows = models::lease_emails(
        &mut conn,
        (start, stale, BATCH),
        (&lists.0, &lists.1),
        [LINK_MARK, CODE_MARK],
    )
    .await
    .map_err(|_| FAILED)?;
    for (done, row) in rows.iter().enumerate() {
        let now = drain.clock.now();
        if now.duration_since(start) > RUN_BUDGET {
            let left: Vec<Uuid> = rows[done..].iter().map(|r| r.id).collect();
            models::release_emails(&mut conn, &left, start)
                .await
                .map_err(|_| FAILED)?;
            break;
        }
        if drain_one(&mut conn, &drain, row, now).await.is_err() {
            tracing::warn!(email_id = %row.id, "a leased email failed; backed off");
            let _backed = models::back_off_email(&mut conn, row.id, now).await;
        }
    }
    Ok(())
}

/// One leased row (D-3750, D-3889, D-4196, D-4327): given up, expired, marked, gone, or sent.
async fn drain_one<E, R>(
    db: &mut PgConnection,
    drain: &EmailDrain<E, R>,
    row: &LeasedEmail,
    now: Timestamp,
) -> Result<(), EmailError>
where
    E: EmailAdapter + Sync,
    R: BusinessAddresses + Sync,
{
    if row
        .first_unsure_at
        .is_some_and(|t| now - GIVE_UP_AFTER >= t)
    {
        return drop_with_alert(db, row.id, GIVEN_UP_ALERT).await;
    }
    if row.first_unsure_at.is_none() && now - EXPIRE_AFTER >= row.queued_at {
        if models::drop_email(db, row.id).await? {
            raise_kind_once(db, EXPIRED_ALERT, Severity::High)
                .await
                .map_err(|_| EmailError::Database)?;
        }
        return Ok(());
    }
    if row.marked {
        if EXPIRED_MARKS.contains(&row.template_id.as_str()) {
            return models::drop_email(db, row.id).await.map(drop);
        }
        return drop_with_alert(db, row.id, MARKED_ALERT).await;
    }
    let keys = (&drain.secret, &drain.provider, drain.limit);
    let sent = if row.homeowner {
        send_email(db, keys, row.id, EmailTo::Homeowner(&drain.vault), now).await
    } else {
        let due = DueBusiness {
            recipient: row.recipient,
            template: &row.template_id,
            subjects: &row.subject,
        };
        let address = drain.resolver.address(db, due).await;
        match address.map_err(|_| EmailError::Database)? {
            DueAddress::Gone => return models::drop_email(db, row.id).await.map(drop),
            DueAddress::To(to) => {
                let to = EmailTo::Business(to.expose_secret());
                send_email(db, keys, row.id, to, now).await
            }
        }
    };
    after_send(db, row.id, sent, now).await
}

/// What a send's outcome leaves: a missing address drops the row (D-3889), a refusal was dropped
/// by `send_email`, any other failure backs off (D-4327).
async fn after_send(
    db: &mut PgConnection,
    id: Uuid,
    sent: Result<EmailSent, EmailError>,
    now: Timestamp,
) -> Result<(), EmailError> {
    match sent {
        Ok(_) | Err(EmailError::Refused) => Ok(()),
        Err(EmailError::NoAddress) => models::drop_email(db, id).await.map(drop),
        Err(EmailError::Provider) => models::back_off_email(db, id, now).await,
        Err(other) => Err(other),
    }
}

/// Drops row `id` and, if this call dropped it, raises one High `kind` alert naming the row only.
async fn drop_with_alert(
    db: &mut PgConnection,
    id: Uuid,
    kind: &'static str,
) -> Result<(), EmailError> {
    if models::drop_email(db, id).await? {
        let alert = NewAlert {
            kind,
            severity: Severity::High,
            subject: Some((EMAIL_ROW, id)),
        };
        raise_alert(db, &alert)
            .await
            .map_err(|_| EmailError::Database)?;
    }
    Ok(())
}

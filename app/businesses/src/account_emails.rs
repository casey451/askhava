//! The welcome and phone-stopped emails (CON-020, BIZ-017; T268, D-1715, D-3112, D-3185).
//!
//! [`run_welcome`] handles `businesses.welcome` ([`crate::consent::request_welcome`]'s task): it
//! queues one email to the owner's account address whose body is
//! [`crate::consent::welcome_lines`] (ending with the contact-card link, D-2411) in `messaging`'s
//! shared layout, sends it through the email provider and only then calls
//! [`crate::consent::welcome_sent`], so a job card waits for it (`may_get_cards`). It is an email,
//! never a text, so it goes while the business-texting switch is off (TXT-035). [`run_phone_stopped`]
//! handles `businesses.phone_stopped`: one email to the owner with
//! [`crate::consent::stopped_email_line`] naming only the phone's last 4 digits. The handlers live
//! here, not in `messaging`, which may not call `businesses` (D-3112). The owner's address and the
//! last 4 digits come through [`AccountDirectory`], which the composition implements, since
//! `businesses` may not name `privacy` (as `team.rs`'s [`crate::team::SenderLink`]). Each email is
//! keyed by its template, the owner's mailbox and the business (and the STOP's inbox row), so a
//! retried task stores and sends it once. No address or number is stored or logged here.

use std::future::Future;
use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Claimed, Clock, HandlerError, brand};
use askhava_messaging::api::{
    DailyLimit, EmailSent, EmailTo, MailboxKind, queue_account_email, send_email,
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::consent::{may_get_cards, stopped_email_line, welcome_lines, welcome_sent};
use crate::team::NoticePurpose;

/// The email template ID of the CON-020 welcome.
pub const WELCOME_EMAIL: &str = "business_welcome";
/// The email template ID of the BIZ-017 phone-stopped email.
pub const PHONE_STOPPED_EMAIL: &str = "business_phone_stopped";
/// The handler error while the composition has no account email sender (D-3186): the task fails
/// and is tried again, so the welcome is never marked sent without an email.
pub const NO_ACCOUNT_SENDER: &str = "no business account email sender is configured";
const FAILED: HandlerError = HandlerError("business account email failed");

/// What the composition reads for these emails from the vault and the address directory; neither
/// value is stored or logged here.
pub trait AccountDirectory: Send + Sync + 'static {
    /// The address of sign-in mailbox `mailbox`, if one is on file.
    fn address(
        &self,
        db: &mut PgConnection,
        mailbox: Uuid,
    ) -> impl Future<Output = Result<Option<SecretString>, HandlerError>> + Send;
    /// The last 4 digits of the phone of business `business_inbox.0` that texted inbox row
    /// `business_inbox.1`, if known.
    fn last4(
        &self,
        db: &mut PgConnection,
        business_inbox: (Uuid, Uuid),
    ) -> impl Future<Output = Result<Option<String>, HandlerError>> + Send;
}

/// The account email sender: the email key, the footer's postal line, the provider, the daily
/// limit, the directory and the clock.
pub struct AccountMailer<E, D> {
    /// `ASKHAVA_ID_SECRET`, the email outbox key.
    pub secret: SecretString,
    /// The footer's postal line and its mailbox kind (SEC-028).
    pub postal: (String, MailboxKind),
    /// The email provider.
    pub provider: E,
    /// The OPS-038 daily limit.
    pub limit: DailyLimit,
    /// The owner's address and the stopped phone's last 4 digits.
    pub directory: D,
    /// The worker's clock.
    pub clock: Arc<dyn Clock>,
}

fn id(task: &Claimed, field: &str) -> Result<Uuid, HandlerError> {
    task.payload[field]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(HandlerError("business account email payload is malformed"))
}

/// CON-020, D-312: sends the welcome once and marks it sent only after the provider took it.
///
/// # Errors
/// [`NO_ACCOUNT_SENDER`] without a sender; a fixed sentence for a malformed payload, no owner
/// address, a held email or a failed read, store or send (the queue tries again).
pub async fn run_welcome<E: EmailAdapter + Send + Sync, D: AccountDirectory>(
    pool: PgPool,
    mailer: Option<Arc<AccountMailer<E, D>>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let business = id(&task, "business")?;
    let mailer = mailer.ok_or(HandlerError(NO_ACCOUNT_SENDER))?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    if may_get_cards(&mut db, business).await.map_err(|_| FAILED)? {
        return Ok(());
    }
    let subject = format!(
        "{}: Welcome, you're signed up for job requests",
        brand().short_name()
    );
    let email = (WELCOME_EMAIL, [business], (subject, welcome_lines()));
    send_to_owner(&mut db, &mailer, business, email).await?;
    welcome_sent(&mut db, business, mailer.clock.now())
        .await
        .map(drop)
        .map_err(|_| FAILED)
}

/// BIZ-017, D-311: emails the owner that texts to the phone ending in its last 4 digits are off.
///
/// # Errors
/// As [`run_welcome`], and when the phone's digits are not known.
pub async fn run_phone_stopped<E: EmailAdapter + Send + Sync, D: AccountDirectory>(
    pool: PgPool,
    mailer: Option<Arc<AccountMailer<E, D>>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let (business, inbox) = (id(&task, "business")?, id(&task, "inbox")?);
    let mailer = mailer.ok_or(HandlerError(NO_ACCOUNT_SENDER))?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let last4 = mailer.directory.last4(&mut db, (business, inbox)).await?;
    let last4 = last4
        .filter(|d| d.len() == 4 && d.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(HandlerError(
            "the stopped phone's last 4 digits are not known",
        ))?;
    let subject = format!(
        "{}: Texts to your phone ending {last4} are off",
        brand().short_name()
    );
    let lines = vec![stopped_email_line(&last4)];
    let email = (PHONE_STOPPED_EMAIL, [business, inbox], (subject, lines));
    send_to_owner(&mut db, &mailer, business, email).await
}

/// Queues `email` to the owner's account address and sends it; `Ok` only once the provider took
/// it now or earlier (SEC-018: one row and one provider key per template, mailbox and subjects).
async fn send_to_owner<E: EmailAdapter + Send + Sync, D: AccountDirectory, const N: usize>(
    db: &mut PgConnection,
    mailer: &AccountMailer<E, D>,
    business: Uuid,
    (template, subject_ids, (subject, lines)): (&str, [Uuid; N], (String, Vec<String>)),
) -> Result<(), HandlerError> {
    let owner = crate::backup_address::notice_addresses(db, business, NoticePurpose::Account)
        .await
        .map_err(|_| FAILED)?;
    let mailbox = *owner
        .first()
        .ok_or(HandlerError("the business has no owner"))?;
    let to = mailer.directory.address(db, mailbox).await?;
    let to = to.ok_or(HandlerError("the owner's address is not on file"))?;
    let now = mailer.clock.now();
    let (postal, kind) = (mailer.postal.0.as_str(), mailer.postal.1);
    let ids = (template, mailbox, subject_ids.as_slice());
    let words = (subject.as_str(), lines.as_slice());
    let (row, _) = queue_account_email(db, (&mailer.secret, (postal, kind)), ids, words, now)
        .await
        .map_err(|_| FAILED)?;
    let keys = (&mailer.secret, &mailer.provider, mailer.limit);
    let to = EmailTo::Business(to.expose_secret());
    match send_email(db, keys, row, to, now).await {
        Ok(EmailSent::Sent(_) | EmailSent::AlreadySent) => Ok(()),
        Ok(EmailSent::Waiting(_)) => Err(HandlerError("business account email is held")),
        Err(_) => Err(FAILED),
    }
}

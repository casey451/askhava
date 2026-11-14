//! The business mailbox code email (BIZ-040, SEC-018, SEC-026, PRIV-016; T468, D-3743).
//!
//! [`MailboxCodeSender::send`] queues one business email holding a 6-digit mailbox code (the
//! sign-up email's check, `businesses::api::start_signup_check`) through the shared account-email
//! layout (`business_mailbox_code` in the shared templates, the business footer and the checked
//! postal line) and sends it at once. The code is a credential, so the queued row never holds
//! it: the stored copy carries [`CODE_MARK`] where the code goes, and the code is put in only on
//! the way to the provider. A sent row keeps no body (`send_email`); the key is the account-email
//! key over the template, the mailbox and one ID keyed from the code and the send time (a repeat
//! of one send is one email, SEC-018; a resend is a new one); the code is never logged. Built as
//! `signin_mail.rs`' sender is.

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_core::api::brand;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::api::{
    DailyLimit, EmailError, EmailSent, EmailTo, Language, MailboxKind, PostalAddress,
    queue_account_email, render, send_email,
};

/// The mailbox code email's template ID (en, draft es; D-491).
pub const MAILBOX_CODE_TEMPLATE: &str = "business_mailbox_code";
/// What the stored copy shows in place of the code (SEC-026, PRIV-016).
pub const CODE_MARK: &str = "[email code]";

/// The mailbox code mailer: the idempotency key secret, the email provider, the footer's postal
/// line and the daily limit (BIZ-040, SEC-028, OPS-038). It counts nothing itself: the caller
/// admits every send first under SEC-014's per-IP and per-business limits (T461 does, where the
/// client IP is known), and opens the address it is handed under the account-email purpose.
pub struct MailboxCodeSender<E> {
    secret: SecretString,
    provider: E,
    postal: (String, MailboxKind),
    limit: DailyLimit,
}

impl<E: EmailAdapter + Send + Sync> MailboxCodeSender<E> {
    /// A mailer keyed with `secret`, sending through `provider` under `limit`, with `postal` of
    /// mailbox kind `kind` in the footer.
    ///
    /// # Errors
    ///
    /// [`EmailError::PostalAddress`] for a postal line that is not that kind of mailbox.
    pub fn new(
        secret: SecretString,
        provider: E,
        (postal, kind): (&str, MailboxKind),
        limit: DailyLimit,
    ) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        let postal = (postal.trim().to_owned(), kind);
        Ok(Self {
            secret,
            provider,
            postal,
            limit,
        })
    }

    /// BIZ-040, SEC-018, SEC-026: queues and sends `code` to `address`, the address of
    /// `mailbox`; returns the email row's ID. The stored row holds [`CODE_MARK`], never the code.
    ///
    /// # Errors
    ///
    /// [`EmailError::Template`], [`EmailError::Database`], [`EmailError::Provider`] (also when the
    /// provider asked to wait), or a send error of `send_email`.
    pub async fn send(
        &self,
        db: &mut PgConnection,
        (mailbox, address): (Uuid, &SecretString),
        code: &SecretString,
        now: Timestamp,
    ) -> Result<Uuid, EmailError> {
        let values = [("number", CODE_MARK)];
        let text = render(MAILBOX_CODE_TEMPLATE, Language::En, false, &values)
            .map_err(|_| EmailError::Template)?;
        let subject = format!("{}: Your email code", brand().short_name());
        let postal = (self.postal.0.as_str(), self.postal.1);
        let ids = (
            MAILBOX_CODE_TEMPLATE,
            mailbox,
            &[send_id(&self.secret, code, now)][..],
        );
        let lines = [text.body];
        let (row, _) =
            queue_account_email(db, (&self.secret, postal), ids, (&subject, &lines), now).await?;
        let provider = WithCode {
            inner: &self.provider,
            code,
        };
        let to = EmailTo::Business(address.expose_secret());
        match send_email(db, (&self.secret, &provider, self.limit), row, to, now).await? {
            EmailSent::Sent(_) | EmailSent::AlreadySent => Ok(row),
            EmailSent::Waiting(_) => Err(EmailError::Provider),
        }
    }
}

/// An ID for one send of one code, keyed with `secret` so it cannot be turned back into the code.
fn send_id(secret: &SecretString, code: &SecretString, now: Timestamp) -> Uuid {
    let at = now.as_millisecond().to_string();
    let key = IdempotencyKey::derive(secret, "mailbox.code", &[code.expose_secret(), &at]);
    Uuid::parse_str(&key.as_str()[..32]).unwrap_or_default()
}

/// The provider, with [`CODE_MARK`] replaced by the code on the way out only.
struct WithCode<'a, E> {
    inner: &'a E,
    code: &'a SecretString,
}

impl<E: EmailAdapter + Send + Sync> EmailAdapter for WithCode<'_, E> {
    async fn send_email(
        &self,
        idempotency: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let code = self.code.expose_secret();
        let (html, text) = (
            email.html.replace(CODE_MARK, code),
            email.text.replace(CODE_MARK, code),
        );
        let full = Email {
            from: email.from,
            to: email.to,
            reply_to: email.reply_to,
            subject: email.subject,
            html: &html,
            text: &text,
        };
        self.inner.send_email(idempotency, &full).await
    }

    async fn remove_suppression(
        &self,
        idempotency: &IdempotencyKey,
        address: &str,
    ) -> Result<(), AdapterError> {
        self.inner.remove_suppression(idempotency, address).await
    }
}

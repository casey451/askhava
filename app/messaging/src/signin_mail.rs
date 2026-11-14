//! The business sign-in link email (SEC-004, SEC-025, SEC-026; T280, D-1724, D-1844).
//!
//! [`SignInLinkSender::send`] queues one business email to a member's sign-in mailbox through
//! the shared account-email layout (`business_signin_link` in the shared templates, the business
//! footer and the checked postal line) and sends it at once. The link is a credential that
//! `portals` stores only as a SHA256 (SEC-026), so the queued row never holds it: the stored
//! copy carries [`LINK_MARK`] where the link goes, and the link is put in only on the way to the
//! provider. A sent row keeps no body (`send_email`), the key is the account-email key over the
//! template, the mailbox and one ID keyed from the link (so a repeat of one link is one email,
//! SEC-018), and the link is never logged.

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

/// The sign-in link email's template ID (en, draft es; D-491).
pub const SIGNIN_LINK_TEMPLATE: &str = "business_signin_link";
/// What the stored copy shows in place of the link (SEC-026).
pub const LINK_MARK: &str = "[sign-in link]";

/// The sign-in link mailer: the idempotency key secret, the email provider, the footer's postal
/// line and the daily limit (SEC-025, SEC-028, OPS-038).
pub struct SignInLinkSender<E> {
    secret: SecretString,
    provider: E,
    postal: (String, MailboxKind),
    limit: DailyLimit,
}

impl<E: EmailAdapter + Send + Sync> SignInLinkSender<E> {
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

    /// SEC-004, SEC-025, SEC-026: queues and sends `link` to `address`, the sign-in address of
    /// `mailbox`; returns the email row's ID. The stored row holds [`LINK_MARK`], never the link.
    ///
    /// # Errors
    ///
    /// [`EmailError::Template`], [`EmailError::Database`], [`EmailError::Provider`] (also when the
    /// provider asked to wait), or a send error of `send_email`.
    pub async fn send(
        &self,
        db: &mut PgConnection,
        (mailbox, address): (Uuid, &SecretString),
        link: &SecretString,
        now: Timestamp,
    ) -> Result<Uuid, EmailError> {
        let values = [("link", LINK_MARK)];
        let text = render(SIGNIN_LINK_TEMPLATE, Language::En, false, &values)
            .map_err(|_| EmailError::Template)?;
        let subject = format!("{}: Your sign-in link", brand().short_name());
        let postal = (self.postal.0.as_str(), self.postal.1);
        let ids = (
            SIGNIN_LINK_TEMPLATE,
            mailbox,
            &[link_id(&self.secret, link)][..],
        );
        let lines = [text.body];
        let (row, _) =
            queue_account_email(db, (&self.secret, postal), ids, (&subject, &lines), now).await?;
        let provider = WithLink {
            inner: &self.provider,
            link,
        };
        let to = EmailTo::Business(address.expose_secret());
        match send_email(db, (&self.secret, &provider, self.limit), row, to, now).await? {
            EmailSent::Sent(_) | EmailSent::AlreadySent => Ok(row),
            EmailSent::Waiting(_) => Err(EmailError::Provider),
        }
    }
}

/// An ID for one link, keyed with `secret` so it cannot be turned back into the link.
fn link_id(secret: &SecretString, link: &SecretString) -> Uuid {
    let key = IdempotencyKey::derive(secret, "signin.link", &[link.expose_secret()]);
    Uuid::parse_str(&key.as_str()[..32]).unwrap_or_default()
}

/// The provider, with [`LINK_MARK`] replaced by the link on the way out only.
struct WithLink<'a, E> {
    inner: &'a E,
    link: &'a SecretString,
}

impl<E: EmailAdapter + Send + Sync> EmailAdapter for WithLink<'_, E> {
    async fn send_email(
        &self,
        idempotency: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let link = self.link.expose_secret();
        let (html, text) = (
            email.html.replace(LINK_MARK, link),
            email.text.replace(LINK_MARK, link),
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

#[cfg(test)]
mod tests {
    use askhava_core::api::MIGRATOR;
    use sqlx::PgPool;

    use super::*;

    /// A provider that takes nothing, so the row stays unsent with its stored copy.
    struct Refuses;

    impl EmailAdapter for Refuses {
        async fn send_email(
            &self,
            _: &IdempotencyKey,
            _: &Email<'_>,
        ) -> Result<String, AdapterError> {
            Err(AdapterError::NotSent)
        }
        async fn remove_suppression(
            &self,
            _: &IdempotencyKey,
            _: &str,
        ) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    /// TD-SEC-40 (SEC-026, SEC-018): a link queues one row, and the row never holds the link.
    #[sqlx::test(migrator = "MIGRATOR")]
    async fn td_sec_40_one_row_per_link_and_never_the_link(pool: PgPool) {
        let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
        let postal = ("PO Box 1234, Faketown, AZ 00000", MailboxKind::PoBox);
        let limit = DailyLimit::new(100, 90).expect("limit");
        let sender = SignInLinkSender::new(secret, Refuses, postal, limit).expect("sender");
        let token = "ab".repeat(32);
        let link = SecretString::from(format!("https://example.test/b/sign-in/{token}"));
        let address = SecretString::from("crew@x.example".to_owned());
        let mut db = pool.acquire().await.expect("db");
        for _ in 0..2 {
            let to = (Uuid::from_u128(7), &address);
            let sent = sender.send(&mut db, to, &link, Timestamp::now()).await;
            assert!(sent.is_err(), "the provider refused");
        }
        let rows = sqlx::query_scalar!(
            "SELECT concat(subject, html, plain) AS \"body!\" FROM messaging.email_outbox \
             WHERE template_id = $1",
            SIGNIN_LINK_TEMPLATE
        )
        .fetch_all(&mut *db)
        .await
        .expect("rows");
        assert_eq!(rows.len(), 1, "one row per link");
        assert!(
            rows.iter()
                .all(|b| b.contains(LINK_MARK) && !b.contains(&token))
        );
    }
}

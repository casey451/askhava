//! The two emails an opted-out homeowner may get instead of a text (CON-030, CON-009, CON-016,
//! CON-012, T360, D-2629, D-3094).
//!
//! [`queue_optout_confirmation_email`]: while texting is off or suspended an opt-out is recorded
//! at once and confirmed by email when one is on file, never by a later text (CON-030). It queues
//! T094's `Notice::OptedOut` (the CON-009 text in her language) through `queue_email`, with the
//! opt-out event as its subject, so one event gives one email and a repeat none.
//!
//! [`queue_optout_wrote_again`]: a listed homeowner who writes without START (`Outcome::Suppressed`)
//! is emailed how to text again and that her open requests go on on the quote page and by email
//! (CON-016, CON-012), at most once per Arizona day (key over template, contact and day); with no
//! email on file one `ops` audit row `messaging.optout_wrote_again` on the inbox row, IDs only, is
//! what Casey's weekly digest counts (T204), once per message. That email is not one of
//! `queue_email`'s notices, so it renders itself in `templates/email_layout.html` here and is
//! stored through `models::insert_email`, as `checkin_email` does; nothing is in the text registry.
//! Her email is read under `messaging`'s send grant only to learn whether one exists, outside any
//! transaction (the vault's rule); `send_email` reads it again at send time, so no address, phone
//! or body sits in a row, key, audit row or log line. The route that calls both is T357's.

use askama::Template;
use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::arizona_time;
use askhava_ops::api::{Actor, AuditEntry, Change, audit_once};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, VaultError, read_contact};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{
    EMAIL_ACTION, EmailClass, MailboxKind, NewEmail, Notice, OnDomain, PostalAddress, Recipient,
    Rendered, audience_kind, footer_lines, legal_name, queue_email, short_name,
};
use crate::models::{self, EmailError};
use crate::templates::{Audience, Language};

/// The CON-030 confirmation email's template ID; with the contact and opt-out event it keys it.
pub const OPTOUT_CONFIRMATION_EMAIL: &str = "optout_confirmation_email";
/// The CON-016 email's template ID; with the contact and Arizona day it keys it.
pub const OPTOUT_WROTE_AGAIN_EMAIL: &str = "optout_wrote_again_email";
/// The audit action on the inbox row when a listed homeowner with no email wrote (CON-016, T204).
pub const WROTE_AGAIN_ACTION: &str = "messaging.optout_wrote_again";
/// The homeowner's "Manage email notices" link (SEC-028).
const MANAGE: OnDomain = OnDomain("/h");

/// The email key secret, Ask Hava LLC's checked postal line for the footer (SEC-028) and the vault
/// her email on file is checked in (PRIV-004).
pub struct OptOutEmailSender {
    secret: SecretString,
    postal: PostalAddress,
    line: String,
    vault: Vault,
}

impl OptOutEmailSender {
    /// Checks `postal` as `kind` of mailbox before keeping it (SEC-028).
    ///
    /// # Errors
    /// [`EmailError::PostalAddress`] for a line that is not that kind of mailbox.
    pub fn new(
        secret: SecretString,
        (postal, kind): (&str, MailboxKind),
        vault: Vault,
    ) -> Result<Self, EmailError> {
        Ok(Self {
            secret,
            postal: PostalAddress::new(postal, kind)?,
            line: postal.trim().to_owned(),
            vault,
        })
    }
}

/// What [`queue_optout_wrote_again`] did (CON-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WroteAgain {
    /// She has an email on file; `true` when this call stored today's email, `false` when today's
    /// was stored before.
    Emailed(bool),
    /// No email on file; `true` when this call wrote the message's audit row, `false` when it was
    /// written before.
    Listed(bool),
}

/// D-3165: whether homeowner `contact` has an email on file, read under the send grant outside
/// any transaction (the address is dropped at once). A contact that can never be read (no such
/// contact, a sealed value that will not open) counts as none, so she is listed rather than lost,
/// as `outbox_sender::homeowner_phone` does (D-3149); only a failure that may pass (the database,
/// the audit write) is [`EmailError::NoAddress`], which the caller retries.
async fn email_on_file(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<bool, EmailError> {
    let grant = (Reader::Messaging, Purpose::Send);
    match read_contact(db, vault, grant, contact, &[Field::Email]).await {
        Ok(found) => Ok(found.email.is_some()),
        Err(VaultError::NotFound | VaultError::Sealed) => Ok(false),
        Err(_) => Err(EmailError::NoAddress),
    }
}

/// CON-030, CON-009, SEC-018: queues the CON-009 confirmation by email for homeowner `contact`
/// in `language`, under opt-out `event` as its subject; `true` when stored now, `false` when she
/// has no email on file or her contact can never be read (nothing stored), or this event's email
/// was stored before. Never a text.
///
/// # Errors
/// [`EmailError::NoAddress`] when the vault read fails in a way that may pass (the caller
/// retries), [`EmailError::Template`] or a database error from the outbox insert.
pub async fn queue_optout_confirmation_email(
    db: &mut PgConnection,
    sender: &OptOutEmailSender,
    (contact, language): (Uuid, Language),
    event: Uuid,
    now: Timestamp,
) -> Result<bool, EmailError> {
    if !email_on_file(db, &sender.vault, contact).await? {
        return Ok(false);
    }
    let manage = MANAGE.to_string();
    let to = Recipient {
        id: contact,
        audience: Audience::Homeowner,
        language,
        manage_url: &manage,
    };
    let email = NewEmail {
        template_id: OPTOUT_CONFIRMATION_EMAIL,
        subject: std::slice::from_ref(&event),
        class: EmailClass::Notice,
        notice: Notice::OptedOut,
    };
    let keys = (&sender.secret, &sender.postal);
    let (_, new) = queue_email(db, keys, &to, &email, now).await?;
    Ok(new)
}

/// `templates/email_layout.html` with its one message.
#[derive(Template)]
#[template(path = "email_layout.html")]
struct WroteAgainView<'a> {
    lang: &'a str,
    subject: &'a str,
    message: &'a str,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    legal: &'a str,
    postal: &'a str,
}

/// CON-016, CON-012: the subject and message in `language` (Spanish is a draft, TXT-003; the
/// caller has already applied TXT-039, so Spanish comes here only when allowed): she is
/// opted out, START texts again, and her open requests go on on the quote page and by email.
fn wrote_again_words(language: Language) -> (String, String) {
    let short = short_name(language);
    match language {
        Language::En => (
            format!("{short}: You opted out of texts"),
            format!(
                "You opted out of texts from {short}, so we won't text you back. To text us again, send START to the number you texted. Any open request goes on: you'll see its quotes on your quote page and get updates by email."
            ),
        ),
        Language::Es => (
            format!("{short}: Usted canceló los mensajes de texto"),
            format!(
                "Usted canceló los mensajes de texto de {short}, así que no le responderemos por texto. Para volver a escribirnos por texto, envíe START al número al que escribió. Sus solicitudes abiertas siguen: verá sus cotizaciones en su página de cotizaciones y recibirá avisos por correo."
            ),
        ),
    }
}

/// CON-016, SEC-028: the wrote-again email in `language` inside the layout with the homeowner
/// footer; the plain-text part carries the same lines.
///
/// # Errors
/// [`EmailError::Template`] if the layout fails to render.
pub fn render_wrote_again(
    sender: &OptOutEmailSender,
    language: Language,
) -> Result<Rendered, EmailError> {
    let (subject, message) = wrote_again_words(language);
    let (why, manage_label) = footer_lines(Audience::Homeowner, language);
    let (manage, legal) = (MANAGE.to_string(), legal_name(language));
    let view = WroteAgainView {
        lang: if language == Language::Es { "es" } else { "en" },
        subject: &subject,
        message: &message,
        why: &why,
        manage_url: &manage,
        manage_label,
        legal,
        postal: &sender.line,
    };
    let html = view.render().map_err(|_| EmailError::Template)?;
    let plain = format!(
        "{message}\n\n--\n{why}\n{manage_label}: {manage}\n{legal}, {}\n",
        sender.line
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// CON-016, CON-012, SEC-018, JOB-006: listed homeowner `contact` wrote inbox message `inbox`
/// without START at `now`. `language` must be the caller's TXT-039-resolved choice, as
/// `email::Recipient::language` is: while Spanish conversations are off every homeowner email but
/// the CON-009 confirmation is English, so the caller passes [`Language::Es`] only while the
/// setting is on and she chose Spanish; this function does not check the setting. With an email
/// on file it stores the wrote-again email at most once per Arizona day (key over template,
/// contact and day; the first message's inbox ID is its subject); with none, or a contact that can
/// never be read (D-3165), it writes one `ops` audit row [`WROTE_AGAIN_ACTION`] on the inbox row,
/// once per message, for the weekly digest. No call-back either way (D-226).
///
/// # Errors
/// [`EmailError::NoAddress`] when the vault read fails in a way that may pass (the caller
/// retries), [`EmailError::Template`] or [`EmailError::Database`].
pub async fn queue_optout_wrote_again(
    db: &mut PgConnection,
    sender: &OptOutEmailSender,
    (contact, language): (Uuid, Language),
    inbox: Uuid,
    now: Timestamp,
) -> Result<WroteAgain, EmailError> {
    if !email_on_file(db, &sender.vault, contact).await? {
        let entry = AuditEntry {
            actor: Actor::System,
            action: WROTE_AGAIN_ACTION,
            subject_kind: "messaging.inbox",
            subject_id: inbox,
            change: Change::Action,
        };
        let new = audit_once(db, &entry)
            .await
            .map_err(|_| EmailError::Database)?;
        return Ok(WroteAgain::Listed(new));
    }
    let rendered = render_wrote_again(sender, language)?;
    let day = arizona_time(now).date().to_string();
    let parts = [OPTOUT_WROTE_AGAIN_EMAIL, &contact.to_string(), &day];
    let key = IdempotencyKey::derive(&sender.secret, EMAIL_ACTION, &parts);
    let ids = (
        OPTOUT_WROTE_AGAIN_EMAIL,
        contact,
        std::slice::from_ref(&inbox),
    );
    let kind = audience_kind(Audience::Homeowner);
    let (_, new) =
        models::insert_email(db, (key.as_str(), ids, kind, false), &rendered, now).await?;
    Ok(WroteAgain::Emailed(new))
}

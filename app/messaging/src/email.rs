//! The email pipeline every sender uses (SEC-028, OPS-038, A11Y-034, SEC-018, SEC-037, D-491).
//!
//! [`queue_email`] renders one email inside `templates/email_layout.html` (the SEC-028 footer: the
//! why-line for its audience, "Manage email notices" and the LLC postal address; `lang` on the
//! root) and stores it once in `messaging.email_outbox` under a key derived from its template,
//! recipient and subject IDs. [`send_email`] sends a due row once: it holds a `Bulk` email (open
//! cards, missed cards, the weekly digest) while more than the limit's share of today's emails
//! went (OPS-038), claims the row, reads a homeowner's address from the vault (a business's comes
//! from the caller), calls the provider outside any transaction and marks it sent with no body
//! left. A provider refusal for a sending limit (HTTP 429) is never dropped: the row waits for the
//! reset and the notice log shows it (`email_log`, BIZ-045). A module beside or below `messaging`
//! never calls the provider itself: it asks `messaging` through a `core.task` event (D-491).
//! Verified email-provider webhooks are stored once in event-time order, a complaint raises an
//! alert, and 24 hours of sends with no webhook raises another (SEC-050, SEC-051). The
//! homeowner's update channel (TXT-018) is read and written here too.

use askama::Template;
use askhava_adapters::api::{
    AdapterError, Email, EmailAdapter, IdempotencyKey, VerifiedWebhook, WebhookVendor,
};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, read_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, EmailError, EmailLog};
use crate::rules::CONFIRMATION_TEMPLATE;
use crate::templates::{Audience, Language, Registry};

/// The From of every email (deliverability checklist).
pub const FROM: &str = "\"Ask Hava\" <jobs@notify.askhava.com>";
/// The monitored help address every email replies to; never a no-reply address.
pub const REPLY_TO: &str = "help@askhava.com";
/// The action every email's idempotency key is derived under (SEC-018).
pub const EMAIL_ACTION: &str = "email.send";
/// How long a provider limit refusal waits while today's count is still under the daily limit.
const PROVIDER_PAUSE: SignedDuration = SignedDuration::from_mins(1);

/// What kind of mailbox Ask Hava LLC's postal address is (SEC-028). There is no home-address kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    /// A post office box ("PO Box" in the line).
    PoBox,
    /// A registered private mailbox ("PMB" or "#" in the line).
    PrivateMailbox,
}

/// Ask Hava LLC's checked postal address for the footer: never empty, and a P.O. box or private
/// mailbox, never a home address (SEC-028, TD-BIZ-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostalAddress(String);

impl PostalAddress {
    /// Checks `line` against `kind`: a `PoBox` line names a box, a `PrivateMailbox` line a PMB or
    /// `#` unit (SEC-028).
    ///
    /// # Errors
    ///
    /// [`EmailError::PostalAddress`] for an empty line or one that is not that kind of mailbox.
    pub fn new(line: &str, kind: MailboxKind) -> Result<Self, EmailError> {
        let folded = line.trim().to_ascii_lowercase().replace('.', "");
        let fits = match kind {
            MailboxKind::PoBox => folded.contains("po box"),
            MailboxKind::PrivateMailbox => folded.contains("pmb") || folded.contains('#'),
        };
        if fits {
            Ok(Self(line.trim().to_owned()))
        } else {
            Err(EmailError::PostalAddress)
        }
    }
}

/// Whether OPS-038 may hold an email above the daily limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailClass {
    /// Job cards, picks, reminders, codes, receipts, card-fix, release, help: always go.
    Notice,
    /// Open-cards emails (CARD-022), the monthly missed-cards email (BIZ-044) and the weekly digest
    /// (OPS-030): wait for the reset above the limit's share (OPS-038).
    Bulk,
}

/// What an email says; each has an English and a draft Spanish line (TXT-014, CON-028, TD-CON-28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice<'a> {
    /// An important text that failed 3 retries, its body as sent (TXT-014).
    TextFailed(&'a str),
    /// The confirmation of an opt-out recorded through any channel, never by text (CON-035).
    OptedOut,
    /// After an opt-out from a number with a live job, request `n` is still open (CON-028).
    StillOpen(i64),
}

/// Who an email goes to: the opaque ID (a vault contact or a business address), the audience the
/// why-line names, the person's language and their portal link (SEC-028, BIZ-018).
#[derive(Debug, Clone, Copy)]
pub struct Recipient<'a> {
    /// The vault contact or business address ID, never the address.
    pub id: Uuid,
    /// Homeowner or business.
    pub audience: Audience,
    /// The language the caller chose for this person (TXT-039, BIZ-018).
    pub language: Language,
    /// The person's portal page, the "Manage email notices" link.
    pub manage_url: &'a str,
}

/// One email to queue: its template ID, the record IDs it is about, its class and what it says.
#[derive(Debug, Clone, Copy)]
pub struct NewEmail<'a> {
    /// The email's template ID; with the recipient and subject IDs it makes the key.
    pub template_id: &'a str,
    /// The record IDs the email identifies.
    pub subject: &'a [Uuid],
    /// Whether OPS-038 may hold it.
    pub class: EmailClass,
    /// What it says.
    pub notice: Notice<'a>,
}

/// A rendered email: subject, HTML and plain-text parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The subject line, stating the fact.
    pub subject: String,
    /// The HTML part, `email_layout.html` with `lang` set (A11Y-034).
    pub html: String,
    /// The plain-text part.
    pub plain: String,
}

/// `templates/email_layout.html`: every email template extends it (SEC-028, D-491).
#[derive(Template)]
#[template(path = "email_layout.html")]
struct Layout<'a> {
    lang: &'a str,
    subject: &'a str,
    message: &'a str,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    postal: &'a str,
}

/// The SEC-028 why-line and the "Manage email notices" label for an audience and language.
fn footer_lines(audience: Audience, language: Language) -> (&'static str, &'static str) {
    match (language, audience) {
        (Language::En, Audience::Business) => (
            "You get these emails because your business signed up for job requests from Ask Hava.",
            "Manage email notices",
        ),
        (Language::En, Audience::Homeowner) => (
            "You get these emails because you asked Ask Hava for quotes.",
            "Manage email notices",
        ),
        (Language::Es, Audience::Business) => (
            "Recibe estos correos porque su negocio se inscribió para recibir solicitudes de trabajo de Ask Hava.",
            "Administrar avisos por correo",
        ),
        (Language::Es, Audience::Homeowner) => (
            "Recibe estos correos porque pidió cotizaciones a Ask Hava.",
            "Administrar avisos por correo",
        ),
    }
}

/// The subject and message of `notice` for `to` (TXT-014, CON-028); the opt-out confirmation is
/// the CON-009 text itself, `opt_out_confirmation` in the person's language (CON-035).
fn notice_lines(notice: Notice<'_>, to: &Recipient<'_>) -> Result<(String, String), EmailError> {
    let es = to.language == Language::Es;
    Ok(match notice {
        Notice::TextFailed(text) => text_failed_lines(text, to),
        Notice::OptedOut => {
            let registry = Registry::embedded().map_err(|_| EmailError::Template)?;
            let text = registry
                .render(CONFIRMATION_TEMPLATE, to.language, true, &[])
                .map_err(|_| EmailError::Template)?;
            let subject = if es {
                "Ask Hava: Ya no recibirá mensajes de texto"
            } else {
                "Ask Hava: You're unsubscribed from texts"
            };
            (subject.to_owned(), text.body)
        }
        Notice::StillOpen(n) if es => (
            format!("Ask Hava: La solicitud {n} sigue abierta"),
            format!("Su solicitud {n} sigue abierta. Ciérrela en su página de cotizaciones."),
        ),
        Notice::StillOpen(n) => (
            format!("Ask Hava: Request {n} is still open"),
            format!("Your request {n} is still open. Close it on your quote page."),
        ),
    })
}

/// The subject and message of a failed important text sent by email, pointing to where the
/// portal shows it: the homeowner's quote page or the business portal (TXT-014).
fn text_failed_lines(text: &str, to: &Recipient<'_>) -> (String, String) {
    let page = match (to.language, to.audience) {
        (Language::En, Audience::Homeowner) => "You can also see it on your quote page.",
        (Language::En, Audience::Business) => "You can also see it in your Ask Hava portal.",
        (Language::Es, Audience::Homeowner) => "También puede verlo en su página de cotizaciones.",
        (Language::Es, Audience::Business) => "También puede verlo en su portal de Ask Hava.",
    };
    match to.language {
        Language::En => (
            "Ask Hava: A text to you didn't go through".to_owned(),
            format!("This text to you didn't go through, so here it is by email: {text} {page}"),
        ),
        Language::Es => (
            "Ask Hava: No le llegó un mensaje de texto".to_owned(),
            format!(
                "Este mensaje de texto no le llegó, así que se lo enviamos por correo: {text} {page}"
            ),
        ),
    }
}

/// Renders `notice` for `to` inside the layout, with the SEC-028 footer and `postal` (SEC-028,
/// A11Y-034); the plain-text part carries the same lines.
///
/// # Errors
///
/// [`EmailError::Template`] if the layout fails to render.
pub fn render(
    postal: &PostalAddress,
    to: &Recipient<'_>,
    notice: Notice<'_>,
) -> Result<Rendered, EmailError> {
    let (subject, message) = notice_lines(notice, to)?;
    let (why, manage_label) = footer_lines(to.audience, to.language);
    let lang = match to.language {
        Language::En => "en",
        Language::Es => "es",
    };
    let layout = Layout {
        lang,
        subject: &subject,
        message: &message,
        why,
        manage_url: to.manage_url,
        manage_label,
        postal: &postal.0,
    };
    let html = layout.render().map_err(|_| EmailError::Template)?;
    let plain = format!(
        "{message}\n\n--\n{why}\n{manage_label}: {}\nAsk Hava LLC, {}\n",
        to.manage_url, postal.0
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// Renders and queues `email` to `to` once at `now` (SEC-018, SEC-028); returns the row ID and
/// whether this call created it.
///
/// # Errors
///
/// [`EmailError::Template`] or [`EmailError::Database`].
pub async fn queue_email(
    db: &mut PgConnection,
    (secret, postal): (&SecretString, &PostalAddress),
    to: &Recipient<'_>,
    email: &NewEmail<'_>,
    now: Timestamp,
) -> Result<(Uuid, bool), EmailError> {
    let rendered = render(postal, to, email.notice)?;
    let ids = (email.template_id, to.id, email.subject);
    let key = email_key(secret, ids);
    let kind = audience_kind(to.audience);
    let bulk = email.class == EmailClass::Bulk;
    models::insert_email(db, (key.as_str(), ids, kind, bulk), &rendered, now).await
}

/// The key of an email: [`EMAIL_ACTION`] over its template, recipient and sorted subject IDs, no
/// personal data (SEC-018).
fn email_key(
    secret: &SecretString,
    (template, to, subject): (&str, Uuid, &[Uuid]),
) -> IdempotencyKey {
    let mut sorted = subject.to_vec();
    sorted.sort_unstable();
    let ids: Vec<String> = [template.to_owned(), to.to_string()]
        .into_iter()
        .chain(sorted.iter().map(Uuid::to_string))
        .collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    IdempotencyKey::derive(secret, EMAIL_ACTION, &ids)
}

/// The stored recipient kind of an audience.
pub(crate) fn audience_kind(audience: Audience) -> &'static str {
    match audience {
        Audience::Homeowner => "homeowner",
        Audience::Business => "business",
    }
}

/// The provider's daily sending limit and the share above which `Bulk` emails wait (OPS-038).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyLimit {
    per_day: u32,
    hold_above_percent: u8,
}

impl DailyLimit {
    /// A limit of `per_day` emails (100 on the free plan) holding bulk email above
    /// `hold_above_percent` of it (90, a setting, OPS-038).
    ///
    /// # Errors
    ///
    /// [`EmailError::Limit`] for a zero limit or a share outside 1 to 100.
    pub fn new(per_day: u32, hold_above_percent: u8) -> Result<Self, EmailError> {
        if per_day == 0 || !(1..=100).contains(&hold_above_percent) {
            return Err(EmailError::Limit);
        }
        Ok(Self {
            per_day,
            hold_above_percent,
        })
    }

    /// Whether, with `sent_today` sent, a bulk email waits: more than the share went (OPS-038).
    #[must_use]
    pub fn holds_bulk(self, sent_today: i64) -> bool {
        sent_today * 100 > i64::from(self.per_day) * i64::from(self.hold_above_percent)
    }
}

/// When the provider's daily count resets: the next midnight UTC after `now` (OPS-038, D-1379).
#[must_use]
pub fn next_reset(now: Timestamp) -> Timestamp {
    day_start(now) + SignedDuration::from_hours(24)
}

/// The midnight UTC that began `now`'s sending day.
fn day_start(now: Timestamp) -> Timestamp {
    let secs = now.as_second().div_euclid(86_400) * 86_400;
    Timestamp::from_second(secs).unwrap_or(now)
}

/// Where an email goes, resolved at send time; it must name the kind it was queued for.
#[derive(Clone, Copy)]
pub enum EmailTo<'a> {
    /// A homeowner: her address is read from the vault under `messaging`'s send grant.
    Homeowner(&'a Vault),
    /// A business sign-in address, given by the caller from `businesses`.
    Business(&'a str),
}

/// What [`send_email`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailSent {
    /// The provider took it; its email ID.
    Sent(String),
    /// It waits until then: the daily-limit share (OPS-038), a provider limit refusal, or an
    /// earlier wait not yet over.
    Waiting(Timestamp),
    /// It was already sent; the provider is not called again.
    AlreadySent,
}

/// Sends email row `id` to `to` at `now` once, under the key re-derived from its stored IDs
/// (SEC-018, OPS-038, SEC-037). No transaction is open during the provider call.
///
/// # Errors
///
/// [`EmailError::NotFound`], [`EmailError::RecipientKind`], [`EmailError::NoAddress`],
/// [`EmailError::Provider`] (the row stays `sending`; a resend carries the same key) or
/// [`EmailError::Database`].
pub async fn send_email(
    db: &mut PgConnection,
    (secret, provider, limit): (&SecretString, &impl EmailAdapter, DailyLimit),
    id: Uuid,
    to: EmailTo<'_>,
    now: Timestamp,
) -> Result<EmailSent, EmailError> {
    let sent_today = models::emails_sent_between(db, (day_start(now), now)).await?;
    let hold = limit.holds_bulk(sent_today);
    let Some(claim) = models::claim_email(db, id, (send_kind(to), hold), now).await? else {
        return waiting_or_sent(db, id, (limit, sent_today), now).await;
    };
    let address = email_address(db, to, claim.recipient).await?;
    let key = email_key(
        secret,
        (&claim.template_id, claim.recipient, &claim.subject),
    );
    let email = Email {
        from: FROM,
        to: address.expose_secret(),
        reply_to: REPLY_TO,
        subject: &claim.rendered.subject,
        html: &claim.rendered.html,
        text: &claim.rendered.plain,
    };
    match provider.send_email(&key, &email).await {
        Ok(provider_id) => {
            models::mark_email_sent(db, id, &provider_id, now).await?;
            Ok(EmailSent::Sent(provider_id))
        }
        Err(AdapterError::Vendor { status: 429, .. }) => {
            let full = sent_today >= i64::from(limit.per_day);
            let until = if full {
                next_reset(now)
            } else {
                now + PROVIDER_PAUSE
            };
            models::hold_email(db, id, until, "provider_limit").await?;
            Ok(EmailSent::Waiting(until))
        }
        Err(_) => Err(EmailError::Provider),
    }
}

/// The stored kind a send destination must match.
fn send_kind(to: EmailTo<'_>) -> &'static str {
    match to {
        EmailTo::Homeowner(_) => "homeowner",
        EmailTo::Business(_) => "business",
    }
}

/// Why row `id` was not claimed: sent already, still waiting, or a bulk email OPS-038 now holds
/// until the reset.
async fn waiting_or_sent(
    db: &mut PgConnection,
    id: Uuid,
    (limit, sent_today): (DailyLimit, i64),
    now: Timestamp,
) -> Result<EmailSent, EmailError> {
    let row = models::email_row(db, id)
        .await?
        .ok_or(EmailError::NotFound)?;
    if row.sent_at.is_some() {
        return Ok(EmailSent::AlreadySent);
    }
    if row.not_before > now {
        return Ok(EmailSent::Waiting(row.not_before));
    }
    if row.bulk && limit.holds_bulk(sent_today) {
        let until = next_reset(now);
        models::hold_email(db, id, until, "daily_limit").await?;
        return Ok(EmailSent::Waiting(until));
    }
    Err(EmailError::RecipientKind)
}

/// The address to send to: a homeowner's from the vault (PRIV-004), a business's as given.
async fn email_address(
    db: &mut PgConnection,
    to: EmailTo<'_>,
    recipient: Uuid,
) -> Result<SecretString, EmailError> {
    match to {
        EmailTo::Business(address) => Ok(SecretString::from(address.to_owned())),
        EmailTo::Homeowner(vault) => homeowner_email(db, vault, recipient)
            .await?
            .ok_or(EmailError::NoAddress),
    }
}

/// Homeowner `contact`'s email on file, if any, read under `messaging`'s send grant (PRIV-004).
pub(crate) async fn homeowner_email(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<Option<SecretString>, EmailError> {
    read_contact(
        db,
        vault,
        (Reader::Messaging, Purpose::Send),
        contact,
        &[Field::Email],
    )
    .await
    .map(|c| c.email)
    .map_err(|_| EmailError::NoAddress)
}

/// Email row `id`'s notice-log state: queued time, sent time, and what it waits for until when
/// (BIZ-045, OPS-038).
///
/// # Errors
///
/// [`EmailError::NotFound`] or [`EmailError::Database`].
pub async fn email_log(db: &mut PgConnection, id: Uuid) -> Result<EmailLog, EmailError> {
    models::email_row(db, id).await?.ok_or(EmailError::NotFound)
}

/// How a homeowner gets quote updates (TXT-018); no choice stored means text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    /// By text, the default.
    Text,
    /// By email instead.
    Email,
    /// By text and email.
    Both,
}

impl UpdateChannel {
    /// The stored name.
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::Both => "both",
        }
    }
}

/// Homeowner `contact`'s update channel (TXT-018): text unless she chose otherwise.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn update_channel(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<UpdateChannel, EmailError> {
    Ok(
        match models::update_channel(db, contact).await?.as_deref() {
            Some("email") => UpdateChannel::Email,
            Some("both") => UpdateChannel::Both,
            _ => UpdateChannel::Text,
        },
    )
}

/// Stores homeowner `contact`'s update channel at `now` (TXT-018; the quote-page control is T095's).
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn set_update_channel(
    db: &mut PgConnection,
    contact: Uuid,
    channel: UpdateChannel,
    now: Timestamp,
) -> Result<(), EmailError> {
    models::put_update_channel(db, contact, channel.as_str(), now).await
}

/// The ops alert kind raised for every provider spam complaint (SEC-050).
pub const COMPLAINT_ALERT: &str = "messaging.email_complaint";
/// The ops alert kind raised when emails were sent but no webhook came for 24 hours (SEC-051).
pub const WEBHOOKS_SILENT_ALERT: &str = "messaging.email_webhooks_silent";
/// The `messaging.webhook_event` source of an email-provider event (SEC-051).
const RESEND_SOURCE: &str = "resend";

/// What [`receive_email_event`] did with a verified provider webhook (SEC-051).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailEvent {
    /// Stored for the first time; acknowledge it.
    Recorded,
    /// Its message ID was stored before; acknowledge it and change nothing.
    Repeat,
    /// A well-formed event that is not about an email (not `email.*`, such as `contact.created`
    /// or `domain.updated`); acknowledge it and store nothing, so it is not sent again.
    Ignored,
}

/// The fields read from a Resend event body.
#[derive(serde::Deserialize)]
struct ResendEvent {
    #[serde(rename = "type")]
    kind: String,
    created_at: String,
    data: ResendData,
}

/// The email a Resend event is about; empty when the event has none (a `contact.*` or
/// `domain.*` event).
#[derive(serde::Deserialize)]
struct ResendData {
    #[serde(default)]
    email_id: String,
}

/// Stores one verified email-provider webhook at `now` (SEC-050, SEC-051): deduplicated by its
/// Svix message ID and stored with the provider's event time in one transaction, so `Ok` (and
/// only `Ok`) may be acknowledged; a complaint raises the [`COMPLAINT_ALERT`] in that transaction.
/// An event whose type is not `email.*` is [`EmailEvent::Ignored`] before anything is written, so
/// the table's checks never turn a valid provider event into a 500 sent again forever. The
/// signature, constant-time comparison and 5-minute tolerance are `adapters`' (T018).
///
/// # Errors
///
/// [`EmailError::BadWebhook`] (not Resend's, no message ID, an unreadable body, or an `email.*`
/// event with a malformed type or an empty or over-long email ID; nothing is stored) or
/// [`EmailError::Database`] (answer 500 so the provider sends it again).
pub async fn receive_email_event(
    db: &mut PgConnection,
    hook: &VerifiedWebhook,
    now: Timestamp,
) -> Result<EmailEvent, EmailError> {
    let key = hook
        .event_key()
        .filter(|_| hook.vendor() == WebhookVendor::Resend);
    let key = key.ok_or(EmailError::BadWebhook)?;
    let Some((event, event_at)) = parse_event(hook.body())? else {
        return Ok(EmailEvent::Ignored);
    };
    let mut tx = db.begin().await.map_err(|_| EmailError::Database)?;
    let first = models::first_event(&mut tx, (RESEND_SOURCE, key.as_str()), now)
        .await
        .map_err(|_| EmailError::Database)?;
    if !first {
        return Ok(EmailEvent::Repeat);
    }
    let fields = (event.data.email_id.as_str(), event.kind.as_str());
    let about = models::insert_email_event(&mut tx, fields, (event_at, now)).await?;
    if event.kind == "email.complained" {
        let alert = NewAlert {
            kind: COMPLAINT_ALERT,
            severity: Severity::High,
            subject: about.map(|id| ("messaging.email_outbox", id)),
        };
        raise_alert(&mut tx, &alert)
            .await
            .map_err(|_| EmailError::Database)?;
    }
    tx.commit().await.map_err(|_| EmailError::Database)?;
    Ok(EmailEvent::Recorded)
}

/// The longest provider email ID `messaging.email_event` stores (its `provider_id` check).
const MAX_PROVIDER_ID: usize = 128;
/// The longest event name after `email.` that `messaging.email_event` stores (its `kind` check).
const MAX_EVENT_NAME: usize = 32;

/// Reads a Resend event body and checks it against `messaging.email_event`'s checks (SEC-051):
/// `None` for a type outside `email.*`, the event and its time otherwise.
fn parse_event(body: &[u8]) -> Result<Option<(ResendEvent, Timestamp)>, EmailError> {
    let event: ResendEvent = serde_json::from_slice(body).map_err(|_| EmailError::BadWebhook)?;
    let Some(name) = event.kind.strip_prefix("email.") else {
        return Ok(None);
    };
    let name_ok = (1..=MAX_EVENT_NAME).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    let id = event.data.email_id.as_str();
    if !name_ok || id.is_empty() || id.len() > MAX_PROVIDER_ID {
        return Err(EmailError::BadWebhook);
    }
    let event_at = event
        .created_at
        .parse()
        .map_err(|_| EmailError::BadWebhook)?;
    Ok(Some((event, event_at)))
}

/// The latest provider event of email `provider_id` by the provider's event time, such as
/// `email.delivered` or `email.bounced` (SEC-051, BIZ-045); a late-arriving earlier event never
/// replaces a later one.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn email_status(
    db: &mut PgConnection,
    provider_id: &str,
) -> Result<Option<String>, EmailError> {
    models::latest_email_event(db, provider_id).await
}

/// How long a sent email has to produce a provider webhook before its silence counts (SEC-051):
/// an email sent more recently than this says nothing about whether webhooks arrive.
pub const WEBHOOK_GRACE: SignedDuration = SignedDuration::from_hours(1);

/// Raises the [`WEBHOOKS_SILENT_ALERT`] when no provider webhook arrived in the 24 hours before
/// `now` while emails were sent in them (SEC-051); returns whether it did. Only emails sent at
/// least [`WEBHOOK_GRACE`] before `now` count, so an email sent minutes ago never raises it. A
/// scheduled task calls it.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn check_webhook_silence(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<bool, EmailError> {
    let since = now - SignedDuration::from_hours(24);
    let sent = models::emails_sent_between(db, (since, now - WEBHOOK_GRACE)).await?;
    if sent == 0 || models::email_events_since(db, since).await? > 0 {
        return Ok(false);
    }
    let alert = NewAlert {
        kind: WEBHOOKS_SILENT_ALERT,
        severity: Severity::High,
        subject: None,
    };
    raise_alert(db, &alert)
        .await
        .map_err(|_| EmailError::Database)?;
    Ok(true)
}

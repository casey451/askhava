//! Quote updates by email for a homeowner who chose it (TXT-018, SEC-028, CON-035, TXT-027,
//! TXT-030, TXT-034; T730, D-4727, D-5340, D-5442, D-5533, D-5585, D-5639, D-5640).
//!
//! [`route`] runs inside the shared `service::insert` that `enqueue` and `enqueue_exempt` both
//! call, so an emergency job's update is routed too (D-5585). Only a homeowner row whose template
//! is in [`UPDATE_TEMPLATES`] and whose words name her quote page (its "Get updates by text only"
//! is the email's stop control) is ever emailed; [`TEXT_ONLY_UPDATES`] (direct replies to her own
//! texts, the who's-calling texts, the texts that ask for a text answer, the request-out texts,
//! the closed-job texts) and every other template (the 911 text, codes, the missed-call text) stay text only. With
//! `Email` or `Both` and a stored, unbounced update address, the update's rendered words are
//! queued as an `update.<template>` email in the caller's transaction, its subject IDs the text's
//! plus the address row (a new address is a new email, and a bounce finds its row); with `Email`
//! no text row is stored. While the address is unconfirmed the email carries "Confirm this
//! email", her quote page with T729's confirm token (`LinkKind::Action` over the address row and
//! its salt, D-5340). The footer's postal line is set once at start ([`set_update_footer`]);
//! without it every update stays text. The address itself is read only at send time through
//! `privacy::api::read_update_address` ([`UPDATE_EMAIL_PREFIX`]). A hard bounce ([`bounced`])
//! sets the channel back to text and marks the address bounced (D-5442).
//! [`queue_update_email`] stores the same email alone, never a text, for T840 to call when a
//! text gate (TXT-034's web-and-email mode among them) stops an update (D-5639, D-5640).
//! [`queue_data_request_link`] stores PRIV-031's single-use delete-or-copy confirm link for
//! `portals` (T804) as a [`DATA_REQUEST_TEMPLATE`] email, only while her update address is
//! confirmed; `email.rs` checks it again at send (T816, D-5339, D-5340, D-5670).

use std::sync::OnceLock;

use askhava_core::api::{IdSecret, LinkKind, brand, link_token};
use askhava_privacy::api::{UpdateStatus, mark_update_bounced, update_status};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{
    EmailClass, NewEmail, Notice, OnDomain, PostalAddress, Recipient, UpdateChannel, queue_email,
    set_update_channel, update_channel,
};
use crate::models::{EmailError, OutboxError};
use crate::service::NewText;
use crate::templates::{Audience, Language};

/// TXT-018, D-5533: the proactive quote updates that go by email when she chose it.
pub const UPDATE_TEMPLATES: [&str; 3] = ["first_quote", "quote_update", "all_quotes_in"];

/// TXT-018, TXT-027, D-5533: the update texts that stay text whatever she chose: direct replies
/// to her own texts, the who's-calling texts, texts that ask for a text answer and the
/// request-out texts, whose words promise a text (TXT-032), and the closed-job texts, which name
/// no quote page and so no way to stop update emails (SEC-028, CON-035, D-5562).
pub const TEXT_ONLY_UPDATES: [&str; 22] = [
    "quote_list",
    "quote_list_end",
    "text_pick_package",
    "text_pick_terms",
    "text_pick_which",
    "text_pick_done",
    "check_in_more",
    "feedback_request",
    "whos_calling",
    "whos_calling_email",
    "check_in_day3",
    "check_in_day14",
    "no_quotes_yet",
    "emailed_question",
    "request_out",
    "request_out_held",
    "request_out_today",
    "request_out_slow",
    "closed_no_quotes",
    "closed_no_pick",
    "closed_no_quote_left",
    "unreachable_closed",
];

/// The template prefix of an update email: `update.<text template>` (TXT-018).
pub const UPDATE_EMAIL_PREFIX: &str = "update.";

/// PRIV-031, D-5339: the data-request link email's template ID; not an update template.
pub const DATA_REQUEST_TEMPLATE: &str = "data_request_link";

/// PRIV-031: which of her requests a data-request link confirms (`messaging` may not name `portals`').
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLinkKind {
    /// A copy of her data.
    Copy,
    /// Deleting her data.
    Delete,
}

/// What [`queue_data_request_link`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEmail {
    /// She has no update address or it is unconfirmed: nothing was stored.
    NoConfirmedAddress,
    /// The email is stored (or was already, for a repeat): its row ID.
    Queued(Uuid),
}

/// The homeowner portal, the footer's SEC-028 "Manage email notices" link.
const MANAGE: OnDomain = OnDomain("/h");

/// The footer postal line set at start (SEC-028).
static FOOTER: OnceLock<PostalAddress> = OnceLock::new();

/// SEC-028: sets the update emails' footer postal line once for the process; `false` when one
/// was already set (the first stays). Until it is set every update goes by text.
pub fn set_update_footer(postal: PostalAddress) -> bool {
    FOOTER.set(postal).is_ok()
}

/// How `service::insert` stores an update (TXT-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Routed {
    /// The text only, as before.
    Text,
    /// The email was queued (its ID and whether this call stored it); no text row (`Email`).
    EmailOnly(Uuid, bool),
    /// The email was queued and the text is stored too (`Both`).
    Both,
}

/// TXT-018, TXT-027, TXT-030, D-5585: queues `text`'s update email in the caller's transaction
/// when `homeowner` is true, its template is in [`UPDATE_TEMPLATES`], her channel is `Email` or
/// `Both`, its words name her quote page (the stop control, SEC-028) and her update address is
/// stored and not bounced; says whether the text still goes.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub(crate) async fn route(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
    homeowner: bool,
) -> Result<Routed, OutboxError> {
    if !homeowner {
        return Ok(Routed::Text);
    }
    Ok(match emailed(db, secret, text).await? {
        Some((UpdateChannel::Email, id, created)) => Routed::EmailOnly(id, created),
        Some(_) => Routed::Both,
        None => Routed::Text,
    })
}

/// TXT-018, TXT-034, D-5639, D-5640: in the caller's transaction, stores only `text`'s update
/// email (as the shared insert's routing does) when its template is in [`UPDATE_TEMPLATES`], its
/// words name her quote page and her channel is `Email` or `Both` at this moment with an
/// unbounced update address; returns its ID and whether this call stored it, else `None` and
/// stores nothing. It never writes a `messaging.outbox` row, so no text can result: T840 calls it
/// when a text gate stops an update. `text.recipient` is the homeowner's contact ID.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn queue_update_email(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
) -> Result<Option<(Uuid, bool)>, OutboxError> {
    let queued = emailed(db, secret, text).await?;
    Ok(queued.map(|(_, id, created)| (id, created)))
}

/// TXT-018: queues `text`'s update email when every rule of [`route`] but the homeowner check
/// holds, returning her channel, the email's ID and whether this call stored it.
async fn emailed(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
) -> Result<Option<(UpdateChannel, Uuid, bool)>, OutboxError> {
    let Some(postal) = FOOTER.get() else {
        return Ok(None);
    };
    let Some(link) = quote_link(text.body).filter(|_| UPDATE_TEMPLATES.contains(&text.template_id))
    else {
        return Ok(None);
    };
    let channel = update_channel(db, text.recipient).await.map_err(db_err)?;
    if channel == UpdateChannel::Text {
        return Ok(None);
    }
    let status = update_status(db, text.recipient)
        .await
        .map_err(|_| OutboxError::Database)?;
    let Some(status) = status.filter(|s| !s.bounced) else {
        return Ok(None);
    };
    let (id, created) = queue_update(db, (secret, postal), (text, link), &status).await?;
    Ok(Some((channel, id, created)))
}

/// TXT-018, SEC-028, D-5340: queues `text`'s `update.<template>` email to her at the
/// transaction's time, its subject IDs the text's plus her address row, with the confirm link
/// while `status` is unconfirmed.
async fn queue_update(
    db: &mut PgConnection,
    (secret, postal): (&SecretString, &PostalAddress),
    (text, link): (&NewText<'_>, &str),
    status: &UpdateStatus,
) -> Result<(Uuid, bool), OutboxError> {
    let confirm = if status.confirmed {
        None
    } else {
        Some(confirm_link(secret, link, status)?)
    };
    let template = format!("{UPDATE_EMAIL_PREFIX}{}", text.template_id);
    let mut subject = text.subject.to_vec();
    subject.push(status.row);
    let email = NewEmail {
        template_id: &template,
        subject: &subject,
        class: EmailClass::Notice,
        notice: Notice::Update {
            words: text.body,
            quote_link: link,
            confirm_link: confirm.as_deref(),
        },
    };
    let manage = MANAGE.to_string();
    let to = Recipient {
        id: text.recipient,
        audience: Audience::Homeowner,
        language: language(text),
        manage_url: &manage,
    };
    let now = db_now(db).await?;
    let (id, created) = queue_email(db, (secret, postal), &to, &email, now)
        .await
        .map_err(db_err)?;
    Ok((id, created))
}

/// The language the text was rendered in (TXT-039).
fn language(text: &NewText<'_>) -> Language {
    match crate::retention::language_of(text.template_id, text.body) {
        Some("es") => Language::Es,
        _ => Language::En,
    }
}

/// The transaction's time, the text row's `created_at`, so a `Both` pair is queued at one instant
/// (D-5562).
async fn db_now(db: &mut PgConnection) -> Result<Timestamp, OutboxError> {
    let now = sqlx::query_scalar!(r#"SELECT now() AS "now!""#)
        .fetch_one(db)
        .await
        .map_err(|_| OutboxError::Database)?;
    Ok(now.to_jiff())
}

/// PICK-010: the first word of `body` that is her quote page, `https://<domain>/q/<token>`.
fn quote_link(body: &str) -> Option<&str> {
    let page = format!("https://{}/q/", brand().domain());
    body.split_whitespace()
        .find(|w| w.strip_prefix(page.as_str()).is_some_and(|t| !t.is_empty()))
}

/// D-5340: her quote page with T729's confirm token over the address row and its salt.
fn confirm_link(
    secret: &SecretString,
    link: &str,
    status: &UpdateStatus,
) -> Result<String, OutboxError> {
    let secret = IdSecret::new(1, secret.clone()).map_err(|_| OutboxError::Database)?;
    let token = link_token(&secret, LinkKind::Action, status.row, &status.salt);
    Ok(format!("{link}?confirm={}", token.as_str()))
}

/// An email error as the outbox's database error.
fn db_err(_: EmailError) -> OutboxError {
    OutboxError::Database
}

/// TXT-018, D-5442: after a hard bounce of email row `email`, in the webhook's transaction: when
/// it was an update email to her current address row, her channel goes back to text and the row
/// is marked bounced; a bounce for a replaced address changes nothing. Returns whether it reset.
///
/// # Errors
///
/// [`EmailError::Database`].
pub(crate) async fn bounced(
    db: &mut PgConnection,
    email: Uuid,
    now: Timestamp,
) -> Result<bool, EmailError> {
    let row = sqlx::query!(
        "SELECT recipient_id, subject_ids FROM messaging.email_outbox \
         WHERE id = $1 AND starts_with(template_id, $2)",
        email,
        UPDATE_EMAIL_PREFIX
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| EmailError::Database)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let status = update_status(db, row.recipient_id)
        .await
        .map_err(|_| EmailError::Database)?;
    let Some(status) = status.filter(|s| row.subject_ids.contains(&s.row)) else {
        return Ok(false);
    };
    set_update_channel(db, row.recipient_id, UpdateChannel::Text, now).await?;
    mark_update_bounced(db, status.row, now)
        .await
        .map_err(|_| EmailError::Database)?;
    Ok(true)
}

/// Whether email template `template` is an update email, whose address is her quote-page update
/// address (TXT-018, PICK-029).
pub(crate) fn is_update_email(template: &str) -> bool {
    template.starts_with(UPDATE_EMAIL_PREFIX)
}

/// PRIV-031, TXT-018, SEC-028, CON-035, D-5339: in the caller's transaction, stores one
/// [`DATA_REQUEST_TEMPLATE`] email to homeowner `contact` in `language` holding `link`, keyed on
/// the template, `contact` and `request` (a repeat stores nothing), only when her update address
/// is stored and confirmed; else [`LinkEmail::NoConfirmedAddress`] and nothing is stored. Only the
/// address state is read here, with no plaintext; the address opens at send (`email.rs`).
///
/// # Errors
///
/// [`EmailError::Database`] or [`EmailError::Template`].
pub async fn queue_data_request_link(
    db: &mut PgConnection,
    (secret, postal): (&SecretString, &PostalAddress),
    (contact, language): (Uuid, Language),
    (kind, request, link): (DataLinkKind, Uuid, &str),
    now: Timestamp,
) -> Result<LinkEmail, EmailError> {
    let status = update_status(db, contact)
        .await
        .map_err(|_| EmailError::Database)?;
    if !status.is_some_and(|s| s.confirmed) {
        return Ok(LinkEmail::NoConfirmedAddress);
    }
    let email = NewEmail {
        template_id: DATA_REQUEST_TEMPLATE,
        subject: &[request],
        class: EmailClass::Notice,
        notice: Notice::DataRequestLink { kind, link },
    };
    let manage = MANAGE.to_string();
    let to = Recipient {
        id: contact,
        audience: Audience::Homeowner,
        language,
        manage_url: &manage,
    };
    let (id, _) = queue_email(db, (secret, postal), &to, &email, now).await?;
    Ok(LinkEmail::Queued(id))
}

//! Quote updates by email for a homeowner who chose it (TXT-018, SEC-028, CON-035, TXT-027,
//! TXT-030; T730, D-4727, D-5340, D-5442, D-5533, D-5585, D-5617).
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
    let (Some(postal), true) = (FOOTER.get(), homeowner) else {
        return Ok(Routed::Text);
    };
    let Some(link) = quote_link(text.body).filter(|_| UPDATE_TEMPLATES.contains(&text.template_id))
    else {
        return Ok(Routed::Text);
    };
    let channel = update_channel(db, text.recipient).await.map_err(db_err)?;
    if channel == UpdateChannel::Text {
        return Ok(Routed::Text);
    }
    let status = update_status(db, text.recipient)
        .await
        .map_err(|_| OutboxError::Database)?;
    let Some(status) = status.filter(|s| !s.bounced) else {
        return Ok(Routed::Text);
    };
    let (id, created) = queue_update(db, (secret, postal), (text, link), &status).await?;
    Ok(match channel {
        UpdateChannel::Email => Routed::EmailOnly(id, created),
        _ => Routed::Both,
    })
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
/// and the text wins the tie in `last_quote_text` (D-5617).
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

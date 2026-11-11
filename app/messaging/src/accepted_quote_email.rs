//! The homeowner's copy of the quote she accepted, by email after the Confirm (PICK-042,
//! PICK-054, SEC-028, T388, D-244, D-466, D-3076).
//!
//! `intake`'s `picks.accepted_quote` handler gathers the words (the quote's number, type, amount,
//! earliest visit and the business's own words, worded by `quotes`) and calls
//! [`queue_accepted_quote`] with her vault contact and language. It reads her email on file under
//! `messaging`'s send grant (outside any transaction, the vault's rule) only to learn whether one
//! exists: none stores nothing. Otherwise it stores one email in T094's email outbox under an
//! `email.send` key over the template, contact and pick; `send_email` with `EmailTo::Homeowner`
//! reads the address again at send time, so the address never sits in a row, payload, log or
//! error. The email is rendered inside `templates/email_layout.html` with the homeowner footer and
//! offers "Add Ask Hava to your contacts" as a plain line with the brand's display name and domain
//! (no link: an email may carry no `data:` card, and a home-page link would not add a contact), with
//! no reward (PICK-054); no text carries it (TXT-008, TXT-009), and nothing here is in the text
//! registry. The words say "picked": the homeowner picks, the business accepts (`docs/voice.md`).

use askama::Template;
use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::brand;
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{
    EMAIL_ACTION, MailboxKind, OnDomain, PostalAddress, Rendered, audience_kind, display_name,
    footer_lines, homeowner_email, legal_name, short_name,
};
use crate::models::{self, EmailError};
use crate::templates::{Audience, Language};

/// The accepted-quote email's template ID; with the contact and pick it makes the key.
pub const ACCEPTED_QUOTE_TEMPLATE: &str = "accepted_quote_email";
/// The homeowner's "Manage email notices" link (SEC-028).
const MANAGE: OnDomain = OnDomain("/h");

/// The email key secret, Ask Hava LLC's checked postal line for the footer (SEC-028) and the vault
/// her email on file is checked in (PRIV-004).
pub struct AcceptedQuoteSender {
    secret: SecretString,
    postal: String,
    vault: Vault,
}

impl AcceptedQuoteSender {
    /// Checks `postal` as `kind` of mailbox before keeping it (SEC-028).
    ///
    /// # Errors
    /// [`EmailError::PostalAddress`] for a line that is not that kind of mailbox.
    pub fn new(
        secret: SecretString,
        (postal, kind): (&str, MailboxKind),
        vault: Vault,
    ) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        let postal = postal.trim().to_owned();
        Ok(Self {
            secret,
            postal,
            vault,
        })
    }
}

/// What the email says about the accepted quote, already in her language (PICK-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedQuoteFacts<'a> {
    /// The pick; with the contact it keys the email, so one email per pick.
    pub pick: Uuid,
    /// The request number she knows the job by.
    pub job_number: i64,
    /// The quote's number on her quote page.
    pub number: usize,
    /// The quote's type ("Firm price").
    pub kind: &'a str,
    /// The amount as the business stored it.
    pub amount: &'a str,
    /// The earliest visit day, if the business gave one.
    pub visit: Option<&'a str>,
    /// The business's own words.
    pub words: &'a str,
}

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"{% extends "email_layout.html" %}{% block content %}
<p>{{ lead }}</p>
{% for line in lines %}<p>{{ line }}</p>
{% endfor %}<p>{{ own }}</p>
<blockquote><p>{{ words }}</p></blockquote>
<p>{{ card }}</p>
{% endblock %}"#
)]
struct AcceptedView<'a> {
    lang: &'a str,
    subject: &'a str,
    lead: &'a str,
    lines: &'a [String],
    own: &'static str,
    words: &'a str,
    card: &'a str,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    legal: &'a str,
    postal: &'a str,
}

/// The subject, lead, fact lines, "in their words" label and contact-card line in `language`
/// (Spanish is a draft, TXT-003); the card names the brand and its domain, never a reward
/// (PICK-054).
fn words(
    language: Language,
    f: &AcceptedQuoteFacts<'_>,
) -> (String, String, Vec<String>, &'static str, String) {
    let (short, name, job, n) = (
        short_name(language),
        display_name(language),
        f.job_number,
        f.number,
    );
    let domain = brand().domain();
    let es = language == Language::Es;
    let label = |en: &'static str, sp: &'static str| if es { sp } else { en };
    let mut lines = vec![
        format!("{} {}", label("Type:", "Tipo:"), f.kind),
        format!("{} {}", label("Amount:", "Monto:"), f.amount),
    ];
    if let Some(day) = f.visit {
        lines.push(format!(
            "{} {day}",
            label("Earliest visit:", "Visita más pronto:")
        ));
    }
    if es {
        (
            format!("{short}: La cotización que eligió para la solicitud {job}"),
            format!(
                "Esta es una copia de la cotización que eligió para la solicitud {job} (cotización #{n}). También aparece en su página de cotizaciones."
            ),
            lines,
            "En palabras del negocio:",
            format!(
                "Agregue {name} a sus contactos para reconocer nuestros mensajes: {name}, {domain}."
            ),
        )
    } else {
        (
            format!("{short}: The quote you picked for request {job}"),
            format!(
                "Here's a copy of the quote you picked for request {job} (quote #{n}). It's also on your quote page."
            ),
            lines,
            "In their words:",
            format!("Add {name} to your contacts so you know it's us: {name}, {domain}."),
        )
    }
}

/// PICK-042, PICK-054, SEC-028: the email of `facts` in `language`; HTML and plain text carry the
/// same lines, the business's words, the contact-card line and the homeowner footer.
///
/// # Errors
/// [`EmailError::Template`] if the layout fails to render.
pub fn render_accepted_quote(
    sender: &AcceptedQuoteSender,
    language: Language,
    facts: &AcceptedQuoteFacts<'_>,
) -> Result<Rendered, EmailError> {
    let (subject, lead, lines, own, card) = words(language, facts);
    let (why, manage_label) = footer_lines(Audience::Homeowner, language);
    let (manage, legal) = (MANAGE.to_string(), legal_name(language));
    let view = AcceptedView {
        lang: if language == Language::Es { "es" } else { "en" },
        subject: &subject,
        lead: &lead,
        lines: &lines,
        own,
        words: facts.words,
        card: &card,
        why: &why,
        manage_url: &manage,
        manage_label,
        legal,
        postal: &sender.postal,
    };
    let html = view.render().map_err(|_| EmailError::Template)?;
    let plain = format!(
        "{lead}\n{}\n{own}\n{}\n{card}\n\n--\n{why}\n{manage_label}: {manage}\n{legal}, {}\n",
        lines.join("\n"),
        facts.words,
        sender.postal
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// PICK-042, D-493, SEC-018: stores the accepted-quote email of `facts` for homeowner `contact` in
/// `language` once (key over template, contact and pick); `false` when she has no email on file
/// (nothing stored) or it was stored before. Her email is read only to see that one exists, on
/// `db` outside any transaction, and is never kept.
///
/// # Errors
/// [`EmailError::NoAddress`] when the vault read fails (the caller retries), [`EmailError::Template`]
/// or a database error from the outbox insert.
pub async fn queue_accepted_quote(
    db: &mut PgConnection,
    sender: &AcceptedQuoteSender,
    (contact, language): (Uuid, Language),
    facts: &AcceptedQuoteFacts<'_>,
    now: Timestamp,
) -> Result<bool, EmailError> {
    if homeowner_email(db, &sender.vault, contact).await?.is_none() {
        return Ok(false);
    }
    let rendered = render_accepted_quote(sender, language, facts)?;
    let parts = [
        ACCEPTED_QUOTE_TEMPLATE.to_owned(),
        contact.to_string(),
        facts.pick.to_string(),
    ];
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let key = IdempotencyKey::derive(&sender.secret, EMAIL_ACTION, &parts);
    let ids = (
        ACCEPTED_QUOTE_TEMPLATE,
        contact,
        std::slice::from_ref(&facts.pick),
    );
    let kind = audience_kind(Audience::Homeowner);
    let (_, new) =
        models::insert_email(db, (key.as_str(), ids, kind, false), &rendered, now).await?;
    Ok(new)
}

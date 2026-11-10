//! Ask Hava's own receipt email for every charge, beside Stripe's (FEE-031, FEE-053, D-225,
//! D-491, D-492).
//!
//! `picks` handles `payments`' receipt event and queues one [`RECEIPT_EMAIL_KIND`] task with the
//! business's money recipients (opaque address IDs), the pick, the job number, the fee and the
//! charge description "Ask Hava introduction service, job <n>". [`run_receipt_email`] composes the
//! receipt for each recipient inside `templates/email_layout.html` (the SEC-028 footer) and stores
//! it once in T094's email outbox under an `email.send` key over the template, recipient and pick;
//! T094's `send_email` sends it. The receipt names the charge, says "Tax included." and links the
//! pick's "Why was I charged?" page; one pick, one charge, one line (FEE-053).

use std::sync::Arc;

use askama::Template;
use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::{Claimed, HandlerError};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::email::{
    EMAIL_ACTION, MailboxKind, OnDomain, PostalAddress, Recipient, Rendered, audience_kind,
    footer_lines, legal_name, short_name,
};
use crate::models::{self, EmailError};
use crate::templates::{Audience, Language};

/// The `core.task` kind through which `picks` asks for a charge's receipt email (D-492).
pub const RECEIPT_EMAIL_KIND: &str = "messaging.charge_receipt";
/// The receipt email's template ID; with the recipient and pick it makes the email's key.
pub const RECEIPT_TEMPLATE: &str = "charge_receipt";
/// The "Why was I charged?" page of a pick is this prefix, the pick ID and nothing else (FEE-031),
/// on the brand's domain (D-1932).
pub const WHY_CHARGED_PREFIX: OnDomain = OnDomain("/b/charges/");
/// The business portal, the receipt's "Manage email notices" link (SEC-028).
const PORTAL: OnDomain = OnDomain("/b");

/// What one receipt says (FEE-031): the pick, its job number, the fee and the charge description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptFacts {
    /// The pick charged; its page is the "Why was I charged?" link.
    pub pick: Uuid,
    /// The job's request number.
    pub job_number: i64,
    /// The introduction fee by job size charged, in cents, tax included.
    pub fee_cents: u64,
    /// The charge description, "Ask Hava introduction service, job <n>" (FEE-031).
    pub line: String,
}

/// The secret for email keys and Ask Hava LLC's checked postal line for the footer (SEC-028).
pub struct ReceiptSender {
    secret: SecretString,
    postal: String,
}

impl ReceiptSender {
    /// Checks `postal` as `kind` of mailbox before keeping it (SEC-028).
    ///
    /// # Errors
    /// [`EmailError::PostalAddress`] for a line that is not that kind of mailbox.
    pub fn new(secret: SecretString, postal: &str, kind: MailboxKind) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        let postal = postal.trim().to_owned();
        Ok(Self { secret, postal })
    }
}

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"{% extends "email_layout.html" %}{% block content %}
<p>{{ charge }}</p>
<p>{{ tax }}</p>
<p>{{ one_line }}</p>
<p><a href="{{ why_url }}" style="color:#6B4A1E;text-decoration:underline;">{{ why_label }}</a></p>
{% endblock %}"#
)]
struct ReceiptView<'a> {
    lang: &'a str,
    subject: &'a str,
    charge: &'a str,
    tax: &'a str,
    one_line: &'a str,
    why_url: &'a str,
    why_label: &'a str,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    legal: &'a str,
    postal: &'a str,
}

/// The receipt's words in `language`: subject (opening with the brand's short name, D-1932), tax
/// line, one-charge line and link label (FEE-031); the footer is `email.rs`'s business footer
/// (SEC-028). Spanish is a draft (TXT-003).
fn words(language: Language, job: i64) -> (String, [&'static str; 3]) {
    let short = short_name(language);
    match language {
        Language::En => (
            format!("{short}: Receipt for job {job}"),
            [
                "Tax included.",
                "You're charged because a homeowner picked you and you confirmed. Stripe also emails its own receipt for this same charge.",
                "Why was I charged?",
            ],
        ),
        Language::Es => (
            format!("{short}: Recibo del trabajo {job}"),
            [
                "Impuestos incluidos.",
                "Se le cobra porque un cliente lo eligió y usted confirmó. Stripe también le envía por correo su propio recibo de este mismo cargo.",
                "¿Por qué se me cobró?",
            ],
        ),
    }
}

/// FEE-031, SEC-028: the receipt of `facts` for `language`, HTML and plain text with the same lines.
///
/// # Errors
/// [`EmailError::Template`] if the layout fails to render.
pub fn render_receipt(
    sender: &ReceiptSender,
    language: Language,
    facts: &ReceiptFacts,
) -> Result<Rendered, EmailError> {
    let (subject, [tax, one_line, why_label]) = words(language, facts.job_number);
    let (why, manage_label) = footer_lines(Audience::Business, language);
    let (portal, legal) = (PORTAL.to_string(), legal_name(language));
    let charge = format!(
        "{}: ${}.{:02}",
        facts.line,
        facts.fee_cents / 100,
        facts.fee_cents % 100
    );
    let why_url = format!("{WHY_CHARGED_PREFIX}{}", facts.pick);
    let lang = if language == Language::Es { "es" } else { "en" };
    let view = ReceiptView {
        lang,
        subject: &subject,
        charge: &charge,
        tax,
        one_line,
        why_url: &why_url,
        why_label,
        why: &why,
        manage_url: &portal,
        manage_label,
        legal,
        postal: &sender.postal,
    };
    let html = view.render().map_err(|_| EmailError::Template)?;
    let plain = format!(
        "{charge}\n{tax}\n{one_line}\n{why_label} {why_url}\n\n--\n{why}\n{manage_label}: {portal}\n\
         {legal}, {}\n",
        sender.postal
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// Parses the task's payload once: the facts and the recipients' address IDs.
fn parse(payload: &serde_json::Value) -> Option<(ReceiptFacts, Vec<Uuid>)> {
    let pick = Uuid::parse_str(payload["pick"].as_str()?).ok()?;
    let values = &payload["values"];
    let facts = ReceiptFacts {
        pick,
        job_number: values["job"].as_i64()?,
        fee_cents: values["fee_cents"].as_u64()?,
        line: values["line"].as_str()?.to_owned(),
    };
    let to = payload["to"].as_array()?;
    let to = to
        .iter()
        .map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()));
    Some((facts, to.collect::<Option<Vec<_>>>()?))
}

/// The [`RECEIPT_EMAIL_KIND`] handler (FEE-031, D-492): stores one receipt per recipient in the
/// email outbox, in one transaction; a second run stores nothing new (JOB-006, SEC-018). Business
/// notices are in English until the recipient's language is passed (BIZ-018, D-1559).
///
/// # Errors
/// A fixed sentence for a malformed event, an event with no recipient (`picks` alerts and queues
/// none, so one reaching here is a fault and is dead-lettered, never a silent success, D-1561), or
/// a failed render or statement; the queue tries again.
pub async fn run_receipt_email(
    pool: PgPool,
    sender: Arc<ReceiptSender>,
    task: Claimed,
) -> Result<(), HandlerError> {
    const FAILED: HandlerError = HandlerError("receipt email could not be stored");
    let (facts, to) = parse(&task.payload).ok_or(HandlerError("receipt event is malformed"))?;
    if to.is_empty() {
        return Err(HandlerError("receipt event has no recipient"));
    }
    let rendered = render_receipt(&sender, Language::En, &facts).map_err(|_| FAILED)?;
    let mut tx = pool.begin().await.map_err(|_| FAILED)?;
    let portal = PORTAL.to_string();
    for address in to {
        let to = Recipient {
            id: address,
            audience: Audience::Business,
            language: Language::En,
            manage_url: &portal,
        };
        let ids = (RECEIPT_TEMPLATE, to.id, std::slice::from_ref(&facts.pick));
        let parts = [
            RECEIPT_TEMPLATE.to_owned(),
            to.id.to_string(),
            facts.pick.to_string(),
        ];
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let key = IdempotencyKey::derive(&sender.secret, EMAIL_ACTION, &parts);
        let kind = audience_kind(to.audience);
        models::insert_email(
            &mut tx,
            (key.as_str(), ids, kind, false),
            &rendered,
            Timestamp::now(),
        )
        .await
        .map_err(|_| FAILED)?;
    }
    tx.commit().await.map_err(|_| FAILED)
}

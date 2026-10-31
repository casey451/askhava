//! The first reply to a request (CON-001, CON-026, CON-027, CON-031; rust brain R11).
//!
//! [`on_text`] runs after `messaging` has handled dedupe, opt-outs and HELP (`docs/architecture.md`
//! section 8). For a contact with no open request it opens one and queues, in one transaction with
//! its audit row, the 911 text on a 911-list match and then the consent text, each under a key
//! derived from the request (TXT-011), so the consent text goes once per request even when a
//! message is handled twice. Any other text is returned as a conversation step: nothing waits for
//! a reply (CON-004, D-419).

use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, IntakeError};
use crate::rules::{
    DISCLOSURE_TEMPLATE, Inbound, Plan, SAFETY_TEMPLATE, Send, Settings, Then, language_code, plan,
};

/// One inbound text from a homeowner's number, as `messaging` passes it on (IDs and flags only).
#[derive(Debug, Clone, Copy)]
pub struct TextIn<'a> {
    /// Its `messaging.inbox` row.
    pub inbox_id: Uuid,
    /// The sender's vault contact ID, to whom texts go.
    pub contact: Uuid,
    /// The flags and scrubbed words.
    pub inbound: Inbound<'a>,
    /// When it arrived, from the caller's clock.
    pub received_at: Timestamp,
}

/// One text [`on_text`] queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queued {
    /// Its `messaging.outbox` row.
    pub outbox_id: Uuid,
    /// The template it was rendered from.
    pub template: &'static str,
}

/// What [`on_text`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handled {
    /// The open request, if there is one.
    pub conversation: Option<Uuid>,
    /// The texts queued now, in the order they must be sent (D-908).
    pub texts: Vec<Queued>,
    /// What the conversation does next (CON-027).
    pub then: Then,
}

/// Handles one inbound text for the first reply (CON-001, CON-027, D-008, D-131): a new request
/// gets the 911 text on a 911-list match and then the consent text; a reply in an open request,
/// an opt-out and a HELP keyword get no consent text. `secret` is the `ID_SECRET`.
///
/// # Errors
///
/// [`IntakeError::Database`], [`IntakeError::Template`], [`IntakeError::Outbox`] or
/// [`IntakeError::Audit`]; nothing is kept on an error.
pub async fn on_text(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
) -> Result<Handled, IntakeError> {
    let open = models::open_conversation(db, msg.contact).await?;
    let decided = plan(open.is_some(), &msg.inbound, settings);
    if decided.texts.is_empty() {
        return Ok(Handled {
            conversation: open,
            texts: Vec::new(),
            then: decided.then,
        });
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let opened = open_request(&mut tx, secret, msg, (&decided, settings)).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    match opened {
        Some(handled) => Ok(handled),
        None => {
            let conversation = models::open_conversation(db, msg.contact).await?;
            Ok(Handled {
                conversation,
                texts: Vec::new(),
                then: Then::IntakeReply,
            })
        }
    }
}

/// Opens the request and queues the plan's texts in order; `None` when another message opened it
/// first, so its consent text is not sent twice (CON-027).
async fn open_request(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (decided, settings): (&Plan, Settings),
) -> Result<Option<Handled>, IntakeError> {
    let (template, language) = consent_of(decided);
    let homeowner = models::homeowner_for(tx, msg.contact, msg.received_at).await?;
    let ids = (homeowner, msg.inbox_id);
    let names = (language_code(language), template);
    let Some(id) = models::open(tx, ids, names, msg.received_at).await? else {
        return Ok(None);
    };
    let mut texts = Vec::with_capacity(decided.texts.len());
    for send in &decided.texts {
        let (template, language, subject) = match *send {
            Send::Safety(language) => (SAFETY_TEMPLATE, language, vec![id, msg.inbox_id]),
            Send::Consent { template, language } => (template, language, vec![id]),
        };
        let body = render(template, language, settings.spanish_conversations_on, &[])
            .map_err(|_| IntakeError::Template)?
            .body;
        let text = NewText {
            template_id: template,
            subject: &subject,
            recipient: msg.contact,
            relation: Relation::OpenRequest,
            body: &body,
        };
        let (row, _) = enqueue(tx, secret, &text)
            .await
            .map_err(|_| IntakeError::Outbox)?;
        texts.push(Queued {
            outbox_id: row,
            template,
        });
    }
    audit_consent_sent(tx, homeowner, id).await?;
    Ok(Some(Handled {
        conversation: Some(id),
        texts,
        then: decided.then,
    }))
}

/// The consent template and language of a plan that sends the consent text.
fn consent_of(decided: &Plan) -> (&'static str, Language) {
    decided
        .texts
        .iter()
        .find_map(|s| match *s {
            Send::Consent { template, language } => Some((template, language)),
            Send::Safety(_) => None,
        })
        .unwrap_or((crate::rules::CONSENT_TEMPLATE, Language::En))
}

/// The audit row of a request opened with its consent text (JOB-005, PRIV-028: IDs only).
async fn audit_consent_sent(
    tx: &mut PgConnection,
    homeowner: Uuid,
    conversation: Uuid,
) -> Result<(), IntakeError> {
    let entry = AuditEntry {
        actor: Actor::Homeowner(homeowner),
        action: "intake.consent_sent",
        subject_kind: "intake.conversation",
        subject_id: conversation,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Audit)
}

/// The CON-027 line ("Ask Hava is a local service. Our texts are automated. ...") that goes before
/// the next intake question when a reply asks who this is, in the request's language.
///
/// # Errors
///
/// [`IntakeError::Template`] if the compiled-in texts are broken.
pub fn disclosure(
    language: Language,
    spanish_conversations_on: bool,
) -> Result<String, IntakeError> {
    render(DISCLOSURE_TEMPLATE, language, spanish_conversations_on, &[])
        .map(|t| t.body)
        .map_err(|_| IntakeError::Template)
}

/// The stored conversation row (for later intake tasks and tests).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn conversation(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<models::ConversationRow>, IntakeError> {
    models::conversation(db, id).await
}

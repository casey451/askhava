//! The first reply to a request, its consent record and the terms acceptance at a pick (CON-001,
//! CON-005, CON-007, CON-021, CON-026, CON-027, CON-031, TERMS-001, TERMS-016, TERMS-033; rust
//! brain R11).
//!
//! [`receive_text`] runs after `messaging` has handled dedupe, opt-outs and HELP
//! (`docs/architecture.md` section 8). For a contact with no open request it opens one and queues,
//! in one transaction with its consent record and audit row, the 911 text on a 911-list match and
//! then the consent text, each under a key derived from the request (TXT-011), so the consent text
//! goes once per request even when a message is handled twice. Any other text is returned as a
//! conversation step: nothing waits for a reply and no reply is a consent or terms event (CON-004,
//! D-419, D-420). The terms are accepted only by a pick, through [`record_terms_acceptance`]
//! (TERMS-001).

use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, ConsentRecordRow, IntakeError, NewAcceptance, TermsAcceptanceRow};
use crate::rules::{
    CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, DISCLOSURE_TEMPLATE, Inbound, Plan, SAFETY_TEMPLATE,
    Send, Settings, Then, language_code, language_from_code, plan,
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
/// an opt-out and a HELP keyword get no consent text. `secret` is the `ID_SECRET`. The inner step
/// of [`receive_text`], which adds the consent record; other crates reach only that.
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
            // Lost the race: the winner queued the consent text, so this message is a reply in
            // its request ("is this a real person?" still gets the CON-027 line).
            let conversation = models::open_conversation(db, msg.contact).await?;
            Ok(Handled {
                conversation,
                texts: Vec::new(),
                then: plan(true, &msg.inbound, settings).then,
            })
        }
    }
}

/// Handles one inbound homeowner text: [`on_text`] and, when this text opened the request, its
/// consent record (this text's ID and time, channel `sms`, the consent-text version), in one
/// transaction (CON-005, D-419). A reply in an open request, an opt-out and a HELP keyword record
/// nothing, so no reply is ever a consent event. The entry point for every inbound text.
///
/// # Errors
///
/// As [`on_text`]; nothing is kept on an error.
pub async fn receive_text(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
) -> Result<Handled, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let handled = on_text(&mut tx, secret, msg, settings).await?;
    let sent = handled.texts.iter().find(|q| is_consent(q.template));
    if let (Some(consent), Some(id)) = (sent, handled.conversation) {
        let row = models::conversation(&mut tx, id)
            .await?
            .ok_or(IntakeError::Database)?;
        let language = language_from_code(&row.language);
        let body = render(
            consent.template,
            language,
            settings.spanish_conversations_on,
            &[],
        )
        .map_err(|_| IntakeError::Template)?
        .body;
        let version = consent_version(consent.template, language, &body);
        let ids = (row.homeowner_id, id, msg.inbox_id);
        models::record_consent(&mut tx, ids, &version, msg.received_at).await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(handled)
}

/// Whether `template` is a consent text template.
fn is_consent(template: &str) -> bool {
    template == CONSENT_TEMPLATE || template == CONSENT_CALL_TEMPLATE
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

/// SHA256 of `text` as 64 lower-case hex digits.
fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The consent-text version (CON-005): template, language and the first 12 hex digits of the
/// SHA256 of the exact text sent, so any wording change is a new version.
fn consent_version(template: &str, language: Language, body: &str) -> String {
    let hash = sha256_hex(body);
    format!("{template}:{}:{}", language_code(language), &hash[..12])
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
        .unwrap_or((CONSENT_TEMPLATE, Language::En))
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

/// The consent records of a request, oldest first (CON-005, CON-022).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn consent_records(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<ConsentRecordRow>, IntakeError> {
    models::consent_records(db, conversation).await
}

/// Whether a job card may go out for a request: only once its consent is recorded (CON-007). The
/// caller sending a card checks this first.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn card_allowed(db: &mut PgConnection, conversation: Uuid) -> Result<bool, IntakeError> {
    Ok(!models::consent_records(db, conversation).await?.is_empty())
}

/// Whether vault contact `contact` may be texted as a homeowner: only someone who wrote first and
/// so has a consent record; a tenant or on-site contact never has one (CON-021).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn may_text(db: &mut PgConnection, contact: Uuid) -> Result<bool, IntakeError> {
    models::contact_consented(db, contact).await
}

/// How a pick accepted the homeowner terms (TERMS-001, TERMS-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermsChannel {
    /// The Pick button on the private quote page (PICK-016).
    QuotePage,
    /// A "<number> YES" reply to a text ending with the terms line (PICK-046, TXT-031).
    TextPick {
        /// The inbound text that picked.
        inbox_id: Uuid,
    },
}

/// One homeowner terms acceptance at a pick, as the picks module records it (TERMS-033).
#[derive(Debug, Clone, Copy)]
pub struct TermsAcceptance<'a> {
    /// The request.
    pub conversation: Uuid,
    /// The pick that accepted.
    pub pick: Uuid,
    /// The document code, such as `homeowner_terms`.
    pub document: &'a str,
    /// The published version.
    pub version: &'a str,
    /// The exact published text of that version; only its SHA256 is stored.
    pub published_text: &'a str,
    /// The language the terms were shown in.
    pub language: Language,
    /// Quote page or text pick.
    pub channel: TermsChannel,
    /// The assent line shown beside the pick ("By picking, you agree to the Terms").
    pub assent_line: &'a str,
    /// When the pick was made.
    pub accepted_at: Timestamp,
}

/// Records a homeowner terms acceptance at a pick: one row holding the document, version, SHA256
/// of the published text, language, channel, assent line, time, request and pick, and the inbound
/// text for a text pick (TERMS-001, TERMS-016, TERMS-033, D-492). Only a pick calls this; no
/// consent text or reply does. Returns `false` when this pick's acceptance is already stored.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`] when the request does not exist; [`IntakeError::Database`]
/// when a value breaks the table's checks or the call fails.
pub async fn record_terms_acceptance(
    db: &mut PgConnection,
    a: &TermsAcceptance<'_>,
) -> Result<bool, IntakeError> {
    let (channel, inbox) = match a.channel {
        TermsChannel::QuotePage => ("quote_page", None),
        TermsChannel::TextPick { inbox_id } => ("text_pick", Some(inbox_id)),
    };
    let text_sha256 = sha256_hex(a.published_text);
    let row = NewAcceptance {
        conversation: a.conversation,
        pick: a.pick,
        document: a.document,
        version: a.version,
        text_sha256: &text_sha256,
        language: language_code(a.language),
        channel,
        assent_line: a.assent_line,
        inbox,
        at: a.accepted_at,
    };
    if models::insert_acceptance(db, &row).await? {
        return Ok(true);
    }
    match models::conversation(db, a.conversation).await? {
        Some(_) => Ok(false),
        None => Err(IntakeError::UnknownRequest),
    }
}

/// The homeowner terms acceptances of a request, oldest first (TERMS-016).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn terms_acceptances(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<TermsAcceptanceRow>, IntakeError> {
    models::acceptances(db, conversation).await
}

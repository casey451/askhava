//! What opens a job, duplicates, exact answers, the name and address, and the under-18 reply
//! (INT-007 to INT-012, INT-027, INT-032, INT-038; rust brain R5, R6, R11).
//!
//! A request's jobs are kept as `intake.intake_answer` rows named `job.<trade>.<property>` whose
//! choice is the job ID (D-993): no table of its own, so a job link holds IDs and codes only. The
//! rules are in [`crate::rules`]; this file is the database side. [`begin_job`] is the one
//! job-opening path: the first job of a request, and the second job after an ALSO reply (INT-032;
//! the two-trade detection and the reply are T042's). [`stop_if_under_18`] sends the one INT-038
//! reply and closes the request. The name-and-address answer goes to the vault and nowhere else
//! (INT-027, D-146): nothing in this file touches an AI.

use askhava_jobs::api::create;
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, IntakeError};
use crate::rules::{
    AgeStop, NAME_ADDRESS_TEMPLATE, UNDER_18_TEMPLATE, accepts_word_answer, age_stop,
    joins_open_job, opens_job, pick_option, split_name_address, wanted_language,
};
use crate::service::{Queued, TextIn};

/// The stored question of a message the homeowner sent to the request (INT-010's clock).
const HEARD: &str = "heard";
/// The stored question holding the vault contact of the name-and-address answer (INT-027).
const ADDRESS_CONTACT: &str = "address_contact";

/// What a message asks [`begin_job`] to open (INT-007 to INT-009).
#[derive(Debug, Clone, Copy)]
pub struct JobStart<'a> {
    /// The request the message belongs to.
    pub conversation: Uuid,
    /// The scrubbed words of the message (the length and cancel checks read them).
    pub words: &'a str,
    /// The message holds a 911-list keyword (INT-007, D-138).
    pub emergency: bool,
    /// The catalog trade code of the job, such as `plumbing`.
    pub trade: &'a str,
    /// When the message arrived.
    pub at: Timestamp,
}

/// What [`begin_job`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A new job, created now.
    Created(Uuid),
    /// The homeowner already has an active job for this property and trade (INT-009).
    Existing(Uuid),
    /// The message opens no job (INT-007, INT-008), or the request is closed or unknown.
    Refused,
}

/// Opens the job for `start`, unless the message cannot open one (INT-007, INT-008, D-138) or the
/// homeowner already has an active job for the same property and trade (INT-009). One transaction
/// holds the homeowner's row, so two messages at once open one job. The ALSO reply's second job
/// comes through here too, with the second trade (INT-032).
///
/// # Errors
///
/// [`IntakeError::BadTrade`], [`IntakeError::Job`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`]; nothing is kept on an error.
pub async fn begin_job(db: &mut PgConnection, start: &JobStart<'_>) -> Result<Opened, IntakeError> {
    if !opens_job(start.words, start.emergency) {
        return Ok(Opened::Refused);
    }
    let Some((homeowner, true)) = models::conversation_state(db, start.conversation).await? else {
        return Ok(Opened::Refused);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let property = models::latest_property(&mut tx, homeowner).await?;
    let kind = job_kind(start.trade, property)?;
    if let Some(job) = models::active_job(&mut tx, homeowner, &kind).await? {
        tx.commit().await.map_err(|_| IntakeError::Database)?;
        return Ok(Opened::Existing(job));
    }
    let job = create(
        &mut tx,
        start.at,
        start.emergency,
        Actor::Homeowner(homeowner),
    )
    .await
    .map_err(|_| IntakeError::Job)?;
    let choice = job.simple().to_string();
    models::insert_plain(&mut tx, start.conversation, (&kind, &choice), start.at).await?;
    models::set_first_job(&mut tx, start.conversation, job).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Opened::Created(job))
}

/// The stored name of a job link: trade and the first 12 hex digits of the property ID, or `none`
/// before the ZIP and area check (INT-009).
fn job_kind(trade: &str, property: Option<Uuid>) -> Result<String, IntakeError> {
    let valid = !trade.is_empty()
        && trade.len() <= 40
        && trade.starts_with(|c: char| c.is_ascii_lowercase())
        && trade
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(IntakeError::BadTrade);
    }
    let part = property.map_or_else(
        || "none".to_owned(),
        |p| p.simple().to_string()[..12].to_owned(),
    );
    Ok(format!("job.{trade}.{part}"))
}

/// Notes that the homeowner wrote to request `conversation` at `msg.received_at`, the start of
/// INT-010's window for the next message.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn hear(
    db: &mut PgConnection,
    conversation: Uuid,
    msg: &TextIn<'_>,
) -> Result<(), IntakeError> {
    let ids = (conversation, msg.inbox_id);
    models::insert_answer(db, ids, (HEARD, "msg"), msg.received_at).await
}

/// The job a message at `now` joins even if it is read as another trade: the homeowner's newest
/// job when `now` is inside `window` of the last homeowner message to it (INT-010). After the
/// window, a message of the same trade is T042's word-overlap rule.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`] or [`IntakeError::Database`].
pub async fn joining_job(
    db: &mut PgConnection,
    conversation: Uuid,
    now: Timestamp,
    window: SignedDuration,
) -> Result<Option<Uuid>, IntakeError> {
    let (homeowner, _) = models::conversation_state(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let newest = models::newest_job(db, homeowner).await?;
    Ok(newest.and_then(|(job, last)| joins_open_job(last, now, window).then_some(job)))
}

/// What [`record_answer`] did with a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Stored, with this choice code.
    Stored(String),
    /// The request is closed or unknown: the answer counts for nothing (INT-011).
    Closed,
    /// No option matched exactly: ask again (INT-011).
    Unclear,
}

/// One answer to one intake question.
#[derive(Debug, Clone, Copy)]
pub struct Answer<'a> {
    /// The request.
    pub conversation: Uuid,
    /// The question bank ID (lower case letters, digits, `_` and `.`).
    pub question: &'a str,
    /// The options as (code, phrase), or `None` for a free-answer question.
    pub options: Option<&'a [(&'a str, &'a str)]>,
    /// The scrubbed words of the reply.
    pub words: &'a str,
    /// The inbound text that answered, and when.
    pub reply: (Uuid, Timestamp),
}

/// Stores a reply as the answer to a question, only for a request that is still open and only on
/// an exact word match (INT-011): "ok" inside "broke" is not an answer. A free-answer question
/// accepts any words, such as "the AC one", "Yes!" or "my email is ..." (INT-012); the words
/// themselves are not stored, only the code `text`.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn record_answer(
    db: &mut PgConnection,
    answer: &Answer<'_>,
) -> Result<Recorded, IntakeError> {
    let open = models::conversation_state(db, answer.conversation).await?;
    if !matches!(open, Some((_, true))) {
        return Ok(Recorded::Closed);
    }
    let choice = match answer.options {
        Some(options) => pick_option(answer.words, options),
        None => accepts_word_answer(answer.words).then_some("text"),
    };
    let Some(choice) = choice else {
        return Ok(Recorded::Unclear);
    };
    let ids = (answer.conversation, answer.reply.0);
    models::insert_answer(db, ids, (answer.question, choice), answer.reply.1).await?;
    Ok(Recorded::Stored(choice.to_owned()))
}

/// The INT-038 reply, if the sender stated an age under 18 or said they are in school, and the
/// request closed (no card ever goes). Under 13, the request and its data are deleted too, except
/// the consent record. The reply goes once: a closed request gets none. An emergency keyword was
/// handled first by `receive_text` (INT-002), so its 911 text is already queued before this one.
///
/// # Errors
///
/// [`IntakeError::Template`], [`IntakeError::Outbox`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`]; nothing is kept on an error.
pub async fn stop_if_under_18(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    conversation: Uuid,
    spanish_on: bool,
) -> Result<Option<(AgeStop, Queued)>, IntakeError> {
    let Some(stop) = age_stop(msg.inbound.words) else {
        return Ok(None);
    };
    let Some((homeowner, _)) = models::conversation_state(db, conversation).await? else {
        return Err(IntakeError::UnknownRequest);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if !models::close_request(&mut tx, conversation, msg.received_at).await? {
        return Ok(None);
    }
    let language = wanted_language(msg.inbound.words, msg.inbound.ai_spanish);
    let subject = [conversation, msg.inbox_id];
    let queued = queue_text(
        &mut tx,
        secret,
        msg.contact,
        (UNDER_18_TEMPLATE, language),
        &subject,
        spanish_on,
    )
    .await?;
    if stop == AgeStop::Under13 {
        models::purge_request(&mut tx, conversation, homeowner).await?;
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: "intake.under_18_closed",
        subject_kind: "intake.conversation",
        subject_id: conversation,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| IntakeError::Audit)?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some((stop, queued)))
}

/// Asks the name-and-address question (INT-027, D-238): the last intake question, only once the
/// ZIP and area check made a property row; `None` before that. It carries the who-sees-it line
/// and asks nothing about renting or owning (D-437).
///
/// # Errors
///
/// [`IntakeError::Template`], [`IntakeError::Outbox`] or [`IntakeError::Database`].
pub async fn ask_name_address(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    conversation: Uuid,
    spanish_on: bool,
) -> Result<Option<Queued>, IntakeError> {
    let Some((homeowner, true)) = models::conversation_state(db, conversation).await? else {
        return Ok(None);
    };
    if models::latest_property(db, homeowner).await?.is_none() {
        return Ok(None);
    }
    let row = models::conversation(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let language = crate::rules::language_from_code(&row.language);
    let subject = [conversation];
    let ask = queue_text(
        db,
        secret,
        msg.contact,
        (NAME_ADDRESS_TEMPLATE, language),
        &subject,
        spanish_on,
    )
    .await?;
    Ok(Some(ask))
}

/// What [`take_name_address`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Stored in the vault under this contact ID.
    Stored(Uuid),
    /// No street number was given: ask again.
    Again,
}

/// Stores the raw name-and-address answer in the vault only (INT-027, D-146): the first name and
/// the street address as a vault contact, and in `intake` only that contact's ID. The words never
/// reach the AI, the scrubber's output or a log. Pass the raw text, not the scrubbed words.
///
/// # Errors
///
/// [`IntakeError::Vault`] or [`IntakeError::Database`].
pub async fn take_name_address(
    db: &mut PgConnection,
    vault: &Vault,
    (conversation, inbox): (Uuid, Uuid),
    raw: &SecretString,
    at: Timestamp,
) -> Result<Taken, IntakeError> {
    use secrecy::ExposeSecret;
    let Some((name, address)) = split_name_address(raw.expose_secret()) else {
        return Ok(Taken::Again);
    };
    let new = NewContact {
        name: Some(SecretString::from(name)),
        phone: None,
        email: None,
        address: Some(SecretString::from(address)),
        onsite_contact: None,
    };
    let contact = store_contact(db, vault, &new)
        .await
        .map_err(|_| IntakeError::Vault)?;
    let hex = contact.simple().to_string();
    models::insert_answer(db, (conversation, inbox), (ADDRESS_CONTACT, &hex), at).await?;
    Ok(Taken::Stored(contact))
}

/// Renders and queues one text to `recipient` under `subject` (TXT-011).
async fn queue_text(
    db: &mut PgConnection,
    secret: &SecretString,
    recipient: Uuid,
    (template, language): (&'static str, Language),
    subject: &[Uuid],
    spanish_on: bool,
) -> Result<Queued, IntakeError> {
    let body = render(template, language, spanish_on, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    let text = NewText {
        template_id: template,
        subject,
        recipient,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox_id, _) = enqueue(db, secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(Queued {
        outbox_id,
        template,
    })
}

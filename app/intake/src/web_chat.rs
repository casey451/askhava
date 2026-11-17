//! The web-chat conversation (T103; INT-001, INT-020, INT-043, INT-047, CON-025, AI-021, SEC-014,
//! PRIV-001, PRIV-008; D-132, D-419, D-470, D-963, D-1951, D-3144, D-3189, D-4044).
//!
//! T489's site pages call these through `intake::api`. [`start_chat`] keeps a session by the
//! SHA256 digest of its key and its INT-047 source (the `g` code, or `web`), and records no
//! consent. Before the code is checked, [`chat_message`] only screens: a 911-list match answers
//! the INT-004 text for the chat to show (also with the texting number suspended, D-492), a
//! business-intent message `business_signup_reply` (shown, never texted, CON-025); nothing is
//! queued and no AI or adapter is called (AI-021). [`send_code`] puts the typed number in the vault
//! (`privacy::api`, which gives the contact ID) and sends only the Verify code through the
//! [`CodeStep`] trait T489's composition implements over `adapters` (D-3144, D-963).
//! [`check_code`] calls [`CodeStep::check`] only when the number's blind index is the session's
//! own contact; the right code admits SEC-014's new-request limit on the opening transaction,
//! opens the request (`first_inbox_id` is the first chat message's ID, the source stored),
//! queues the consent text as its first text and records consent (channel `web_chat`, this
//! session). After that each chat message runs the text path ([`crate::service::receive_text`])
//! with its chat message ID, and a message in a request with a job is kept through
//! `keep_description` under that ID, so a retried POST stores nothing twice (D-3189). Once the
//! session's request closes, its next message sends it back to before the code, so a new request
//! opens only through [`check_code`] (D-4056). A number on the suppression list gets
//! [`ChatError::OptedOut`] and no code (CON-013, D-4056). The page's consent text is the text the
//! switch sends (D-4057). No number is logged, stored outside the vault or put in an error.

use std::net::IpAddr;

use askhava_messaging::api::{Language, SendWindow, render, send_window};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, VaultError, find_by_phone, store_contact};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::conversation::{BUSINESS_SIGNUP_TEMPLATE, queue_text, shows_business_intent};
use crate::emergency::{Check, check};
use crate::limits::{Admission, admit_new_request};
use crate::models::{self, IntakeError};
use crate::order::{Path, Step, keep_words, order};
use crate::rules::{
    Inbound, SAFETY_TEMPLATE, Settings, consent_language, consent_template, language_code,
    wanted_language,
};
use crate::service::{Handled, Queued, TextIn, audit_action, consent_version, receive_text};

/// Wrong code tries one session may make; past it [`check_code`] calls nothing (CON-025).
pub const MAX_CODE_TRIES: i32 = 5;

/// The page's two calls to Twilio Verify, implemented by T489's composition over
/// `adapters::api::VerifyAdapter` (D-3144, D-4044); a full number is never logged.
pub trait CodeStep: Sync {
    /// Sends a code to `number` under the idempotency `key` (no personal data in it, SEC-018).
    fn send(
        &self,
        number: &SecretString,
        key: &str,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;
    /// Whether `code` is the one sent to `number`.
    fn check(
        &self,
        number: &SecretString,
        code: &str,
    ) -> impl Future<Output = Result<bool, ChatError>> + Send;
}

/// Why a web-chat call failed; no variant carries a value (SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChatError {
    /// An intake step failed.
    #[error(transparent)]
    Intake(#[from] IntakeError),
    /// No session has this key.
    #[error("web-chat session not found")]
    UnknownSession,
    /// The typed number is not a US mobile number.
    #[error("the typed number is not a usable phone number")]
    BadNumber,
    /// The vault failed.
    #[error("the vault failed")]
    Vault,
    /// The code step (Verify) failed.
    #[error("the code step failed")]
    CodeStep,
    /// The number is on the suppression list: no code goes and nothing opens until it texts
    /// START (CON-013, CON-015, D-4056).
    #[error("the number has opted out of texts")]
    OptedOut,
    /// The new-request limit could not be checked.
    #[error("the new-request limit failed")]
    Limit,
}

/// One chat message: its page-sent ID (the PRIV-008 message ID), scrubbed words and time.
#[derive(Debug, Clone, Copy)]
pub struct ChatIn<'a> {
    /// One UUID per chat message, sent again by a retried POST.
    pub message_id: Uuid,
    /// The scrubbed words, never the raw text.
    pub words: &'a str,
    /// When it arrived.
    pub received_at: Timestamp,
}

/// What [`chat_message`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAnswer {
    /// Before the code: a text for the chat to show (`template`, rendered), nothing queued.
    Shown(&'static str, String),
    /// Before the code, nothing to show: the page asks for her mobile number.
    AskNumber,
    /// After the code: the text path's answer.
    Texted(Handled),
}

/// What [`check_code`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The right code: the request and the texts queued now (the consent text first, CON-025).
    Verified(Uuid, Vec<Queued>),
    /// A wrong or missing code, or another number: nothing sent, no consent.
    Wrong,
    /// [`MAX_CODE_TRIES`] used up: nothing called.
    TooManyTries,
    /// SEC-014's new-request limit is full: nothing opened (D-1951).
    Refused,
}

/// The page's settings for one chat: its language, the intake switches and the client IP.
#[derive(Debug, Clone, Copy)]
pub struct ChatSettings {
    /// The page's language.
    pub language: Language,
    /// The intake switches the text path also reads.
    pub settings: Settings,
    /// The client IP, for SEC-014's per-IP limit.
    pub ip: Option<IpAddr>,
}

/// One session row: IDs, codes and flags only (PRIV-001).
struct Session {
    id: Uuid,
    source: String,
    first: Option<Uuid>,
    contact: Option<Uuid>,
    sent: bool,
    tries: i32,
    conversation: Option<Uuid>,
}

/// CON-025, CON-026, D-4044, D-4057: the consent text the request page shows above the chat box,
/// exactly the first text [`check_code`] queues for the same `settings`: the template
/// [`consent_template`] picks, in `language` while Spanish conversations are on and in English
/// while they are off (a Spanish page then shows the English text that will be sent).
///
/// # Errors
///
/// [`IntakeError::Template`].
pub fn web_chat_consent_text(
    language: Language,
    settings: Settings,
) -> Result<String, IntakeError> {
    let spanish = settings.spanish_conversations_on;
    let language = consent_language(language, spanish);
    render(
        consent_template(settings.safety_call_on),
        language,
        spanish,
        &[],
    )
    .map(|r| r.body)
    .map_err(|_| IntakeError::Template)
}

/// INT-047, D-457: a `g` code as stored (lower case, letters, digits, `-` and `_`, at most 32), or
/// `web` for none or one that is not a code.
#[must_use]
pub fn request_source(g_code: Option<&str>) -> String {
    let code = g_code.map(str::to_ascii_lowercase).unwrap_or_default();
    let ok = (1..=32).contains(&code.len())
        && code.starts_with(|c: char| c.is_ascii_alphanumeric())
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && code != "text";
    if ok { code } else { "web".to_owned() }
}

/// INT-001, INT-047, CON-025: starts (or finds) the session of `key` from `g_code`'s link;
/// records no consent.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn start_chat(
    db: &mut PgConnection,
    key: &SecretString,
    g_code: Option<&str>,
    now: Timestamp,
) -> Result<Uuid, IntakeError> {
    sqlx::query_scalar!(
        "INSERT INTO intake.web_chat_session (key_digest, request_source, started_at) \
         VALUES ($1, $2, $3) ON CONFLICT (key_digest) \
         DO UPDATE SET key_digest = EXCLUDED.key_digest RETURNING id",
        digest(key),
        request_source(g_code),
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)
}

fn digest(key: &SecretString) -> Vec<u8> {
    Sha256::digest(key.expose_secret().as_bytes()).to_vec()
}

async fn session(db: &mut PgConnection, key: &SecretString) -> Result<Session, ChatError> {
    let row = sqlx::query!(
        "SELECT id, request_source, first_message_id, contact_id, wrong_tries, conversation_id, \
         code_sent_at IS NOT NULL AS \"sent!\" FROM intake.web_chat_session WHERE key_digest = $1",
        digest(key)
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?
    .ok_or(ChatError::UnknownSession)?;
    Ok(Session {
        id: row.id,
        source: row.request_source,
        first: row.first_message_id,
        contact: row.contact_id,
        sent: row.sent,
        tries: row.wrong_tries,
        conversation: row.conversation_id,
    })
}

/// INT-002, INT-020, CON-025, AI-021, PRIV-008: one chat message. Before the code it is screened
/// only ([`ChatAnswer::Shown`] or [`ChatAnswer::AskNumber`]); after it, it runs the text path
/// and, in a request with a job, is kept under its message ID.
///
/// # Errors
///
/// [`ChatError::UnknownSession`], or the text path's errors.
pub async fn chat_message(
    db: &mut PgConnection,
    secret: &SecretString,
    key: &SecretString,
    msg: &ChatIn<'_>,
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    let s = session(db, key).await?;
    let (Some(contact), Some(request)) = (s.contact, s.conversation) else {
        first_message(db, s.id, msg.message_id).await?;
        return screen(msg.words, settings).map_err(ChatError::from);
    };
    if models::open_conversation(db, contact).await? != Some(request) {
        start_again(db, s.id, msg.message_id).await?;
        return screen(msg.words, settings).map_err(ChatError::from);
    }
    let inbound = Inbound {
        words: msg.words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: msg.message_id,
        contact,
        inbound,
        received_at: msg.received_at,
    };
    let handled = receive_text(db, secret, &text, settings).await?;
    if let Some(job) = models::conversation(db, request)
        .await?
        .and_then(|c| c.job_id)
    {
        let words = (msg.words, msg.received_at);
        keep_words(db, (request, job), msg.message_id, words).await?;
    }
    Ok(ChatAnswer::Texted(handled))
}

/// SEC-014, INT-047, CON-005, D-1951, D-4056: the session's request has closed, so the session
/// goes back to before the code (no contact, no code, no tries, no request, `message` its first
/// message): a new request opens only through [`check_code`], under the new-request limit, with
/// the session's source and a `web_chat` consent record, never through the text path.
async fn start_again(db: &mut PgConnection, id: Uuid, message: Uuid) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET first_message_id = $2, contact_id = NULL, \
         code_sent_at = NULL, wrong_tries = 0, verified_at = NULL, conversation_id = NULL \
         WHERE id = $1",
        id,
        message
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The pre-code screen (INT-002, INT-004, INT-020): the stem match first, then the phrase list.
fn screen(words: &str, settings: Settings) -> Result<ChatAnswer, IntakeError> {
    let template = match check(words, false) {
        Check::Match(_) => SAFETY_TEMPLATE,
        Check::Clear | Check::BeepQuestion if shows_business_intent(words) => {
            BUSINESS_SIGNUP_TEMPLATE
        }
        Check::Clear | Check::BeepQuestion => return Ok(ChatAnswer::AskNumber),
    };
    let wanted = wanted_language(words, None);
    let language = consent_language(wanted, settings.spanish_conversations_on);
    let language = if template == SAFETY_TEMPLATE {
        wanted
    } else {
        language
    };
    let body = render(template, language, settings.spanish_conversations_on, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    Ok(ChatAnswer::Shown(template, body))
}

async fn first_message(db: &mut PgConnection, id: Uuid, message: Uuid) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET first_message_id = COALESCE(first_message_id, $2) \
         WHERE id = $1",
        id,
        message
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// CON-025, TXT-038, PRIV-001: the typed `number` goes into the vault (or is found there by its
/// blind index) and gets only the Verify code; the session keeps the contact ID alone. A
/// verified session sends nothing more.
///
/// # Errors
///
/// [`ChatError::BadNumber`], [`ChatError::Vault`], [`ChatError::CodeStep`] or a database error.
pub async fn send_code<C: CodeStep>(
    db: &mut PgConnection,
    (vault, code): (&Vault, &C),
    key: &SecretString,
    number: &SecretString,
    now: Timestamp,
) -> Result<bool, ChatError> {
    let s = session(db, key).await?;
    if s.conversation.is_some() {
        return Ok(false);
    }
    let contact = contact_of(db, vault, number).await?;
    if opted_out(db, vault, contact, now).await? {
        return Err(ChatError::OptedOut);
    }
    sqlx::query!(
        "UPDATE intake.web_chat_session SET contact_id = $2, code_sent_at = $3 WHERE id = $1",
        s.id,
        contact,
        now.to_sqlx() as _
    )
    .execute(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let idempotency = format!("web-chat-{}-{}", s.id.simple(), now.as_second());
    code.send(number, &idempotency).await?;
    Ok(true)
}

async fn contact_of(
    db: &mut PgConnection,
    vault: &Vault,
    number: &SecretString,
) -> Result<Uuid, ChatError> {
    let vault_err = |e: VaultError| match e {
        VaultError::BadPhone => ChatError::BadNumber,
        _ => ChatError::Vault,
    };
    if let Some(id) = find_by_phone(db, vault, number.expose_secret())
        .await
        .map_err(vault_err)?
    {
        return Ok(id);
    }
    let new = NewContact {
        phone: Some(number.clone()),
        ..NewContact::default()
    };
    store_contact(db, vault, &new).await.map_err(vault_err)
}

/// CON-025, TXT-038, SEC-014, D-1951, D-4044: checks the typed `code` for `number`. A missing
/// code, a session with no code sent, or a number whose blind index is not the session's contact
/// is a wrong try with nothing called; the right code opens the request ([`open_verified`]).
///
/// # Errors
///
/// [`ChatError::CodeStep`], [`ChatError::Limit`], [`ChatError::Vault`] or an intake error.
pub async fn check_code<C: CodeStep>(
    db: &mut PgConnection,
    (secret, vault, code): (&SecretString, &Vault, &C),
    key: &SecretString,
    (number, typed): (&SecretString, &str),
    (chat, now): (ChatSettings, Timestamp),
) -> Result<Checked, ChatError> {
    let s = session(db, key).await?;
    if let Some(id) = s.conversation {
        return Ok(Checked::Verified(id, Vec::new()));
    }
    if s.tries >= MAX_CODE_TRIES {
        return Ok(Checked::TooManyTries);
    }
    if let Some(contact) = s.contact
        && opted_out(db, vault, contact, now).await?
    {
        return Err(ChatError::OptedOut);
    }
    let typed = typed.trim();
    let same = match find_by_phone(db, vault, number.expose_secret()).await {
        Ok(found) => found.is_some() && found == s.contact,
        Err(VaultError::BadPhone) => false,
        Err(_) => return Err(ChatError::Vault),
    };
    if !s.sent || typed.is_empty() || !same || !code.check(number, typed).await? {
        wrong_try(db, s.id).await?;
        return Ok(Checked::Wrong);
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let done = open_verified(&mut tx, secret, &s, chat, now).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(done)
}

/// CON-013, CON-015, D-4056: whether `contact`'s number is on the suppression list.
async fn opted_out(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    now: Timestamp,
) -> Result<bool, ChatError> {
    let window = send_window(db, vault, contact, now)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(window == SendWindow::OptedOut)
}

async fn wrong_try(db: &mut PgConnection, id: Uuid) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET wrong_tries = wrong_tries + 1 WHERE id = $1",
        id
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The right code, in one transaction: SEC-014's limit first (a refusal opens nothing), then the
/// request (or her open one, with no second consent text, CON-027), its source, the consent text
/// and the consent record (channel `web_chat`, this session, the text's version; CON-005).
async fn open_verified(
    tx: &mut PgConnection,
    secret: &SecretString,
    s: &Session,
    chat: ChatSettings,
    now: Timestamp,
) -> Result<Checked, ChatError> {
    let contact = s.contact.ok_or(IntakeError::Database)?;
    let admitted = admit_new_request(&mut *tx, (contact, chat.ip), now)
        .await
        .map_err(|_| ChatError::Limit)?;
    if admitted != Admission::Admitted {
        return Ok(Checked::Refused);
    }
    let settings = chat.settings;
    let template = consent_template(settings.safety_call_on);
    let language = consent_language(chat.language, settings.spanish_conversations_on);
    let homeowner = models::homeowner_for(tx, contact, now).await?;
    let first = (homeowner, s.first.unwrap_or(s.id));
    let names = (language_code(language), template);
    let Some(id) = models::open(tx, first, names, now).await? else {
        let open = models::open_conversation(tx, contact).await?;
        let id = open.ok_or(IntakeError::Database)?;
        verified(tx, s.id, id, now).await?;
        return Ok(Checked::Verified(id, Vec::new()));
    };
    let text = (template, language);
    let spanish = settings.spanish_conversations_on;
    let queued = queue_text(tx, secret, contact, text, &[id], spanish).await?;
    let body = render(template, language, spanish, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    let version = consent_version(template, language, &body);
    record(tx, (homeowner, id, s), &version, now).await?;
    verified(tx, s.id, id, now).await?;
    audit_action(tx, Actor::Homeowner(homeowner), "intake.consent_sent", id).await?;
    Ok(Checked::Verified(id, vec![queued]))
}

/// The request's INT-047 source and its CON-005 consent record (channel `web_chat`).
async fn record(
    tx: &mut PgConnection,
    (homeowner, id, s): (Uuid, Uuid, &Session),
    version: &str,
    now: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.conversation SET request_source = $2 WHERE id = $1",
        id,
        s.source
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| IntakeError::Database)?;
    sqlx::query!(
        "INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, \
         web_chat_session, consent_version, consented_at) VALUES ($1, $2, 'web_chat', $3, $4, $5)",
        homeowner,
        id,
        s.id,
        version,
        now.to_sqlx() as _
    )
    .execute(tx)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

async fn verified(
    tx: &mut PgConnection,
    s: Uuid,
    id: Uuid,
    now: Timestamp,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET verified_at = $2, conversation_id = $3 WHERE id = $1",
        s,
        now.to_sqlx() as _,
        id
    )
    .execute(tx)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// One step of INT-043's web-chat column: steps 1 to 3 shown on the page, CON-025's code, then
/// the text path's steps from 4 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatStep {
    /// 1: chat started (CON-025).
    ChatStarted,
    /// 2: the 911 check, its INT-004 text shown on the page (INT-002).
    SafetyCheck,
    /// 3: the consent text shown above the chat box (CON-031).
    ConsentShown,
    /// CON-025: the number and its Verify code, then the consent text as the first text.
    CodeChecked,
    /// 4 to 13: as the path the chat is on.
    Then(Step),
}

/// INT-043, D-446: the web-chat column of the "Intake order" table for `path` (the path it's on).
#[must_use]
pub fn web_chat_order(path: Path, emergency: bool) -> Vec<ChatStep> {
    let page = [
        ChatStep::ChatStarted,
        ChatStep::SafetyCheck,
        ChatStep::ConsentShown,
        ChatStep::CodeChecked,
    ];
    let shown = [
        Step::FirstText,
        Step::SafetyCheck,
        Step::SafetyText,
        Step::FirstReply,
    ];
    let rest = order(path, emergency)
        .into_iter()
        .filter(|s| !shown.contains(s));
    page.into_iter().chain(rest.map(ChatStep::Then)).collect()
}

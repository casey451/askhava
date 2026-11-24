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
//!
//! T645 (INT-038, CON-008, CON-010, CON-022, CON-035, TXT-002, TXT-041, D-4292, D-4473): after
//! the code an opt-out word or phrase lists the session's number by its stored blind-index digests
//! (`privacy::api::phone_digests`, `messaging::api::suppress_indexed`, channel `web_chat`) and
//! shows CON-009's words; nothing else runs. INT-038's stop runs on the pre-code screen (after the
//! 911 check) and after `receive_text` (`conversation::stop_if_under_18`; under 13 the session row
//! goes in the same transaction). The first message's language code is kept on the session and
//! decides the consent text's language, and TXT-041's line, in [`check_code`].
//!
//! T649 (INT-027, PRIV-005, PRIV-008, AI-003, AI-020, D-4294): [`chat_raw_message`], the
//! composition's entry, also holds the raw words, the vault and the LIST's names; while her
//! name-and-address question is open the turn goes to `replies::receive_raw_reply`, so her answer
//! lands on her vault contact and never in the kept description. [`chat_contact`] gives the
//! composition the verified session's contact for its vault-name scrub, and [`chat_opt_out`] is
//! the turn after a vault read that failed (D-2088).

use std::net::IpAddr;

use askhava_messaging::api::{
    CONFIRMATION_TEMPLATE, Detected, Language, RevokeChannel, SendWindow, detect_opt_out, render,
    send_window, suppress_indexed,
};
use askhava_ops::api::Actor;
use askhava_privacy::api::{
    NewContact, Vault, VaultError, find_by_phone, phone_digests, store_contact,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::conversation::{
    BUSINESS_SIGNUP_TEMPLATE, name_address_open, queue_text, shows_business_intent,
    stop_if_under_18,
};
use crate::emergency::{Check, check};
use crate::limits::{Admission, admit_new_request};
use crate::models::{self, IntakeError};
use crate::order::{Path, SPANISH_NOT_YET_TEMPLATE, Step, keep_words, order};
use crate::replies::{RawReply, Reply, receive_raw_reply};
use crate::rules::{
    AgeStop, Inbound, SAFETY_TEMPLATE, Settings, UNDER_18_TEMPLATE, age_stop, consent_language,
    consent_template, language_code, language_from_code, wanted_language,
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
    /// A text for the chat to show (`template`, rendered): before the code, with nothing queued;
    /// at any time, an opt-out's CON-009 confirmation (on screen, never texted, CON-035); and
    /// after the code, INT-038's reply, also queued as a text as on the text path (D-4292).
    Shown(&'static str, String),
    /// Before the code, nothing to show: the page asks for her mobile number.
    AskNumber,
    /// After the code: the text path's answer.
    Texted(Handled),
    /// After the code, while her name-and-address question is open: the reply path's answer
    /// (INT-027, D-4294), as on a text.
    Replied(Reply),
}

/// The raw words of one chat message, the vault its name-and-address answer goes to and the
/// public names a LIST reads (INT-027, TXT-031, D-4294); held only while
/// [`chat_raw_message`] runs. No `Debug`: it holds the raw text.
pub type ChatRaw<'a> = (RawReply<'a>, &'a [(Uuid, String)]);

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
    /// The first message's language code (`en` or `es`), never its words (PRIV-008, D-4292).
    language: Option<String>,
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
         language, code_sent_at IS NOT NULL AS \"sent!\" FROM intake.web_chat_session \
         WHERE key_digest = $1",
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
        language: row.language,
    })
}

/// INT-002, INT-020, INT-038, CON-008, CON-010, CON-025, CON-035, AI-021, PRIV-008, D-4292: one
/// chat message. Before the code it is screened only ([`ChatAnswer::Shown`] or
/// [`ChatAnswer::AskNumber`]) and its language code kept. After it, an opt-out word or phrase
/// lists the session's number at once ([`ChatAnswer::Shown`] with CON-009's words, nothing texted,
/// no job changed); any other message runs the text path and then INT-038's stop, in one
/// transaction, and, in a request with a job, is kept under its message ID.
///
/// # Errors
///
/// [`ChatError::UnknownSession`], [`ChatError::Vault`], or the text path's errors.
pub async fn chat_message(
    db: &mut PgConnection,
    secret: &SecretString,
    key: &SecretString,
    msg: &ChatIn<'_>,
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    chat_turn(db, (secret, key), (msg, None), settings).await
}

/// INT-027, PRIV-001, PRIV-005, D-4294, D-5387: [`chat_message`] with her raw words, the vault
/// and the LIST's public names. After the code, while her name-and-address question is open
/// (`name_address_open`), the turn goes to `replies::receive_raw_reply` as a text would (LIST
/// and CLOSE keep their paths; `reads_as_address` decides the screen): the answer lands on her
/// own vault contact and its words are never kept as the job description. The opt-out check
/// stays first and INT-038's stop runs once, inside the reply path; under 13 the session row
/// goes in the same transaction. Otherwise exactly [`chat_message`].
///
/// # Errors
///
/// As [`chat_message`], and `receive_raw_reply`'s; nothing is kept on an error.
pub async fn chat_raw_message(
    db: &mut PgConnection,
    (secret, key): (&SecretString, &SecretString),
    (msg, raw): (&ChatIn<'_>, ChatRaw<'_>),
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    chat_turn(db, (secret, key), (msg, Some(raw)), settings).await
}

/// The one body of [`chat_message`] and [`chat_raw_message`].
async fn chat_turn(
    db: &mut PgConnection,
    (secret, key): (&SecretString, &SecretString),
    (msg, raw): (&ChatIn<'_>, Option<ChatRaw<'_>>),
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    let s = session(db, key).await?;
    let (Some(contact), Some(request)) = (s.contact, s.conversation) else {
        first_message(db, s.id, msg).await?;
        return screen(msg.words, settings).map_err(ChatError::from);
    };
    if let Some(how) = detect_opt_out(msg.words) {
        return opt_out(db, (contact, &s), how, msg.received_at).await;
    }
    if models::open_conversation(db, contact).await? != Some(request) {
        start_again(db, s.id, msg).await?;
        return screen(msg.words, settings).map_err(ChatError::from);
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let answer = match raw {
        Some(raw) if name_address_open(&mut tx, contact).await? => {
            raw_turn(&mut tx, (secret, s.id, contact), (msg, raw), settings).await?
        }
        _ => text_turn(&mut tx, secret, (&s, contact, request), msg, settings).await?,
    };
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(answer)
}

/// D-2088, CON-008, D-4294: the turn of a message whose vault read failed (a value that will not
/// open, a contact not found): after the code an opt-out word or phrase still lists the
/// session's number ([`ChatAnswer::Shown`] with CON-009's words); anything else is `None` and
/// stops before the text path and the AI, keeping nothing.
///
/// # Errors
///
/// [`ChatError::UnknownSession`], [`ChatError::Vault`] or a database error.
pub async fn chat_opt_out(
    db: &mut PgConnection,
    key: &SecretString,
    msg: &ChatIn<'_>,
) -> Result<Option<ChatAnswer>, ChatError> {
    let s = session(db, key).await?;
    let (Some(contact), Some(_), Some(how)) =
        (s.contact, s.conversation, detect_opt_out(msg.words))
    else {
        return Ok(None);
    };
    opt_out(db, (contact, &s), how, msg.received_at)
        .await
        .map(Some)
}

/// AI-003, AI-020, D-4294: the vault contact of the session of `key` once its code was checked
/// (a request opened), whose vault name and stated names the composition scrubs the chat's words
/// with; `None` before the code or for no session.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn chat_contact(
    db: &mut PgConnection,
    key: &SecretString,
) -> Result<Option<Uuid>, IntakeError> {
    match session(db, key).await {
        Ok(s) => Ok(s.contact.filter(|_| s.conversation.is_some())),
        Err(ChatError::UnknownSession) => Ok(None),
        Err(_) => Err(IntakeError::Database),
    }
}

/// The chat message as the text path's inbound text from `contact`: opt-out and HELP are false,
/// since T645's opt-out check ran first (D-4292, D-4294).
fn text_in<'a>(contact: Uuid, msg: &ChatIn<'a>) -> TextIn<'a> {
    let inbound = Inbound {
        words: msg.words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: msg.message_id,
        contact,
        inbound,
        received_at: msg.received_at,
    }
}

/// INT-027, INT-038, PRIV-005, PRIV-007, PRIV-008, D-4294: the turn while her name-and-address
/// question is open: `receive_raw_reply` with the raw text and the vault, nothing kept as the
/// job description. An INT-038 stop (its `Reply::UnderAge`, or its text last in `Handled`)
/// shows INT-038's reply; under 13 the session row goes in this transaction too.
async fn raw_turn(
    tx: &mut PgConnection,
    (secret, session, contact): (&SecretString, Uuid, Uuid),
    (msg, (raw, names)): (&ChatIn<'_>, ChatRaw<'_>),
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    let text = text_in(contact, msg);
    let reply = receive_raw_reply(tx, secret, (&text, raw), (settings, names)).await?;
    let stop = match &reply {
        Reply::UnderAge(stop, _) => Some(*stop),
        Reply::Handled(h)
            if h.texts
                .last()
                .is_some_and(|q| q.template == UNDER_18_TEMPLATE) =>
        {
            age_stop(msg.words)
        }
        _ => None,
    };
    let Some(stop) = stop else {
        return Ok(ChatAnswer::Replied(reply));
    };
    if stop == AgeStop::Under13 {
        forget_session(tx, session).await?;
    }
    let (language, spanish_on) = (
        wanted_language(msg.words, None),
        settings.spanish_conversations_on,
    );
    shown(UNDER_18_TEMPLATE, language, spanish_on).map_err(ChatError::from)
}

/// After the code (INT-002, INT-038, PRIV-008, D-4142, D-4292): the text path, then INT-038's
/// stop on the request it went on, as `replies::stop_after` does after the 911 screen. A stop
/// shows INT-038's reply (its text queued by the stop); under 13 the session row goes too, in this
/// transaction, so nothing of it remains. Otherwise, in a request with a job, the words are kept.
async fn text_turn(
    tx: &mut PgConnection,
    secret: &SecretString,
    (s, contact, request): (&Session, Uuid, Uuid),
    msg: &ChatIn<'_>,
    settings: Settings,
) -> Result<ChatAnswer, ChatError> {
    let text = text_in(contact, msg);
    let handled = receive_text(tx, secret, &text, settings).await?;
    let spanish_on = settings.spanish_conversations_on;
    if let Some(conversation) = handled.conversation
        && let Some((stop, _)) =
            stop_if_under_18(tx, secret, &text, conversation, spanish_on).await?
    {
        if stop == AgeStop::Under13 {
            forget_session(tx, s.id).await?;
        }
        let language = wanted_language(msg.words, None);
        return shown(UNDER_18_TEMPLATE, language, spanish_on).map_err(ChatError::from);
    }
    if let Some(job) = models::conversation(tx, request)
        .await?
        .and_then(|c| c.job_id)
    {
        let words = (msg.words, msg.received_at);
        keep_words(tx, (request, job), msg.message_id, words).await?;
    }
    Ok(ChatAnswer::Texted(handled))
}

/// INT-038, PRIV-007, D-4726: an under-13 stop deletes the session row (its contact and language).
async fn forget_session(tx: &mut PgConnection, id: Uuid) -> Result<(), IntakeError> {
    sqlx::query!("DELETE FROM intake.web_chat_session WHERE id = $1", id)
        .execute(tx)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Database)
}

/// CON-008, CON-010, CON-022, CON-035, D-4418, D-4473: an opt-out typed after the code lists the
/// session contact's number by its stored blind-index digests (no vault, no plaintext read, no
/// contact row lock) with channel `web_chat` at `now`, and answers CON-009's words on screen,
/// never by text; no job or request changes (JOB-020). A contact with no digest lists nothing.
async fn opt_out(
    db: &mut PgConnection,
    (contact, s): (Uuid, &Session),
    how: Detected,
    now: Timestamp,
) -> Result<ChatAnswer, ChatError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let digests = phone_digests(&mut tx, contact)
        .await
        .map_err(|_| ChatError::Vault)?;
    if digests.is_empty() {
        return Err(ChatError::Vault);
    }
    suppress_indexed(
        &mut tx,
        &digests,
        (RevokeChannel::WebChat, &how.label()),
        now,
    )
    .await
    .map_err(|_| IntakeError::Outbox)?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    let kept = s.language.as_deref().map(language_from_code);
    confirmation(how, kept.unwrap_or(Language::En)).map_err(ChatError::from)
}

/// CON-009, CON-035: the opt-out confirmation's words for the page, in Spanish for a Spanish word
/// or phrase or a Spanish chat.
fn confirmation(how: Detected, language: Language) -> Result<ChatAnswer, IntakeError> {
    let spanish = how.spanish() || language == Language::Es;
    let language = if spanish { Language::Es } else { Language::En };
    shown(CONFIRMATION_TEMPLATE, language, false)
}

/// `template` rendered in `language` (as the registry allows) for the chat to show.
fn shown(
    template: &'static str,
    language: Language,
    spanish_on: bool,
) -> Result<ChatAnswer, IntakeError> {
    let body = render(template, language, spanish_on, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    Ok(ChatAnswer::Shown(template, body))
}

/// SEC-014, INT-047, CON-005, D-1951, D-4056: the session's request has closed, so the session
/// goes back to before the code (no contact, no code, no tries, no request, `message` its first
/// message, its language code kept): a new request opens only through [`check_code`], under the
/// new-request limit, with the session's source and a `web_chat` consent record, never through
/// the text path.
async fn start_again(db: &mut PgConnection, id: Uuid, msg: &ChatIn<'_>) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET first_message_id = $2, contact_id = NULL, \
         code_sent_at = NULL, wrong_tries = 0, verified_at = NULL, conversation_id = NULL, \
         language = $3 WHERE id = $1",
        id,
        msg.message_id,
        language_code(wanted_language(msg.words, None))
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| IntakeError::Database)
}

/// The pre-code screen (INT-002, INT-004, INT-020, INT-038, CON-008, CON-010, D-4292): the stem
/// match first, then an opt-out (CON-009's words, nothing listed: no number yet), a stated age
/// under 18 (INT-038's reply), then the phrase list; only a message with none asks the number.
fn screen(words: &str, settings: Settings) -> Result<ChatAnswer, IntakeError> {
    let wanted = wanted_language(words, None);
    let spanish_on = settings.spanish_conversations_on;
    if matches!(check(words, false), Check::Match(_)) {
        return shown(SAFETY_TEMPLATE, wanted, spanish_on);
    }
    if let Some(how) = detect_opt_out(words) {
        return confirmation(how, wanted);
    }
    if age_stop(words).is_some() {
        return shown(UNDER_18_TEMPLATE, wanted, spanish_on);
    }
    if shows_business_intent(words) {
        let language = consent_language(wanted, spanish_on);
        return shown(BUSINESS_SIGNUP_TEMPLATE, language, spanish_on);
    }
    Ok(ChatAnswer::AskNumber)
}

/// PRIV-008, TXT-002, D-4292: the session's first message ID and its language code (never its
/// words), both kept from the first message on.
async fn first_message(
    db: &mut PgConnection,
    id: Uuid,
    msg: &ChatIn<'_>,
) -> Result<(), IntakeError> {
    sqlx::query!(
        "UPDATE intake.web_chat_session SET first_message_id = COALESCE(first_message_id, $2), \
         language = CASE WHEN first_message_id IS NULL THEN $3 ELSE language END WHERE id = $1",
        id,
        msg.message_id,
        language_code(wanted_language(msg.words, None))
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
/// and the consent record (channel `web_chat`, this session, the text's version; CON-005). The
/// language is Spanish when the session's first message read as Spanish (TXT-002, D-4292), else
/// the page's (D-4057, D-4468); with Spanish conversations off, a Spanish first message gets
/// TXT-041's line right after the consent text, as `order::spanish_line` queues it (same subject,
/// so once per request, TXT-011).
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
    let first_spanish = s.language.as_deref() == Some("es");
    let wanted = if first_spanish {
        Language::Es
    } else {
        chat.language
    };
    let language = consent_language(wanted, settings.spanish_conversations_on);
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
    let mut queued = vec![queue_text(tx, secret, contact, text, &[id], spanish).await?];
    if !spanish && first_spanish {
        let not_yet = (SPANISH_NOT_YET_TEMPLATE, Language::Es);
        queued.push(queue_text(tx, secret, contact, not_yet, &[id], false).await?);
    }
    let body = render(template, language, spanish, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    let version = consent_version(template, language, &body);
    record(tx, (homeowner, id, s), &version, now).await?;
    verified(tx, s.id, id, now).await?;
    audit_action(tx, Actor::Homeowner(homeowner), "intake.consent_sent", id).await?;
    Ok(Checked::Verified(id, queued))
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

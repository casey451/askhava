//! The first reply to a request, the 911-list screen on every message, its consent record and the
//! terms acceptance at a pick (CON-001, CON-005, CON-007, CON-021, CON-026, CON-027, CON-031,
//! INT-002 to INT-005, INT-025, INT-026, INT-037, TERMS-001, TERMS-016, TERMS-033; rust brain R11).
//!
//! [`receive_text`] runs after `messaging` has handled dedupe, opt-outs and HELP
//! (`docs/architecture.md` section 8). The 911-list stem match ([`crate::emergency::check`]) runs
//! first on every message and alone decides the INT-004 text; the AI's reading never does
//! (INT-026). For a contact with no open request it opens one and queues, in one transaction with
//! its consent record, emergency event and audit rows, the 911 text on a match and then the
//! consent text, each under a key derived from the request (TXT-011). A reply in an open request
//! gets the 911 text on a match, the INT-037 question on a beeping alarm, or ends the SAFE hold
//! (INT-005, INT-025); nothing waits for a consent reply (CON-004, D-419). The urgency, choice and
//! release steps follow: [`ask_how_soon`], [`answer_how_soon`] (with the INT-033 night count),
//! [`answer_choice`] and [`release_emergency`], which enqueues the event `quotes` turns into cards.
//! [`on_opt_out`] runs after `messaging` has listed an opted-out number: it changes no job status
//! (CON-012, JOB-020, D-005), marks the live jobs paused by the homeowner on a sole-word CANCEL or
//! CANCELAR (CON-029) and enqueues [`OPTED_OUT_LIVE_KIND`] for the CON-028 notice (D-129, D-372).

use askhava_core::api::{NewTask, enqueue as enqueue_task};
use askhava_jobs::api::current;
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_picks::api::{
    PicksError, TextPickDone, TextPickIn, terms_line, text_pick, withdraw_paused,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::emergency::{self, Check, Choice, Hazard, Urgency, beep_is_alarm, is_safe_reply};
use crate::emergency_count;
use crate::models::{
    self, ConsentRecordRow, EmergencyEventRow, IntakeError, NewAcceptance, TermsAcceptanceRow,
};
use crate::rules::{
    CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, DISCLOSURE_TEMPLATE, Inbound, Plan, SAFETY_TEMPLATE,
    Send, Settings, Then, fold, language_code, language_from_code, plan, wanted_language,
};

/// The INT-037 question to a smoke or CO alarm said only to beep.
pub const BEEP_TEMPLATE: &str = "beep_question";
/// The stored question ID of the INT-037 answer (`alarm` or `chirp`).
const BEEP_QUESTION: &str = "emergency_beep";
/// The `intake.emergency_event.matched` code of an INT-037 question asked.
const BEEP_EVENT: &str = "beep_question";

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
    /// What the 911-list screen did with this message (INT-002, INT-025, INT-037).
    pub screened: Screened,
}

/// What the 911-list screen did with one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screened {
    /// Nothing on the list.
    Clear,
    /// A 911-list match: the INT-004 text is queued and cards wait for SAFE (INT-004, INT-005).
    Matched(Hazard),
    /// A smoke or CO alarm said only to beep: the INT-037 question is queued.
    BeepAsked,
    /// The safe reply to a 911 text that was waiting for it (INT-025): the request continues
    /// (INT-024).
    Safe,
}

/// Which question a reply answers, as the conversation knows it (INT-002, D-165).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Any question but the name-and-address one, or none.
    Other,
    /// The name-and-address question, whose answer is never checked against the 911 list.
    NameAndAddress,
}

/// Handles one inbound text as [`receive_answer`] does with [`Asked::Other`], without the consent
/// record (CON-001, CON-027, INT-002). `secret` is the `ID_SECRET`. Other crates reach only
/// [`receive_text`] and [`receive_answer`].
///
/// # Errors
///
/// [`IntakeError::Database`], [`IntakeError::Template`], [`IntakeError::Outbox`] or
/// [`IntakeError::Audit`]; nothing is kept on an error.
#[cfg(test)]
pub async fn on_text(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
) -> Result<Handled, IntakeError> {
    handle(db, secret, msg, settings, Asked::Other).await
}

/// One message being handled, with what the 911-list screen found.
struct Step<'s, 'm> {
    secret: &'s SecretString,
    msg: &'s TextIn<'m>,
    settings: Settings,
    check: Check,
}

/// The screen and first reply (INT-002, INT-003, CON-001, D-008): the 911-list stem match runs
/// first and alone decides the 911 text (the caller's `emergency` flag is replaced, INT-026); a new
/// request gets the 911 text on a match, then the consent text; a reply in an open request goes
/// to [`screen_reply`]. Opt-outs and HELP were answered by `messaging` and get nothing.
async fn handle(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
    asked: Asked,
) -> Result<Handled, IntakeError> {
    let check = emergency::check(msg.inbound.words, asked == Asked::NameAndAddress);
    let inbound = Inbound {
        emergency: matches!(check, Check::Match(_)),
        ..msg.inbound
    };
    let open = models::open_conversation(db, msg.contact).await?;
    let decided = plan(open.is_some(), &inbound, settings);
    if decided.then == Then::Nothing {
        return Ok(handled(open, Vec::new(), decided.then, Screened::Clear));
    }
    let step = Step {
        secret,
        msg,
        settings,
        check,
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let done = match open {
        None => open_request(&mut tx, &step, &decided).await?,
        Some(id) => Some(screen_reply(&mut tx, &step, id, decided.then).await?),
    };
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    if let Some(done) = done {
        return Ok(done);
    }
    // Lost the race: the winner queued the consent text, so this message is a reply in its
    // request ("is this a real person?" still gets the CON-027 line).
    let id = models::open_conversation(db, msg.contact)
        .await?
        .ok_or(IntakeError::Database)?;
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let done = screen_reply(&mut tx, &step, id, plan(true, &inbound, settings).then).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(done)
}

fn handled(conversation: Option<Uuid>, texts: Vec<Queued>, then: Then, s: Screened) -> Handled {
    Handled {
        conversation,
        texts,
        then,
        screened: s,
    }
}

/// Handles one inbound homeowner text: [`on_text`] and, when this text opened the request, its
/// consent record (this text's ID and time, channel `sms`, the consent-text version), in one
/// transaction (CON-005, D-419). A reply in an open request, an opt-out and a HELP keyword record
/// nothing, so no reply is ever a consent event. The entry point for every inbound text whose
/// open question is not the name-and-address one.
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
    receive_answer(db, secret, msg, settings, Asked::Other).await
}

/// As [`receive_text`], for a reply to question `asked`: the answer to the name-and-address
/// question is never checked against the 911 list (INT-002, D-165).
///
/// # Errors
///
/// As [`on_text`]; nothing is kept on an error.
pub async fn receive_answer(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
    asked: Asked,
) -> Result<Handled, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let handled = handle(&mut tx, secret, msg, settings, asked).await?;
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

/// Opens the request and queues the plan's texts in order, then the INT-037 question on a
/// beeping alarm, and records a 911-list match; `None` when another message opened it first, so
/// its consent text is not sent twice (CON-027).
async fn open_request(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    decided: &Plan,
) -> Result<Option<Handled>, IntakeError> {
    let msg = step.msg;
    let (template, language) = consent_of(decided);
    let homeowner = models::homeowner_for(tx, msg.contact, msg.received_at).await?;
    let ids = (homeowner, msg.inbox_id);
    let names = (language_code(language), template);
    let Some(id) = models::open(tx, ids, names, msg.received_at).await? else {
        return Ok(None);
    };
    let mut texts = Vec::with_capacity(decided.texts.len() + 1);
    for send in &decided.texts {
        let (template, language, subject) = match *send {
            Send::Safety(language) => (SAFETY_TEMPLATE, language, vec![id, msg.inbox_id]),
            Send::Consent { template, language } => (template, language, vec![id]),
        };
        texts.push(queue(tx, step, (template, language), &subject, &[]).await?);
    }
    let screened = match step.check {
        Check::Match(hazard) => record_match(tx, step, id, hazard).await?,
        Check::BeepQuestion => ask_beep(tx, step, id, language, &mut texts).await?,
        Check::Clear => Screened::Clear,
    };
    audit_action(tx, Actor::Homeowner(homeowner), "intake.consent_sent", id).await?;
    Ok(Some(handled(Some(id), texts, decided.then, screened)))
}

/// Renders and queues one text to the message's sender under `subject` (TXT-011).
async fn queue(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    (template, language): (&'static str, Language),
    subject: &[Uuid],
    values: &[(&str, &str)],
) -> Result<Queued, IntakeError> {
    let spanish_on = step.settings.spanish_conversations_on;
    let body = render(template, language, spanish_on, values)
        .map_err(|_| IntakeError::Template)?
        .body;
    let text = NewText {
        template_id: template,
        subject,
        recipient: step.msg.contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox_id, _) = enqueue(tx, step.secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    Ok(Queued {
        outbox_id,
        template,
    })
}

/// Records a 911-list match whose INT-004 text was queued now: the event (the SAFE hold,
/// INT-005) and its audit row (JOB-005, IDs only).
async fn record_match(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    conversation: Uuid,
    hazard: Hazard,
) -> Result<Screened, IntakeError> {
    let at = step.msg.received_at;
    let ids = (conversation, step.msg.inbox_id);
    models::insert_event(tx, ids, hazard.code(), Some(at), at).await?;
    audit_action(tx, Actor::System, "intake.emergency_matched", conversation).await?;
    Ok(Screened::Matched(hazard))
}

/// Queues the INT-037 question in the request's language and records that it was asked.
async fn ask_beep(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    conversation: Uuid,
    language: Language,
    texts: &mut Vec<Queued>,
) -> Result<Screened, IntakeError> {
    let subject = [conversation, step.msg.inbox_id];
    texts.push(queue(tx, step, (BEEP_TEMPLATE, language), &subject, &[]).await?);
    let at = step.msg.received_at;
    let ids = (conversation, step.msg.inbox_id);
    models::insert_event(tx, ids, BEEP_EVENT, None, at).await?;
    Ok(Screened::BeepAsked)
}

/// Screens a reply in open request `id` (INT-002, INT-025, INT-037): a 911-list match queues the
/// INT-004 text (in the message's language; its Spanish stays on); a beeping alarm gets the
/// INT-037 question; the safe reply ends a waiting SAFE hold; the answer to an open INT-037
/// question is stored, and answer 2 sends the INT-004 text and holds the cards.
async fn screen_reply(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    id: Uuid,
    then: Then,
) -> Result<Handled, IntakeError> {
    let msg = step.msg;
    let wanted = wanted_language(msg.inbound.words, msg.inbound.ai_spanish);
    let mut texts = Vec::new();
    let screened = match step.check {
        Check::Match(hazard) => match_reply(tx, step, (id, wanted), hazard, &mut texts).await?,
        Check::BeepQuestion => {
            let row = models::conversation(tx, id)
                .await?
                .ok_or(IntakeError::Database)?;
            let language = language_from_code(&row.language);
            ask_beep(tx, step, id, language, &mut texts).await?
        }
        Check::Clear => clear_reply(tx, step, id, &mut texts).await?,
    };
    Ok(handled(Some(id), texts, then, screened))
}

/// A reply with no 911-list words: the safe reply while a 911 text waits for it (INT-025), or the
/// answer to an open INT-037 question (D-361).
async fn clear_reply(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    id: Uuid,
    texts: &mut Vec<Queued>,
) -> Result<Screened, IntakeError> {
    let msg = step.msg;
    if is_safe_reply(msg.inbound.words) && models::mark_safe(tx, id, msg.received_at).await? {
        audit_action(tx, Actor::System, "intake.safe_reply", id).await?;
        return Ok(Screened::Safe);
    }
    if !beep_open(tx, id).await? {
        return Ok(Screened::Clear);
    }
    let alarm = beep_is_alarm(msg.inbound.words);
    let answer = (BEEP_QUESTION, if alarm { "alarm" } else { "chirp" });
    models::insert_answer(tx, (id, msg.inbox_id), answer, msg.received_at).await?;
    if !alarm {
        return Ok(Screened::Clear);
    }
    let wanted = wanted_language(msg.inbound.words, msg.inbound.ai_spanish);
    match_reply(tx, step, (id, wanted), Hazard::CoAlarm, texts).await
}

/// A 911-list match in a reply (INT-004, INT-005, D-982): the INT-004 text goes once per hold, so
/// while an earlier 911 text still waits for SAFE the match is recorded with no text queued and
/// the hold stays ("we're safe outside but still smell gas" keeps the cards held and gets no
/// second text); otherwise the text is queued and the new hold recorded.
async fn match_reply(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    (id, language): (Uuid, Language),
    hazard: Hazard,
    texts: &mut Vec<Queued>,
) -> Result<Screened, IntakeError> {
    if !models::release_state(tx, id).await?.waiting_safe {
        let subject = [id, step.msg.inbox_id];
        texts.push(queue(tx, step, (SAFETY_TEMPLATE, language), &subject, &[]).await?);
        return record_match(tx, step, id, hazard).await;
    }
    let at = step.msg.received_at;
    models::insert_event(tx, (id, step.msg.inbox_id), hazard.code(), None, at).await?;
    Ok(Screened::Matched(hazard))
}

/// Whether an INT-037 question was asked in `id` and not yet answered.
async fn beep_open(tx: &mut PgConnection, id: Uuid) -> Result<bool, IntakeError> {
    let asked = models::events(tx, id)
        .await?
        .iter()
        .any(|e| e.matched == BEEP_EVENT);
    Ok(asked
        && models::latest_answer(tx, id, BEEP_QUESTION)
            .await?
            .is_none())
}

/// SHA256 of `text` as 64 lower-case hex digits.
pub(crate) fn sha256_hex(text: &str) -> String {
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

/// One audit row about request `conversation` (JOB-005, PRIV-028: IDs only).
pub(crate) async fn audit_action(
    tx: &mut PgConnection,
    actor: Actor,
    action: &'static str,
    conversation: Uuid,
) -> Result<(), IntakeError> {
    let entry = AuditEntry {
        actor,
        action,
        subject_kind: "intake.conversation",
        subject_id: conversation,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Audit)
}

/// The `core.task` kind `quotes` handles: an emergency job released for cards (INT-029, D-491).
pub const EMERGENCY_RELEASED_KIND: &str = "intake.emergency_released";
/// The "How soon?" question (INT-030).
pub const HOW_SOON_TEMPLATE: &str = "how_soon";
/// The question after answer 4, "By a certain day" (INT-030, D-292).
pub const NEEDED_BY_TEMPLATE: &str = "needed_by_day";
const HOW_SOON_QUESTION: &str = "how_soon";
const CHOICE_QUESTION: &str = "emergency_choice";

/// The trade and town a request is in, for the INT-033 count.
#[derive(Debug, Clone, Copy)]
pub struct Trade<'a> {
    /// The catalog category code, such as `hvac_repair`.
    pub category: &'a str,
    /// The catalog town code.
    pub town: &'a str,
    /// The trade as the homeowner reads it, such as "HVAC".
    pub label: &'a str,
}

/// What an answer to "How soon?" or to the emergency-or-regular choice did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The texts queued now, in order.
    pub texts: Vec<Queued>,
    /// The urgency or choice read, `None` when the reply was neither (the question is the
    /// conversation's to ask again, T040).
    pub read: Option<&'static str>,
    /// Whether the job was released for cards now (INT-029).
    pub released: bool,
    /// Whether the choice is due but waits for the SAFE reply: the conversation then calls
    /// [`offer_emergency_choice`] once SAFE is in (INT-005, INT-006, D-982).
    pub choice_after_safe: bool,
}

/// Asks "How soon do you need this?" in request `conversation` (INT-030); the conversation asks it
/// right after the category check (T040).
///
/// # Errors
///
/// [`IntakeError::Database`], [`IntakeError::Template`] or [`IntakeError::Outbox`].
pub async fn ask_how_soon(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    conversation: Uuid,
    settings: Settings,
) -> Result<Queued, IntakeError> {
    let step = plain_step(secret, msg, settings);
    let language = request_language(db, conversation).await?;
    let subject = [conversation, msg.inbox_id];
    queue(db, &step, (HOW_SOON_TEMPLATE, language), &subject, &[]).await
}

/// Stores the "How soon?" answer as the urgent flag and card field (INT-030, CARD-007). Answer 4
/// asks the day, whatever came before; answer 1, or any other answer once a 911 text was answered
/// SAFE, leads to the emergency-or-regular choice ([`offer_choice`], INT-006); 2 and 3 send
/// nothing. While a 911 text still waits for SAFE the choice is never offered (INT-005, D-982):
/// `Answered::choice_after_safe` is then true and the conversation calls
/// [`offer_emergency_choice`] once the safe reply is in.
///
/// # Errors
///
/// As [`ask_how_soon`]; nothing is kept on an error.
pub async fn answer_how_soon(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    trade: &Trade<'_>,
    settings: Settings,
) -> Result<Answered, IntakeError> {
    let Some(urgency) = emergency::urgency(msg.inbound.words) else {
        return Ok(answered(Vec::new(), None, false));
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let ids = (conversation, msg.inbox_id);
    let answer = (HOW_SOON_QUESTION, urgency.code());
    models::insert_answer(&mut tx, ids, answer, msg.received_at).await?;
    let events = models::events(&mut tx, conversation).await?;
    let waiting = events
        .iter()
        .any(|e| e.text_queued_at.is_some() && e.safe_at.is_none());
    let had_911 = events.iter().any(|e| e.text_queued_at.is_some());
    let step = plain_step(secret, msg, settings);
    let wants_choice = urgency != Urgency::ByDay && (urgency == Urgency::Today || had_911);
    let texts = if urgency == Urgency::ByDay {
        let language = request_language(&mut tx, conversation).await?;
        let subject = [conversation, msg.inbox_id];
        let template = (NEEDED_BY_TEMPLATE, language);
        vec![queue(&mut tx, &step, template, &subject, &[]).await?]
    } else if wants_choice && !waiting {
        offer_choice(&mut tx, &step, conversation, trade).await?
    } else {
        Vec::new()
    };
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    let mut done = answered(texts, Some(urgency.code()), false);
    done.choice_after_safe = wants_choice && waiting;
    Ok(done)
}

/// Offers the emergency-or-regular choice (INT-006, INT-033) to a request whose "How soon?" answer
/// came while a 911 text waited for SAFE (`Answered::choice_after_safe`); the conversation calls
/// it once the safe reply is in. Nothing is sent while the hold is still on (INT-005, D-982).
///
/// # Errors
///
/// As [`ask_how_soon`]; nothing is kept on an error.
pub async fn offer_emergency_choice(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    trade: &Trade<'_>,
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if models::release_state(&mut tx, conversation)
        .await?
        .waiting_safe
    {
        return Ok(Vec::new());
    }
    let step = plain_step(secret, msg, settings);
    let texts = offer_choice(&mut tx, &step, conversation, trade).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(texts)
}

/// Queues the INT-006 choice, at night after the INT-033 count; with a night count of 0 only the
/// zero text goes and the job is stored as regular (INT-033).
async fn offer_choice(
    tx: &mut PgConnection,
    step: &Step<'_, '_>,
    conversation: Uuid,
    trade: &Trade<'_>,
) -> Result<Vec<Queued>, IntakeError> {
    let at = step.msg.received_at;
    let n = emergency_count::count(tx, trade.category, trade.town).await?;
    let offer = emergency_count::offer(at, n);
    let language = request_language(tx, conversation).await?;
    let n_text = n.to_string();
    let values = [("count", n_text.as_str()), ("trade", trade.label)];
    let subject = [conversation, step.msg.inbox_id];
    let mut texts = Vec::with_capacity(offer.templates.len());
    for template in offer.templates {
        texts.push(queue(tx, step, (template, language), &subject, &values).await?);
    }
    if !offer.offered {
        let ids = (conversation, step.msg.inbox_id);
        let answer = (CHOICE_QUESTION, Choice::Regular.code());
        models::insert_answer(tx, ids, answer, at).await?;
    }
    Ok(texts)
}

/// Stores the emergency-or-regular choice (INT-006); an emergency is released for cards at once
/// when its other conditions are met ([`release_emergency`], INT-029).
///
/// # Errors
///
/// [`IntakeError::Database`], [`IntakeError::UnknownRequest`] or [`IntakeError::Audit`].
pub async fn answer_choice(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
    conversation: Uuid,
) -> Result<Answered, IntakeError> {
    let Some(choice) = emergency::choice(msg.inbound.words) else {
        return Ok(answered(Vec::new(), None, false));
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let ids = (conversation, msg.inbox_id);
    let answer = (CHOICE_QUESTION, choice.code());
    models::insert_answer(&mut tx, ids, answer, msg.received_at).await?;
    let released = choice == Choice::Emergency
        && release_emergency(&mut tx, conversation, msg.received_at).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(answered(Vec::new(), Some(choice.code()), released))
}

/// Releases an emergency job for cards (INT-029, D-164): once consent is recorded, no 911 text
/// waits for SAFE (INT-005), the homeowner chose emergency and the ZIP and area check stored her
/// town, it enqueues one [`EMERGENCY_RELEASED_KIND`] event for `quotes`, before the trade
/// questions, photos and the name-and-address question. `true` only when released by this call;
/// the conversation calls it again once the ZIP is in.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Database`] or [`IntakeError::Audit`].
pub async fn release_emergency(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    let s = models::release_state(db, conversation).await?;
    if !(s.consented && !s.waiting_safe && s.emergency && s.area_done) {
        return Ok(false);
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let payload = serde_json::json!({ "conversation": conversation });
    let mut task = NewTask::new(EMERGENCY_RELEASED_KIND, payload, at);
    task.dedupe_key = Some(format!("{EMERGENCY_RELEASED_KIND}.{conversation}"));
    let queued = enqueue_task(&mut tx, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    if queued.is_some() {
        audit_action(
            &mut tx,
            Actor::System,
            "intake.emergency_released",
            conversation,
        )
        .await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(queued.is_some())
}

/// Whether request `conversation` has a 911 text still waiting for SAFE; while it does no job card
/// may go (INT-005). Card tasks check this as well as [`card_allowed`].
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`] or [`IntakeError::Database`].
pub async fn safe_hold(db: &mut PgConnection, conversation: Uuid) -> Result<bool, IntakeError> {
    Ok(models::release_state(db, conversation).await?.waiting_safe)
}

/// The stored "How soon?" answer code of a request, for the card field (INT-030, CARD-007).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn how_soon(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<String>, IntakeError> {
    models::latest_answer(db, conversation, HOW_SOON_QUESTION).await
}

/// The emergency events of a request, oldest first (INT-004, INT-005, INT-037).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn emergency_events(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<EmergencyEventRow>, IntakeError> {
    models::events(db, conversation).await
}

fn answered(texts: Vec<Queued>, read: Option<&'static str>, released: bool) -> Answered {
    Answered {
        texts,
        read,
        released,
        choice_after_safe: false,
    }
}

fn plain_step<'s, 'm>(
    secret: &'s SecretString,
    msg: &'s TextIn<'m>,
    settings: Settings,
) -> Step<'s, 'm> {
    Step {
        secret,
        msg,
        settings,
        check: Check::Clear,
    }
}

/// The language a request's texts use (CON-026).
async fn request_language(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Language, IntakeError> {
    let row = models::conversation(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    Ok(language_from_code(&row.language))
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
/// caller sending a card checks this first. This gates text-delivered cards only; CON-007's
/// web-chat request with no verified mobile number (no texts needed) has no consent record and
/// is the card task's to allow (T103), not this gate's.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn card_allowed(db: &mut PgConnection, conversation: Uuid) -> Result<bool, IntakeError> {
    Ok(!models::consent_records(db, conversation).await?.is_empty())
}

/// Whether vault contact `contact` may be texted as a homeowner: only someone who wrote first and
/// so has a consent record; a tenant or on-site contact never has one (CON-021). It covers
/// homeowner request texts only and ignores opt-outs: required HELP and opt-out confirmations
/// and the suppression list are decided by the send path, not by this gate.
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

/// The event an opt-out from a number with a live job enqueues (CON-028, CON-029, D-129, D-372):
/// its handlers send the "still open" line by email (T094), show it on the quote page (T052) and
/// withdraw a paused job's waiting pick (T054). Payload: `conversation`, `jobs`, `paused`, `inbox`.
pub const OPTED_OUT_LIVE_KIND: &str = "intake.opted_out_live_job";

/// What [`on_opt_out`] did for an opt-out from a number with a live job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptOutJobs {
    /// The open request.
    pub conversation: Uuid,
    /// Its live jobs (any status before a final one), none of which changed status (CON-012).
    pub jobs: Vec<Uuid>,
    /// Whether the text was a sole-word CANCEL or CANCELAR, which paused them (CON-029).
    pub paused: bool,
    /// The [`OPTED_OUT_LIVE_KIND`] task, `None` when this inbox row already queued it.
    pub task: Option<i64>,
}

/// Whether `words` is only CANCEL or CANCELAR, in any case or punctuation (CON-029, D-129).
#[must_use]
pub fn sole_cancel(words: &str) -> bool {
    matches!(fold(words).as_str(), "cancel" | "cancelar")
}

/// Handles an opt-out text `messaging` has already listed and confirmed (CON-012, CON-028, CON-029,
/// JOB-020, D-005, D-129). With no open request or no live job it does nothing (`None`). Otherwise,
/// in one transaction under the homeowner lock, it changes no job status, sends no text (the
/// confirmation stays bare, CON-009), marks every live job paused by the homeowner on a sole-word
/// CANCEL or CANCELAR, and enqueues one [`OPTED_OUT_LIVE_KIND`] task per inbox row. A text that is
/// not an opt-out does nothing.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Audit`] or [`IntakeError::Database`].
pub async fn on_opt_out(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
) -> Result<Option<OptOutJobs>, IntakeError> {
    let found = models::open_conversation(db, msg.contact).await?;
    let Some(conversation) = found.filter(|_| msg.inbound.opt_out) else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let jobs = live_jobs(&mut tx, conversation).await?;
    if jobs.is_empty() {
        return Ok(None);
    }
    let paused = sole_cancel(msg.inbound.words);
    if paused {
        let mut inserted = false;
        for job in &jobs {
            let ids = (conversation, *job);
            inserted |= models::insert_pause(&mut tx, ids, msg.inbox_id, msg.received_at).await?;
        }
        let actor = Actor::Homeowner(homeowner);
        if inserted {
            audit_action(&mut tx, actor, "intake.paused_by_homeowner", conversation).await?;
        }
        withdraw_paused(&mut tx, &jobs, actor, msg.received_at)
            .await
            .map_err(|_| IntakeError::Database)?;
    }
    let payload = serde_json::json!({
        "conversation": conversation, "jobs": jobs, "paused": paused, "inbox": msg.inbox_id,
    });
    let mut task = NewTask::new(OPTED_OUT_LIVE_KIND, payload, msg.received_at);
    task.dedupe_key = Some(format!("{OPTED_OUT_LIVE_KIND}.{}", msg.inbox_id));
    let task = enqueue_task(&mut tx, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some(OptOutJobs {
        conversation,
        jobs,
        paused,
        task,
    }))
}

/// The request's jobs before a final status (CON-028's "live job").
async fn live_jobs(db: &mut PgConnection, conversation: Uuid) -> Result<Vec<Uuid>, IntakeError> {
    let mut live = Vec::new();
    for job in models::request_jobs(db, conversation).await? {
        let status = current(db, job).await.map_err(|_| IntakeError::Job)?.status;
        if !status.is_final() {
            live.push(job);
        }
    }
    Ok(live)
}

/// Whether job `job` is paused by the homeowner (CON-029): while it is, no new pick, Confirm,
/// charge or release happens for it; quotes still arrive and timers keep running.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn paused_by_homeowner(db: &mut PgConnection, job: Uuid) -> Result<bool, IntakeError> {
    models::is_paused(db, job).await
}

/// The published homeowner Terms a text pick accepts: its version and exact text (TERMS-016).
#[derive(Debug, Clone, Copy)]
pub struct PublishedTerms<'a> {
    /// The published version.
    pub version: &'a str,
    /// The exact published text; only its SHA256 is stored.
    pub text: &'a str,
}

/// Handles a "<number> YES" text from the homeowner's own phone (PICK-015, PICK-016, PICK-031,
/// PICK-032, PICK-036, CON-029): under the homeowner lock, `picks` plans and acts on her open
/// request's live jobs, each with its CANCEL pause, so a paused job takes no new pick; a pick
/// stores one terms acceptance with channel `text_pick` and this inbound text (TERMS-016,
/// TERMS-033). `None` for a text that is not a pick, an opt-out or HELP, a number with no open
/// request (another phone gets nothing), or a pick the job moved away from.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Template`], [`IntakeError::Outbox`],
/// [`IntakeError::Audit`] or [`IntakeError::Database`]; nothing is kept on an error.
pub async fn answer_text_pick(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (settings, terms): (Settings, &PublishedTerms<'_>),
) -> Result<Option<TextPickDone>, IntakeError> {
    let found = models::open_conversation(db, msg.contact).await?;
    let in_scope = !msg.inbound.opt_out && !msg.inbound.help;
    let Some(conversation) = found.filter(|_| in_scope) else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let mut jobs = Vec::new();
    for job in live_jobs(&mut tx, conversation).await? {
        jobs.push((job, models::is_paused(&mut tx, job).await?));
    }
    let language = request_language(&mut tx, conversation).await?;
    let input = TextPickIn {
        contact: msg.contact,
        inbox: msg.inbox_id,
        words: msg.inbound.words,
        jobs: &jobs,
        language: (language, settings.spanish_conversations_on),
        actor: Actor::Homeowner(homeowner),
        at: msg.received_at,
    };
    let done = match text_pick(&mut tx, secret, &input).await {
        Ok(Some(done)) => done,
        Ok(None) | Err(PicksError::MovedOn) => return Ok(None),
        Err(e) => return Err(pick_error(&e)),
    };
    if let Some(pick) = done.pick {
        let acceptance = TermsAcceptance {
            conversation,
            pick,
            document: "homeowner_terms",
            version: terms.version,
            published_text: terms.text,
            language,
            channel: TermsChannel::TextPick {
                inbox_id: msg.inbox_id,
            },
            assent_line: terms_line(language),
            accepted_at: msg.received_at,
        };
        record_terms_acceptance(&mut tx, &acceptance).await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some(done))
}

/// The intake error for a `picks` failure.
fn pick_error(e: &PicksError) -> IntakeError {
    match e {
        PicksError::Job | PicksError::Quotes | PicksError::MovedOn => IntakeError::Job,
        PicksError::Template => IntakeError::Template,
        PicksError::Outbox => IntakeError::Outbox,
        PicksError::Audit => IntakeError::Audit,
        PicksError::Database | PicksError::Verify => IntakeError::Database,
    }
}

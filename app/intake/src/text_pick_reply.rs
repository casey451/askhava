//! A homeowner's "<number> YES" text from her own phone in an open request (PICK-015, PICK-016,
//! PICK-031, TERMS-016, INT-002, D-4727, D-5732).
//!
//! `replies::in_request` calls [`text_pick_reply`] after the close by text and before the
//! name-and-address answer, `receive_text` and the order dispatcher; a pick ends the reply as
//! `Reply::Picked`, `None` falls through unchanged. "From her phone" is read from the data: the
//! text's ID is a `messaging.inbox` row; web chat's page-sent IDs are not, so web chat never
//! picks. A 911-list match never picks (`receive_text` keeps the only 911 screen, INT-002). The
//! pick is `service::answer_text_pick` (its `begin()` a savepoint in the reply's transaction),
//! logged by `picks` with channel `text`, its terms acceptance with channel `text_pick` (D-5646).
//!
//! [`answer_opted_out_pick`] (T741, D-4727, D-5670) is the same text from a number on the
//! suppression list: `server`'s opt-out screen hands it here, and `picks::api::opted_out_yes`
//! stores a pick with channel `page` and queues no text (PICK-030, CON-013, D-1614).

use askhava_core::api::homeowner_terms;
use askhava_messaging::api::inbox_message;
use askhava_ops::api::Actor;
use askhava_picks::api::rules::read_text_pick;
use askhava_picks::api::{OptedOutYes, PicksError, TextPickDone, opted_out_yes};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::emergency::{Check, check};
use crate::job_texts::paused_under_lock;
use crate::models::{self, IntakeError};
use crate::rules::Settings;
use crate::service::{PublishedTerms, TextIn, answer_text_pick, live_jobs};

/// Picks by text in open request `conversation` (PICK-015, PICK-016, PICK-031, TERMS-016,
/// INT-002, D-4727): when `msg` is not an opt-out or HELP, has no 911-list match (neither the
/// reader's flag nor the stem list), reads as "<number> YES", "<number> SÍ" or "<number> SI"
/// (`picks::api::read_text_pick`, "2 yes please" too), came in as a text from her phone (an inbox
/// row) and the request has a live job, `answer_text_pick` answers it with the published Terms:
/// its texts (the pick reply, "Which job?", the Terms line first, a status) and the pick, if any.
/// `None` otherwise, and when `answer_text_pick` gives `None` (a pick the job moved away from).
///
/// # Errors
///
/// [`IntakeError::Database`] when the inbox read fails, and `answer_text_pick`'s errors; nothing
/// is kept on an error.
pub(crate) async fn text_pick_reply(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (conversation, settings): (Uuid, Settings),
) -> Result<Option<TextPickDone>, IntakeError> {
    let inbound = msg.inbound;
    let screened = inbound.opt_out || inbound.help || inbound.emergency;
    if screened
        || check(inbound.words, false) != Check::Clear
        || read_text_pick(inbound.words).is_none()
    {
        return Ok(None);
    }
    let from_phone = inbox_message(tx, msg.inbox_id)
        .await
        .map_err(|_| IntakeError::Database)?
        .is_some();
    if !from_phone || live_jobs(tx, conversation).await?.is_empty() {
        return Ok(None);
    }
    let (version, text) = homeowner_terms();
    let terms = PublishedTerms { version, text };
    answer_text_pick(tx, secret, msg, (settings, &terms)).await
}

/// PICK-030, CON-013, D-050, D-478, D-1614, D-4727 (T741): a "<number> YES" text from opted-out
/// homeowner `contact`'s own phone, inside the delivery's transaction. Her open request's live
/// jobs are read under the homeowner lock, each pause with [`paused_under_lock`] (CON-029,
/// D-1605), then `picks::api::opted_out_yes` acts as `Actor::Homeowner`: `Picked` and `Same`
/// store the pick with channel `page`, `TermsFirst` and `Nothing` store nothing. No text is
/// queued on any branch (CON-013). `None` with no open request, words that are not a text pick,
/// or a pick the job moved away from (rolled back).
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Job`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`]; nothing is kept on an error.
pub async fn answer_opted_out_pick(
    tx: &mut PgConnection,
    contact: Uuid,
    words: &str,
    at: Timestamp,
) -> Result<Option<OptedOutYes>, IntakeError> {
    if read_text_pick(words).is_none() {
        return Ok(None);
    }
    let Some(conversation) = models::open_conversation(tx, contact).await? else {
        return Ok(None);
    };
    let mut savepoint = tx.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut savepoint, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let mut jobs = Vec::new();
    for job in live_jobs(&mut savepoint, conversation).await? {
        let paused = paused_under_lock(&mut savepoint, conversation, job).await?;
        jobs.push((job, paused));
    }
    let actor = (Actor::Homeowner(homeowner), at);
    let done = match opted_out_yes(&mut savepoint, words, &jobs, actor).await {
        Ok(done) => done,
        Err(PicksError::MovedOn) => return Ok(None),
        Err(PicksError::Audit) => return Err(IntakeError::Audit),
        Err(PicksError::Database) => return Err(IntakeError::Database),
        Err(_) => return Err(IntakeError::Job),
    };
    savepoint
        .commit()
        .await
        .map_err(|_| IntakeError::Database)?;
    Ok(done)
}

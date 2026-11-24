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

use askhava_core::api::homeowner_terms;
use askhava_messaging::api::inbox_message;
use askhava_picks::api::TextPickDone;
use askhava_picks::api::rules::read_text_pick;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::emergency::{Check, check};
use crate::models::IntakeError;
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

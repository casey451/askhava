//! The dispatcher of a homeowner's inbound text inside an open request (D-999, D-1199 (4), T241).
//!
//! `server`'s inbound-text route calls [`receive_reply`] last, after `messaging` recorded the text
//! once (TXT-012), the reader scrubbed it, the opt-out check let it through (CON-008) and the
//! HELP reply was queued (CON-018). In an open request a text that is only LIST or LISTA,
//! optionally with a request number, gets `answer_list`'s lines (TXT-031); any other reply first
//! restarts each Intake job's 72-hour no-reply wait at the reply (`hear`, INT-019, D-807), then
//! goes to `receive_text` as before, so consent, the 911 screen, CLOSE, cancel and `<n> YES` keep
//! their own paths. The INT-032 "That's 2 jobs" wiring stays D-999's open gap.

use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::conversation::hear;
use crate::handlers::{answer_list, list_ask};
use crate::models::{self, IntakeError};
use crate::rules::Settings;
use crate::service::{Handled, Queued, TextIn, receive_text};

/// What [`receive_reply`] did with one inbound text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// A LIST or LISTA text in an open request: the list or which-request texts queued (TXT-031).
    Listed(Vec<Queued>),
    /// Any other text: `receive_text`'s result, after the wait restarted in an open request.
    Handled(Handled),
}

/// Handles one inbound homeowner text that `messaging` let through (TXT-031, INT-019, D-999,
/// D-1197, D-1199 (4)), in one transaction. With an open request: LIST or LISTA (alone or with
/// one request number) answered from a job holding a shown quote gives [`Reply::Listed`], `names`
/// being the businesses' public names; a LIST with no quoted job, or any other text that is not
/// an opt-out or a HELP keyword, is heard (each Intake job's no-reply wait restarts at
/// `msg.received_at`) and then handled by `receive_text`. Without one, `receive_text` alone
/// (the consent text first, CON-001).
///
/// # Errors
///
/// As `answer_list`, `hear` and `receive_text`; nothing is kept on an error.
pub async fn receive_reply(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (settings, names): (Settings, &[(Uuid, String)]),
) -> Result<Reply, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let open = models::open_conversation(&mut tx, msg.contact).await?;
    let answered_elsewhere = msg.inbound.opt_out || msg.inbound.help;
    if let Some(conversation) = open.filter(|_| !answered_elsewhere) {
        if list_ask(msg.inbound.words).is_some() {
            let spanish_on = settings.spanish_conversations_on;
            let queued = answer_list(&mut tx, secret, msg, (conversation, spanish_on), names);
            let queued = queued.await?;
            if !queued.is_empty() {
                tx.commit().await.map_err(|_| IntakeError::Database)?;
                return Ok(Reply::Listed(queued));
            }
        }
        hear(&mut tx, conversation, msg).await?;
    }
    let handled = receive_text(&mut tx, secret, msg, settings).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Reply::Handled(handled))
}

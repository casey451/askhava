//! The dispatcher of a homeowner's inbound text inside an open request (D-999, D-1199 (4), T241).
//!
//! `server`'s inbound-text route calls [`receive_raw_reply`] (as [`receive_reply`]) last, after
//! `messaging` recorded the text once (TXT-012), the reader scrubbed it, the opt-out check let it
//! through (CON-008) and the HELP reply was queued (CON-018). In an open request a text that is
//! only LIST or LISTA, optionally with a request number, gets `answer_list`'s lines (TXT-031); any
//! other reply first restarts each Intake job's 72-hour no-reply wait at the reply (`hear`,
//! INT-019, D-807), then goes to `receive_text` as before, so consent, the 911 screen, CLOSE,
//! cancel and `<n> YES` keep their own paths. The INT-032 "That's 2 jobs" wiring stays D-999's open
//! gap. Before all of that, a check-in answer on one of her Connected jobs goes to
//! `check_in_replies` in the same transaction (JOB-015, D-1840).
//!
//! The name and address (INT-027, INT-002, D-146, D-165, D-3703): [`receive_raw_reply`], the
//! route's entry, also holds the raw text and the vault. While her request's name-and-address
//! question is open (`conversation::name_address_open`), a reply that is not LIST or a CLOSE
//! goes to `take_name_address` with the raw text, never the scrubbed words, the AI or a log; it
//! is screened with `Asked::NameAndAddress` only when [`reads_as_address`] holds (a name and a
//! house number with no 911-list words around the street, D-3705), else as any reply, so a
//! hazard keeps the 911 path; a SAFE is never her answer. Any other handled reply asks step 11
//! once the request reached it (`order::ask_last_question`).

use askhava_privacy::api::Vault;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::check_in_replies::check_in_reply;
use crate::conversation::{Taken, ask_name_address, hear, name_address_open, take_name_address};
use crate::emergency::{Check, check};
use crate::handlers::{answer_list, list_ask};
use crate::models::{self, IntakeError};
use crate::order::ask_last_question;
use crate::rules::{CloseReading, Settings, close_reading, split_name_address};
use crate::service::{Asked, Handled, Queued, Screened, TextIn, receive_answer, receive_text};

/// What [`receive_reply`] did with one inbound text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// A LIST or LISTA text in an open request: the list or which-request texts queued (TXT-031).
    Listed(Vec<Queued>),
    /// Any other text: `receive_text`'s result, after the wait restarted in an open request.
    Handled(Handled),
    /// A check-in answer on this Connected job (JOB-015, JOB-016, D-1840).
    CheckedIn(Uuid),
    /// The answer to her open name-and-address question (INT-027, D-3703): what
    /// `take_name_address` did, and the screen's result, whose texts end with the one ask again
    /// on [`Taken::Again`].
    NameAddress(Taken, Handled),
}

/// The raw text of a reply and the vault its name-and-address answer is written to (INT-027,
/// D-146). No `Debug`: it holds the raw text.
#[derive(Clone, Copy)]
pub struct RawReply<'a> {
    /// The raw text as it came in, read only by `take_name_address`.
    pub raw: &'a SecretString,
    /// The contact vault.
    pub vault: &'a Vault,
}

/// Handles one inbound homeowner text that `messaging` let through (TXT-031, INT-019, D-999,
/// D-1197, D-1199 (4), JOB-015, D-1840), in one transaction. A check-in answer (DONE, NO,
/// PROBLEM and the rest) on one of her Connected jobs with a check-in due, whether or not a
/// request is open, gives [`Reply::CheckedIn`]: a PROBLEM's scrubbed words are stored before the
/// answer is applied. Otherwise, with an open request: LIST or LISTA (alone or with
/// one request number) answered from a job holding a shown quote gives [`Reply::Listed`], `names`
/// being the businesses' public names; a LIST with no quoted job or whose list `quotes` cannot
/// build (`IntakeError::Quotes`, e.g. a name missing from `names`, D-1310), or any other text
/// that is not an opt-out or a HELP keyword, is heard (each Intake job's no-reply wait restarts
/// at `msg.received_at`) and then handled by `receive_text`. Without one, `receive_text` alone
/// (the consent text first, CON-001).
///
/// # Errors
///
/// As `answer_list` (but `IntakeError::Quotes`), `hear` and `receive_text`; nothing is kept on
/// an error.
pub async fn receive_reply(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (settings, names): (Settings, &[(Uuid, String)]),
) -> Result<Reply, IntakeError> {
    reply_in(db, secret, (msg, None), (settings, names)).await
}

/// As [`receive_reply`], with the raw text and the vault, so an answer to her open
/// name-and-address question goes onto her own vault contact and a reply that reaches step 11
/// asks it once (INT-027, INT-029, INT-002, PRIV-001, D-146, D-165, D-3703). The route's entry.
///
/// # Errors
///
/// As [`receive_reply`], and `take_name_address`'s and `ask_name_address`'s; nothing is kept on
/// an error.
pub async fn receive_raw_reply(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, raw): (&TextIn<'_>, RawReply<'_>),
    (settings, names): (Settings, &[(Uuid, String)]),
) -> Result<Reply, IntakeError> {
    reply_in(db, secret, (msg, Some(raw)), (settings, names)).await
}

/// The one body of [`receive_reply`] and [`receive_raw_reply`], in one transaction.
async fn reply_in(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, raw): (&TextIn<'_>, Option<RawReply<'_>>),
    (settings, names): (Settings, &[(Uuid, String)]),
) -> Result<Reply, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let answered_elsewhere = msg.inbound.opt_out || msg.inbound.help;
    if !answered_elsewhere && let Some(job) = check_in_reply(&mut tx, msg).await? {
        tx.commit().await.map_err(|_| IntakeError::Database)?;
        return Ok(Reply::CheckedIn(job));
    }
    let open = models::open_conversation(&mut tx, msg.contact).await?;
    let mut step_11 = None;
    if let Some(conversation) = open.filter(|_| !answered_elsewhere) {
        if list_ask(msg.inbound.words).is_some() {
            let spanish_on = settings.spanish_conversations_on;
            let queued = listed(&mut tx, secret, msg, (conversation, spanish_on), names).await?;
            if !queued.is_empty() {
                tx.commit().await.map_err(|_| IntakeError::Database)?;
                return Ok(Reply::Listed(queued));
            }
        }
        hear(&mut tx, conversation, msg).await?;
        if !closes(msg.inbound.words) {
            if let Some(raw) = raw.filter(|_| list_ask(msg.inbound.words).is_none())
                && name_address_open(&mut tx, msg.contact).await?
            {
                let reply = answer_name_address(&mut tx, secret, (msg, raw), settings).await?;
                tx.commit().await.map_err(|_| IntakeError::Database)?;
                return Ok(reply);
            }
            step_11 = Some(conversation);
        }
    }
    let mut handled = receive_text(&mut tx, secret, msg, settings).await?;
    if let Some(conversation) = step_11.filter(|_| moves_on(&handled)) {
        let spanish_on = settings.spanish_conversations_on;
        let ask = ask_last_question(&mut tx, secret, (msg, conversation), spanish_on).await?;
        handled.texts.extend(ask);
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Reply::Handled(handled))
}

/// A CLOSE text, a "Close which?" answer or a cancel intent keeps its own path (JOB-019, D-3703).
fn closes(words: &str) -> bool {
    matches!(
        close_reading(words, &[]),
        CloseReading::Close(_) | CloseReading::Intent
    )
}

/// The reply opened no 911 text or alarm question, so intake may go on (INT-004, INT-037).
fn moves_on(handled: &Handled) -> bool {
    matches!(handled.screened, Screened::Clear | Screened::Safe)
}

/// At most this many words after the house number are taken as the street whatever they say,
/// as `emergency`'s address strip takes after a house number (D-165, D-982, D-3705).
const STREET_WORDS: usize = 3;

/// Whether raw text `raw` reads as her answer to the name-and-address question, so the 911-list
/// screen skips it (INT-002, INT-027, D-165, D-3705): a name, then a house number of digits only,
/// the name with no 911-list match, and the street (from the house number on) either with no match
/// once its street address is removed or at most [`STREET_WORDS`] words after the number ("Testa 21
/// smoke tree"). "Smoke in 2 rooms", "Fire at 12 Sample St" and "Testa 12 Sample St gas leak" are
/// not her answer, so they are screened as any reply.
#[must_use]
pub fn reads_as_address(raw: &str) -> bool {
    let Some((name, street)) = split_name_address(raw) else {
        return false;
    };
    let mut words = street.split_whitespace();
    let number = words
        .next()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .is_some_and(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()));
    let short = words.count() <= STREET_WORDS;
    number && clear(&name) && (short || clear(&street))
}

/// No 911-list match and no alarm question in `words` (INT-002, INT-037).
fn clear(words: &str) -> bool {
    check(words, false) == Check::Clear
}

/// The reply to her open name-and-address question (INT-027, INT-002, INT-025, D-146, D-165,
/// D-3703, D-3705): screened with `Asked::NameAndAddress` when [`reads_as_address`] holds, else
/// as any reply, so a hazard keeps its 911 text and ends here. Only a clear reply is her answer:
/// its raw text goes to `take_name_address` (which queues T255's `address_arrived` itself) and
/// [`Taken::Again`] asks once more; [`Taken::Stored`] and [`Taken::Ended`] send nothing. A SAFE
/// after a 911 text is never her answer: the question is asked once more.
async fn answer_name_address(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, raw): (&TextIn<'_>, RawReply<'_>),
    settings: Settings,
) -> Result<Reply, IntakeError> {
    let asked = if reads_as_address(raw.raw.expose_secret()) {
        Asked::NameAndAddress
    } else {
        Asked::Other
    };
    let mut handled = receive_answer(tx, secret, msg, settings, asked).await?;
    let Some(conversation) = handled.conversation else {
        return Ok(Reply::Handled(handled));
    };
    let spanish_on = settings.spanish_conversations_on;
    match handled.screened {
        Screened::Clear => {}
        Screened::Safe => {
            let again = ask_name_address(tx, secret, msg, conversation, spanish_on).await?;
            handled.texts.extend(again);
            return Ok(Reply::Handled(handled));
        }
        Screened::Matched(_) | Screened::BeepAsked => return Ok(Reply::Handled(handled)),
    }
    let ids = (conversation, msg.inbox_id);
    let taken = take_name_address(tx, raw.vault, ids, raw.raw, msg.received_at).await?;
    if taken == Taken::Again {
        let again = ask_name_address(tx, secret, msg, conversation, spanish_on).await?;
        handled.texts.extend(again);
    }
    Ok(Reply::NameAddress(taken, handled))
}

/// `answer_list` inside a savepoint (D-1310): when `quotes` cannot build the list (a business
/// with no public name in `names`, say) the savepoint is rolled back and nothing is listed, so the
/// text is heard and handled as any other reply instead of being lost.
async fn listed(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    request: (Uuid, bool),
    names: &[(Uuid, String)],
) -> Result<Vec<Queued>, IntakeError> {
    let mut point = db.begin().await.map_err(|_| IntakeError::Database)?;
    match answer_list(&mut point, secret, msg, request, names).await {
        Ok(queued) => {
            point.commit().await.map_err(|_| IntakeError::Database)?;
            Ok(queued)
        }
        Err(IntakeError::Quotes) => {
            point.rollback().await.map_err(|_| IntakeError::Database)?;
            Ok(Vec::new())
        }
        Err(e) => Err(e),
    }
}

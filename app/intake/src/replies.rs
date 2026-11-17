//! The dispatcher of a homeowner's inbound text inside an open request (D-999, D-1199 (4), T241).
//!
//! `server`'s inbound-text route calls [`receive_raw_reply`] (as [`receive_reply`]) last, after
//! `messaging` recorded the text once (TXT-012), the reader scrubbed it, the opt-out check let it
//! through (CON-008) and the HELP reply was queued (CON-018). In an open request a text that is
//! only LIST or LISTA, optionally with a request number, gets `answer_list`'s lines (TXT-031); any
//! other reply first restarts each Intake job's 72-hour no-reply wait at the reply (`hear`,
//! INT-019, D-807). A clear text (no 911-list match, INT-002) then goes to `answer_close`
//! (JOB-019 to JOB-023, D-4051, D-4064): a CLOSE or CERRAR text, a cancel-intent message or a lone
//! open number answering "Close which?" that closes jobs or asks which ends the reply as
//! [`Reply::Closed`]; a mere mention adds the JOB-023 line and goes on. Everything else goes to
//! `receive_text` as before, so consent, the 911 screen and `<n> YES` keep their own paths; a
//! text that is not clear never reaches `answer_close`. The INT-032 "That's 2 jobs" wiring stays D-999's open
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
//!
//! The under-18 stop (INT-038, INT-002, D-4064, D-4142): a clear text stating an age under 18 in
//! an open request is stopped (`conversation::stop_if_under_18`) before the close and
//! name-and-address steps; a text that is not clear, or a first text, is stopped after its screen,
//! so a 911 text goes first. Under 13 the stop also clears her vault contact's name and address.

use askhava_privacy::api::Vault;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::check_in_replies::check_in_reply;
use crate::closing::{Closing, answer_close};
use crate::conversation::{
    Taken, ask_name_address, hear, name_address_open, stop_if_under_18, take_name_address,
};
use crate::emergency::{Check, check};
use crate::handlers::{answer_list, list_ask};
use crate::models::{self, IntakeError};
use crate::order::ask_last_question;
use crate::rules::{AgeStop, CloseReading, Settings, close_reading, split_name_address};
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
    /// A CLOSE text, a cancel-intent message or a "Close which?" answer in an open request:
    /// `answer_close` closed these jobs or asked "Close which?" (JOB-019 to JOB-022, D-4051),
    /// with the text it queued.
    Closed(Closing, Option<Queued>),
    /// A clear text in an open request stating an age under 18 (INT-038, D-4142): the request
    /// closed with this INT-038 text, and under 13 its data and her vault contact's name and
    /// address are gone. A stop after a 911-list match or on a first text is a
    /// [`Reply::Handled`] whose texts end with the INT-038 text.
    UnderAge(AgeStop, Queued),
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

/// The one body of [`receive_reply`] and [`receive_raw_reply`], in one transaction. A text that
/// states an age under 18 (INT-038, D-4142) ends the reply with the INT-038 text and no last
/// question: a clear one in an open request as [`Reply::UnderAge`], before the close and
/// name-and-address steps; any other (a 911-list match, or a first text) after `receive_text` or
/// `answer_name_address` screened it, its texts first (INT-002).
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
    let (mut step_11, mut mention) = (None, None);
    if let Some(conversation) = open.filter(|_| !answered_elsewhere) {
        let request = (conversation, settings, names);
        let (ended, said) = in_request(&mut tx, secret, (msg, raw), request).await?;
        if let Some(reply) = ended {
            tx.commit().await.map_err(|_| IntakeError::Database)?;
            return Ok(reply);
        }
        mention = said;
        step_11 = (!closes(msg.inbound.words)).then_some(conversation);
    }
    let mut handled = receive_text(&mut tx, secret, msg, settings).await?;
    handled.texts.extend(mention);
    let spanish_on = settings.spanish_conversations_on;
    let stopped =
        !answered_elsewhere && stop_after(&mut tx, secret, msg, &mut handled, spanish_on).await?;
    if let Some(conversation) = step_11.filter(|_| !stopped && moves_on(&handled)) {
        let ask = ask_last_question(&mut tx, secret, (msg, conversation), spanish_on).await?;
        handled.texts.extend(ask);
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Reply::Handled(handled))
}

/// A text in open request `conversation` that is not an opt-out or HELP (TXT-031, INT-019,
/// INT-038, JOB-019, INT-027, D-4142): a LIST answered, the wait restarted, a clear under-18
/// text stopped, a close by text, then the name-and-address answer (a stop after its screen).
/// The first item ends the reply; the second is the JOB-023 line for a reply that goes on.
async fn in_request(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, raw): (&TextIn<'_>, Option<RawReply<'_>>),
    (conversation, settings, names): (Uuid, Settings, &[(Uuid, String)]),
) -> Result<(Option<Reply>, Option<Queued>), IntakeError> {
    let spanish_on = settings.spanish_conversations_on;
    if list_ask(msg.inbound.words).is_some() {
        let queued = listed(tx, secret, msg, (conversation, spanish_on), names).await?;
        if !queued.is_empty() {
            return Ok((Some(Reply::Listed(queued)), None));
        }
    }
    hear(tx, conversation, msg).await?;
    if clear(msg.inbound.words)
        && let Some((stop, queued)) =
            stop_if_under_18(tx, secret, msg, conversation, spanish_on).await?
    {
        return Ok((Some(Reply::UnderAge(stop, queued)), None));
    }
    let (ended, said) = by_text(tx, secret, msg).await?;
    if ended.is_some() {
        return Ok((ended, None));
    }
    if !closes(msg.inbound.words)
        && let Some(raw) = raw.filter(|_| list_ask(msg.inbound.words).is_none())
        && name_address_open(tx, msg.contact).await?
    {
        let mut reply = answer_name_address(tx, secret, (msg, raw), settings).await?;
        if let Reply::Handled(handled) = &mut reply {
            stop_after(tx, secret, msg, handled, spanish_on).await?;
        }
        return Ok((Some(reply), None));
    }
    Ok((None, said))
}

/// INT-038 after the 911 screen (INT-002, D-4142): when `handled`'s request, the one the text
/// opened or went on, is stopped for a stated age under 18, the INT-038 text joins its texts
/// last; true when it stopped.
async fn stop_after(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    handled: &mut Handled,
    spanish_on: bool,
) -> Result<bool, IntakeError> {
    let Some(conversation) = handled.conversation else {
        return Ok(false);
    };
    let stop = stop_if_under_18(tx, secret, msg, conversation, spanish_on).await?;
    let stopped = stop.is_some();
    handled.texts.extend(stop.map(|(_, queued)| queued));
    Ok(stopped)
}

/// A clear text about closing goes to `answer_close` inside the reply's transaction (JOB-019 to
/// JOB-023, INT-002, D-4051, D-4064). `Closing::Closed` and `Closing::Which` end the reply (the
/// first item); on `Closing::Explained` its JOB-023 line (the second item) joins the reply's texts
/// and the text goes on. A text with a 911-list match never goes there: `shows_intent` reads
/// "never mind, I smell gas" as intent, and `receive_text` holds the only 911 screen.
async fn by_text(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
) -> Result<(Option<Reply>, Option<Queued>), IntakeError> {
    if !clear(msg.inbound.words) {
        return Ok((None, None));
    }
    let (closing, text) = answer_close(tx, secret, msg).await?;
    Ok(match closing {
        Closing::Closed(_) | Closing::Which(_) => (Some(Reply::Closed(closing, text)), None),
        Closing::Explained(_) | Closing::Nothing => (None, text),
    })
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

//! The one walker of INT-043's order on the text paths after the consent text (D-4360, T671).
//!
//! `replies::reply_in` calls it last, inside the reply's transaction, only for a text that nothing
//! earlier ended and that moves on (no 911 text or alarm question opened, INT-004, INT-037).
//! Under `models::lock_homeowner` it reads where the request is from stored rows only ([`Place`]:
//! the `zip` asked rows, the property stored since the request opened, the `trade_reading` rows,
//! the `trade_check` marker, the request's first job and a 911 text waiting for SAFE), and calls
//! the existing step functions; it never re-implements a step's rule.
//!
//! [`first_text`] stores up to two `trade_reading` rows, opens her first job at once ([`open_job`],
//! property `none`, no INT-009 lookup) when the text names a trade and opens a job (INT-007), keeps
//! its scrubbed words once through `order::keep_words` (PRIV-008), and asks the ZIP after the
//! consent text (CON-031). [`reply_step`] sends nothing to a contact `service::may_text` refuses
//! (CON-009) or while a 911 text waits for SAFE (INT-005). SAFE, and a reply that answered an open
//! INT-037 question (its `emergency_beep` row carries the reply's inbox ID), are never a step
//! answer: they go to the open step ([`ask_open_again`]: the ZIP question when never asked, its
//! text again with no row written when asked and her town is not stored, else nothing), so the one
//! ZIP re-ask is neither used nor renewed (D-4441). Any other reply asks the ZIP when it never was,
//! else is read by `zip_step::answer_zip`; once her town is stored the trade check tells the
//! coverage (`routing::apply` for a closed category, else `routing::tell_coverage` with
//! `quotes::api::request_coverage`'s count). With no trade read, each later reply is the trade
//! answer (the ZIP reply itself too, "86403 my sink leaks"); its job opens through [`open_job`]
//! keyed with her new property, never `begin_job`, and nothing is written or sent when it names no
//! trade. Step 11 (`order::ask_last_question`, once per request) is called last, only once step 5
//! told the coverage (the `trade_check` marker; D-3702, D-4441), and not in a turn that sent a
//! SPLIT offer ([`offered_now`]): it waits for her next reply, so SPLIT reaches [`second_job`].
//!
//! Steps 6 to 8 (T672, D-4360, D-4822): once step 5 told the coverage, [`open_step`] reads the
//! open step from the stored rows and [`ask_step`] asks it in the same turn (not in a SPLIT
//! offer's): "How soon?" (`service::ask_how_soon`), the day after answer 4, the emergency-or-regular
//! choice (`service::offer_emergency_choice`: at night the INT-033 count first, none offered at N
//! of 0), then the trade questions ([`questions`], `order::bank_turn`) and, when they are done,
//! step 11. Each later reply answers the open step ([`timing_answer`]: `service::answer_how_soon`,
//! `order::answer_day`, `order::choose_with_card` with the card it stores first, INT-029); an
//! unreadable one gets the question again. The choice is due after answer 1, or after any answer
//! but 4 when a 911 text was queued before it (INT-006); a 911 text queued later (a smell answer,
//! [`hold_911`]) holds the order until SAFE, which resumes at the open step ([`ask_open_again`]).
//! An Emergency choice stored before her town is released when the ZIP comes in ([`zip_answer`]).
//! TXT-041's Spanish line follows the first reply ([`first_text`]).
//!
//! INT-032's two jobs (T711, D-4439, corrected by T717, D-4441): a first text whose two
//! `trade_reading` rows opened the first trade's job gets the `two_jobs` text once, right after
//! the first ZIP question in the same turn ([`ask_first_zip`]): at the first text on the clear
//! path, after SAFE or the INT-037 answer on the 911 and alarm paths, never during a SAFE hold.
//! [`second_job`] runs before the step's answer: SPLIT or DIVIDIR (`routing::is_split_reply`)
//! counts only as the first `heard` reply after the turn that sent a SPLIT offer (its
//! [`SPLIT_OFFER`] marker, written beside the `trade_check` marker) and opens the offer's other
//! trade; ALSO or TAMBIÉN (`matching::starts_second`), never while the name-and-address question
//! is open, in a request whose two `trade_reading` rows came from its first text and whose second
//! trade has no job link yet, opens the second trade's job. Both open through [`open_job`] (ALSO
//! with property `none`, SPLIT keyed with her newest property; no INT-009 lookup, no INT-007 word
//! count), never `begin_job` (which refuses a one-word reply) or `routing::split_request`, answer
//! no step and get the open question again ([`ask_open_again`]); an ALSO's coverage line goes
//! through `routing::tell_coverage` at once when step 5 is done, else right after the first job's.
//! Any other ALSO or SPLIT goes on as any other reply; nothing is sent during a SAFE hold.
//!
//! Step 12 (T673, D-4360, D-4880, D-5039): `replies::answer_name_address` calls
//! [`show_card_step`] once her name and address are stored. It builds the card from her kept words
//! ([`card_in`]) and shows it through `order::show_built_card` (INT-041: the card text when it
//! fits 2 segments, else her page link), which queues the 30-minute `intake.card_timer`. It is
//! skipped, with nothing built, stored or sent, when the request has no job, step 5 never told
//! the coverage, the job was chosen as an emergency (its card went at the choice, INT-029), a 911
//! text waits for SAFE or `service::may_text` refuses; any failure after that is an error. While a
//! card row exists the next reply goes to [`card_answer`] before any other step: a shown card to
//! `summary_step::answer_card` (EDIT or SEND; any other reply sends nothing and the timer goes
//! on), and the reply after EDIT is kept through `order::keep_words` (PRIV-008) and the new card
//! shown. [`ask_open_again`] shows a shown, waiting card again ([`card_again`]).
//! When her name and address were stored during a SAFE hold, so no card was shown, SAFE shows it
//! for the first time ([`card_first`], D-5041); START never does.
//!
//! A ZIP reply, a trade answer, ALSO or SPLIT is never kept as words (INT-049). No words or ZIP
//! are logged.

use askhava_jobs::api::create;
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardFacts, CardParts, Danger, Place as Site, Timing, card_pool, description, request_coverage,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use askhava_messaging::api::render;

use askhava_quotes::api::Coverage;

use crate::conversation::{ADDRESS_CONTACT, name_address_open, queue_text};
use crate::emergency::{Hazard, Urgency};
use crate::handlers::{queue_body, schedule_no_reply};
use crate::matching::{Reading, candidates, keyword_reading, starts_second, trade_label};
use crate::models::{self, IntakeError};
use crate::order::{
    BankTurn, CardIn, NEEDED_BY, TRADE_QUESTION_TEMPLATE, answer_day, ask_last_question, bank_turn,
    choose_with_card, keep_words, show_built_card, spanish_line,
};
use crate::question_bank::starts_now;
use crate::replies::quote_link;
use crate::routing::{
    Outcome, Request, Routing, SPLIT_TEMPLATE, apply, is_split_reply, route, tell_coverage,
    trade_is_open,
};
use crate::rules::{SAFETY_TEMPLATE, Settings, Then, opens_job};
use crate::service::{
    Handled, NEEDED_BY_TEMPLATE, Queued, Screened, TextIn, Trade, answer_how_soon, ask_how_soon,
    audit_action, may_text, offer_emergency_choice, release_emergency, request_language,
};
use crate::summary_step::{AUTO_SEND_AFTER, CardStep, Shown, answer_card, card_reply};
use crate::zip_step::{
    ZIP_QUESTION_TEMPLATE, ZipPlace, ZipStep, answer_zip, ask_zip, read_zip, zip_body,
};

/// The step-marker question holding one trade the keyword reading named (codes only, at most two,
/// first-named first).
pub(crate) const TRADE_READING: &str = "trade_reading";
/// The step-marker question written once the trade check told her the coverage (INT-017).
const TRADE_CHECK: &str = "trade_check";
/// The INT-037 answer row `service::clear_reply` writes, keyed with the answering reply's inbox ID.
const BEEP_ANSWER: &str = "emergency_beep";
/// A first message's trades kept as `trade_reading` rows (INT-032: at most two jobs).
const MAX_TRADES: usize = 2;
/// The INT-032 "That's 2 jobs" text a first message naming two trades gets (T711).
pub(crate) const TWO_JOBS_TEMPLATE: &str = "two_jobs";
/// The step marker written when the trade check sent the SPLIT offer; its choice is the trade
/// SPLIT would open (INT-032, D-1020).
const SPLIT_OFFER: &str = "split_offer";
/// The row `conversation::hear` writes for each reply heard in a request, keyed with its inbox ID.
const HEARD: &str = "heard";
/// The answer row `service::answer_how_soon` writes: the INT-030 urgency code.
const HOW_SOON: &str = "how_soon";
/// The answer row of the INT-006 choice (`service::answer_choice`, or regular at a night count of 0).
const CHOICE: &str = "emergency_choice";
/// The card state `summary_step::answer_card` stores on EDIT: the next reply says what to change.
const CARD_EDIT: &str = "edit";
/// The prefix of the card state `summary_step::show_card` stores for a shown card.
const CARD_SHOWN: &str = "shown.";

/// Where request `conversation` is in the order, read from its stored rows.
struct Place {
    homeowner: Uuid,
    open: bool,
    job: Option<Uuid>,
    zip_asked: bool,
    town: Option<String>,
    zip: Option<String>,
    trades: Vec<String>,
    told: bool,
    waiting_safe: bool,
    job_trades: Vec<String>,
}

/// Reads [`Place`] for `conversation`; `None` for an unknown request.
async fn place(db: &mut PgConnection, conversation: Uuid) -> Result<Option<Place>, IntakeError> {
    sqlx::query_as!(
        Place,
        r#"SELECT c.homeowner_id AS homeowner, c.closed_at IS NULL AS "open!", c.job_id AS job,
             EXISTS (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = c.id
               AND a.question_id = 'zip') AS "zip_asked!",
             p.town AS "town?", p.zip AS "zip?",
             ARRAY(SELECT a.choice FROM intake.intake_answer a WHERE a.conversation_id = c.id
               AND a.question_id = $2 AND a.choice IS NOT NULL
               ORDER BY a.answered_at, a.id) AS "trades!: Vec<String>",
             EXISTS (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = c.id
               AND a.question_id = $3) AS "told!",
             EXISTS (SELECT 1 FROM intake.emergency_event e WHERE e.conversation_id = c.id
               AND e.text_queued_at IS NOT NULL AND e.safe_at IS NULL) AS "waiting_safe!",
             ARRAY(SELECT split_part(a.question_id, '.', 2) FROM intake.intake_answer a
               WHERE a.conversation_id = c.id AND a.question_id LIKE 'job.%')
               AS "job_trades!: Vec<String>"
           FROM intake.conversation c
           LEFT JOIN LATERAL (SELECT q.town, q.zip FROM intake.property q
             WHERE q.homeowner_id = c.homeowner_id AND q.created_at >= c.opened_at
             ORDER BY q.created_at DESC, q.id DESC LIMIT 1) p ON true
           WHERE c.id = $1"#,
        conversation,
        TRADE_READING,
        TRADE_CHECK
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// Locks the request's homeowner and reads its [`Place`]; `None` for a closed or unknown request.
async fn locked(db: &mut PgConnection, conversation: Uuid) -> Result<Option<Place>, IntakeError> {
    let Some((homeowner, true)) = models::conversation_state(db, conversation).await? else {
        return Ok(None);
    };
    models::lock_homeowner(db, homeowner).await?;
    Ok(place(db, conversation).await?.filter(|p| p.open))
}

/// CON-031, INT-043, PRIV-008, INT-007, INT-009, TXT-041: the first text of a new request, after
/// the consent text (and the 911 text or alarm question), as the module comment says: TXT-041's
/// Spanish line once, then the ZIP question. Its texts join `handled`'s.
///
/// # Errors
///
/// `ask_zip`'s, `keep_words`'s, [`IntakeError::Job`] or [`IntakeError::Database`].
pub(crate) async fn first_text(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    handled: &mut Handled,
    settings: Settings,
) -> Result<(), IntakeError> {
    let Some(conversation) = handled
        .conversation
        .filter(|_| handled.then == Then::FirstQuestion)
    else {
        return Ok(());
    };
    let Some(place) = locked(tx, conversation).await? else {
        return Ok(());
    };
    let (words, at) = (msg.inbound.words, msg.received_at);
    let trades = keyword_reading(words).trades;
    for (n, trade) in (0_i64..).zip(trades.iter().take(MAX_TRADES)) {
        let when = at + SignedDuration::from_micros(n);
        models::insert_plain(tx, conversation, (TRADE_READING, trade), when).await?;
    }
    let emergency = matches!(handled.screened, Screened::Matched(_));
    if let Some(trade) = trades.first().filter(|_| opens_job(words, emergency)) {
        let ids = (place.homeowner, conversation);
        let job = open_job(tx, ids, (trade, None), (emergency, at)).await?;
        keep_words(tx, (conversation, job), msg.inbox_id, (words, at)).await?;
    }
    let line = spanish_line(tx, secret, (msg, conversation), settings).await?;
    handled.texts.extend(line);
    if handled.screened == Screened::Clear {
        let asked = ask_first_zip(tx, secret, (msg, conversation), settings).await?;
        handled.texts.extend(asked);
    }
    Ok(())
}

/// INT-049, INT-032, CON-031, D-4441: the first ZIP question, then, when the request's first text
/// named two trades (two `trade_reading` rows) and opened the first one's job but not yet the
/// second's, the `two_jobs` text right after it, in the same turn; once, as the first ZIP question
/// is asked once.
async fn ask_first_zip(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let mut texts = vec![ask_zip(tx, secret, request, settings).await?];
    let Some(place) = place(tx, request.1).await? else {
        return Ok(texts);
    };
    let linked = |t: &String| place.job_trades.contains(t);
    if let [first, second] = place.trades.as_slice()
        && linked(first)
        && !linked(second)
    {
        let pair = (first.as_str(), second.as_str());
        texts.push(tell_two_jobs(tx, secret, request, pair, settings).await?);
    }
    Ok(texts)
}

/// INT-032, TXT-003: queues the `two_jobs` text naming the `(first, second)` trades in the
/// request's language (Spanish only while Spanish conversations are on).
async fn tell_two_jobs(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (first, second): (&str, &str),
    settings: Settings,
) -> Result<Queued, IntakeError> {
    let language = request_language(tx, conversation).await?;
    let (first, second) = (trade_label(first), trade_label(second));
    let values = [("trade", first.as_str()), ("business", second.as_str())];
    let spanish_on = settings.spanish_conversations_on;
    let body = render(TWO_JOBS_TEMPLATE, language, spanish_on, &values)
        .map_err(|_| IntakeError::Template)?
        .body;
    let to = (msg.contact, TWO_JOBS_TEMPLATE);
    queue_body(tx, secret, to, &[conversation, msg.inbox_id], &body).await
}

/// INT-009, INT-019, INT-032: opens a job of request `conversation` for `trade` (its first, or
/// the ALSO or SPLIT second) from `begin_job`'s pieces, keyed with `property` (`none` before the
/// area check and for ALSO) and with no INT-009 lookup and no INT-007 word count, so a new request
/// never joins another request's job (D-4360, D-4439, D-4441).
async fn open_job(
    tx: &mut PgConnection,
    (homeowner, conversation): (Uuid, Uuid),
    (trade, property): (&str, Option<Uuid>),
    (emergency, at): (bool, Timestamp),
) -> Result<Uuid, IntakeError> {
    let job = create(tx, at, emergency, Actor::Homeowner(homeowner))
        .await
        .map_err(|_| IntakeError::Job)?;
    let part = property.map_or_else(
        || "none".to_owned(),
        |p| p.simple().to_string()[..12].to_owned(),
    );
    let kind = format!("job.{trade}.{part}");
    let choice = job.simple().to_string();
    models::insert_plain(tx, conversation, (&kind, &choice), at).await?;
    models::set_first_job(tx, conversation, job).await?;
    schedule_no_reply(tx, (job, conversation)).await?;
    Ok(job)
}

/// INT-043, INT-049, INT-025, INT-037, INT-016, INT-017, INT-027, CON-009, D-3702, D-4441: one
/// reply in open request `conversation` that moved on (`screened` is clear or SAFE), as the module
/// comment says. Returns the texts queued, in order; none to a contact `service::may_text` refuses.
///
/// # Errors
///
/// The step functions' errors, [`IntakeError::Quotes`] or [`IntakeError::Database`].
pub(crate) async fn reply_step(
    tx: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (conversation, screened): (Uuid, Screened),
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let Some(place) = locked(tx, conversation).await? else {
        return Ok(Vec::new());
    };
    if place.waiting_safe || !may_text(tx, msg.contact).await? {
        return Ok(Vec::new());
    }
    let request = (msg, conversation);
    let mut texts = Vec::new();
    if screened == Screened::Safe || answered_beep(tx, request).await? {
        texts.extend(ask_open_again(tx, secret, request, settings).await?);
        if screened == Screened::Safe && texts.is_empty() {
            texts.extend(card_first(tx, secret, request).await?);
        }
    } else {
        texts.extend(step_answer(tx, secret, request, &place, settings).await?);
    }
    if !place.told
        && !offered_now(tx, request).await?
        && let Some(now) = locked(tx, conversation).await?.filter(|p| p.told)
    {
        texts.extend(ask_step(tx, secret, request, &now, settings).await?);
    }
    Ok(texts)
}

/// INT-032, D-1020, INT-027, D-4441: whether this reply's turn sent a SPLIT offer (its
/// [`SPLIT_OFFER`] marker carries this reply's time). Step 11 then waits for her next reply, so
/// a SPLIT or DIVIDIR answering the offer reaches [`second_job`], not the name-and-address answer.
async fn offered_now(
    tx: &mut PgConnection,
    (msg, conversation): (&TextIn<'_>, Uuid),
) -> Result<bool, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM intake.intake_answer WHERE conversation_id = $1
             AND question_id = $2 AND answered_at = $3) AS "offered!""#,
        conversation,
        SPLIT_OFFER,
        msg.received_at.to_sqlx() as _
    )
    .fetch_one(tx)
    .await
    .map_err(|_| IntakeError::Database)
}

/// INT-043, INT-049, INT-032: her clear reply read as the open step's answer: ALSO or SPLIT, the
/// trade check once her town is stored, the first ZIP question, or the ZIP answer.
async fn step_answer(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    place: &Place,
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    if let Some(texts) = card_answer(tx, secret, request, place).await? {
        return Ok(texts);
    }
    if let Some(texts) = second_job(tx, secret, request, place, settings).await? {
        return Ok(texts);
    }
    Ok(match (place.zip_asked, place.town.as_deref()) {
        (_, Some(town)) if !place.told => {
            let reached = (town, place.zip.as_deref());
            trade_check(tx, secret, request, (place, reached), settings).await?
        }
        (_, Some(_)) => timing_answer(tx, secret, request, place, settings).await?,
        (false, None) => ask_first_zip(tx, secret, request, settings).await?,
        (true, None) => zip_answer(tx, secret, request, place, settings).await?,
    })
}

/// INT-037, D-361, D-4441: whether this reply answered an open INT-037 question (its
/// `emergency_beep` answer row, written by `service::clear_reply`, carries the reply's inbox ID).
async fn answered_beep(
    tx: &mut PgConnection,
    (msg, conversation): (&TextIn<'_>, Uuid),
) -> Result<bool, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM intake.intake_answer WHERE conversation_id = $1
             AND question_id = $2 AND inbox_id = $3) AS "answered!""#,
        conversation,
        BEEP_ANSWER,
        msg.inbox_id
    )
    .fetch_one(tx)
    .await
    .map_err(|_| IntakeError::Database)
}

/// INT-049, INT-025, INT-037, CON-009, D-4441: the open question asked again after SAFE, an
/// INT-037 answer (or, for T691, START), as the module comment says: the ZIP question when it was
/// never asked (`ask_zip`), its text again with no row written when it was asked and her town is
/// not stored, so the one re-ask is neither used nor renewed; nothing while a SAFE hold waits,
/// nothing is open, the request is closed or `service::may_text` refuses the contact. A first ZIP
/// question goes through [`ask_first_zip`] (INT-032's `two_jobs` after it, D-4441).
///
/// # Errors
///
/// `ask_zip`'s, [`IntakeError::Outbox`] or [`IntakeError::Database`].
pub(crate) async fn ask_open_again(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let Some(place) = locked(tx, request.1).await? else {
        return Ok(Vec::new());
    };
    if place.waiting_safe || !may_text(tx, request.0.contact).await? {
        return Ok(Vec::new());
    }
    let (msg, conversation) = request;
    if place.town.is_some() {
        if !place.told {
            return Ok(Vec::new());
        }
        let mut texts = ask_step(tx, secret, request, &place, settings).await?;
        if texts.is_empty() {
            texts.extend(card_again(tx, secret, request, &place).await?);
        }
        return Ok(texts);
    }
    if !place.zip_asked {
        return ask_first_zip(tx, secret, request, settings).await;
    }
    let language = request_language(tx, conversation).await?;
    let template = ZIP_QUESTION_TEMPLATE;
    let body = zip_body(template, language, settings.spanish_conversations_on);
    let subject = [conversation, msg.inbox_id];
    let queued = queue_body(tx, secret, (msg.contact, template), &subject, &body).await?;
    Ok(vec![queued])
}

/// The step-marker question holding `job`'s card state, as `summary_step` writes it (INT-041).
fn card_question(job: Uuid) -> String {
    format!("card.{}", job.simple())
}

/// INT-041, INT-043, INT-029, CARD-007, PRIV-008, D-4360, D-4880: step 12, as the module comment
/// says: the card of the request's first job built from her kept words and shown, with the wait
/// before it goes out as shown. Empty when the step is skipped.
///
/// # Errors
///
/// `order::show_built_card`'s, [`IntakeError::Job`] or [`IntakeError::Database`]; never swallowed.
pub(crate) async fn show_card_step(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
) -> Result<Vec<Queued>, IntakeError> {
    let Some(place) = locked(tx, conversation).await?.filter(|p| p.told) else {
        return Ok(Vec::new());
    };
    let emergency = models::release_state(tx, conversation).await?.emergency;
    if emergency || place.waiting_safe || !may_text(tx, msg.contact).await? {
        return Ok(Vec::new());
    }
    let soon = open_step(tx, conversation).await?.1;
    let Some(card) = card_in(tx, conversation, &place, soon.as_deref()).await? else {
        return Ok(Vec::new());
    };
    let at = msg.received_at;
    let link = quote_link(tx, secret, card.job, at).await?;
    let shown = show_built_card(tx, secret, &card, (&link, AUTO_SEND_AFTER, at)).await?;
    Ok(match shown {
        Shown::Texted(queued) => vec![queued],
        Shown::Emergency | Shown::NotAllowed | Shown::AlreadySent => Vec::new(),
    })
}

/// INT-041, PRIV-008, D-4360: her reply while the request's first job has a card row. A shown card
/// takes EDIT or SEND (`summary_step::answer_card`; any other reply sends nothing); after EDIT the
/// reply is kept as her words and the new card shown (a SEND or EDIT word there is not kept).
/// `None` when no card waits (never shown, or already sent), so the reply goes on as before.
async fn card_answer(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    place: &Place,
) -> Result<Option<Vec<Queued>>, IntakeError> {
    let Some(job) = place.job.filter(|_| place.told) else {
        return Ok(None);
    };
    let state = models::latest_answer(tx, conversation, &card_question(job)).await?;
    match state.as_deref() {
        Some(CARD_EDIT) => {
            if card_reply(msg.inbound.words).is_none() {
                let words = (msg.inbound.words, msg.received_at);
                keep_words(tx, (conversation, job), msg.inbox_id, words).await?;
            }
            let shown = show_card_step(tx, secret, (msg, conversation)).await?;
            Ok(Some(shown))
        }
        Some(shown) if shown.starts_with(CARD_SHOWN) => {
            let step = answer_card(tx, secret, msg, (conversation, job)).await?;
            Ok(Some(match step {
                CardStep::Edit(asked) => vec![asked],
                CardStep::Sent | CardStep::Nothing => Vec::new(),
            }))
        }
        _ => Ok(None),
    }
}

/// INT-041, INT-025, D-4360: the card shown again for [`ask_open_again`] (after SAFE or START):
/// only a card that is shown and waiting (a new show with its own wait); nothing when none was
/// shown, after EDIT or once it is sent.
async fn card_again(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    place: &Place,
) -> Result<Vec<Queued>, IntakeError> {
    let Some(job) = place.job else {
        return Ok(Vec::new());
    };
    let state = models::latest_answer(tx, conversation, &card_question(job)).await?;
    if !state.is_some_and(|s| s.starts_with(CARD_SHOWN)) {
        return Ok(Vec::new());
    }
    show_card_step(tx, secret, (msg, conversation)).await
}

/// INT-041, INT-043, INT-005, D-4360, D-5041: step 12 after SAFE when her name and address were
/// stored while a 911 text waited (the hold skipped [`show_card_step`]): the card shown for the
/// first time, so the job never stalls. Only with an `address_contact` row and no card row of the
/// job; [`show_card_step`]'s own skip rules still hold. Never on START (TD-INT-54).
async fn card_first(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
) -> Result<Vec<Queued>, IntakeError> {
    let Some(job) = locked(tx, conversation).await?.and_then(|p| p.job) else {
        return Ok(Vec::new());
    };
    let stored = models::latest_answer_time(tx, conversation, ADDRESS_CONTACT).await?;
    let carded = models::latest_answer_time(tx, conversation, &card_question(job)).await?;
    if stored.is_none() || carded.is_some() {
        return Ok(Vec::new());
    }
    show_card_step(tx, secret, (msg, conversation)).await
}

/// INT-049, INT-022, INT-029: `answer_zip` on her reply; a stored town releases a job already
/// chosen as an emergency (`service::release_emergency`) and goes straight on to the trade check.
async fn zip_answer(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    place: &Place,
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    Ok(
        match answer_zip(tx, secret, (msg, conversation), settings).await? {
            ZipStep::Stored(ZipPlace::Town { town, .. }) => {
                release_emergency(tx, conversation, msg.received_at).await?;
                let reached = (town.as_str(), read_zip(msg.inbound.words));
                trade_check(tx, secret, (msg, conversation), (place, reached), settings).await?
            }
            ZipStep::OutOfArea(queued) => queued.into_iter().collect(),
            ZipStep::AskedAgain(queued) => vec![queued],
            ZipStep::Stored(_) | ZipStep::Waiting => Vec::new(),
        },
    )
}

/// INT-016, INT-017, INT-009, INT-032: step 5 once her town is stored. The request's own first
/// job and its first `trade_reading` trade; a short first text that read a trade but opened no job
/// opens it now, keyed with her new property; with no trade read, this reply is read as the trade
/// answer ([`trade_answer`]; nothing is written or sent when it names no trade). Then the coverage
/// is told, and an ALSO second job's right after it ([`tell_also`], D-4441); empty when nothing
/// went.
async fn trade_check(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (place, reached): (&Place, (&str, Option<&str>)),
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let at = msg.received_at;
    let (trade, job, words) = match (place.trades.first(), place.job) {
        (Some(trade), Some(job)) => (trade.clone(), job, kept_words(tx, job).await?),
        (Some(trade), None) => {
            let property = models::latest_property(tx, place.homeowner).await?;
            let ids = (place.homeowner, conversation);
            let job = open_job(tx, ids, (trade, property), (false, at)).await?;
            (trade.clone(), job, String::new())
        }
        (None, _) => match trade_answer(tx, msg, (place.homeowner, conversation)).await? {
            Some((trade, job)) => (trade, job, msg.inbound.words.to_owned()),
            None => return Ok(Vec::new()),
        },
    };
    let request = (msg, conversation);
    let told = (trade.as_str(), job, words.as_str());
    let mut texts = tell(tx, secret, request, told, (reached, settings, true)).await?;
    let other = place.trades.iter().find(|t| **t != trade);
    if let Some(other) = other.filter(|_| texts.iter().any(|q| q.template == SPLIT_TEMPLATE)) {
        models::insert_plain(tx, conversation, (SPLIT_OFFER, other), at).await?;
    }
    let also = (job, words.as_str());
    texts.extend(tell_also(tx, secret, request, (place, also), (reached, settings)).await?);
    Ok(texts)
}

/// INT-032, INT-017, D-4441: after the first job's coverage line, the coverage line of the second
/// job an ALSO opened before step 5 (the request's other job), through `routing::tell_coverage`;
/// nothing when step 5 told nothing (no `trade_check` marker) or no ALSO job is open.
async fn tell_also(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    (place, (first, words)): (&Place, (Uuid, &str)),
    (reached, settings): ((&str, Option<&str>), Settings),
) -> Result<Vec<Queued>, IntakeError> {
    let Some(second) = place.trades.get(1).filter(|t| place.job_trades.contains(t)) else {
        return Ok(Vec::new());
    };
    let conversation = request.1;
    if models::latest_answer(tx, conversation, TRADE_CHECK)
        .await?
        .is_none()
    {
        return Ok(Vec::new());
    }
    let jobs = models::request_jobs(tx, conversation).await?;
    let Some(job) = jobs.into_iter().find(|j| *j != first) else {
        return Ok(Vec::new());
    };
    let told = (second.as_str(), job, words);
    tell(tx, secret, request, told, (reached, settings, false)).await
}

/// INT-032, D-1020, D-4441: SPLIT or ALSO before the step's answer, as the module comment says;
/// `Some` with the texts queued when the reply was taken as one (the step then reads nothing of
/// it), `None` otherwise. No words are kept.
async fn second_job(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    place: &Place,
    settings: Settings,
) -> Result<Option<Vec<Queued>>, IntakeError> {
    let (msg, conversation) = request;
    let ids = (place.homeowner, conversation);
    let at = msg.received_at;
    let mut texts = Vec::new();
    if let Some(trade) = split_trade(tx, request, place).await? {
        let property = models::latest_property(tx, place.homeowner).await?;
        open_job(tx, ids, (&trade, property), (false, at)).await?;
    } else if let Some(trade) = also_trade(tx, msg, place).await? {
        let job = open_job(tx, ids, (trade, None), (false, at)).await?;
        if let Some(town) = place.town.as_deref().filter(|_| place.told) {
            let words = match place.job {
                Some(first) => kept_words(tx, first).await?,
                None => String::new(),
            };
            let told = (trade, job, words.as_str());
            let reached = ((town, place.zip.as_deref()), settings, false);
            texts.extend(tell(tx, secret, request, told, reached).await?);
        }
    } else {
        return Ok(None);
    }
    texts.extend(ask_open_again(tx, secret, request, settings).await?);
    Ok(Some(texts))
}

/// INT-032, D-1020, D-4441: the trade a SPLIT or DIVIDIR reply opens: the choice of the request's
/// SPLIT offer when this reply is the first `heard` reply after the turn that sent it (no other
/// `heard` row after the offer's marker) and that trade has no job link yet; else `None`.
async fn split_trade(
    tx: &mut PgConnection,
    (msg, conversation): (&TextIn<'_>, Uuid),
    place: &Place,
) -> Result<Option<String>, IntakeError> {
    if !is_split_reply(msg.inbound.words) {
        return Ok(None);
    }
    let offered = sqlx::query_scalar!(
        r#"SELECT o.choice AS "choice!" FROM intake.intake_answer o
           WHERE o.conversation_id = $1 AND o.question_id = $2 AND o.choice IS NOT NULL
             AND EXISTS (SELECT 1 FROM intake.intake_answer h WHERE h.conversation_id = $1
               AND h.question_id = $3 AND h.inbox_id = $4 AND h.answered_at > o.answered_at)
             AND NOT EXISTS (SELECT 1 FROM intake.intake_answer h WHERE h.conversation_id = $1
               AND h.question_id = $3 AND h.inbox_id IS DISTINCT FROM $4
               AND h.answered_at > o.answered_at)
           ORDER BY o.answered_at DESC, o.id DESC LIMIT 1"#,
        conversation,
        SPLIT_OFFER,
        HEARD,
        msg.inbox_id
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(offered.filter(|t| !place.job_trades.contains(t)))
}

/// INT-032, INT-027: the second trade an ALSO or TAMBIÉN reply opens: the request's second
/// `trade_reading` trade when its first one has a job link and the second none, and the
/// name-and-address question is not open; else `None`.
async fn also_trade<'p>(
    tx: &mut PgConnection,
    msg: &TextIn<'_>,
    place: &'p Place,
) -> Result<Option<&'p str>, IntakeError> {
    let linked = |t: &&String| place.job_trades.contains(t);
    let first_open = place.trades.first().is_some_and(|t| linked(&t));
    let second = place.trades.get(1).filter(|t| first_open && !linked(t));
    let Some(trade) = second.filter(|_| starts_second(msg.inbound.words)) else {
        return Ok(None);
    };
    if name_address_open(tx, msg.contact).await? {
        return Ok(None);
    }
    Ok(Some(trade.as_str()))
}

/// Job `job`'s kept words (its scrubbed description), which the coverage count reads; empty when
/// none were kept.
async fn kept_words(tx: &mut PgConnection, job: Uuid) -> Result<String, IntakeError> {
    let kept = description(tx, job)
        .await
        .map_err(|_| IntakeError::Quotes)?;
    Ok(kept.map(|d| d.text()).unwrap_or_default())
}

/// INT-016, INT-043, INT-009, D-4441: her reply read as the trade answer; its first keyword trade
/// opens the request's first job through [`open_job`], keyed with her newest property and with no
/// INT-009 lookup (INT-007 governs a message's own words, not the order opening the job her answer
/// named, so a one-word "plumbing" opens it too), then its `trade_reading` row is written.
async fn trade_answer(
    tx: &mut PgConnection,
    msg: &TextIn<'_>,
    (homeowner, conversation): (Uuid, Uuid),
) -> Result<Option<(String, Uuid)>, IntakeError> {
    let Some(trade) = keyword_reading(msg.inbound.words).trades.first().copied() else {
        return Ok(None);
    };
    let at = msg.received_at;
    let property = models::latest_property(tx, homeowner).await?;
    let job = open_job(
        tx,
        (homeowner, conversation),
        (trade, property),
        (false, at),
    )
    .await?;
    models::insert_plain(tx, conversation, (TRADE_READING, trade), at).await?;
    Ok(Some((trade.to_owned(), job)))
}

/// INT-016, INT-017, LIC-046, D-4441: `job`'s coverage line. For the first job (`first`) a trade
/// in no open category gets the INT-017 reply through `routing::apply`, else the `trade_check`
/// marker is written; a second job's is `Coverage::NotCovered` for that job alone. Otherwise the
/// count, by `quotes::api::request_coverage` for the trade's candidates read from `words`, goes
/// through `routing::tell_coverage`.
async fn tell(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (trade, job, words): (&str, Uuid, &str),
    ((town, zip), settings, first): ((&str, Option<&str>), Settings, bool),
) -> Result<Vec<Queued>, IntakeError> {
    let area = [town];
    let req = Request {
        town,
        area: &area,
        trade,
        trade_open: trade_is_open(tx, trade).await?,
        words: "",
        move_states: None,
        job_types: &[],
    };
    let label = trade_label(trade);
    let routing = Routing {
        conversation,
        msg,
        language: request_language(tx, conversation).await?,
        spanish_on: settings.spanish_conversations_on,
        label: &label,
        job_type: None,
    };
    let coverage = match (route(&req), first) {
        (Outcome::Continue, _) => count(tx, (town, zip), trade, words).await?,
        (outcome, true) => {
            let told = apply(tx, secret, (&req, outcome), &routing).await?;
            return Ok(told.into_iter().collect());
        }
        (_, false) => Coverage::NotCovered,
    };
    if first {
        models::insert_plain(tx, conversation, (TRADE_CHECK, "told"), msg.received_at).await?;
    }
    let told = tell_coverage(tx, secret, (&req, coverage, job), &routing).await?;
    Ok(told.into_iter().collect())
}

/// CARD-001, INT-017: the coverage `quotes::api::request_coverage` counts in `(town, zip)` for
/// `trade`'s candidates read from `words`.
async fn count(
    tx: &mut PgConnection,
    (town, zip): (&str, Option<&str>),
    trade: &str,
    words: &str,
) -> Result<Coverage, IntakeError> {
    let facts = facts(trade, words);
    let quotes = |_| IntakeError::Quotes;
    let pool = card_pool(tx, ((town, zip), &facts), String::new(), None)
        .await
        .map_err(quotes)?;
    request_coverage(tx, pool.map, ((town, zip), &facts), None)
        .await
        .map_err(quotes)
}

/// CARD-001, D-102: the card facts the count reads: `trade`'s candidate job types from the
/// keyword reading of `words` (`<trade>_other` when none is named).
fn facts(trade: &str, words: &str) -> CardFacts {
    let reading = Reading {
        trade,
        job_type: None,
    };
    let found = candidates(trade, [reading, reading], &keyword_reading(words));
    CardFacts {
        category: trade.to_owned(),
        candidates: found
            .job_types
            .into_iter()
            .filter(|(t, _)| t == trade)
            .map(|(_, j)| j)
            .collect(),
        emergency: false,
    }
}

/// The open step after step 5, read from the stored rows (INT-043).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    /// Step 6: "How soon?" has no answer.
    HowSoon,
    /// Step 6, answer 4: the day has no answer.
    Day,
    /// Step 7: the choice is due and not made.
    Choice,
    /// Step 8 and after: the trade questions, then step 11.
    Questions,
}

/// INT-030, INT-006, D-4360: the open step of `conversation` and its "How soon?" code. The choice
/// is due with no `emergency_choice` row after answer 1, or after any answer but 4 when a 911 text
/// was queued before that answer (as `service::answer_how_soon` reads it).
async fn open_step(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<(Open, Option<String>), IntakeError> {
    let r = sqlx::query!(
        r#"SELECT h.choice AS "soon?",
             EXISTS (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = $1
               AND a.question_id = $3) AS "day!",
             EXISTS (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = $1
               AND a.question_id = $4) AS "chosen!",
             EXISTS (SELECT 1 FROM intake.emergency_event e WHERE e.conversation_id = $1
               AND e.text_queued_at IS NOT NULL AND e.created_at <= h.answered_at) AS "listed!"
           FROM (VALUES (1)) one(n) LEFT JOIN LATERAL (SELECT a.choice, a.answered_at
             FROM intake.intake_answer a WHERE a.conversation_id = $1 AND a.question_id = $2
             ORDER BY a.answered_at DESC, a.id DESC LIMIT 1) h ON true"#,
        conversation,
        HOW_SOON,
        NEEDED_BY,
        CHOICE
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let open = match r.soon.as_deref() {
        None => Open::HowSoon,
        Some(s) if s == Urgency::ByDay.code() && !r.day => Open::Day,
        Some(s) if s == Urgency::ByDay.code() => Open::Questions,
        Some(s) if !r.chosen && (s == Urgency::Today.code() || r.listed) => Open::Choice,
        Some(_) => Open::Questions,
    };
    Ok((open, r.soon))
}

/// INT-043, INT-030, INT-006, INT-033, INT-039, INT-027: asks the open step's question of a
/// request step 5 told the coverage: "How soon?", the day, the choice (the night count first,
/// INT-033; nothing while a SAFE hold waits, INT-005), else the next trade question or step 11.
async fn ask_step(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    place: &Place,
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let (msg, conversation) = request;
    let (Some(trade), Some(town)) = (place.trades.first(), place.town.as_deref()) else {
        return Ok(Vec::new());
    };
    let label = trade_label(trade);
    let within = Trade {
        category: trade,
        town,
        label: &label,
    };
    Ok(match open_step(tx, conversation).await?.0 {
        Open::HowSoon => vec![ask_how_soon(tx, secret, msg, conversation, settings).await?],
        Open::Day => {
            let template = (
                NEEDED_BY_TEMPLATE,
                request_language(tx, conversation).await?,
            );
            let (subject, on) = (
                [conversation, msg.inbox_id],
                settings.spanish_conversations_on,
            );
            vec![queue_text(tx, secret, msg.contact, template, &subject, on).await?]
        }
        Open::Choice => offer_emergency_choice(tx, secret, request, &within, settings).await?,
        Open::Questions => questions(tx, secret, request, place, (settings, false)).await?,
    })
}

/// INT-043, INT-030, INT-006, INT-029, INT-039: her reply read as the open step's answer after
/// step 5, through the step's own function. An unreadable answer gets the question again; an
/// answer that leaves no question open (2 or 3, the day, the choice, a night count of 0) goes on
/// to the trade questions in the same turn, after the cards on the emergency path.
async fn timing_answer(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    place: &Place,
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let (msg, conversation) = request;
    let (Some(trade), Some(town)) = (place.trades.first(), place.town.as_deref()) else {
        return Ok(Vec::new());
    };
    let label = trade_label(trade);
    let within = Trade {
        category: trade,
        town,
        label: &label,
    };
    let (mut texts, read) = match open_step(tx, conversation).await? {
        (Open::HowSoon, _) => {
            let answered = answer_how_soon(tx, secret, request, &within, settings).await?;
            (answered.texts, answered.read.is_some())
        }
        (Open::Day, _) => (Vec::new(), answer_day(tx, request).await?),
        (Open::Choice, soon) => {
            let Some(card) = card_in(tx, conversation, place, soon.as_deref()).await? else {
                return Ok(Vec::new());
            };
            let answered = choose_with_card(tx, msg, &card).await?;
            (answered.texts, answered.read.is_some())
        }
        (Open::Questions, _) => {
            return questions(tx, secret, request, place, (settings, true)).await;
        }
    };
    if !read || open_step(tx, conversation).await?.0 == Open::Questions {
        texts.extend(ask_step(tx, secret, request, place, settings).await?);
    }
    Ok(texts)
}

/// INT-029, CARD-007, INT-030: the card `order::choose_with_card` stores before the choice can
/// release the request's first job: its trade's and town's display names (`card_pool`), the
/// candidates read from her kept words, the "How soon?" answer `soon` and, when a 911 text was
/// sent, its situation ([`danger`]); the description is hers.
async fn card_in(
    tx: &mut PgConnection,
    conversation: Uuid,
    place: &Place,
    soon: Option<&str>,
) -> Result<Option<CardIn>, IntakeError> {
    let (Some(trade), Some(job), Some(town)) =
        (place.trades.first(), place.job, place.town.as_deref())
    else {
        return Ok(None);
    };
    let facts = facts(trade, &kept_words(tx, job).await?);
    let reached = (town, place.zip.as_deref());
    let pool = card_pool(tx, (reached, &facts), String::new(), None)
        .await
        .map_err(|_| IntakeError::Quotes)?;
    let timing = match soon {
        Some(s) if s == Urgency::Today.code() => Timing::Today,
        Some(s) if s == Urgency::NoRush.code() => Timing::NoRush,
        _ => Timing::NextFewDays,
    };
    let parts = CardParts {
        category: pool.category_name,
        town: pool.town_name,
        place: Site::House,
        description: String::new(),
        answers: Vec::new(),
        timing: Some(timing),
        danger: danger(tx, conversation).await?,
        photos: Vec::new(),
        concept: None,
    };
    Ok(Some(CardIn {
        conversation,
        job,
        parts,
        category: trade.clone(),
        candidates: facts.candidates,
    }))
}

/// CARD-007, INT-004 (D-436, D-4833): the situation of the request's latest 911 text that was
/// sent, for the card's "911 text sent: ..." line; `None` when no 911 text went out.
async fn danger(tx: &mut PgConnection, conversation: Uuid) -> Result<Option<Danger>, IntakeError> {
    let kinds = [
        (Hazard::Gas, Danger::Gas),
        (Hazard::CoAlarm, Danger::CoAlarm),
        (Hazard::SmokeFire, Danger::SmokeOrFire),
        (Hazard::Electrical, Danger::Electrical),
        (Hazard::PowerLine, Danger::PowerLine),
    ];
    let events = models::events(tx, conversation).await?;
    let sent = events.iter().rev().find(|e| e.text_queued_at.is_some());
    Ok(sent.and_then(|e| {
        let kind = kinds.iter().find(|(hazard, _)| hazard.code() == e.matched);
        kind.map(|(_, danger)| *danger)
    }))
}

/// INT-039, INT-029, INT-004, INT-027: step 8 for the request's first job, through
/// `order::bank_turn` (`hear`: this text answers the open question). Nothing while a job chosen as
/// an emergency is not released (`question_bank::starts_now`). The next question goes as
/// [`TRADE_QUESTION_TEMPLATE`]; a gas or burning smell answer holds the order ([`hold_911`]); when
/// the set is done, step 11 (`order::ask_last_question`, once per request).
async fn questions(
    tx: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    place: &Place,
    (settings, hear): (Settings, bool),
) -> Result<Vec<Queued>, IntakeError> {
    let (msg, conversation) = request;
    let (Some(trade), Some(job)) = (place.trades.first(), place.job) else {
        return Ok(Vec::new());
    };
    let state = models::release_state(tx, conversation).await?;
    if !starts_now(state.emergency, state.consented && state.area_done) {
        return Ok(Vec::new());
    }
    let candidates = facts(trade, &kept_words(tx, job).await?).candidates;
    let spanish_on = settings.spanish_conversations_on;
    let spanish = spanish_on && request_language(tx, conversation).await? == Language::Es;
    let about = (trade.as_str(), &candidates[..], (state.emergency, spanish));
    let turn = bank_turn(tx, (conversation, job), about, hear.then_some(msg)).await?;
    Ok(match turn {
        BankTurn::Ask(prompt) => {
            let (to, subject) = (
                (msg.contact, TRADE_QUESTION_TEMPLATE),
                [conversation, msg.inbox_id],
            );
            vec![queue_body(tx, secret, to, &subject, &prompt.render()).await?]
        }
        BankTurn::Safety(kind) => vec![hold_911(tx, secret, request, kind).await?],
        BankTurn::Done => ask_last_question(tx, secret, request, spanish_on)
            .await?
            .into_iter()
            .collect(),
    })
}

/// INT-039, INT-004, INT-005: a smell answer of gas (`kind` 1) or burning (2) holds the order as a
/// 911-list match does (`service::record_match` is private): the INT-004 text in the request's
/// language (its Spanish stays on), the SAFE hold keyed with this reply's inbox ID, and the
/// `intake.emergency_matched` audit row. SAFE then resumes at the next question.
async fn hold_911(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    kind: u32,
) -> Result<Queued, IntakeError> {
    let template = (SAFETY_TEMPLATE, request_language(tx, conversation).await?);
    let subject = [conversation, msg.inbox_id];
    let queued = queue_text(tx, secret, msg.contact, template, &subject, true).await?;
    let hazard = if kind == 1 {
        Hazard::Gas
    } else {
        Hazard::SmokeFire
    };
    let (ids, at) = ((conversation, msg.inbox_id), msg.received_at);
    models::insert_event(tx, ids, hazard.code(), Some(at), at).await?;
    audit_action(tx, Actor::System, "intake.emergency_matched", conversation).await?;
    Ok(queued)
}

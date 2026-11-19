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
//! told the coverage (the `trade_check` marker; D-3702, D-4441).
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
//! A ZIP reply, a trade answer, ALSO or SPLIT is never kept as words (INT-049). No words or ZIP
//! are logged.

use askhava_jobs::api::create;
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardFacts, card_pool, description, request_coverage};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use askhava_messaging::api::render;

use askhava_quotes::api::Coverage;

use crate::conversation::name_address_open;
use crate::handlers::{queue_body, schedule_no_reply};
use crate::matching::{Reading, candidates, keyword_reading, starts_second, trade_label};
use crate::models::{self, IntakeError};
use crate::order::{ask_last_question, keep_words};
use crate::routing::{
    Outcome, Request, Routing, SPLIT_TEMPLATE, apply, is_split_reply, route, tell_coverage,
    trade_is_open,
};
use crate::rules::{Settings, Then, opens_job};
use crate::service::{Handled, Queued, Screened, TextIn, may_text, request_language};
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

/// CON-031, INT-043, PRIV-008, INT-007, INT-009: the first text of a new request, after the consent
/// text (and the 911 text or alarm question), as the module comment says. Its texts join
/// `handled`'s.
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
    } else {
        texts.extend(step_answer(tx, secret, request, &place, settings).await?);
    }
    if models::latest_answer(tx, conversation, TRADE_CHECK)
        .await?
        .is_some()
    {
        let spanish_on = settings.spanish_conversations_on;
        texts.extend(ask_last_question(tx, secret, request, spanish_on).await?);
    }
    Ok(texts)
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
    if let Some(texts) = second_job(tx, secret, request, place, settings).await? {
        return Ok(texts);
    }
    Ok(match (place.zip_asked, place.town.as_deref()) {
        (_, Some(town)) if !place.told => {
            let reached = (town, place.zip.as_deref());
            trade_check(tx, secret, request, (place, reached), settings).await?
        }
        (_, Some(_)) => Vec::new(),
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
        return Ok(Vec::new());
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

/// INT-049, INT-022: `answer_zip` on her reply; a stored town goes straight on to the trade check.
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

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
//! consent text (CON-031). [`reply_step`] asks the ZIP when it never was (after SAFE or an alarm
//! answer), asks again on a SAFE while it is open, else reads it with `zip_step::answer_zip`; once
//! her town is stored the trade check tells the coverage (`routing::apply` for a closed category,
//! else `routing::tell_coverage` with `quotes::api::request_coverage`'s count). With no trade
//! read, each later reply is the trade answer; its job opens through `begin_job`, and nothing is
//! written until it does (D-4428). Step 11 (`order::ask_last_question`, once per request once her
//! town is stored) is called last on every such reply, where it fired before (D-3702).
//!
//! A ZIP reply or a trade answer is never kept as words (INT-049). No words or ZIP are logged.

use askhava_jobs::api::create;
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardFacts, card_pool, description, request_coverage};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::conversation::{JobStart, Opened, begin_job};
use crate::handlers::schedule_no_reply;
use crate::matching::{Reading, candidates, keyword_reading, trade_label};
use crate::models::{self, IntakeError};
use crate::order::{ask_last_question, keep_words};
use crate::routing::{Outcome, Request, Routing, apply, route, tell_coverage, trade_is_open};
use crate::rules::{Settings, Then, opens_job};
use crate::service::{Handled, Queued, Screened, TextIn, request_language};
use crate::zip_step::{ZipPlace, ZipStep, answer_zip, ask_zip, read_zip};

/// The step-marker question holding one trade the keyword reading named (codes only, at most two,
/// first-named first).
pub(crate) const TRADE_READING: &str = "trade_reading";
/// The step-marker question written once the trade check told her the coverage (INT-017).
const TRADE_CHECK: &str = "trade_check";
/// A first message's trades kept as `trade_reading` rows (INT-032's two jobs are T711's).
const MAX_TRADES: usize = 2;

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
               AND e.text_queued_at IS NOT NULL AND e.safe_at IS NULL) AS "waiting_safe!"
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
        let asked = ask_zip(tx, secret, (msg, conversation), settings).await?;
        handled.texts.push(asked);
    }
    Ok(())
}

/// INT-009, INT-019: opens request `conversation`'s first job for `trade` from `begin_job`'s
/// pieces, keyed with `property` (`none` before the area check) and with no INT-009 lookup, so a
/// new request never joins another request's job (D-4360).
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

/// INT-043, INT-049, INT-025, INT-016, INT-017, INT-027, D-3702: one reply in open request
/// `conversation` that moved on (`screened` is clear or SAFE), as the module comment says. Returns
/// the texts queued, in order.
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
    if place.waiting_safe {
        return Ok(Vec::new());
    }
    let request = (msg, conversation);
    let mut texts = Vec::new();
    match (place.zip_asked, place.town.clone()) {
        (_, Some(town)) if !place.told => {
            let reached = (town.as_str(), place.zip.as_deref());
            texts.extend(trade_check(tx, secret, request, (&place, reached), settings).await?);
        }
        (_, Some(_)) => {}
        (asked, None) if !asked || screened == Screened::Safe => {
            texts.push(ask_zip(tx, secret, request, settings).await?);
        }
        (_, None) => texts.extend(zip_answer(tx, secret, request, &place, settings).await?),
    }
    let spanish_on = settings.spanish_conversations_on;
    texts.extend(ask_last_question(tx, secret, request, spanish_on).await?);
    Ok(texts)
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

/// INT-016, INT-017, INT-009: step 5 once her town is stored. The request's own first job and its
/// first `trade_reading` trade; a short first text that read a trade but opened no job opens it
/// now, keyed with her new property; with no trade read, this reply is read as the trade answer
/// and its job opens through `begin_job` (nothing is written when it opens none). Then the
/// coverage is told; empty when nothing went.
async fn trade_check(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (place, reached): (&Place, (&str, Option<&str>)),
    settings: Settings,
) -> Result<Vec<Queued>, IntakeError> {
    let at = msg.received_at;
    let (trade, job, words) = match (place.trades.first(), place.job) {
        (Some(trade), Some(job)) => {
            let kept = description(tx, job)
                .await
                .map_err(|_| IntakeError::Quotes)?;
            (
                trade.clone(),
                job,
                kept.map(|d| d.text()).unwrap_or_default(),
            )
        }
        (Some(trade), None) => {
            let property = models::latest_property(tx, place.homeowner).await?;
            let ids = (place.homeowner, conversation);
            let job = open_job(tx, ids, (trade, property), (false, at)).await?;
            (trade.clone(), job, String::new())
        }
        (None, _) => match trade_answer(tx, msg, conversation).await? {
            Some((trade, job)) => (trade, job, msg.inbound.words.to_owned()),
            None => return Ok(Vec::new()),
        },
    };
    let told = (trade.as_str(), job, words.as_str());
    tell(tx, secret, (msg, conversation), told, (reached, settings)).await
}

/// INT-016, INT-007, INT-009: her reply read as the trade answer; its first keyword trade opens
/// the request's first job through `begin_job`, and only then is its `trade_reading` row written.
async fn trade_answer(
    tx: &mut PgConnection,
    msg: &TextIn<'_>,
    conversation: Uuid,
) -> Result<Option<(String, Uuid)>, IntakeError> {
    let words = msg.inbound.words;
    let Some(trade) = keyword_reading(words).trades.first().copied() else {
        return Ok(None);
    };
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade,
        at: msg.received_at,
    };
    let Opened::Created(job) = begin_job(tx, &start).await? else {
        return Ok(None);
    };
    models::insert_plain(tx, conversation, (TRADE_READING, trade), msg.received_at).await?;
    Ok(Some((trade.to_owned(), job)))
}

/// INT-016, INT-017, LIC-046: a trade in no open category gets the INT-017 reply through
/// `routing::apply`; otherwise `job`'s coverage, counted by `quotes::api::request_coverage` for
/// the trade's candidates read from `words`, goes through `routing::tell_coverage`.
async fn tell(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (trade, job, words): (&str, Uuid, &str),
    ((town, zip), settings): ((&str, Option<&str>), Settings),
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
    let outcome = route(&req);
    if outcome != Outcome::Continue {
        return Ok(apply(tx, secret, (&req, outcome), &routing)
            .await?
            .into_iter()
            .collect());
    }
    let facts = facts(trade, words);
    let quotes = |_| IntakeError::Quotes;
    let pool = card_pool(tx, ((town, zip), &facts), String::new(), None)
        .await
        .map_err(quotes)?;
    let coverage = request_coverage(tx, pool.map, ((town, zip), &facts), None)
        .await
        .map_err(quotes)?;
    models::insert_plain(tx, conversation, (TRADE_CHECK, "told"), msg.received_at).await?;
    let told = tell_coverage(tx, secret, (&req, coverage, job), &routing).await?;
    Ok(told.into_iter().collect())
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

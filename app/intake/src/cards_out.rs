//! The card sender: every approved or released job card goes out once (T251; CARD-025, CARD-001,
//! CARD-019, CARD-020, CARD-021, INT-029, INT-041, TXT-032, QH-005, CON-020; D-1521, D-2203,
//! D-2330, D-2708, D-3189).
//!
//! Every fact comes from `quotes`' job-text store (T411): the card, its category, its remaining
//! candidate job types and its emergency mark (`StoredCard`), and the scrubbed description. On
//! `intake.cards_approved` ([`send_approved`]) the card is `quotes::api::stored_card` for the
//! approved digest, sent only while `approved_card` holds (a changed card is
//! [`CardsSent::Refused`]); on `intake.emergency_released` ([`send_emergency`]) it is the first
//! of `quotes::api::unsent_cards` carrying the emergency mark, never a regular job's card waiting
//! in its approval window (one release per request: D-3189's known issue); once that card is sent,
//! a rerun takes the request's sent emergency card again, so a retry after the commit still sends
//! TXT-032 once (D-3182). No stored card (or no town) is [`IntakeError::NoStoredCard`]: nothing is
//! sent and the task fails, so it retries.
//!
//! Gates first, on both paths and every retry (D-3182): no consent record (`card_allowed`) sends
//! nothing ([`CardsSent::Stopped`]); a 911 text waiting for SAFE (`safe_hold`, INT-005) sends
//! nothing ([`CardsSent::Held`]) and the handler queues the same event again [`HOLD_RECHECK`] later.
//! In the transaction the job's row is locked first (`jobs::api::lock_for_quote_change`): a job in
//! Intake moves to Open (JOB-008, `move_to`), Open and No quotes yet go on (a rerun), and any other
//! status (a job cancelled by CLOSE, closed, not covered) sends nothing ([`CardsSent::Stopped`]).
//!
//! One transaction marks the card sent, builds the pool (`quotes::api::card_pool`, the job's own
//! town from `intake.property`, the band floor from the `band.<job>` answer T162 stores), records
//! one card per selected business (`send_job_cards`, `ON CONFLICT DO NOTHING`, so a repeat records
//! nothing), writes the job's first facts row (`jobs::api::record_job_facts`: exactly the stored
//! category and candidates) and queues each new card's notice: `queue_card_notice` (held whole
//! by quiet hours in `messaging`, its 24 hours from the actual send) or, on the emergency path,
//! `queue_emergency_card`. After the commit the homeowner's TXT-032 text goes through the steps
//! T240's job texts use: `may_text`, `messaging::api::send_window` (opt-out, quiet hours: a held
//! row waits in the outbox, whose sender holds it to the window) and one outbox row per job.
//!
//! T249 (CARD-024, D-1519, D-1521): the pool also gets the ZIP of the same `intake.property` row,
//! so `card_pool` fills the job's location and each business's reach, and the cards go through
//! `send_job_cards_in_reach`; a property with no stored ZIP matches by towns only.

use std::sync::Arc;

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{JobStatus, job_number, lock_for_quote_change, move_to, record_job_facts};
use askhava_messaging::api::{SendWindow, held_until, send_window};
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{
    CardLines, CardNotice, CardPool, CardsOut, Pool, Reader, Recipient, SLOW_AT_MOST, StoredCard,
    band_floor_cents, card_holders, card_pool, cards_out_text, description, mark_card_sent,
    queue_card_notice, queue_emergency_card, send_job_cards_in_reach, sent_card, stored_card,
    unsent_cards,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::handlers::queue_body;
use crate::models::{self, IntakeError};
use crate::rules::language_from_code;
use crate::service::{
    EMERGENCY_RELEASED_KIND, Queued, card_allowed, how_soon, may_text, safe_hold,
};
use crate::summary_step::{CARDS_APPROVED_KIND, approved_card};

/// TXT-032's template ID, the cards-out text's first sentence.
pub const CARDS_OUT_TEMPLATE: &str = "request_out";
/// The "How soon?" code of "Today, it's urgent" (INT-030), for TXT-032's extra lines.
const TODAY: &str = "today";
/// Business texting is off until a business number exists (D-492), so notices go by email.
const BUSINESS_TEXTING_ON: bool = false;
const BAD_PAYLOAD: &str = "bad intake card sender payload";
const FAILED: &str = "the intake card sender failed";
/// INT-005, D-3182: how long a card event held by a 911 text waiting for SAFE waits before it runs
/// again; it repeats until SAFE comes or the job leaves Intake (INT-019's close ends it).
pub const HOLD_RECHECK: SignedDuration = SignedDuration::from_mins(5);

/// What the sender needs and never logs: the outbox key and the vault `messaging` reads her number
/// from for the send window (SEC-018, PRIV-004).
pub struct CardsOutKeys {
    outbox: SecretString,
    vault: Vault,
}

impl CardsOutKeys {
    /// The `ID_SECRET` as outbox key and the vault.
    #[must_use]
    pub fn new(outbox: SecretString, vault: Vault) -> Self {
        Self { outbox, vault }
    }
}

/// What one run of the sender did (CARD-025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardsSent {
    /// Sent: the businesses newly carded by this run and the TXT-032 text, if one was queued.
    Sent {
        /// The new recipients (none on a repeat).
        recipients: Vec<Recipient>,
        /// The cards-out text queued now, if any.
        text: Option<Queued>,
    },
    /// The stored card is not the one the homeowner approved (CARD-007): nothing is sent.
    Refused,
    /// A 911 text of the request waits for SAFE (INT-005): nothing is sent; the handler queues the
    /// event again [`HOLD_RECHECK`] later.
    Held,
    /// No consent record (CON-007), or the job's status takes no cards (JOB-008: cancelled,
    /// closed, not covered or past the first quote): nothing is sent and the task is done.
    Stopped,
}

/// The `intake.cards_approved` handler `server` registers: [`send_approved`] at `now()`.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed send, so the task retries.
pub async fn run_cards_approved(
    pool: PgPool,
    keys: Arc<CardsOutKeys>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let id = |key: &str| {
        task.payload[key]
            .as_str()
            .and_then(|v| v.parse::<Uuid>().ok())
    };
    let digest = task.payload["card"].as_str().map(str::to_owned);
    let (Some(conversation), Some(job), Some(digest)) = (id("conversation"), id("job"), digest)
    else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    let now = Timestamp::now();
    let sent = send_approved(&mut db, &keys, (conversation, job, &digest), now).await;
    settle(&mut db, (CARDS_APPROVED_KIND, &task), sent, now).await
}

/// The `intake.emergency_released` handler `server` registers: [`send_emergency`] at `now()`.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed send, so the task retries.
pub async fn run_emergency_released(
    pool: PgPool,
    keys: Arc<CardsOutKeys>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let conversation = task.payload["conversation"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
        .ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    let now = Timestamp::now();
    let sent = send_emergency(&mut db, &keys, conversation, now).await;
    settle(&mut db, (EMERGENCY_RELEASED_KIND, &task), sent, now).await
}

/// INT-005, D-3182: a held run queues `kind` with the same payload [`HOLD_RECHECK`] from `now`,
/// keyed by `task`'s row ID (a second run of the same task queues nothing more); an error fails
/// the task so it retries; anything else finishes it.
async fn settle(
    db: &mut PgConnection,
    (kind, task): (&'static str, &Claimed),
    sent: Result<CardsSent, IntakeError>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    match sent {
        Ok(CardsSent::Held) => {
            let mut again = NewTask::new(kind, task.payload.clone(), now + HOLD_RECHECK);
            again.dedupe_key = Some(format!("{kind}:after:{}", task.id));
            enqueue(db, &again)
                .await
                .map(|_| ())
                .map_err(|_| HandlerError(FAILED))
        }
        Ok(_) => Ok(()),
        Err(_) => Err(HandlerError(FAILED)),
    }
}

/// CON-007, INT-005: `Some` when request `conversation` may not get cards now: no consent record
/// ([`CardsSent::Stopped`]) or a 911 text waiting for SAFE ([`CardsSent::Held`]).
async fn gate(db: &mut PgConnection, conversation: Uuid) -> Result<Option<CardsSent>, IntakeError> {
    if !card_allowed(db, conversation).await? {
        return Ok(Some(CardsSent::Stopped));
    }
    Ok(safe_hold(db, conversation)
        .await?
        .then_some(CardsSent::Held))
}

/// CARD-025, INT-041, CARD-007: sends job `job`'s card approved under `digest` in request
/// `conversation`, only while `approved_card` holds for it and the gates pass (module comment).
/// Safe to run twice. Call it outside a transaction (the vault read).
///
/// # Errors
///
/// [`IntakeError::NoStoredCard`] when no stored card has the digest; otherwise as [`send_stored`].
pub async fn send_approved(
    db: &mut PgConnection,
    keys: &CardsOutKeys,
    (conversation, job, digest): (Uuid, Uuid, &str),
    now: Timestamp,
) -> Result<CardsSent, IntakeError> {
    if let Some(stop) = gate(db, conversation).await? {
        return Ok(stop);
    }
    let stored = stored_card(db, job, digest)
        .await
        .map_err(|_| IntakeError::Quotes)?
        .ok_or(IntakeError::NoStoredCard)?;
    if !approved_card(db, (conversation, job), &stored.card).await? {
        return Ok(CardsSent::Refused);
    }
    send_stored(db, keys, (conversation, &stored), now).await
}

/// INT-029, CARD-021, CARD-025: sends at once the first unsent stored card of request
/// `conversation` that carries the emergency mark, the gates passing; a regular job's card is never
/// sent here. With none unsent it reruns the request's sent emergency card, which records nothing
/// new and sends the TXT-032 text a failed run left unsent, once (D-3182).
///
/// # Errors
///
/// [`IntakeError::NoStoredCard`] when no emergency card is stored; otherwise as [`send_stored`].
pub async fn send_emergency(
    db: &mut PgConnection,
    keys: &CardsOutKeys,
    conversation: Uuid,
    now: Timestamp,
) -> Result<CardsSent, IntakeError> {
    if let Some(stop) = gate(db, conversation).await? {
        return Ok(stop);
    }
    let stored = emergency_card(db, conversation).await?;
    send_stored(db, keys, (conversation, &stored), now).await
}

/// The first unsent emergency card of `conversation`, else its first sent one (D-3182).
async fn emergency_card(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<StoredCard, IntakeError> {
    let quotes = |_| IntakeError::Quotes;
    let unsent = unsent_cards(db, conversation).await.map_err(quotes)?;
    if let Some(card) = unsent.into_iter().find(|c| c.facts.emergency) {
        return Ok(card);
    }
    for job in models::request_jobs(db, conversation).await? {
        let sent = sent_card(db, job).await.map_err(quotes)?;
        if let Some(card) = sent.filter(|c| c.facts.emergency && c.request == conversation) {
            return Ok(card);
        }
    }
    Err(IntakeError::NoStoredCard)
}

/// The send itself, then the TXT-032 text (module comment).
///
/// # Errors
///
/// [`IntakeError::NoStoredCard`] with no property town, [`IntakeError::Quotes`] when matching or a
/// notice fails, [`IntakeError::Job`] when the facts row or number fails, and the text's errors.
async fn send_stored(
    db: &mut PgConnection,
    keys: &CardsOutKeys,
    (conversation, stored): (Uuid, &StoredCard),
    now: Timestamp,
) -> Result<CardsSent, IntakeError> {
    let (town, zip) = property_place(db, conversation).await?;
    let band = models::latest_answer(db, conversation, &band_question(stored.job)).await?;
    let text = description(db, stored.job)
        .await
        .map_err(|_| IntakeError::Quotes)?
        .map(|d| d.text())
        .unwrap_or_default();
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if !takes_cards(&mut tx, stored.job, now).await? {
        return Ok(CardsSent::Stopped);
    }
    let floor = band.as_deref().and_then(band_floor_cents);
    let quotes = |_| IntakeError::Quotes;
    let pool = card_pool(
        &mut tx,
        ((&town, zip.as_deref()), &stored.facts),
        text,
        floor,
    )
    .await
    .map_err(quotes)?;
    mark_card_sent(&mut tx, stored.job, &stored.digest, now)
        .await
        .map_err(quotes)?;
    let chosen = Pool {
        businesses: &pool.businesses,
        pauses: &pool.pauses,
    };
    let matched = (&pool.job, &chosen, &pool.reach);
    let new = send_job_cards_in_reach(&mut tx, pool.map, stored.job, matched, now)
        .await
        .map_err(quotes)?;
    record_job_facts(
        &mut tx,
        stored.job,
        &stored.facts.category,
        &stored.facts.candidates,
    )
    .await
    .map_err(|_| IntakeError::Job)?;
    let count = notices(&mut tx, (conversation, stored), &pool, &new, now).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    let text = cards_out(db, keys, (conversation, stored), (&pool, count), now).await?;
    Ok(CardsSent::Sent {
        recipients: new,
        text,
    })
}

/// CARD-001, CARD-024, D-1521: the job's own town and its ZIP (none: towns only) from the
/// newest property of request `conversation`'s homeowner.
///
/// # Errors
/// [`IntakeError::NoStoredCard`] with no property town.
async fn property_place(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<(String, Option<String>), IntakeError> {
    let town = models::property_town(db, conversation)
        .await?
        .ok_or(IntakeError::NoStoredCard)?;
    let zip = property_zip(db, conversation, &town).await?;
    Ok((town, zip))
}

/// CARD-024, INT-049, D-1521: the ZIP of the newest property of request `conversation`'s
/// homeowner in `town` (the town `models::property_town` read), in the same order; none for a row
/// stored before the ZIP. Keyed by the town, the pair is always one row's: a property stored
/// between the two reads in another town is skipped here, never paired with the older town.
pub(crate) async fn property_zip(
    db: &mut PgConnection,
    conversation: Uuid,
    town: &str,
) -> Result<Option<String>, IntakeError> {
    sqlx::query_scalar!(
        "SELECT p.zip FROM intake.property p JOIN intake.conversation c \
         ON c.homeowner_id = p.homeowner_id WHERE c.id = $1 AND p.town = $2 \
         ORDER BY p.created_at DESC, p.id DESC LIMIT 1",
        conversation,
        town
    )
    .fetch_optional(db)
    .await
    .map(Option::flatten)
    .map_err(|_| IntakeError::Database)
}

/// JOB-008, D-3182: locks job `job`'s row in the caller's transaction, then the guarded move
/// from Intake to Open (its cards go now; under the lock it applies only to a job in Intake);
/// Open and No quotes yet go on (a rerun or retry), any other status takes no cards.
async fn takes_cards(
    tx: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<bool, IntakeError> {
    let status = lock_for_quote_change(tx, job)
        .await
        .map_err(|_| IntakeError::Job)?;
    let arrow = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(tx, job, arrow, now, Actor::System)
        .await
        .map_err(|_| IntakeError::Job)?;
    Ok(matches!(moved, Guarded::Applied(())) || status.takes_late_cards())
}

/// The `intake_answer` question T162 stores the size or budget band floor under (BIZ-049).
fn band_question(job: Uuid) -> String {
    format!("band.{}", job.simple())
}

/// CARD-019, CARD-020, CARD-021: queues each new recipient's notice with the card text sent to
/// it; returns N, the job's card holders.
async fn notices(
    tx: &mut PgConnection,
    (conversation, stored): (Uuid, &StoredCard),
    pool: &CardPool,
    new: &[Recipient],
    now: Timestamp,
) -> Result<u32, IntakeError> {
    let quotes = |_| IntakeError::Quotes;
    let holders = card_holders(tx, stored.job).await.map_err(quotes)?;
    let count = u32::try_from(holders.len()).map_err(|_| IntakeError::Quotes)?;
    let number = job_number(tx, stored.job)
        .await
        .map_err(|_| IntakeError::Job)?;
    let (_, language) = models::recipient(tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let lines = CardLines {
        sent_to: count,
        prefers_spanish: language == "es",
        fee_free: pool.fee_free,
    };
    for r in new {
        let speaks = askhava_businesses::api::business(tx, r.business)
            .await
            .map_err(|_| IntakeError::Quotes)?
            .speaks_spanish;
        let card = stored.card.text_for(&lines, speaks);
        let notice = CardNotice {
            number,
            category: &pool.category_name,
            town: &pool.town_name,
            card: &card,
        };
        let pair = (stored.job, r.business);
        if stored.facts.emergency {
            queue_emergency_card(tx, pair, &notice, now).await
        } else {
            queue_card_notice(tx, pair, &notice, (BUSINESS_TEXTING_ON, now)).await
        }
        .map_err(quotes)?;
    }
    Ok(count)
}

/// TXT-032, CON-021, QH-001: the cards-out text, once per job, when at least one business holds
/// the card, the homeowner consented and her number may be texted.
async fn cards_out(
    db: &mut PgConnection,
    keys: &CardsOutKeys,
    (conversation, stored): (Uuid, &StoredCard),
    (pool, count): (&CardPool, u32),
    now: Timestamp,
) -> Result<Option<Queued>, IntakeError> {
    let Some((contact, language)) = models::recipient(db, conversation).await? else {
        return Err(IntakeError::UnknownRequest);
    };
    if count == 0 || !may_text(db, contact).await? {
        return Ok(None);
    }
    let window = send_window(db, &keys.vault, contact, now)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    if !matches!(window, SendWindow::Now | SendWindow::Hold { .. }) {
        return Ok(None);
    }
    let number = job_number(db, stored.job)
        .await
        .map_err(|_| IntakeError::Job)?
        .to_string();
    let out = CardsOut {
        job: &number,
        count,
        trade: &pool.category_name,
        held_until: (!stored.facts.emergency).then(|| held_until(now)).flatten(),
        today: how_soon(db, conversation).await?.as_deref() == Some(TODAY),
        slow_at_most: SLOW_AT_MOST,
    };
    let reader = Reader {
        wanted: language_from_code(&language),
        spanish_on: true,
    };
    let body = cards_out_text(reader, &out, &[]).map_err(|_| IntakeError::Template)?;
    let to = (contact, CARDS_OUT_TEMPLATE);
    Ok(Some(
        queue_body(db, &keys.outbox, to, &[stored.job], &body).await?,
    ))
}

//! Late job cards for a business that became eligible (T677; CARD-018, CARD-024, CARD-020;
//! D-042, D-1021, D-4360, D-5075).
//!
//! `businesses` queues one `businesses.became_eligible` task (`business` only) when a business
//! moves into a status that receives cards, or adds a town, a job type or a tick
//! (`businesses::api::BECAME_ELIGIBLE_KIND`). [`run_became_eligible`] handles it: [`late_cards`]
//! lists the jobs whose card went out (`quotes.shown_card` with `sent_at`) in a category the
//! business ticked and whose card it does not hold, and for each, in one transaction, locks the
//! job's row (`jobs::api::lock_for_quote_change`), goes on only while the job `takes_late_cards`
//! (Open or No quotes yet), builds the pool from the stored card and the job's town as the card
//! sender does (`card_pool_at` at its `now`), keeps in it only the job's card holders and the business itself (so
//! only that business can get a new card, and overflow still reads the holders), records the cards
//! through [`send_late_cards_in_reach`] (sent now, so each card's 24 hours start now) and queues
//! its notice: `queue_card_notice`, or
//! `queue_emergency_card` for an emergency job's card. A rerun records nothing new and the notices
//! are deduplicated per job and business, so nothing goes twice (JOB-006).
//!
//! No card goes before the welcome email (CON-020), which `businesses` queues on the first
//! activation and sends later: while an eligible business may not get cards yet and its status
//! moved less than [`WELCOME_WAIT`] ago, the run queues the task again for [`WELCOME_RECHECK`]
//! later (`businesses::api::queue_eligible`, D-5076) and sends nothing.
//!
//! Known gap (D-5075): the job's ZIP, the homeowner's band answer and her language live in
//! `intake`, a higher layer this crate cannot read, so a late card matches by towns only (no
//! radius reach), with no band floor, and without the CARD-012 Spanish line. So no business with
//! a largest-job limit gets a late card while the band is unknown (CARD-001, BIZ-049, D-5077).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    LargestJob, business, capabilities, job_types, may_get_cards, queue_eligible,
};
use askhava_catalog::api::service_area_towns;
use askhava_core::api::{Claimed, HandlerError};
use askhava_jobs::api::{job_number, lock_for_quote_change};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::card_notices::{CardNotice, queue_card_notice};
use crate::card_pool::{CardPool, card_pool_at};
use crate::emergency_card_notice::queue_emergency_card;
use crate::job_text::{StoredCard, description, sent_card};
use crate::models::{self, CardLines, Pool, QuotesError, Recipient};
use crate::service::send_late_cards_in_reach;

/// Business texting is off until a business number exists (D-492), as in the card sender.
const BUSINESS_TEXTING_ON: bool = false;
/// The handler's fixed failure sentences (no values, rust brain R5).
const BAD_PAYLOAD: &str = "businesses.became_eligible payload is not a business";
const FAILED: &str = "the late card sender failed";
/// CON-020, D-5076: how long after a status move the run waits for the welcome email.
const WELCOME_WAIT: SignedDuration = SignedDuration::from_hours(24);
/// CON-020, D-5076: how soon a run held for the welcome email looks again.
const WELCOME_RECHECK: SignedDuration = SignedDuration::from_mins(5);

/// CARD-018, CARD-024, CARD-020, D-4360: sends business `id` the card of every job it may now
/// get while the job is Open or No quotes yet, with its card notice, at `now`; no other business
/// gets a card. A business with a largest-job limit gets none, since the band is unknown here
/// (CARD-001, BIZ-049, D-5077). Returns the jobs whose card `id` got.
///
/// # Errors
///
/// [`QuotesError::Business`] when the business cannot be read; [`QuotesError::Database`],
/// [`QuotesError::Job`] or [`QuotesError::LicenseMap`] from a job's send (that job keeps nothing).
pub async fn late_cards(
    db: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<Vec<Uuid>, QuotesError> {
    let found = business(db, id).await.map_err(|_| QuotesError::Business)?;
    if !found.status.receives_cards() {
        return Ok(Vec::new());
    }
    if !may_get_cards(db, id)
        .await
        .map_err(|_| QuotesError::Business)?
    {
        if now < found.status_changed_at + WELCOME_WAIT {
            queue_eligible(db, id, now + WELCOME_RECHECK)
                .await
                .map_err(|_| QuotesError::Business)?;
        }
        return Ok(Vec::new());
    }
    if limited(db, id).await? {
        return Ok(Vec::new());
    }
    let categories: BTreeSet<String> = job_types(db, id)
        .await
        .map_err(|_| QuotesError::Business)?
        .into_iter()
        .map(|c| c.category)
        .collect();
    let categories: Vec<String> = categories.into_iter().collect();
    let mut got = Vec::new();
    for job in uncarded_jobs(db, id, &categories).await? {
        if !job_late_cards(db, (job, id), now).await?.is_empty() {
            got.push(job);
        }
    }
    Ok(got)
}

/// CARD-001, BIZ-049, D-5077: business `id` answered a largest-job limit, which the unknown band
/// could exceed.
async fn limited(db: &mut PgConnection, id: Uuid) -> Result<bool, QuotesError> {
    let answers = capabilities(db, &[id])
        .await
        .map_err(|_| QuotesError::Business)?;
    Ok(answers
        .get(&id)
        .is_some_and(|c| c.largest != LargestJob::NoLimit))
}

/// The jobs whose card went out in one of `categories` and whose card business `id` does not
/// hold, oldest send first.
async fn uncarded_jobs(
    db: &mut PgConnection,
    id: Uuid,
    categories: &[String],
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT s.job_id FROM quotes.shown_card s WHERE s.sent_at IS NOT NULL \
         AND s.category = ANY($2) AND NOT EXISTS (SELECT 1 FROM quotes.job_card c \
         WHERE c.job_id = s.job_id AND c.business_id = $1) ORDER BY s.sent_at, s.job_id",
        id,
        categories
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)
}

/// CARD-018, CARD-024, D-1021, D-5077: business `id`'s late card of `job` and its notice in one
/// transaction, under the job's row lock; nothing unless the job `takes_late_cards`. The pool keeps
/// only the job's card holders and `id`, so no other business gets a card.
async fn job_late_cards(
    db: &mut PgConnection,
    (job, id): (Uuid, Uuid),
    now: Timestamp,
) -> Result<Vec<Recipient>, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let status = lock_for_quote_change(&mut tx, job)
        .await
        .map_err(|_| QuotesError::Job)?;
    if !status.takes_late_cards() {
        return Ok(Vec::new());
    }
    let Some(stored) = sent_card(&mut tx, job).await? else {
        return Ok(Vec::new());
    };
    let Some(town) = town_code(stored.card.town())? else {
        return Ok(Vec::new());
    };
    let text = description(&mut tx, job)
        .await?
        .map(|d| d.text())
        .unwrap_or_default();
    let pool = card_pool_at(&mut tx, ((&town, None), &stored.facts), text, None, now).await?;
    let holders = models::card_holders(&mut tx, job).await?;
    let kept: Vec<_> = pool
        .businesses
        .iter()
        .filter(|p| p.card.id == id || holders.contains(&p.card.id))
        .cloned()
        .collect();
    let chosen = Pool {
        businesses: &kept,
        pauses: &pool.pauses,
    };
    let matched = (&pool.job, &chosen, &pool.reach);
    let new = send_late_cards_in_reach(&mut tx, pool.map, job, matched, now).await?;
    notices(&mut tx, &stored, &pool, &new, now).await?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(new)
}

/// The town code of the town name the stored card shows (CARD-006), from the catalog's towns.
fn town_code(name: &str) -> Result<Option<String>, QuotesError> {
    Ok(service_area_towns()
        .map_err(|_| QuotesError::Database)?
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.code))
}

/// CARD-020, CARD-021: queues each new recipient's card notice with the card text sent to it,
/// counted from `now` (its 24 hours start at the send, PICK-001).
async fn notices(
    tx: &mut PgConnection,
    stored: &StoredCard,
    pool: &CardPool,
    new: &[Recipient],
    now: Timestamp,
) -> Result<(), QuotesError> {
    if new.is_empty() {
        return Ok(());
    }
    let holders = models::card_holders(tx, stored.job).await?;
    let lines = CardLines {
        sent_to: u32::try_from(holders.len()).map_err(|_| QuotesError::Database)?,
        prefers_spanish: false,
        fee_free: pool.fee_free,
    };
    let number = job_number(tx, stored.job)
        .await
        .map_err(|_| QuotesError::Job)?;
    for r in new {
        let speaks = business(tx, r.business)
            .await
            .map_err(|_| QuotesError::Business)?
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
            queue_emergency_card(tx, pair, &notice, now).await?;
        } else {
            queue_card_notice(tx, pair, &notice, (BUSINESS_TEXTING_ON, now)).await?;
        }
    }
    Ok(())
}

/// The `businesses.became_eligible` handler (CARD-018, JOB-006): runs [`late_cards`] for the
/// payload's business at `now`, the worker's clock; safe to run twice.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed send.
pub async fn run_became_eligible(
    pool: PgPool,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let Some(id) = task.payload["business"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut conn = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    late_cards(&mut conn, id, now)
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

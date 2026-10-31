//! Choosing and recording a job's card recipients (CARD-001 to CARD-003).
//!
//! The caller reads the job and the businesses (towns, ticks, licenses) through the lower crates'
//! `api` and passes them in; this records one `quotes.job_card` row per recipient, safe to run
//! twice. Sending the card email is a later task's.
//!
//! T184: [`send_job_cards`] records the full match (owner pauses, "jobs nobody else covers"),
//! [`send_late_cards`] gives a business that became eligible its card while the job is Open or No
//! quotes yet (CARD-018, D-042) and [`coverage`] counts it for intake (INT-017, INT-032).

use askhava_jobs::api::current;
use askhava_licensing::api::ClassMap;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, CardBusiness, CardJob, Coverage, Pool, QuotesError, Recipient};
use crate::rules;

/// CARD-001, CARD-002, CARD-003: chooses every business that receives the job's card, with no cap,
/// and records a card for each one not already holding one. Returns the recipients chosen.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn send_cards(
    db: &mut PgConnection,
    map: &ClassMap,
    job_id: Uuid,
    job: &CardJob,
    businesses: &[CardBusiness],
    now: Timestamp,
) -> Result<Vec<Recipient>, QuotesError> {
    let chosen = rules::recipients(map, job, businesses);
    models::insert_cards(db, job_id, &chosen, now).await?;
    Ok(chosen)
}

/// The businesses holding a card for the job, sorted by ID (CARD-001).
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn card_holders(db: &mut PgConnection, job_id: Uuid) -> Result<Vec<Uuid>, QuotesError> {
    models::card_holders(db, job_id).await
}

/// CARD-001, CARD-003, CARD-004, CARD-016: records a card for every business the full match
/// (`rules::match_job_held`, given the job's current card holders, so a rerun never turns
/// overflow on for a job a holder's checklist covers, D-1018) reaches that holds none yet, each
/// sent `now`. Returns only the new ones.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn send_job_cards(
    db: &mut PgConnection,
    map: &ClassMap,
    job_id: Uuid,
    job: &CardJob,
    pool: &Pool<'_>,
    now: Timestamp,
) -> Result<Vec<Recipient>, QuotesError> {
    let holders = models::card_holders(db, job_id).await?;
    let new: Vec<Recipient> = rules::match_job_held(map, job, pool, &holders)
        .into_iter()
        .filter(|r| holders.binary_search(&r.business).is_err())
        .collect();
    models::insert_cards(db, job_id, &new, now).await?;
    Ok(new)
}

/// CARD-018, D-042: once a business becomes eligible (newly active, unpaused or newly ticked),
/// the caller runs this; while the job is Open or No quotes yet each business newly reached gets
/// its card now, so its 24 hours start now. Any other status sends nothing. The status read and
/// the inserts share one transaction; a move committed between them orders as a card sent just
/// before the move (D-1021).
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`]; nothing is kept on an error.
pub async fn send_late_cards(
    db: &mut PgConnection,
    map: &ClassMap,
    job_id: Uuid,
    job: &CardJob,
    pool: &Pool<'_>,
    now: Timestamp,
) -> Result<Vec<Recipient>, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let status = current(&mut tx, job_id)
        .await
        .map_err(|_| QuotesError::Job)?
        .status;
    if !status.takes_late_cards() {
        return Ok(Vec::new());
    }
    let sent = send_job_cards(&mut tx, map, job_id, job, pool, now).await?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(sent)
}

/// INT-017, INT-032: the coverage intake tells the homeowner, counted with the shipped license
/// map exactly as [`send_job_cards`] would send the card now.
///
/// # Errors
///
/// [`QuotesError::LicenseMap`] when the shipped map did not load.
pub fn coverage(job: &CardJob, pool: &Pool<'_>) -> Result<Coverage, QuotesError> {
    let map = ClassMap::shipped().map_err(|_| QuotesError::LicenseMap)?;
    Ok(rules::coverage(map, job, pool))
}

//! Choosing and recording a job's card recipients (CARD-001 to CARD-003).
//!
//! The caller reads the job and the businesses (towns, ticks, licenses) through the lower crates'
//! `api` and passes them in; this records one `quotes.job_card` row per recipient, safe to run
//! twice. Sending the card email is a later task's.

use askhava_licensing::api::ClassMap;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, CardBusiness, CardJob, QuotesError, Recipient};
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

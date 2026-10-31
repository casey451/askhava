//! The records matching reads and returns, and the `quotes.job_card` query (migration
//! `0067_quotes_matching.sql`; CARD-001 to CARD-003, LIC-035).
//!
//! Nothing here holds a homeowner's detail: a job is its offer towns, its candidate job types and
//! its scrubbed text (PRIV-001); a business is IDs, codes and ticks.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_licensing::api::{Holder, Job, NoCardReason};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// Why a quotes call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QuotesError {
    /// A database call failed.
    #[error("quotes database call failed")]
    Database,
}

/// A town a job is offered in: the job's own town first, then any neighbouring towns (CARD-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferTown {
    /// The catalog town code, as `businesses.town` stores it.
    pub code: String,
    /// Its two-letter state; only `AZ` towns are ever offered (INT-022, D-382).
    pub state: String,
}

/// What matching knows of one job (CARD-001, CARD-002).
#[derive(Debug, Clone)]
pub struct CardJob {
    /// The towns the job is offered in.
    pub towns: Vec<OfferTown>,
    /// Every remaining candidate job type, each with its category's license rule (LIC-043).
    pub candidates: Vec<Job>,
    /// The scrubbed request text, read only for the handyman block (AI-012, LIC-021).
    pub text: String,
}

/// What matching knows of one business (CARD-001).
#[derive(Debug, Clone)]
pub struct CardBusiness {
    /// The business ID.
    pub id: Uuid,
    /// Its account status; only `Active` receives cards (CARD-016).
    pub status: AccountStatus,
    /// Whether its identity and license or city-list checks stand verified (BIZ-016, CARD-015).
    pub verified: bool,
    /// The town codes it serves (BIZ-010).
    pub towns: BTreeSet<String>,
    /// The (category, job type) pairs it ticked (BIZ-004).
    pub ticked: BTreeSet<(String, String)>,
    /// The ROC classes and subclassifications on its state record, as named there (LIC-035).
    pub classes: Vec<String>,
    /// Its other licenses; `roc_covers` is ignored and filled per job type from `classes`.
    pub licenses: Holder,
}

/// Whether one business receives the card (CARD-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardDecision {
    /// It receives the card.
    Card,
    /// It receives the card as a handyman: under $1,000 with no permit (LIC-014, LIC-019).
    HandymanCard,
    /// It does not.
    Skip(Skip),
}

/// Why a business does not receive a card, first failing check first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Its status is not `Active` (CARD-001, CARD-016).
    NotActive,
    /// It is not verified (CARD-001).
    NotVerified,
    /// It serves none of the job's Arizona offer towns (CARD-001, CARD-002).
    OutOfTown,
    /// The job has no candidate job type left.
    NoCandidates,
    /// It ticked none of the candidate job types (CARD-001).
    NothingTicked,
    /// Its licenses do not cover one candidate job type (CARD-001, LIC-035, LIC-043).
    NotCovered(NoCardReason),
}

/// One business chosen to receive the card (CARD-001, CARD-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipient {
    /// The business ID.
    pub business: Uuid,
    /// Whether the card carries the handyman condition (LIC-014, LIC-019).
    pub handyman: bool,
}

/// Records one card per recipient; a card already sent for this job and business is kept, so a
/// second run adds nothing (JOB-006). Returns how many rows were added.
pub(crate) async fn insert_cards(
    db: &mut PgConnection,
    job: Uuid,
    recipients: &[Recipient],
    now: Timestamp,
) -> Result<u64, QuotesError> {
    let businesses: Vec<Uuid> = recipients.iter().map(|r| r.business).collect();
    let handyman: Vec<bool> = recipients.iter().map(|r| r.handyman).collect();
    let done = sqlx::query!(
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) \
         SELECT $1, b, h, $4 FROM unnest($2::uuid[], $3::bool[]) AS t(b, h) \
         ON CONFLICT (job_id, business_id) DO NOTHING",
        job,
        &businesses,
        &handyman,
        now.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(done.rows_affected())
}

/// The businesses holding a card for this job, sorted by ID.
pub(crate) async fn card_holders(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT business_id FROM quotes.job_card WHERE job_id = $1 ORDER BY business_id",
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)
}

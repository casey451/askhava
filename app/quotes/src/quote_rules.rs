//! The rules a quote is checked against, read from the stored job, catalog and card (PICK-053,
//! LIC-019, LIC-025; D-3675).
//!
//! [`quote_rules`] builds the [`QuoteRules`] that `intake::parse_quote` takes, so the served quote
//! form checks a quote against its job: the job type and category come from the job's latest
//! `jobs::api::job_facts` row (as `quote_facts` reads the job type), the format, construction flag
//! and license rule from `catalog::api::category`, the handyman flag from the business's own
//! `quotes.job_card` row (the card it was sent) and the certification tick from the caller. It
//! never guesses: a job with no facts, a category or job type not in the catalog, or a business
//! that was sent no card reads none.

use askhava_catalog::api::category;
use askhava_jobs::api::job_facts;
use askhava_licensing::api::LicenseKind;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::formats::format_for;
use crate::intake::QuoteRules;
use crate::models::QuotesError;

/// PICK-053, LIC-019, LIC-025, D-3675: the rules `business`'s quote on `job` is checked against.
/// The job type is the first candidate of the job's latest facts row and the category that row's;
/// `format` is `format_for` of the stored category and job type (PICK-053); `construction_type`
/// and `license` are the category's; `handyman` is the business's card's flag (LIC-019); and
/// `certified` is the form's tick, passed through (LIC-025). None when the job has no facts row,
/// the category or the job type is not in the catalog, or no card went to the business.
///
/// # Errors
///
/// [`QuotesError::Job`] when the job's facts cannot be read; [`QuotesError::Database`] when the
/// catalog or the card cannot be read, or the stored category's format or license kind is not one
/// the code knows.
pub async fn quote_rules(
    conn: &mut PgConnection,
    job: Uuid,
    business: Uuid,
    certified: bool,
) -> Result<Option<QuoteRules>, QuotesError> {
    let facts = job_facts(conn, job).await.map_err(|_| QuotesError::Job)?;
    let Some(facts) = facts else { return Ok(None) };
    let Some(job_type) = facts.job_types.first() else {
        return Ok(None);
    };
    let stored = category(conn, &facts.category)
        .await
        .map_err(|_| QuotesError::Database)?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    if !stored.job_types.iter().any(|t| &t.name == job_type) {
        return Ok(None);
    }
    let Some(handyman) = card_handyman(conn, job, business).await? else {
        return Ok(None);
    };
    let format = format_for(&stored, job_type).ok_or(QuotesError::Database)?;
    let license = LicenseKind::parse(&stored.license_kind).map_err(|_| QuotesError::Database)?;
    Ok(Some(QuoteRules {
        format,
        construction_type: stored.construction_type,
        license,
        handyman,
        certified,
    }))
}

/// LIC-019: the handyman flag of the card `job` sent `business`, or none when no card went to it.
async fn card_handyman(
    conn: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Option<bool>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT handyman FROM quotes.job_card WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| QuotesError::Database)
}

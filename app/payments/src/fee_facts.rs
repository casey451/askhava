//! The job facts the introduction fee by job size needs at the pick, resolved from the catalog
//! (FEE-004, FEE-005, FEE-007, FEE-045, REC-001; T340, D-2331, D-3547).
//!
//! `picks` reads a job's latest stored category and candidate job types through `jobs::api` and
//! passes the codes here; [`resolve_fee_facts`] looks them up through `catalog::api` (payments may
//! call catalog, picks may not). With several candidates the category decides the visit tier,
//! any fee-free candidate makes the pick fee-free, and the pick is one-time only when every
//! candidate is one-time. No row, an unknown category or job type, or a set tier that is not a
//! visit tier gives `None`: the facts are unsettled and the caller decides (a non-emergency pick
//! is refused, an emergency one proceeds on [`JobFeeFacts::EMERGENCY_DEFAULT`] with an alert).

use askhava_catalog::api::{Category, category};
use sqlx::PgConnection;

use crate::rules::{FeeJob, PeriodJob, RecurringCap, Service, VisitTier};

/// A job's stored codes as `jobs::api::job_facts` gives them: the category code and the
/// remaining candidate job types (never empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobCodes<'a> {
    /// The catalog category code.
    pub category: &'a str,
    /// The remaining candidate job type codes.
    pub job_types: &'a [String],
}

/// What the catalog makes of a job's codes for the fee (FEE-004, FEE-045, REC-001, D-2331).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobFeeFacts {
    /// The category's set tier for a diagnostic-visit quote.
    pub visit_tier: VisitTier,
    /// Some remaining candidate is one of the catalog's `fee_free_job_types`.
    pub fee_free: bool,
    /// Every remaining candidate is one of its recurring category's `one_time_job_types`.
    pub one_time: bool,
}

impl JobFeeFacts {
    /// D-2331: an emergency job with unsettled facts is priced as a one-time job at the default
    /// $25 set tier, never fee-free.
    pub const EMERGENCY_DEFAULT: Self = Self {
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
        one_time: true,
    };

    /// FEE-004, FEE-045, REC-001, D-2210, D-2331: the job as the fee sees it for a quote whose
    /// format is a diagnostic visit (`diagnostic_visit`) and which has a period or not. A one-time
    /// job is a [`Service::OneTimeJobType`]; otherwise the quote's period decides: a period is a
    /// recurring job type (capped at [`RecurringCap::DEFAULT`]), none is not recurring.
    #[must_use]
    pub const fn period_job(self, diagnostic_visit: bool, has_period: bool) -> PeriodJob {
        let service = match (self.one_time, has_period) {
            (true, _) => Service::OneTimeJobType,
            (false, true) => Service::RecurringJobType,
            (false, false) => Service::NotRecurring,
        };
        PeriodJob {
            job: FeeJob {
                diagnostic_visit,
                visit_tier: self.visit_tier,
                fee_free: self.fee_free,
            },
            service,
            cap: RecurringCap::DEFAULT,
        }
    }
}

/// Why [`resolve_fee_facts`] could not answer. It carries no code or database message (R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeeFactsError {
    /// The catalog read failed.
    #[error("the catalog could not be read")]
    Database,
}

/// FEE-004, FEE-005, FEE-045, D-2331: resolves a job's latest stored `codes` (none when the job
/// has no row) through the catalog. `Ok(None)` when the facts are unsettled: no row, a category
/// the catalog does not hold, a candidate that is not one of that category's job types (a set
/// spanning categories included) or a set tier that is not $25 or $75.
///
/// # Errors
///
/// [`FeeFactsError::Database`] when the catalog read fails.
pub async fn resolve_fee_facts(
    conn: &mut PgConnection,
    codes: Option<JobCodes<'_>>,
) -> Result<Option<JobFeeFacts>, FeeFactsError> {
    let Some(codes) = codes else {
        return Ok(None);
    };
    let stored = category(conn, codes.category)
        .await
        .map_err(|_| FeeFactsError::Database)?;
    Ok(stored.and_then(|c| facts_of(&c, codes.job_types)))
}

/// FEE-004, FEE-045, REC-001, D-2331: the tie rules over `job_types`, each of which must be one
/// of `category`'s job types; the category's `trip_fee_tier` is the visit tier.
fn facts_of(category: &Category, job_types: &[String]) -> Option<JobFeeFacts> {
    let found = job_types
        .iter()
        .map(|code| category.job_types.iter().find(|t| t.name == *code))
        .collect::<Option<Vec<_>>>()?;
    let visit_tier = VisitTier::from_dollars(category.trip_fee_tier).ok()?;
    Some(JobFeeFacts {
        visit_tier,
        fee_free: found.iter().any(|t| t.fee_free),
        one_time: found.iter().all(|t| t.one_time),
    })
}

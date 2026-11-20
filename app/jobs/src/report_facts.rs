//! Each listed job's category and job types for the monthly intake accuracy report (OPS-040;
//! T434, D-3753, D-3890).
//!
//! [`report_facts`] reads, in one statement however many jobs are listed, the latest
//! `jobs.job_facts` row by `seq` of each job: its catalog category code and all of its candidate
//! job type codes. A job with no row is left out, as [`crate::job_facts::job_facts`]' `None`.
//! Codes only, no personal data.

use std::collections::BTreeMap;

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::job_facts::FactsError;

/// One job's latest facts as the report groups them (OPS-040).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportFacts {
    /// The catalog category code.
    pub category: String,
    /// Every candidate job type code of the latest row.
    pub job_types: Vec<String>,
}

/// OPS-040, D-3890: the [`ReportFacts`] of each job in `jobs` that has a facts row, by job ID.
///
/// # Errors
///
/// [`FactsError::Database`].
pub async fn report_facts(
    conn: &mut PgConnection,
    jobs: &[Uuid],
) -> Result<BTreeMap<Uuid, ReportFacts>, FactsError> {
    let rows = sqlx::query!(
        "SELECT DISTINCT ON (job_id) job_id, category, job_types FROM jobs.job_facts \
         WHERE job_id = ANY($1) ORDER BY job_id, seq DESC",
        jobs
    )
    .fetch_all(conn)
    .await
    .map_err(|_| FactsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let facts = ReportFacts {
                category: r.category,
                job_types: r.job_types,
            };
            (r.job_id, facts)
        })
        .collect())
}

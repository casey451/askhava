//! A job's business of record (JOB-016, BIZ-011, D-4423): [`settled_pick`] answers the job's newest
//! pick, by `picked_at`, whose status is `paid`, `free` or `no_show`, ended or not, so the business
//! a PROBLEM report is about is found also after the pick became a no-show. A `fraud_refunded` pick
//! was never released, so it is never the business of record. `settle::accepted_pick` answers only
//! an unended `paid` or `free` pick and so misses the no-show case.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;
use crate::status::PickStatus;

/// A job's settled pick: the pick and the business it went to (JOB-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettledPick {
    /// The pick, `paid`, `free` or `no_show`.
    pub pick: Uuid,
    /// The business picked.
    pub business: Uuid,
}

/// The statuses of a pick that settled and was released (D-4423).
const SETTLED: [PickStatus; 3] = [PickStatus::Paid, PickStatus::Free, PickStatus::NoShow];

/// JOB-016, BIZ-011, D-4423: `job`'s newest pick by `picked_at` (ties by ID) that is `paid`, `free`
/// or `no_show`, ended or not; `None` when the job has none. Reads only.
///
/// # Errors
/// [`PicksError::Database`].
pub async fn settled_pick(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Option<SettledPick>, PicksError> {
    let codes: Vec<&str> = SETTLED.iter().map(|s| s.code()).collect();
    let row = sqlx::query!(
        "SELECT id, business_id FROM picks.pick WHERE job_id = $1 AND status = ANY($2)
         ORDER BY picked_at DESC, id DESC LIMIT 1",
        job,
        &codes as &[&str],
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.map(|r| SettledPick {
        pick: r.id,
        business: r.business_id,
    }))
}

//! The owner flags' pick inputs (OPS-027, OPS-034, PICK-019; D-284, D-464, D-4280, D-4311),
//! read-only, from `picks.pick` only.
//!
//! Every window is half-open on the pick's `picked_at` (`from` counted, `to` not), as BIZ-025's
//! lapsed-pick count. [`lapsed_businesses`] lists the businesses with `min` or more lapsed
//! (`expired`) picks; [`wins`] counts the paid or free picks on given jobs, for T524's quotes per
//! win; [`picked_jobs`] says which given jobs have any pick row at all, for `intake`'s repeat
//! requests. Business and job IDs and counts only; no contact field (PRIV-028).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;
use crate::status::PickStatus;

/// OPS-034, PICK-019, D-284: each business with `min` or more picks made in `[from, to)` that
/// lapsed (`expired`), with its count, by business ID.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn lapsed_businesses(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
    min: i64,
) -> Result<Vec<(Uuid, i64)>, PicksError> {
    let rows = sqlx::query!(
        "SELECT business_id, count(*) AS \"n!\" FROM picks.pick \
         WHERE status = $1 AND picked_at >= $2 AND picked_at < $3 \
         GROUP BY business_id HAVING count(*) >= $4 ORDER BY business_id",
        PickStatus::Expired.code(),
        from.to_sqlx(),
        to.to_sqlx(),
        min
    )
    .fetch_all(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(rows.into_iter().map(|r| (r.business_id, r.n)).collect())
}

/// OPS-027, D-464: the wins on `jobs`: their picks made in `[from, to)` now `paid` or `free`.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn wins(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<i64, PicksError> {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM picks.pick WHERE job_id = ANY($1) \
           AND (status = $2 OR status = $3) AND picked_at >= $4 AND picked_at < $5",
        jobs,
        PickStatus::Paid.code(),
        PickStatus::Free.code(),
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| PicksError::Database)
}

/// OPS-034, D-284: the IDs of `jobs` with any `picks.pick` row, whatever its status or time, in
/// ID order.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn picked_jobs(db: &mut PgConnection, jobs: &[Uuid]) -> Result<Vec<Uuid>, PicksError> {
    sqlx::query_scalar!(
        "SELECT DISTINCT job_id FROM picks.pick WHERE job_id = ANY($1) ORDER BY job_id",
        jobs
    )
    .fetch_all(db)
    .await
    .map_err(|_| PicksError::Database)
}

//! The pilot scorecard's pick inputs (OPS-039; D-458, D-468, D-3753, D-3877, D-3920), read-only,
//! from `picks.pick` only.
//!
//! No `picks` table stores a trade or a quote fact, so each reader takes job IDs from a `quotes`
//! reader: the trade's jobs (`quotes::api::jobs_by_trade`), the requests with 1 or with 2 or more
//! quotes (`quotes::api::requests_with_quotes`, windowed on the request's own first card), or the
//! diagnostic-visit quotes (`quotes::api::diagnostic_quotes`). Shares only; `None` when empty.

use askhava_payments::api::Share;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;
use crate::status::{PickStatus, confirmed};

/// OPS-039, D-468: picks per request with quotes, and picks among requests with 2 or more quotes:
/// of `requests` (the window's requests with enough quotes), the part with a pick made before
/// `to`; a request picked again after a lapse counts once. `None` with no request.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn picked_requests(
    db: &mut PgConnection,
    requests: &[Uuid],
    to: Timestamp,
) -> Result<Option<Share>, PicksError> {
    let part = sqlx::query_scalar!(
        "SELECT count(DISTINCT job_id) AS \"n!\" FROM picks.pick \
         WHERE job_id = ANY($1) AND picked_at < $2",
        requests,
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| PicksError::Database)?;
    let whole = i64::try_from(requests.len()).map_err(|_| PicksError::Database)?;
    Ok((whole > 0).then_some(Share { part, whole }))
}

/// OPS-039: picks confirmed rather than lapsed: of the picks on `jobs` made in the window that
/// were confirmed (`status::confirmed`, as BIZ-025's numbers) or lapsed (`expired`), the
/// confirmed part. `None` with neither.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn confirmed_share(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<Share>, PicksError> {
    let done: Vec<String> = PickStatus::ALL
        .into_iter()
        .filter(|s| confirmed(*s))
        .map(|s| s.code().to_owned())
        .collect();
    let r = sqlx::query!(
        "SELECT count(*) FILTER (WHERE status = ANY($4::text[])) AS \"part!\", \
           count(*) AS \"whole!\" FROM picks.pick WHERE job_id = ANY($1) AND picked_at >= $2 \
           AND picked_at < $3 AND (status = ANY($4::text[]) OR status = $5)",
        jobs,
        from.to_sqlx(),
        to.to_sqlx(),
        &done,
        PickStatus::Expired.code()
    )
    .fetch_one(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok((r.whole > 0).then_some(Share {
        part: r.part,
        whole: r.whole,
    }))
}

/// OPS-039: the share of picks that were diagnostic-visit quotes: of the picks on `jobs` made in
/// the window, the part whose (job, business) is in `diagnostic`. `None` with no pick.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn diagnostic_share(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
    diagnostic: &[(Uuid, Uuid)],
) -> Result<Option<Share>, PicksError> {
    let (quote_jobs, businesses): (Vec<Uuid>, Vec<Uuid>) = diagnostic.iter().copied().unzip();
    let r = sqlx::query!(
        "SELECT count(*) FILTER (WHERE (job_id, business_id) IN \
           (SELECT * FROM unnest($4::uuid[], $5::uuid[]))) AS \"part!\", count(*) AS \"whole!\" \
         FROM picks.pick WHERE job_id = ANY($1) AND picked_at >= $2 AND picked_at < $3",
        jobs,
        from.to_sqlx(),
        to.to_sqlx(),
        &quote_jobs,
        &businesses
    )
    .fetch_one(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok((r.whole > 0).then_some(Share {
        part: r.part,
        whole: r.whole,
    }))
}

//! One business's pick counts for the portal's "Your numbers" panel (BIZ-025; D-250, D-284;
//! T451).
//!
//! [`pick_numbers`] counts, for one business and one Arizona month `[from, to)` the caller gives,
//! the picks made in the month, how many of those were confirmed (a status only a Confirm reaches,
//! `status::confirmed`) and how many ran out of time ([`PickStatus::Expired`], D-284). It also lists
//! `(job, connected_at)` for every release of this business, whatever its month, so that
//! `jobs::api::finished_counts` can count the homeowner's DONE, NO and other replies (D-250): the
//! release is the moment the business's pick connected the job. IDs and counts only.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;
use crate::status::{PickStatus, confirmed};

/// One business's pick counts for one month (BIZ-025).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickNumbers {
    /// Picks of this business made in the month.
    pub picks: u32,
    /// Of those, the picks the business confirmed.
    pub confirms: u32,
    /// Of those, the picks that ran out of time (D-284).
    pub lapsed: u32,
    /// `(job, connected_at)` for every release of this business, any month (D-250).
    pub connected: Vec<(Uuid, Timestamp)>,
}

/// BIZ-025, D-284, D-250: `business`'s picks, confirms and lapsed picks made in `[from, to)`,
/// and every `(job, connected_at)` its releases made.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn pick_numbers(
    db: &mut PgConnection,
    business: Uuid,
    (from, to): (Timestamp, Timestamp),
) -> Result<PickNumbers, PicksError> {
    let done: Vec<String> = PickStatus::ALL
        .into_iter()
        .filter(|s| confirmed(*s))
        .map(|s| s.code().to_owned())
        .collect();
    let row = sqlx::query!(
        r#"SELECT count(*) AS "picks!",
                  count(*) FILTER (WHERE status = ANY($4::text[])) AS "confirms!",
                  count(*) FILTER (WHERE status = $5) AS "lapsed!"
             FROM picks.pick
            WHERE business_id = $1 AND picked_at >= $2 AND picked_at < $3"#,
        business,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        &done,
        PickStatus::Expired.code(),
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| PicksError::Database)?;
    let connected = sqlx::query!(
        "SELECT job_id, released_at FROM picks.release WHERE business_id = $1 ORDER BY released_at",
        business,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(PickNumbers {
        picks: count(row.picks),
        confirms: count(row.confirms),
        lapsed: count(row.lapsed),
        connected: connected
            .into_iter()
            .map(|r| (r.job_id, r.released_at.to_jiff()))
            .collect(),
    })
}

/// A database count as a panel number, saturating.
fn count(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

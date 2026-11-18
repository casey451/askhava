//! The repeat-requester flag's input (OPS-034; D-284, D-4280, D-4311), read-only.
//!
//! A request is one `intake.conversation`, counted when opened in `[from, to)`. A homeowner is
//! her vault contact ID (`intake.homeowner.contact_id`), never a phone. "Never picked" means no
//! request of hers in the window has a `picks.pick` row on its job, read through
//! `picks::api::picked_jobs`, so `intake` queries no `picks` table; a request not yet carded has
//! no job and so no pick (D-4311). Vault IDs and counts only; no contact field (PRIV-028).

use std::collections::BTreeMap;

use askhava_picks::api::picked_jobs;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::IntakeError;

/// OPS-034, D-284, D-4311: each homeowner (vault contact ID) with `min` or more requests opened
/// in `[from, to)` and no pick on any of them, with her request count, by vault ID.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn repeat_requesters(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
    min: i64,
) -> Result<Vec<(Uuid, i64)>, IntakeError> {
    let rows = sqlx::query!(
        "SELECT h.contact_id, c.job_id FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE c.opened_at >= $1 AND c.opened_at < $2",
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let jobs: Vec<Uuid> = rows.iter().filter_map(|r| r.job_id).collect();
    let picked = picked_jobs(db, &jobs)
        .await
        .map_err(|_| IntakeError::Database)?;
    let mut per_home: BTreeMap<Uuid, Option<i64>> = BTreeMap::new();
    for r in rows {
        let seen = per_home.entry(r.contact_id).or_insert(Some(0));
        let was_picked = r.job_id.is_some_and(|j| picked.contains(&j));
        *seen = seen.filter(|_| !was_picked).map(|n| n + 1);
    }
    Ok(per_home
        .into_iter()
        .filter_map(|(home, n)| n.filter(|n| *n >= min).map(|n| (home, n)))
        .collect())
}

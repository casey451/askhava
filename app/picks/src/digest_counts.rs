//! The weekly digest's no-shows (OPS-030; D-219, D-4044, D-4202; T605).
//!
//! [`no_shows`] lists the picks that ended as `no_show` in a half-open window (`from` counted,
//! `to` not), by `picks.pick.ended_at`, each with its job number from `jobs` (JOB-024). It returns
//! IDs, job numbers and times only, never a homeowner or business contact detail.

use askhava_jobs::api::job_number;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;

/// One no-show for the weekly digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoShow {
    /// The pick.
    pub pick: Uuid,
    /// The job's number, the one its texts carry.
    pub job_number: i64,
    /// The business picked.
    pub business: Uuid,
    /// When the pick ended as a no-show.
    pub ended_at: Timestamp,
}

/// OPS-030: the picks with status `no_show` that ended at or after `from` and before `to`, oldest
/// first.
///
/// # Errors
///
/// [`PicksError::Database`]; [`PicksError::Job`] when a job number cannot be read.
pub async fn no_shows(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<NoShow>, PicksError> {
    let rows = sqlx::query!(
        r#"SELECT id, job_id, business_id, ended_at AS "ended_at!" FROM picks.pick
           WHERE status = 'no_show' AND ended_at >= $1 AND ended_at < $2
           ORDER BY ended_at, id"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| PicksError::Database)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let number = job_number(db, r.job_id)
            .await
            .map_err(|_| PicksError::Job)?;
        out.push(NoShow {
            pick: r.id,
            job_number: number,
            business: r.business_id,
            ended_at: r.ended_at.to_jiff(),
        });
    }
    Ok(out)
}

//! INT-029's emergency choice on a job already open (PICK-044, PICK-043, D-4748).
//!
//! A job is created with `emergency` from the first text's 911-list match; the homeowner may choose
//! "emergency" later, at INT-006's urgent choice. [`mark_emergency`] then sets the flag false to
//! true, never back, so every reader (PICK-044's 60-minute Confirm and card-fix windows, the
//! facts rule of T340, the HOLD refusal of PICK-043, the release order and the quiet-hours
//! exemptions) sees the chosen emergency. Like a guarded update it bumps `version` and
//! `timer_version`, so a pending timer counted on the ordinary clock goes stale, and it queues the
//! current status's timer again under the new fence, in the caller's transaction; the daily timer
//! backstop finds that timer by the same key (T366).

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;
use crate::service::queue_timer;
use crate::status::JobStatus;

/// The audit subject kind of a job (PRIV-028).
const SUBJECT: &str = "jobs.job";

/// Marks job `id` an emergency at `at` (INT-029, PICK-044, PICK-043, D-4748): `emergency` goes from
/// false to true only, with `version` and `timer_version` bumped, one audit row and the status's
/// timer queued again from its current start under the new fence. A final job, one already marked
/// or none with this ID changes nothing. Returns `true` only when this call marked it. Runs in the
/// caller's transaction.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn mark_emergency(
    tx: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, JobsError> {
    let finals: Vec<&str> = JobStatus::ALL
        .into_iter()
        .filter(|s| s.targets().is_empty())
        .map(JobStatus::code)
        .collect();
    let row = sqlx::query!(
        "UPDATE jobs.job SET emergency = true, version = version + 1, \
         timer_version = timer_version + 1 \
         WHERE id = $1 AND NOT emergency AND status <> ALL($2::text[]) \
         RETURNING status, version, timer_version, timer_from",
        id,
        &finals as &[&str],
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| JobsError::Database)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let status = JobStatus::from_code(&row.status).ok_or(JobsError::Database)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: "emergency_marked",
        subject_kind: SUBJECT,
        subject_id: id,
        change: Change::Field("emergency"),
    };
    audit(&mut *tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    let fenced = (row.version, row.timer_version, true);
    queue_timer(tx, id, status, fenced, row.timer_from.to_jiff()).await?;
    tracing::debug!(job = %id, at = %at, "job marked an emergency");
    Ok(true)
}

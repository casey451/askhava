//! The final-status event (PICK-026, D-044, D-491, D-4361): a move into a final status queues one
//! [`FINAL_STATUS_KIND`] task `{job, status}` in the move's transaction, so `picks` (which `jobs`
//! may not call) tells every quoter but the connected one "This job is no longer active." once.
//! `server` registers `picks::api::run_final_status` under the kind.

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;
use crate::status::JobStatus;

/// The `core.task` kind a move into a final status queues; `picks` handles it (PICK-026, D-044).
pub const FINAL_STATUS_KIND: &str = "jobs.final_status";

/// PICK-026, D-044, D-4361: queues one [`FINAL_STATUS_KIND`] task for job `job` when `status` is
/// final (`JobStatus::is_final`), due at `at`, on `db` (the move's transaction); a final status
/// has no arrow out, so the key is the job's alone. Nothing for any other status.
///
/// # Errors
///
/// [`JobsError::Database`].
pub(crate) async fn queue_final_status(
    db: &mut PgConnection,
    job: Uuid,
    status: JobStatus,
    at: Timestamp,
) -> Result<(), JobsError> {
    if !status.is_final() {
        return Ok(());
    }
    let payload = json!({ "job": job, "status": status.code() });
    let mut task = NewTask::new(FINAL_STATUS_KIND, payload, at);
    task.dedupe_key = Some(format!("{FINAL_STATUS_KIND}.{job}"));
    enqueue(db, &task)
        .await
        .map(drop)
        .map_err(|_| JobsError::Database)
}

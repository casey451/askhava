//! The masked call line's close after a job ends early: one queued task per move (CALL-009,
//! CALL-010, JOB-005; D-1694, D-4338).
//!
//! `jobs` and `intake` may not call `calls` (`kit/layers.json`, D-491), so a cancel, a check-in
//! PROBLEM or a check-in NO queues one [`LINE_CLOSE_KIND`] task on the mover's connection, right
//! after the move applied, so it commits or rolls back with the move (R11). Its payload is
//! `{"job", "number", "reason"}` (IDs, the job number and a reason code; never a phone), and
//! `server` runs `calls::api::run_close_line` on it. The dedupe key names the job's version after
//! the move: `core.task` keeps a key once its task is done, so a per-job key would drop a later
//! move's close (a re-pick after a no-show); `close_line` is idempotent, so a repeat is harmless.

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, JobsError};
use crate::service::job_number;

/// The `core.task` kind that closes a job's masked line; `calls` handles it (D-4338).
pub const LINE_CLOSE_KIND: &str = "calls.close_line";

/// Why a job's masked line closes (CALL-009, CALL-010); its [`code`](Self::code) is the payload's
/// `reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCloseReason {
    /// The homeowner cancelled the job (CALL-010: the business is told, with no strike).
    Cancelled,
    /// A check-in PROBLEM closed the job (CALL-009, JOB-015).
    Complaint,
    /// A check-in NO: the business never reached her (CALL-009, JOB-015).
    NoShow,
}

impl LineCloseReason {
    /// The payload's reason code: `cancelled`, `complaint` or `no_show`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Complaint => "complaint",
            Self::NoShow => "no_show",
        }
    }
}

/// CALL-009, CALL-010, JOB-005, D-4338: queues one [`LINE_CLOSE_KIND`] task for job `job` closing
/// its masked line for `reason`, due at `now`, on `db` (the move's transaction). Call it only after
/// the move applied; the key is `calls.close_line.<job>.<version after the move>`. Returns whether
/// a task was new.
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn queue_line_close(
    db: &mut PgConnection,
    job: Uuid,
    reason: LineCloseReason,
    now: Timestamp,
) -> Result<bool, JobsError> {
    let number = job_number(db, job).await?;
    let version = models::load(db, job).await?.version;
    let payload = json!({ "job": job, "number": number, "reason": reason.code() });
    let mut task = NewTask::new(LINE_CLOSE_KIND, payload, now);
    task.dedupe_key = Some(format!("{LINE_CLOSE_KIND}.{job}.{version}"));
    enqueue(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| JobsError::Database)
}

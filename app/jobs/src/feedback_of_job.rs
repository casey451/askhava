//! A job's own feedback for the homeowner's "Download my data" file (T820, D-5399, D-5526,
//! D-5709; PRIV-031, JOB-017).
//!
//! `feedback::shown` gives only ticked, kept feedback for the public pages. [`of_job`] gives the
//! one feedback of a job, shared or not and removed or not, with its share tick, so her file holds
//! all she wrote; words blanked by the purge or "Delete my data" (`body_purged_at`) are not held
//! and are left out. Reached as `jobs::api::feedback::of_job`.

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;

/// A homeowner's feedback on one job, as she sent it. `Debug` prints no words (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct GivenFeedback {
    /// The words exactly as written.
    pub body: String,
    /// Whether she ticked the box to share it.
    pub share: bool,
    /// When it was received.
    pub received_at: Timestamp,
}

impl std::fmt::Debug for GivenFeedback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GivenFeedback")
            .field("share", &self.share)
            .field("received_at", &self.received_at)
            .finish_non_exhaustive()
    }
}

/// PRIV-031, JOB-017, D-5526: job `job`'s feedback with its share tick, or `None` when it has
/// none or its words were blanked.
///
/// # Errors
/// [`JobsError::Database`].
pub async fn of_job(db: &mut PgConnection, job: Uuid) -> Result<Option<GivenFeedback>, JobsError> {
    let row = sqlx::query!(
        "SELECT body, share, received_at FROM jobs.feedback \
         WHERE job_id = $1 AND body_purged_at IS NULL",
        job
    )
    .fetch_optional(db)
    .await
    .map_err(|_| JobsError::Database)?;
    Ok(row.map(|r| GivenFeedback {
        body: r.body,
        share: r.share,
        received_at: r.received_at.to_jiff(),
    }))
}

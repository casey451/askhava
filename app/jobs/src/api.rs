//! The public face of `askhava-jobs`: everything other crates may call (D-481).
//!
//! The twelve statuses and their arrows (JOB-001, JOB-002), creating and moving a job with its
//! history, audit row and timer (JOB-005, JOB-006), restarting a wait (D-807), and the timer
//! handler `server` registers under [`TIMER_KIND`] (JOB-003, JOB-006), and the quote-page link
//! tokens: issue, revoke and resolve (SEC-037, PICK-028).

use sqlx::PgConnection;
use sqlx::types::Uuid;

pub use crate::models::{JobRow, JobsError};
pub use crate::service::{TIMER_KIND, create, move_to, restart_timer, run_timer};
pub use crate::status::JobStatus;

/// Quote-page link tokens (SEC-037, PICK-028, D-491).
pub mod quote_token {
    pub use crate::quote_token::{TokenError, issue, resolve, revoke};
}

/// The pure lifecycle rules, callable without a database (D-481).
pub mod rules {
    pub use crate::rules::{Refused, Timeout, Wait, check_move, timeout};
}

/// Job `id`'s current status, version and entry time (JOB-001).
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn current(db: &mut PgConnection, id: Uuid) -> Result<JobRow, JobsError> {
    crate::models::load(db, id).await
}

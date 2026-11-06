//! The public face of `askhava-jobs`: everything other crates may call (D-481).
//!
//! The twelve statuses and their arrows (JOB-001, JOB-002), creating and moving a job with its
//! history, audit row and timer (JOB-005, JOB-006), restarting a wait (D-807), and the timer
//! handler `server` registers under [`TIMER_KIND`] (JOB-003, JOB-006), and the quote-page link
//! tokens: issue, revoke and resolve (SEC-037, PICK-028), and job numbers, never reused, even after
//! a restore (JOB-024); the homeowner-text event and its due check, the every-window-ended entry
//! point and the Connected check-in reply (JOB-010 to JOB-015, JOB-025, JOB-026, TXT-042); the
//! first-quote move to Quotes ready that `quotes` calls in the quote's transaction (JOB-009,
//! JOB-011, TXT-016, D-1517).

use askhava_core::api::Guarded;
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

pub use crate::handlers::{TEXT_KIND, all_windows_ended, connected_reply, text_due};
pub use crate::models::{JobRow, JobsError};
pub use crate::service::{
    TIMER_KIND, create, job_number, move_to, raise_number_floor, restart_timer, run_timer,
};
pub use crate::status::JobStatus;

/// Quote-page link tokens (SEC-037, PICK-028, D-491).
pub mod quote_token {
    pub use crate::quote_token::{TokenError, issue, resolve, revoke};
}

/// The pure lifecycle rules, callable without a database (D-481).
pub mod rules {
    pub use crate::rules::{
        CheckIn, Contact, Refused, Reply, Timeout, Wait, Windows, WindowsEnd, after_windows,
        check_move, connected_move, timeout,
    };
}

/// Job `id`'s current status, version and entry time (JOB-001).
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn current(db: &mut PgConnection, id: Uuid) -> Result<JobRow, JobsError> {
    crate::models::load(db, id).await
}

/// The homeowner text the first-quote move sends (TXT-016; English and Spanish templates, T047).
const FIRST_QUOTE: &str = "first_quote";
/// How many times [`first_quote`] re-reads the job after losing a race to another move.
const FIRST_QUOTE_TRIES: usize = 3;

/// The first confirmed quote on job `id` arrived at `at` (JOB-009, JOB-011, TXT-016, D-1517):
/// when the job's status `moves_on_first_quote` (Open, No quotes yet) it moves to Quotes ready
/// with the `first_quote` homeowner-text event, in the caller's transaction (a savepoint), so a
/// quote that is not stored rolls the move and its text back (R11). The move bumps the version, so
/// the old wait applies nothing and a held no-quotes-yet text is dropped (JOB-006), and Quotes
/// ready's 3-day wait counts from `at` (JOB-012, JOB-025). On a lost race (a timer moving Open to
/// No quotes yet first) it re-reads, at most 3 times; any other status moves nothing and writes
/// nothing ([`Guarded::MovedOn`]), so a second quote never moves or texts again.
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn first_quote(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<Guarded<()>, JobsError> {
    for _ in 0..FIRST_QUOTE_TRIES {
        let from = crate::models::load(db, id).await?.status;
        if !from.moves_on_first_quote() {
            return Ok(Guarded::MovedOn);
        }
        let step = (from, None, JobStatus::QuotesReady);
        let moved = crate::service::apply(db, id, step, (at, Actor::System), Some(FIRST_QUOTE));
        if moved.await?.applied().is_some() {
            return Ok(Guarded::Applied(()));
        }
    }
    Ok(Guarded::MovedOn)
}

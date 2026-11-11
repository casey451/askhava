//! The public face of `askhava-jobs`: everything other crates may call (D-481).
//!
//! The twelve statuses and their arrows (JOB-001, JOB-002), creating and moving a job with its
//! history, audit row and timer (JOB-005, JOB-006), restarting a wait (D-807), and the timer
//! handler `server` registers under [`TIMER_KIND`] (JOB-003, JOB-006), and the quote-page link
//! tokens: issue, revoke and resolve (SEC-037, PICK-028), and job numbers, never reused, even after
//! a restore (JOB-024); the homeowner-text event and its due check, the every-window-ended entry
//! point and the Connected check-in reply (JOB-010 to JOB-015, JOB-025, JOB-026, TXT-042); the
//! first-quote move to Quotes ready that `quotes` calls in the quote's transaction (JOB-009,
//! JOB-011, TXT-016, D-1517); the day-3 and day-14 check-ins, their answers, the PROBLEM listing
//! and the feedback request after Done (JOB-015, JOB-016, JOB-028, TXT-029, T097).

use askhava_core::api::Guarded;
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

pub use crate::followups::{
    Answer, CHECK_IN_DAY3, CHECK_IN_DAY14, CHECK_IN_KIND, FEEDBACK_REQUEST, Inbound, Next,
    PROBLEM_ALERT, PROBLEM_REPLY, PROBLEM_SAFETY_ALERT, after_answer, answer_check_in,
    check_in_times, parse_answer, run_check_in, schedule_check_ins,
};
pub use crate::handlers::{TEXT_KIND, all_windows_ended, connected_reply, text_due};
pub use crate::models::{JobRow, JobsError};
pub use crate::purge_events::{
    PURGE_DUE_KIND, PURGE_STAGES, SWEEP_KIND, enqueue_purge, run_sweep, schedule_sweep,
    sweep_closed,
};
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

/// PICK-051, D-2912 (T164): takes job `id`'s row lock (`SELECT ... FOR UPDATE`) in the caller's
/// transaction and returns its status read under it. A pick's status move updates this row, so a
/// quote edit or withdrawal holding the lock either commits before the move or reads the moved
/// status and refuses.
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn lock_for_quote_change(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<JobStatus, JobsError> {
    let code = sqlx::query_scalar!("SELECT status FROM jobs.job WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(db)
        .await
        .map_err(|_| JobsError::Database)?
        .ok_or(JobsError::NotFound)?;
    JobStatus::from_code(&code).ok_or(JobsError::Database)
}

/// The homeowner's private feedback after Done, its share tick, removals and author marks
/// (JOB-017, T230, D-083, D-466, D-493). Table `jobs.feedback` (migration 0139): one row per job,
/// the words and tick as sent and never changed; there is no rating or tone column, so
/// [`feedback::shown`] cannot leave feedback out for being negative.
pub mod feedback {
    use askhava_ops::api::{Actor, AuditEntry, Change, audit};
    use jiff::Timestamp;
    use jiff_sqlx::ToSqlx;
    use sqlx::types::Uuid;
    use sqlx::{Connection, PgConnection};

    use crate::models::JobsError;

    /// The longest feedback the page takes, in characters (the table's `CHECK`).
    pub const MAX_CHARS: usize = 2000;

    /// The only four reasons ticked feedback is left off askhava.com (JOB-017).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Removal {
        /// It gives another person's personal details.
        PersonalDetails,
        /// It is abusive or discriminatory.
        Abusive,
        /// It is believed fake.
        Fake,
        /// It is unrelated to the service.
        Unrelated,
    }

    impl Removal {
        /// The code stored in `jobs.feedback.removed_reason`.
        #[must_use]
        pub const fn code(self) -> &'static str {
            match self {
                Self::PersonalDetails => "personal_details",
                Self::Abusive => "abusive",
                Self::Fake => "fake",
                Self::Unrelated => "unrelated",
            }
        }
    }

    /// A writer whose feedback is never shown, whatever the tick (JOB-017).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Author {
        /// The Ask Hava owner.
        Owner,
        /// A relative of the owner.
        OwnerRelative,
        /// A business.
        Business,
    }

    impl Author {
        /// The code stored in `jobs.feedback.author_kind`.
        #[must_use]
        pub const fn code(self) -> &'static str {
            match self {
                Self::Owner => "owner",
                Self::OwnerRelative => "owner_relative",
                Self::Business => "business",
            }
        }
    }

    /// One feedback that may appear on askhava.com (JOB-017).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Shown {
        /// The feedback's ID.
        pub id: Uuid,
        /// The job it is about; the page reads the first name and town through it.
        pub job: Uuid,
        /// The words exactly as written.
        pub body: String,
    }

    /// JOB-017: stores job `job`'s feedback `body` with the share tick as sent, at `at`, only
    /// while the job is Done and has no feedback yet; its ID, or `None` when nothing was stored
    /// (a second send, or a job not Done).
    ///
    /// # Errors
    /// [`JobsError::Database`], an empty or too long `body` included.
    pub async fn store(
        db: &mut PgConnection,
        job: Uuid,
        (body, share): (&str, bool),
        at: Timestamp,
    ) -> Result<Option<Uuid>, JobsError> {
        sqlx::query_scalar!(
            "INSERT INTO jobs.feedback (job_id, body, share, received_at) \
             SELECT id, $2, $3, $4 FROM jobs.job WHERE id = $1 AND status = 'done' \
             ON CONFLICT (job_id) DO NOTHING RETURNING id",
            job,
            body,
            share,
            at.to_sqlx() as _,
        )
        .fetch_optional(db)
        .await
        .map_err(|_| JobsError::Database)
    }

    /// Whether job `job` already has feedback (JOB-017: one per job).
    ///
    /// # Errors
    /// [`JobsError::Database`].
    pub async fn given(db: &mut PgConnection, job: Uuid) -> Result<bool, JobsError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM jobs.feedback WHERE job_id = $1) AS "given!""#,
            job
        )
        .fetch_one(db)
        .await
        .map_err(|_| JobsError::Database)
    }

    /// JOB-017: leaves ticked feedback `id` off askhava.com for `reason`, once, at `at`, logged
    /// with an audit row by `actor` in the same transaction; `false` changed nothing (unticked,
    /// already removed or unknown).
    ///
    /// # Errors
    /// [`JobsError::Database`].
    pub async fn remove(
        db: &mut PgConnection,
        id: Uuid,
        reason: Removal,
        (actor, at): (Actor, Timestamp),
    ) -> Result<bool, JobsError> {
        let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
        let done = sqlx::query!(
            "UPDATE jobs.feedback SET removed_reason = $2, removed_at = GREATEST($3, received_at) \
             WHERE id = $1 AND share AND removed_reason IS NULL",
            id,
            reason.code(),
            at.to_sqlx() as _,
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| JobsError::Database)?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        let action = match reason {
            Removal::PersonalDetails => "feedback_removed_personal_details",
            Removal::Abusive => "feedback_removed_abusive",
            Removal::Fake => "feedback_removed_fake",
            Removal::Unrelated => "feedback_removed_unrelated",
        };
        logged(&mut tx, actor, id, action).await?;
        tx.commit().await.map_err(|_| JobsError::Database)?;
        Ok(true)
    }

    /// JOB-017: marks feedback `id` as written by `author`, so it is never shown; audited by
    /// `actor` in the same transaction. `false` changed nothing (unknown feedback).
    ///
    /// # Errors
    /// [`JobsError::Database`].
    pub async fn mark_author(
        db: &mut PgConnection,
        id: Uuid,
        author: Author,
        actor: Actor,
    ) -> Result<bool, JobsError> {
        let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
        let done = sqlx::query!(
            "UPDATE jobs.feedback SET author_kind = $2 WHERE id = $1",
            id,
            author.code(),
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| JobsError::Database)?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        logged(&mut tx, actor, id, "feedback_author_marked").await?;
        tx.commit().await.map_err(|_| JobsError::Database)?;
        Ok(true)
    }

    /// JOB-017: every feedback that may appear on askhava.com, newest first: ticked, not removed
    /// and not from the owner, a relative of the owner or a business. Nothing else filters it.
    ///
    /// # Errors
    /// [`JobsError::Database`].
    pub async fn shown(db: &mut PgConnection) -> Result<Vec<Shown>, JobsError> {
        let rows = sqlx::query!(
            "SELECT id, job_id, body FROM jobs.feedback \
             WHERE share AND removed_reason IS NULL AND author_kind IS NULL \
             ORDER BY received_at DESC, id"
        )
        .fetch_all(db)
        .await
        .map_err(|_| JobsError::Database)?;
        Ok(rows
            .into_iter()
            .map(|r| Shown {
                id: r.id,
                job: r.job_id,
                body: r.body,
            })
            .collect())
    }

    /// The audit row for a change to feedback `id` (its words are never logged, PRIV-028).
    async fn logged(
        db: &mut PgConnection,
        actor: Actor,
        id: Uuid,
        action: &'static str,
    ) -> Result<(), JobsError> {
        let entry = AuditEntry {
            actor,
            action,
            subject_kind: "jobs.feedback",
            subject_id: id,
            change: Change::Action,
        };
        audit(db, &entry)
            .await
            .map(|_| ())
            .map_err(|_| JobsError::Database)
    }
}

//! The weekly digest's business counts (OPS-030, BIZ-054; D-219, D-463, D-4202; T605).
//!
//! [`leaving_answers`] lists the BIZ-054 answers given in a window through the crate-private
//! reader in `leaving.rs`, the one file that reads the answers (TD-BIZ-32's text scan), so the
//! answer never reaches routing or fee code. [`license_paused`] lists the businesses now paused
//! for an unknown, ended or lapsed license (LIC-008, LIC-009) whose pause began in the window,
//! for OPS-030's license-unknown counts. Both return business IDs and codes only, never a contact
//! detail and never the "Something else" box's words. Every window is half-open: `from` counts,
//! `to` does not.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::leaving::{AskedBy, LeaveReason, answers_between};
use crate::models::{BusinessesError, db};
use crate::status::{AccountStatus, PauseReason};

/// One BIZ-054 answer for the weekly digest: no words, no contact detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeavingAnswer {
    /// The business that answered.
    pub business: Uuid,
    /// Whether leaving or a long pause asked.
    pub asked_by: AskedBy,
    /// The choice, `None` when the business skipped the question.
    pub choice: Option<LeaveReason>,
}

/// BIZ-054, OPS-030: the answers given at or after `from` and before `to`, oldest first.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn leaving_answers(
    conn: &mut PgConnection,
    window: (Timestamp, Timestamp),
) -> Result<Vec<LeavingAnswer>, BusinessesError> {
    let rows = answers_between(conn, window).await?;
    Ok(rows
        .into_iter()
        .map(|(business, asked_by, choice)| LeavingAnswer {
            business,
            asked_by,
            choice,
        })
        .collect())
}

/// OPS-030, LIC-008, LIC-009: the businesses now paused with reason `license` whose status
/// changed at or after `from` and before `to`, oldest first.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn license_paused(
    conn: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<Uuid>, BusinessesError> {
    db(sqlx::query_scalar!(
        "SELECT id FROM businesses.business WHERE status = $1 AND pause_reason = $2 \
         AND status_changed_at >= $3 AND status_changed_at < $4 \
         ORDER BY status_changed_at, id",
        AccountStatus::Paused.code(),
        PauseReason::License.code(),
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(conn)
    .await)
}

//! A failed text's call-back urgency (INT-023, TXT-015, D-4052): whether the homeowner with a vault
//! contact has an urgent job, for `messaging`'s outbox sender when her texts are given up with no
//! email on file. The composition passes [`contact_urgent`] to the sender as its urgency source,
//! since `messaging` never reads `intake`.
//!
//! The request asked about is her open one or, when none is open, her latest one closed within the
//! call-back window ([`CALL_BACK_DAYS`] days before now); it is urgent by the rule
//! [`queue_call_back`](crate::handlers::queue_call_back) uses
//! ([`counts_urgent`](crate::handlers::counts_urgent)). No such request is not urgent. A read
//! error is an error, never a guess.

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::handlers::{CALL_BACK_DAYS, request_urgent};
use crate::models::IntakeError;

/// INT-023, TXT-015, D-4052: true when the homeowner with vault contact `contact` has, at `now`, an
/// open request, or else a latest one closed within [`CALL_BACK_DAYS`] days, that counts as urgent
/// ([`counts_urgent`](crate::handlers::counts_urgent): emergency, "Today, it's urgent", or a
/// 911-list keyword before "How soon?").
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn contact_urgent(
    db: &mut PgConnection,
    contact: Uuid,
    now: Timestamp,
) -> Result<bool, IntakeError> {
    let since = now - SignedDuration::from_hours(24 * CALL_BACK_DAYS);
    let request = sqlx::query_scalar!(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 AND (c.closed_at IS NULL OR c.closed_at >= $2) \
         ORDER BY c.closed_at IS NULL DESC, c.opened_at DESC, c.id LIMIT 1",
        contact,
        since.to_sqlx() as _
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| IntakeError::Database)?;
    match request {
        Some(conversation) => request_urgent(db, conversation).await,
        None => Ok(false),
    }
}

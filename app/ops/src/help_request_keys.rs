//! The help requests one homeowner asked, by ID, for "Delete my data" (PRIV-011, PRIV-006,
//! PRIV-030; T757, D-4765).
//!
//! A help request with no job keeps its words in `privacy.message_body` under the request's own ID
//! (`open_help` in `intake`), so `privacy`'s `delete_my_data` reads her request IDs here to delete
//! those words too. This is the second reader of `ops.help_request`
//! (`migrations/0194_help_requests.sql`) beside `help_requests.rs`, the one writer; it reads IDs
//! only and writes nothing.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OpsError;

/// PRIV-011, PRIV-030, D-4765: the ID of every help request the homeowner with vault contact
/// `contact` asked (`Asker::Homeowner(contact)`), open or closed, oldest first; empty when none.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn help_requests_of_homeowner(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Vec<Uuid>, OpsError> {
    sqlx::query_scalar!(
        "SELECT id FROM ops.help_request WHERE asker = 'homeowner' AND asker_id = $1 \
         ORDER BY opened_at, id",
        contact
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)
}

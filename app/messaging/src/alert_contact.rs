//! The vault contact behind a critical safety alert's inbound text (T484; OPS-028, OPS-035,
//! D-3890).
//!
//! A critical safety alert's subject is a `messaging.inbox` message, and `messaging.inbox` has no
//! contact column; T321's `messaging.inbox_contact` row records which contact sent it.
//! [`alert_contact`] reads that contact ID for one inbox ID, never a phone or any other detail. Its
//! only callers are `server`'s two owner-call handlers (the OPS-035 automated call and the OPS-028
//! bridge press), which keep the ID inside the handler and `calls`: no page, payload or log holds it.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;

/// OPS-028, OPS-035, D-3890: the vault contact ID that sent the inbound text `inbox`, or `None`
/// when no `messaging.inbox_contact` row records one (a text from before T321, or not a text).
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn alert_contact(
    db: &mut PgConnection,
    inbox: Uuid,
) -> Result<Option<Uuid>, OutboxError> {
    sqlx::query_scalar!(
        "SELECT contact_id FROM messaging.inbox_contact WHERE inbox_id = $1",
        inbox
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)
}

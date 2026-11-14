//! The hold on a phone waiting for the owner's approval (BIZ-033, CALL-012; D-3081, T396).
//!
//! A phone a member other than the owner links or changes is filed in the sender directory as
//! usual, then [`sender_hold`] moves its digests into `messaging.sender_held`, so neither
//! `sender_business` nor `sender_line` finds it. The owner's approval calls [`sender_release`],
//! which moves them back; a decline, unlink, number change or member removal calls
//! [`sender_drop_held`]. Each move runs in a savepoint of the caller's transaction: a digest another
//! phone holds rolls back only the savepoint and is [`OutboxError::SenderTaken`], so nothing moves
//! and the caller's transaction stays usable. Only keyed digests move, never a number, and no
//! digest is logged (PRIV-002).

use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::OutboxError;

fn taken_or_database(e: &sqlx::Error) -> OutboxError {
    match e.as_database_error() {
        Some(d) if d.is_unique_violation() => OutboxError::SenderTaken,
        _ => OutboxError::Database,
    }
}

/// Moves phone `line`'s sender-directory digests into the held list, so no lookup finds it until
/// [`sender_release`] (BIZ-033, CALL-012, D-3081). A line with no digests holds nothing.
///
/// # Errors
///
/// [`OutboxError::SenderTaken`] when another waiting phone already holds a digest (nothing
/// moves), or [`OutboxError::Database`].
pub async fn sender_hold(db: &mut PgConnection, line: Uuid) -> Result<(), OutboxError> {
    let mut sp = db.begin().await.map_err(|_| OutboxError::Database)?;
    sqlx::query!(
        "INSERT INTO messaging.sender_held (line_id, index_version, digest, business_id) \
         SELECT line_id, index_version, digest, business_id FROM messaging.sender_directory \
         WHERE line_id = $1",
        line
    )
    .execute(&mut *sp)
    .await
    .map_err(|e| taken_or_database(&e))?;
    sqlx::query!(
        "DELETE FROM messaging.sender_directory WHERE line_id = $1",
        line
    )
    .execute(&mut *sp)
    .await
    .map_err(|_| OutboxError::Database)?;
    sp.commit().await.map_err(|_| OutboxError::Database)
}

/// Moves phone `line`'s held digests back into the sender directory, so the phone works (BIZ-033,
/// D-3081). A line with nothing held releases nothing.
///
/// # Errors
///
/// [`OutboxError::SenderTaken`] when another phone now holds a digest in the directory (nothing
/// moves and the phone stays held), or [`OutboxError::Database`].
pub async fn sender_release(db: &mut PgConnection, line: Uuid) -> Result<(), OutboxError> {
    let mut sp = db.begin().await.map_err(|_| OutboxError::Database)?;
    sqlx::query!(
        "INSERT INTO messaging.sender_directory (line_id, index_version, digest, business_id) \
         SELECT line_id, index_version, digest, business_id FROM messaging.sender_held \
         WHERE line_id = $1",
        line
    )
    .execute(&mut *sp)
    .await
    .map_err(|e| taken_or_database(&e))?;
    sender_drop_held(&mut sp, line).await?;
    sp.commit().await.map_err(|_| OutboxError::Database)
}

/// Deletes phone `line`'s held digests, if any (BIZ-033, D-3081): a declined, unlinked or
/// renumbered phone, or one whose member was removed, leaves nothing held.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn sender_drop_held(db: &mut PgConnection, line: Uuid) -> Result<(), OutboxError> {
    sqlx::query!("DELETE FROM messaging.sender_held WHERE line_id = $1", line)
        .execute(db)
        .await
        .map(|_| ())
        .map_err(|_| OutboxError::Database)
}

//! The sender-directory steps of a "LINK <PIN>" reply (CALL-011, CALL-012; D-492, D-3079, T391).
//!
//! [`sender_line`] answers which linked phone wrote, its business and its line ID (the
//! `businesses.staff_phone` ID), so `calls::api::link_reply` can check the reply came from the
//! business's main phone. [`sender_put_pending`] files a blocked phone's already-keyed `sender-dir`
//! digests (the pending link's, never a number) under a new staff phone line, in a savepoint of
//! the caller's transaction: a digest another phone holds rolls back only the savepoint and is
//! [`OutboxError::SenderTaken`], so the caller's transaction stays usable (PRIV-002).

use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, OutboxError};

/// The business and line ID of linked phone `phone`, if it is a linked main or staff phone
/// (D-492, CALL-012). Matches any `sender-dir` key version (PRIV-024).
///
/// # Errors
///
/// [`OutboxError::BadPhone`] or [`OutboxError::Database`].
pub async fn sender_line(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
) -> Result<Option<(Uuid, Uuid)>, OutboxError> {
    let phone = normalize_phone(phone).map_err(|_| OutboxError::BadPhone)?;
    let (versions, hashes): (Vec<i16>, Vec<Vec<u8>>) = vault
        .blind_index(IndexPurpose::SenderDir, &phone)
        .iter()
        .filter_map(|(v, d)| Some((i16::try_from(*v).ok()?, d.to_vec())))
        .unzip();
    let row = sqlx::query!(
        "SELECT d.business_id, d.line_id FROM messaging.sender_directory d \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON d.index_version = k.v AND d.digest = k.h LIMIT 1",
        &versions,
        &hashes
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| (r.business_id, r.line_id)))
}

/// Files `digests` (one per `sender-dir` key version, already keyed) as phone `line` of
/// `business`, in a savepoint of the caller's transaction (CALL-011, D-3079). A digest another
/// phone holds rolls back the savepoint only and is [`OutboxError::SenderTaken`].
///
/// # Errors
///
/// [`OutboxError::SenderTaken`] or [`OutboxError::Database`].
pub async fn sender_put_pending(
    db: &mut PgConnection,
    (business, line): (Uuid, Uuid),
    digests: &[(u16, [u8; 32])],
) -> Result<(), OutboxError> {
    let mut savepoint = db.begin().await.map_err(|_| OutboxError::Database)?;
    models::put_sender(&mut savepoint, (business, line), digests).await?;
    savepoint.commit().await.map_err(|_| OutboxError::Database)
}

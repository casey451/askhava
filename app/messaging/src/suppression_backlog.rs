//! How many opt-out listings the suppression re-keying batch still has to move (PRIV-024,
//! CON-035, D-5412).
//!
//! `server rotate-keys` caps its [`crate::api::rekey_suppression_batch`] loop by this count and,
//! once the batch returns 0, reports what is left as skipped: listings whose phone no contact
//! holds, which the batch only logs as a warning. Only a count is read, never a digest.

use askhava_privacy::api::Vault;
use sqlx::PgConnection;

use crate::suppression_rekey::RekeyError;

/// Counts the `messaging.suppression` rows held under a version other than `vault`'s active index
/// version whose opt-out event has no row under the active version: the rows the re-keying batch
/// selects (PRIV-024, CON-035, D-4748, D-5412).
///
/// # Errors
///
/// [`RekeyError::Database`].
pub async fn suppression_backlog(db: &mut PgConnection, vault: &Vault) -> Result<u64, RekeyError> {
    let active = i16::try_from(vault.index_version()).map_err(|_| RekeyError::Database)?;
    let count = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM messaging.suppression s \
         WHERE s.index_version <> $1 AND NOT EXISTS (SELECT 1 FROM messaging.suppression a \
           WHERE a.index_version = $1 AND a.event_id = s.event_id)",
        active
    )
    .fetch_one(db)
    .await
    .map_err(|_| RekeyError::Database)?;
    Ok(count.cast_unsigned())
}

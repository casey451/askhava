//! How many rows each key rotation batch still has to move (PRIV-024, D-5412, D-5420).
//!
//! `server rotate-keys` caps each batch loop at its starting backlog divided by the batch size plus
//! 2 passes (D-753), refuses to drop a `VAULT_KEY` version while a photo it cannot move is under
//! it, and reports what a loop left behind. The counts use exactly the predicates the batches
//! select with: [`crate::api::rotate_batch`] (contacts, then stated names),
//! [`crate::api::rotate_business_batch`] (staff phones, then mailboxes) and
//! [`crate::api::rotate_photos`]. Only counts are read: no value, digest or ID leaves here.

use sqlx::PgConnection;

use crate::models::{IndexPurpose, VaultError};
use crate::vault::Vault;

/// The rows each rotation batch would still select under `vault`'s keys (PRIV-024, D-5420).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationBacklog {
    /// Contacts `rotate_batch` selects plus stated names under an older seal or index version.
    pub vault: u64,
    /// Staff phones under an older seal version plus mailboxes under an older seal or index version.
    pub business: u64,
    /// Photos under a `VAULT_KEY` version below the active one.
    pub photos: u64,
}

/// Counts the rotation backlog under `vault`'s active versions, in one read-only statement
/// (PRIV-024, D-5420): (a) contacts by `rotate_batch`'s own `WHERE`, dropped-version phone digests
/// included; (b) stated names with `key_version` below the seal version or `index_version` below
/// the index version; (c) staff phones below the seal version; (d) mailboxes below the seal
/// version or with an index row below the index version; (e) photos below the seal version (the
/// photo seal reads the same `VAULT_KEY`). `vault` is (a)+(b), `business` (c)+(d).
///
/// # Errors
///
/// [`VaultError::Database`].
pub async fn rotation_backlog(
    db: &mut PgConnection,
    vault: &Vault,
) -> Result<RotationBacklog, VaultError> {
    let seal = i32::from(vault.seal_version());
    let index = i32::from(vault.index_version());
    let versions: Vec<i32> = vault.index_versions().into_iter().map(i32::from).collect();
    let row = sqlx::query!(
        "SELECT \
         (SELECT count(*) FROM privacy.contact c WHERE c.oldest_key_version < $1 \
           OR (c.phone IS NOT NULL AND ( \
             EXISTS (SELECT 1 FROM privacy.contact_index i WHERE i.contact_id = c.id \
               AND i.purpose = $4 AND i.key_version <> ALL($3::int4[])) \
             OR EXISTS (SELECT 1 FROM unnest($3::int4[]) AS k(v) WHERE NOT EXISTS ( \
               SELECT 1 FROM privacy.contact_index i WHERE i.contact_id = c.id \
               AND i.purpose = $4 AND i.key_version = k.v))))) AS \"contacts!\", \
         (SELECT count(*) FROM privacy.stated_name \
           WHERE key_version < $1 OR index_version < $2) AS \"stated!\", \
         (SELECT count(*) FROM privacy.staff_phone WHERE key_version < $1) AS \"staff!\", \
         (SELECT count(*) FROM privacy.business_mailbox WHERE key_version < $1 OR id IN \
           (SELECT mailbox_id FROM privacy.business_mailbox_index WHERE key_version < $2)) \
           AS \"mailboxes!\", \
         (SELECT count(*) FROM privacy.photo WHERE key_version < $1) AS \"photos!\"",
        seal,
        index,
        &versions,
        IndexPurpose::HomeownerPhone.label(),
    )
    .fetch_one(db)
    .await
    .map_err(|_| VaultError::Database)?;
    let count = |n: i64| n.cast_unsigned();
    Ok(RotationBacklog {
        vault: count(row.contacts) + count(row.stated),
        business: count(row.staff) + count(row.mailboxes),
        photos: count(row.photos),
    })
}

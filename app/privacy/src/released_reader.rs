//! The released view's read: a released pick's name, address and (when she chose email contact)
//! email, again, for the signed-in business, through `picks` only (PICK-022 to PICK-024, PRIV-003,
//! PRIV-005, SEC-046; D-1739).
//!
//! [`read_released`] takes a [`BusinessSessionProof`], which only `portals`' business session
//! extractor makes, and the pick and the contact its `picks.release` row is bound to; `picks`
//! decides first that the pick is released, paid or free, and that the person is the owner or a
//! "Confirm and billing" member of its business (`picks::api::released_view`). It reads under the
//! `(Reader::Picks, Purpose::ReleasedView)` grant (name, address, email; never the phone,
//! PICK-023), audited per field with the person as actor and one more row with the pick as
//! subject (PRIV-003). A contact whose released fields the PRIV-007 purge or "Delete my data"
//! cleared opens nothing and writes no read row: the answer is `None`.

use askhava_ops::api::Actor;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::api::BusinessSessionProof;
use crate::models::{ContactDetails, Field, Purpose, Reader, VaultError, grant_for};
use crate::vault::{self, Vault};

/// Reads `fields` of `contact`, the contact bound to released pick `pick`, for the signed-in
/// business person `proof` names (PICK-024, PRIV-003, PRIV-005, SEC-046, D-1739): one audited read
/// row per field (`vault_read.picks.released_view`, actor the person as `Actor::Business`) and one
/// with subject `picks.pick` and the pick ID. `None` when the purge cleared her name, email and
/// address (PRIV-007, PRIV-011): nothing is opened or audited. A field outside the grant (the
/// phone) is refused and alerted (SEC-047). Only `picks` calls it (`vault_readers.rs`).
///
/// # Errors
///
/// [`VaultError::InTransaction`] on a connection inside a transaction, [`VaultError::NotFound`],
/// [`VaultError::UnregisteredPurpose`], [`VaultError::Sealed`], [`VaultError::Audit`] or
/// [`VaultError::Database`].
pub async fn read_released(
    db: &mut PgConnection,
    vault: &Vault,
    proof: &BusinessSessionProof,
    (pick, contact): (Uuid, Uuid),
    fields: &[Field],
) -> Result<Option<ContactDetails>, VaultError> {
    if db.is_in_transaction() {
        return Err(VaultError::InTransaction);
    }
    let access = (Reader::Picks, Purpose::ReleasedView);
    let Some(grant) = grant_for(access.0, access.1, fields) else {
        return vault::refuse(db, access, contact).await.map(Some);
    };
    if cleared(db, contact).await? {
        return Ok(None);
    }
    let gate = (grant, Actor::Business(proof.person()), Some(pick));
    vault::read(db, vault, gate, contact, fields)
        .await
        .map(Some)
}

/// Whether contact `id`'s name, email and address are all gone (PRIV-007, PRIV-011);
/// [`VaultError::NotFound`] when there is no such contact.
async fn cleared(db: &mut PgConnection, id: Uuid) -> Result<bool, VaultError> {
    sqlx::query_scalar!(
        r#"SELECT (name IS NULL AND email IS NULL AND address IS NULL) AS "cleared!"
           FROM privacy.contact WHERE id = $1"#,
        id,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| VaultError::Database)?
    .ok_or(VaultError::NotFound)
}

//! The kept vault phone behind one opt-out listing, for the suppression re-keying run (PRIV-024,
//! CON-035, CON-013, PRIV-003, D-079, D-4748).
//!
//! `messaging.suppression` holds a listed number only as its homeowner-phone digest under an index
//! key version. To list it under a newer version, `messaging::api::rekey_suppression_batch` needs
//! the number itself: D-079 keeps it, encrypted, on its contact while it is listed.
//! [`phone_for_index`] finds the contact whose stored digest under that version matches, opens only
//! its phone under `Purpose::Rotation`, and audits and counts the read as `rotate_batch` does. The
//! phone comes back as a `SecretString`; neither it nor the digest is logged or put in an error.

use askhava_ops::api::Actor;
use secrecy::SecretString;
use sqlx::PgConnection;

use crate::models::{Field, IndexPurpose, Purpose, VaultError};
use crate::vault::{Vault, db_err, ensure_bare, own_grant, read_audited};

/// The kept vault phone of the number whose homeowner-phone digest under index key `version` is
/// `digest`, or `None` when no contact with a phone holds that digest (the caller then leaves the
/// listing, never guessing; PRIV-024, CON-035, D-079, D-4748). The read is `privacy`'s rotation
/// read of the phone field alone, audited and counted in a transaction that commits before the
/// phone is returned (PRIV-003, SEC-047), so `conn` must be outside any transaction (D-752).
///
/// # Errors
///
/// [`VaultError::InTransaction`], [`VaultError::Sealed`], [`VaultError::Audit`] or
/// [`VaultError::Database`]; none carries the phone or the digest.
pub async fn phone_for_index(
    conn: &mut PgConnection,
    vault: &Vault,
    (version, digest): (u16, [u8; 32]),
) -> Result<Option<SecretString>, VaultError> {
    ensure_bare(conn).await?;
    let contact = sqlx::query_scalar!(
        "SELECT i.contact_id FROM privacy.contact_index i \
         JOIN privacy.contact c ON c.id = i.contact_id \
         WHERE i.purpose = $1 AND i.key_version = $2 AND i.digest = $3 AND c.phone IS NOT NULL \
         ORDER BY i.contact_id LIMIT 1",
        IndexPurpose::HomeownerPhone.label(),
        i32::from(version),
        &digest[..],
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?;
    let Some(contact) = contact else {
        return Ok(None);
    };
    let grant = (own_grant(Purpose::Rotation), Actor::System, None);
    let details = read_audited(conn, vault, grant, contact, &[Field::Phone]).await?;
    Ok(details.phone)
}

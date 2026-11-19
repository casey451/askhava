//! The consent sweep's vault phone clear (CON-022, CON-035, PRIV-024; D-079, D-2208, D-4280,
//! D-4387, D-4418).
//!
//! CON-022 keeps a number's consent records and message logs 5 years after its last message; once
//! they are gone, a number not on the suppression list has nothing left its locked vault phone
//! proves, so the daily `intake.consent_sweep` (T563) clears it here, as the last write of that
//! number's transaction. A listed number keeps its phone: PRIV-024's index-key rotation rebuilds the
//! suppression digest from it (D-079). The sweep reads the contact's homeowner-phone digests with
//! `phone_digests` (no plaintext read) before its transaction; [`clear_consent_phone`] takes the
//! contact row with `FOR UPDATE NOWAIT`, so the sweep never waits on a row while an opt-out waits on
//! the sweep's lock, and clears the phone only while the contact's digests are still exactly the ones
//! the sweep checked the suppression list by (a phone changed meanwhile is kept). The phone and its
//! homeowner-phone index rows go together, with one `ops` audit row naming the field only.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::VaultError;
use crate::vault::{db_err, phone_digests};

/// The audit action of a phone cleared by the consent sweep (CON-022, D-4418).
pub const CONSENT_PHONE_CLEARED: &str = "consent_phone_cleared";

/// PostgreSQL's `lock_not_available`, raised by `NOWAIT` on a row another transaction holds.
const LOCK_NOT_AVAILABLE: &str = "55P03";

/// What [`clear_consent_phone`] did (CON-022, D-4418).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneClear {
    /// The sealed phone and its homeowner-phone index rows are gone, audited once.
    Cleared,
    /// Nothing changed: no such contact, no phone, or its digests are no longer the checked ones.
    Kept,
    /// Another transaction holds the contact row; the caller's transaction is aborted and must roll
    /// back, leaving the number for the next run.
    Busy,
}

/// Clears contact `contact`'s sealed phone and its homeowner-phone index rows inside the caller's
/// transaction, only while those index rows are exactly `checked` (the digests the sweep checked the
/// suppression list by, one per configured key version), and audits it once as
/// [`CONSENT_PHONE_CLEARED`] with the `phone` field named, never its value (CON-022, CON-035,
/// PRIV-024, D-079, D-4418). It takes the contact row first with `FOR UPDATE NOWAIT`: a row another
/// transaction holds answers [`PhoneClear::Busy`] at once, and the caller must roll back.
///
/// # Errors
///
/// [`VaultError::Database`] or [`VaultError::Audit`].
pub async fn clear_consent_phone(
    tx: &mut PgConnection,
    contact: Uuid,
    checked: &[(u16, [u8; 32])],
) -> Result<PhoneClear, VaultError> {
    let held = sqlx::query_scalar!(
        "SELECT phone IS NOT NULL AS \"phone!\" FROM privacy.contact WHERE id = $1 \
         FOR UPDATE NOWAIT",
        contact
    )
    .fetch_optional(&mut *tx)
    .await;
    let has_phone = match held {
        Ok(found) => found.unwrap_or(false),
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some(LOCK_NOT_AVAILABLE) => {
            return Ok(PhoneClear::Busy);
        }
        Err(_) => return Err(VaultError::Database),
    };
    if !has_phone || !same_digests(phone_digests(tx, contact).await?, checked) {
        return Ok(PhoneClear::Kept);
    }
    sqlx::query!(
        "UPDATE privacy.contact SET phone = NULL WHERE id = $1",
        contact
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    sqlx::query!(
        "DELETE FROM privacy.contact_index WHERE contact_id = $1 AND purpose = 'homeowner-phone'",
        contact
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: CONSENT_PHONE_CLEARED,
        subject_kind: "privacy.contact",
        subject_id: contact,
        change: Change::Field("phone"),
    };
    audit(tx, &entry).await.map_err(|_| VaultError::Audit)?;
    Ok(PhoneClear::Cleared)
}

/// Whether the stored digests are exactly the checked set, order aside (D-4418).
fn same_digests(mut stored: Vec<(u16, [u8; 32])>, checked: &[(u16, [u8; 32])]) -> bool {
    let mut checked = checked.to_vec();
    stored.sort_unstable();
    checked.sort_unstable();
    !checked.is_empty() && stored == checked
}

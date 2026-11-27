//! A homeowner's HELP record and her vault contact's creation time, for her "Download my data"
//! file (T820, D-5399, D-5804; PRIV-031, CON-022, CON-034).
//!
//! HELP is not an opt-out event: `record_help_keyword` keeps one `ops.help_keyword` row per contact
//! with the time of her latest HELP. [`help_keyword_of`] reads it. [`contact_created_at`] reads the
//! `contact_created` audit row `privacy::vault::store_contact` writes in the contact's own
//! transaction (the audit log is append-only, so the row is never lost), whose `at` equals the
//! contact's `created_at`; `messaging`'s opt-out records start there so a reused number never shows
//! an earlier holder's events. Both read only the `ops` schema.

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OpsError;

/// CON-034, PRIV-031: when `contact` last texted HELP, or `None` when she never did.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn help_keyword_of(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<Timestamp>, OpsError> {
    let at = sqlx::query_scalar!(
        "SELECT at FROM ops.help_keyword WHERE contact_id = $1",
        contact
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(at.map(jiff_sqlx::Timestamp::to_jiff))
}

/// CON-022, PRIV-031, D-5804: when vault contact `contact` was created (its `contact_created`
/// audit row), or `None` when no such row is held.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn contact_created_at(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<Timestamp>, OpsError> {
    let at = sqlx::query_scalar!(
        "SELECT min(at) FROM ops.audit_log \
         WHERE subject_kind = 'privacy.contact' AND subject_id = $1 AND action = 'contact_created'",
        contact
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(at.map(jiff_sqlx::Timestamp::to_jiff))
}

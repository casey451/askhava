//! Whether a pick's release is waiting for an emergency address (INT-029, PICK-022, SEC-030;
//! T255, D-1739, D-3358, D-3384, D-3348).
//!
//! `picks.address_wait` holds one row per pick whose release came before the street address; the
//! row exists only after `release` was given a proof `payments` minted for that pick. `server`'s
//! `intake.address_in` handler calls [`awaiting_address`] first and re-clears the pick through
//! `payments::api::reclear_release` only when it answers `true`, so a stored success alone never
//! makes a first release. The read takes `FOR SHARE` the pick's own `picks.pick` row in the
//! caller's transaction: `picks.address_wait` is append-only for the app role, which has no
//! `UPDATE` on it, and PostgreSQL refuses a row lock without one (D-3348); the pick row is the one
//! every status move updates, so no move can commit between this read and the caller's settle.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;

/// True when `pick` has its `picks.address_wait` row (INT-029, D-3358, D-3384, D-3348); takes
/// the pick's `picks.pick` row `FOR SHARE` until the caller's transaction ends. An unknown pick
/// is `false`.
///
/// # Errors
/// [`PicksError::Database`].
pub async fn awaiting_address(conn: &mut PgConnection, pick: Uuid) -> Result<bool, PicksError> {
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM picks.address_wait w WHERE w.pick_id = p.id) AS "found!"
           FROM picks.pick p WHERE p.id = $1 FOR SHARE OF p"#,
        pick
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(found.unwrap_or(false))
}

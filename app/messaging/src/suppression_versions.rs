//! The start check that no `VAULT_INDEX_KEY` version is retired while an opt-out is held under it
//! (PRIV-024, CON-013, CON-035, D-4418).
//!
//! `record` lists a number on `messaging.suppression` under every index key version configured at
//! that moment, and nothing re-keys a listing to a newer version. A number that opted out while
//! only v1 existed has v1 rows only, so dropping v1 from `VAULT_INDEX_KEY` would make
//! `send_checked` miss it and text it again. [`check_suppression_versions`] reads the versions the
//! list holds and fails, naming the missing version numbers only (never a digest), when the
//! vault's key set lacks one; the server and the worker run it once at start and refuse to start
//! on a failure, so nothing is texted (fail closed). A process with no vault skips it.

use askhava_privacy::api::Vault;
use sqlx::PgConnection;
use thiserror::Error;

/// Why [`check_suppression_versions`] refused the start; it names version numbers only, never a
/// digest or a number (PRIV-016, D-4418).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SuppressionVersionsError {
    /// Opt-outs are held under these index key versions, ascending, and `VAULT_INDEX_KEY` lacks
    /// them.
    #[error(
        "opt-outs are held under VAULT_INDEX_KEY versions {0:?} that are not configured; \
         configure them again (D-4418)"
    )]
    Missing(Vec<u16>),
    /// The suppression list could not be read (the database unreachable): the start is refused too.
    #[error("the suppression list's key versions could not be read (D-4418)")]
    Database,
}

/// Fails with every index key version a `messaging.suppression` row is held under that `vault`
/// does not configure; passes on an empty list with any key set (PRIV-024, CON-013, CON-035,
/// D-4418). Static SQL over `messaging`'s own table; reads no digest out.
///
/// # Errors
///
/// [`SuppressionVersionsError::Missing`] or [`SuppressionVersionsError::Database`].
pub async fn check_suppression_versions(
    conn: &mut PgConnection,
    vault: &Vault,
) -> Result<(), SuppressionVersionsError> {
    let held = sqlx::query_scalar!(
        "SELECT DISTINCT index_version FROM messaging.suppression ORDER BY index_version"
    )
    .fetch_all(conn)
    .await
    .map_err(|_| SuppressionVersionsError::Database)?;
    let configured = vault.index_versions();
    let missing: Vec<u16> = held
        .into_iter()
        .filter_map(|v| u16::try_from(v).ok())
        .filter(|v| !configured.contains(v))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(SuppressionVersionsError::Missing(missing))
    }
}

//! The newest audit row of one action (OPS-013, OPS-008; T109, D-3753, D-3882, D-3883): how a
//! module that may not call `recovery` (`portals`, the weekly digest) reads the last backup or
//! test-restore result from the audit entry `recovery` writes with each `recovery.backup_check`
//! row. Record IDs, code names and a time only (PRIV-028).

use sqlx::PgConnection;

use crate::audit_subjects::AuditSubject;
use crate::models::OpsError;

/// The audit action of a nightly encrypted export that was stored (OPS-013, PRIV-018); its
/// subject is the `recovery.backup_check` row. `recovery` writes it, the health page reads it.
pub const BACKUP_EXPORTED_ACTION: &str = "recovery.export_stored";

/// The audit action of a nightly export that failed (OPS-013); subject as above.
pub const BACKUP_EXPORT_FAILED_ACTION: &str = "recovery.export_failed";

/// OPS-013, D-3882: the newest audit row with `action` (latest `at`, ties by row ID), or `None`
/// when there is none.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn latest_audit(
    db: &mut PgConnection,
    action: &str,
) -> Result<Option<AuditSubject>, OpsError> {
    let row = sqlx::query!(
        "SELECT subject_kind, subject_id, field, at FROM ops.audit_log \
         WHERE action = $1 ORDER BY at DESC, id DESC LIMIT 1",
        action,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(row.map(|r| AuditSubject {
        subject_kind: r.subject_kind,
        subject_id: r.subject_id,
        field: r.field,
        at: r.at.to_jiff(),
    }))
}

//! The subjects of one audit action's rows in a window (OPS-039, D-3844, D-3882): a read-only
//! reader for modules that may not query `ops.audit_log` themselves. Each row gives its subject
//! kind, subject ID and time only (record IDs and code names, PRIV-028), oldest first.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OpsError;

/// One audit row's subject and time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSubject {
    /// The kind of record, a code name such as `jobs.job`.
    pub subject_kind: String,
    /// The record's ID.
    pub subject_id: Uuid,
    /// When the row was written (its transaction's start).
    pub at: Timestamp,
}

/// OPS-039, D-3844, D-3882: the subject of every `action` row written at or after `from` and
/// before `to`, oldest first (ties by row ID).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn audit_subjects(
    db: &mut PgConnection,
    action: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<AuditSubject>, OpsError> {
    let rows = sqlx::query!(
        "SELECT subject_kind, subject_id, at FROM ops.audit_log \
         WHERE action = $1 AND at >= $2 AND at < $3 ORDER BY at, id",
        action,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| AuditSubject {
            subject_kind: r.subject_kind,
            subject_id: r.subject_id,
            at: r.at.to_jiff(),
        })
        .collect())
}

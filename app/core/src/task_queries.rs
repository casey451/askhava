//! Read-only questions about the task queue that other modules ask before they decide (JOB-006).
//!
//! The retention purge (PRIV-007, D-1839) must count a hold that is still on its way: a
//! hold-producing task that has not finished (pending, running, or failed for good) means the
//! hold it would place may not be in `privacy.retention_hold` yet, so the purge keeps the contact.
//! [`unfinished`] answers that from `core.task` alone, by kind and dedupe-key prefix.

use sqlx::PgConnection;

use crate::tasks::TaskError;

/// PRIV-007, D-1839: true while any `core.task` of `kind` whose dedupe key starts with
/// `key_prefix` has not finished: still due, running, or dead-lettered after its last attempt
/// (`done_at` unset). The prefix is compared as plain text, so `%` and `_` match only themselves.
///
/// # Errors
/// [`TaskError::Db`] when the read fails.
pub async fn unfinished(
    db: &mut PgConnection,
    kind: &str,
    key_prefix: &str,
) -> Result<bool, TaskError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM core.task WHERE kind = $1 AND done_at IS NULL
           AND starts_with(dedupe_key, $2)) AS "unfinished!""#,
        kind,
        key_prefix,
    )
    .fetch_one(db)
    .await
    .map_err(|_| TaskError::Db)
}

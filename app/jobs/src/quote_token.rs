//! Quote-page link tokens: issue, re-issue, revoke and resolve (SEC-037, PICK-028, D-491).
//!
//! The token is `core`'s [`link_token`] of kind `QuotePage` over the job ID and a fresh 128-bit
//! salt; `jobs.quote_token` keeps the salt, the secret's version and the token's SHA256, never
//! the token. Issuing revokes the job's live token in the same transaction, so a re-issued link
//! gives a new token and the old one resolves to nothing (the caller answers 404), as does a
//! revoked one. Each issue and revoke writes an audit row in its transaction (JOB-005). The salt
//! with the job ID and the secret reproduces the token on replay; its copy to the off-database
//! stream waits for an `ops` stream event that can carry it (D-817).

use askhava_core::api::{IdSecret, LinkKind, LinkSalt, LinkToken, link_token};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

/// Why a quote-token call failed. No variant carries a token, salt or the database's message.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenError {
    /// The OS random source failed, so no salt and no token were made.
    #[error("no random salt could be made")]
    NoRandom,
    /// Another issue for the same job committed its live token first (the one-live-token index),
    /// so nothing was written; the caller may issue again, which revokes that token.
    #[error("another quote-token issue for this job won the race")]
    Busy,
    /// The database refused the statement, or the audit row could not be written.
    #[error("the quote-token database statement failed")]
    Database,
}

/// Issues a new quote-page token for `job` at `at`, revoking any live one first, and returns it
/// for the link (SEC-037). Never log the result.
///
/// # Errors
///
/// [`TokenError::NoRandom`] before anything is written, [`TokenError::Busy`] when a concurrent
/// issue for `job` committed first, or [`TokenError::Database`].
pub async fn issue(
    db: &mut PgConnection,
    secret: &IdSecret,
    job: Uuid,
    at: Timestamp,
) -> Result<LinkToken, TokenError> {
    let salt = LinkSalt::generate().map_err(|_| TokenError::NoRandom)?;
    let token = link_token(secret, LinkKind::QuotePage, job, &salt);
    let hash = token.hash();
    let mut tx = db.begin().await.map_err(|_| TokenError::Database)?;
    revoke_live(&mut tx, job, at).await?;
    sqlx::query!(
        "INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at) \
         VALUES ($1, $2, $3, $4, $5)",
        job,
        salt.as_bytes().as_slice(),
        i32::from(secret.version()),
        hash.as_slice(),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(insert_error)?;
    record(&mut tx, job, "quote_token_issued").await?;
    tx.commit().await.map_err(|_| TokenError::Database)?;
    Ok(token)
}

/// [`TokenError::Busy`] for a clash on the one-live-token index (a concurrent issue for the job
/// committed first); [`TokenError::Database`] for anything else. Carries no database message.
fn insert_error(err: sqlx::Error) -> TokenError {
    let live_clash = err
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation() && db.constraint() == Some("quote_token_live"));
    if live_clash {
        TokenError::Busy
    } else {
        TokenError::Database
    }
}

/// Revokes `job`'s live quote-page token at `at`, audited (SEC-037). Returns false, writing
/// nothing, when it had none.
///
/// # Errors
///
/// [`TokenError::Database`].
pub async fn revoke(db: &mut PgConnection, job: Uuid, at: Timestamp) -> Result<bool, TokenError> {
    let mut tx = db.begin().await.map_err(|_| TokenError::Database)?;
    if !revoke_live(&mut tx, job, at).await? {
        return Ok(false);
    }
    record(&mut tx, job, "quote_token_revoked").await?;
    tx.commit().await.map_err(|_| TokenError::Database)?;
    Ok(true)
}

/// The job a presented token opens, if it is live; `None` for an unknown, replaced or revoked
/// token, which the page answers with 404 (SEC-037). Looks up by hash only.
///
/// # Errors
///
/// [`TokenError::Database`].
pub async fn resolve(db: &mut PgConnection, token: &LinkToken) -> Result<Option<Uuid>, TokenError> {
    let hash = token.hash();
    sqlx::query_scalar!(
        "SELECT job_id FROM jobs.quote_token WHERE token_hash = $1 AND revoked_at IS NULL",
        hash.as_slice(),
    )
    .fetch_optional(db)
    .await
    .map_err(|_| TokenError::Database)
}

/// Marks `job`'s live token revoked at `at`, or at its `issued_at` if `at` is earlier (clock skew
/// between app instances), so the `revoked_at >= issued_at` check never blocks revoking a leaked
/// link; true if there was one.
async fn revoke_live(db: &mut PgConnection, job: Uuid, at: Timestamp) -> Result<bool, TokenError> {
    let done = sqlx::query!(
        "UPDATE jobs.quote_token SET revoked_at = GREATEST($2, issued_at) \
         WHERE job_id = $1 AND revoked_at IS NULL",
        job,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| TokenError::Database)?;
    Ok(done.rows_affected() > 0)
}

/// The audit row for an issue or revoke on `job`, by the system (JOB-005, PRIV-028).
async fn record(db: &mut PgConnection, job: Uuid, action: &'static str) -> Result<(), TokenError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "jobs.job",
        subject_id: job,
        change: Change::Action,
    };
    audit(db, &entry)
        .await
        .map(|_| ())
        .map_err(|_| TokenError::Database)
}

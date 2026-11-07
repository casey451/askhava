//! The retention purge's vault side: holds, the photo purge, the contact purge and "Delete my data"
//! (PRIV-007, PRIV-009, PRIV-011; D-013, D-079, D-491).
//!
//! `privacy` calls only `adapters` and `ops`, so it never reads another module's tables: the job's
//! close date reaches it on a `core.task` (`handlers.rs`), a dispute's hold comes from `payments`
//! and a complaint's from `businesses`' events, and the module that knows a job's contact calls
//! [`purge_contact`]. The contact purge clears name, email, address and on-site contact and keeps
//! the phone, sealed, with its homeowner-phone index, for the consent and opt-out records (D-079).

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{HoldSource, VaultError};

/// The PRIV-011 sentence "Delete my data" confirms with (D-214).
pub const DELETE_CONFIRMATION: &str = "We keep your phone number, locked, for 5 years only as proof of your texting permission and opt-out.";

fn db_err(_: sqlx::Error) -> VaultError {
    VaultError::Database
}

/// PRIV-007, D-491: holds job `job`'s contact details for `source`'s record `id` from `at`. A record
/// already held or released is left as it is, so a repeated event changes nothing.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn place_hold(
    db: &mut PgConnection,
    source: HoldSource,
    id: Uuid,
    job: Uuid,
    at: Timestamp,
) -> Result<(), VaultError> {
    sqlx::query!(
        "INSERT INTO privacy.retention_hold (source, source_id, job_id, placed_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (source, source_id) DO NOTHING",
        source.code(),
        id,
        job,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// PRIV-007, D-491: releases `source`'s hold for record `id` at `at`; a release that arrives
/// before its hold is stored released, so the late hold changes nothing.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn release_hold(
    db: &mut PgConnection,
    source: HoldSource,
    id: Uuid,
    job: Uuid,
    at: Timestamp,
) -> Result<(), VaultError> {
    sqlx::query!(
        "INSERT INTO privacy.retention_hold (source, source_id, job_id, placed_at, released_at) \
         VALUES ($1, $2, $3, $4, $4) ON CONFLICT (source, source_id) DO UPDATE \
         SET released_at = COALESCE(privacy.retention_hold.released_at, EXCLUDED.released_at)",
        source.code(),
        id,
        job,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// PRIV-007: true while any dispute or complaint hold on `job` is unreleased.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn is_held(db: &mut PgConnection, job: Uuid) -> Result<bool, VaultError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM privacy.retention_hold
           WHERE job_id = $1 AND released_at IS NULL) AS "held!""#,
        job,
    )
    .fetch_one(db)
    .await
    .map_err(db_err)
}

/// PRIV-009, PRIV-011: deletes every photo row of `job` (its links go with it), so no link to them
/// can be issued or opened again; returns how many went. The sealed objects stay in storage until
/// the storage adapter can delete (D-1803).
///
/// # Errors
/// [`VaultError::Database`].
pub async fn purge_photos(db: &mut PgConnection, job: Uuid) -> Result<u64, VaultError> {
    let gone = sqlx::query!("DELETE FROM privacy.photo WHERE job_id = $1", job)
        .execute(db)
        .await
        .map_err(db_err)?;
    Ok(gone.rows_affected())
}

/// PRIV-007, D-079: clears contact `id`'s name, email, address and on-site contact and its email
/// index, keeps the sealed phone and its homeowner-phone index, and audits it once
/// (`contact_purged`); true when anything was left to clear.
///
/// # Errors
/// [`VaultError::Database`], [`VaultError::Audit`].
pub async fn purge_contact(db: &mut PgConnection, id: Uuid) -> Result<bool, VaultError> {
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    let cleared = sqlx::query!(
        "UPDATE privacy.contact SET name = NULL, email = NULL, address = NULL, \
         onsite_contact = NULL WHERE id = $1 AND (name IS NOT NULL OR email IS NOT NULL \
         OR address IS NOT NULL OR onsite_contact IS NOT NULL)",
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    sqlx::query!(
        "DELETE FROM privacy.contact_index WHERE contact_id = $1 AND purpose = 'email'",
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if cleared > 0 {
        let entry = AuditEntry {
            actor: Actor::System,
            action: "contact_purged",
            subject_kind: "privacy.contact",
            subject_id: id,
            change: Change::Action,
        };
        audit(&mut tx, &entry)
            .await
            .map_err(|_| VaultError::Audit)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(cleared > 0)
}

/// PRIV-011, D-214: "Delete my data" for contact `id` and the jobs the caller found for it: the
/// vault entry is cleared as in [`purge_contact`] (the phone kept, locked, for the consent records,
/// D-079) and every photo of those jobs deleted; returns [`DELETE_CONFIRMATION`]. Ledger rows hold
/// IDs only, so they stay as they are (PRIV-010).
///
/// # Errors
/// [`VaultError::Database`], [`VaultError::Audit`].
pub async fn delete_my_data(
    db: &mut PgConnection,
    id: Uuid,
    jobs: &[Uuid],
) -> Result<&'static str, VaultError> {
    purge_contact(db, id).await?;
    for job in jobs {
        purge_photos(db, *job).await?;
    }
    Ok(DELETE_CONFIRMATION)
}

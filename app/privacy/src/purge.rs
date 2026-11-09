//! The retention purge's vault side: holds, the photo purge, the contact purge and "Delete my data"
//! (PRIV-007, PRIV-009, PRIV-011; D-013, D-079, D-491).
//!
//! `privacy` calls only `adapters` and `ops`, so it never reads another module's tables: the job's
//! close date reaches it on a `core.task` (`handlers.rs`), a dispute's hold comes from `payments`
//! and a complaint's from `businesses`' events, and the module that knows a job's contact calls
//! [`purge_contact`]. The contact purge clears name, email, address, on-site contact and her stated
//! names (D-1752) and keeps the phone, sealed, with its homeowner-phone index, for the consent and
//! opt-out records (D-079).
//!
//! Retention lock order (D-1839, `docs/architecture.md` section 8): (1) the transaction-scoped
//! retention advisory key ([`lock_retention_for_purge`] exclusive for a purge's decide-and-clear
//! transaction, shared inside every [`place_hold`] and [`release_hold`]); (2) `intake`'s homeowner
//! row; (3) the vault contact row, which [`purge_contact`]'s `UPDATE` locks last. A hold therefore
//! commits wholly before or wholly after a purge's decision, never between its read and its clear.
//!
//! Sealed photo objects (PRIV-009, D-1793, D-1841): [`purge_photos`] queues one
//! [`PHOTO_DELETE_KIND`] task per deleted row in the rows' own transaction, so the object delete
//! runs only after the rows' delete commits and a failed delete is retried by the task, never
//! blocking the rows. The daily [`ORPHAN_SWEEP_KIND`] lists `photos/` and queues the same delete,
//! a day later, for every object without a `privacy.photo` row (`store_photo` writes the object
//! before its row); the delete re-checks that no row names the object, so only objects at least
//! a day old with no row ever go.

use std::sync::Arc;

use askhava_adapters::api::{IdempotencyKey, StorageDelete};
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::{HoldSource, VaultError};

/// The PRIV-011 sentence "Delete my data" confirms with (D-214).
pub const DELETE_CONFIRMATION: &str = "We keep your phone number, locked, for 5 years only as proof of your texting permission and opt-out.";

/// The `core.task` kind that deletes one sealed photo object, payload `{"name"}` (PRIV-009).
pub const PHOTO_DELETE_KIND: &str = "privacy.photo_delete";
/// The daily `core.task` kind that sweeps `photos/` for objects with no row (PRIV-009, D-1793).
pub const ORPHAN_SWEEP_KIND: &str = "privacy.photo_orphan_sweep";
/// How long an object with no row is left alone first, so a `store_photo` between its object and
/// its row is never touched (D-1791, D-1841).
pub const ORPHAN_MIN_AGE: SignedDuration = SignedDuration::from_hours(24);
const PHOTO_PREFIX: &str = "photos/";
const DELETE_ATTEMPTS: i32 = 10;
const DB_FAILED: HandlerError = HandlerError("photo object delete database call failed");
const STORE_FAILED: HandlerError = HandlerError("photo object storage call failed");
const BAD_NAME: HandlerError = HandlerError("photo delete task payload is not a photo object");

fn db_err(_: sqlx::Error) -> VaultError {
    VaultError::Database
}

/// PRIV-007, D-1839: takes the retention advisory key exclusively until the caller's transaction
/// ends, so no hold is placed or released while a purge decides and clears. Lock (1) of the
/// retention lock order: call it first in the transaction, before the homeowner row.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn lock_retention_for_purge(db: &mut PgConnection) -> Result<(), VaultError> {
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(
           hashtextextended('privacy.retention', 274))) AS lock"#
    )
    .fetch_one(db)
    .await
    .map(|_| ())
    .map_err(db_err)
}

/// D-1839: the same key shared, held by each hold placement or release until it commits.
async fn lock_retention_shared(db: &mut PgConnection) -> Result<(), VaultError> {
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock_shared(
           hashtextextended('privacy.retention', 274))) AS lock"#
    )
    .fetch_one(db)
    .await
    .map(|_| ())
    .map_err(db_err)
}

/// PRIV-007, D-491: holds job `job`'s contact details for `source`'s record `id` from `at`. A record
/// already held or released is left as it is, so a repeated event changes nothing. Runs in its own
/// transaction (a savepoint inside the caller's) under the shared retention key (D-1839).
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
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    lock_retention_shared(&mut tx).await?;
    sqlx::query!(
        "INSERT INTO privacy.retention_hold (source, source_id, job_id, placed_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (source, source_id) DO NOTHING",
        source.code(),
        id,
        job,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// PRIV-007, D-491: releases `source`'s hold for record `id` at `at`; a release that arrives
/// before its hold is stored released, so the late hold changes nothing. Runs in its own
/// transaction (a savepoint inside the caller's) under the shared retention key (D-1839).
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
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    lock_retention_shared(&mut tx).await?;
    sqlx::query!(
        "INSERT INTO privacy.retention_hold (source, source_id, job_id, placed_at, released_at) \
         VALUES ($1, $2, $3, $4, $4) ON CONFLICT (source, source_id) DO UPDATE \
         SET released_at = COALESCE(privacy.retention_hold.released_at, EXCLUDED.released_at)",
        source.code(),
        id,
        job,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
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

/// PRIV-007, D-1839: true while any job in `jobs` has an unreleased dispute or complaint hold, in
/// one statement; the contact purge asks it with the retention key held.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn any_held(db: &mut PgConnection, jobs: &[Uuid]) -> Result<bool, VaultError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM privacy.retention_hold
           WHERE job_id = ANY($1) AND released_at IS NULL) AS "held!""#,
        jobs,
    )
    .fetch_one(db)
    .await
    .map_err(db_err)
}

/// PRIV-009, PRIV-011, D-1841: deletes every photo row of `job` (its links go with it), so no link
/// to them can be issued or opened again, and in the same transaction (a savepoint inside the
/// caller's) queues a [`PHOTO_DELETE_KIND`] task per row for its sealed object, which therefore
/// runs only once the rows' delete commits; returns how many rows went.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn purge_photos(db: &mut PgConnection, job: Uuid) -> Result<u64, VaultError> {
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    let names = sqlx::query_scalar!(
        "DELETE FROM privacy.photo WHERE job_id = $1 RETURNING storage_key",
        job
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    for name in &names {
        queue_object_delete(&mut tx, name, "purged", Timestamp::now()).await?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(u64::try_from(names.len()).unwrap_or(u64::MAX))
}

/// Queues the delete of object `name` at `at`, once per `why` (`purged` or `orphan`): an object
/// the sweep saw before its row existed is queued again when its row is purged (SEC-018).
async fn queue_object_delete(
    db: &mut PgConnection,
    name: &str,
    why: &str,
    at: Timestamp,
) -> Result<bool, VaultError> {
    let mut task = NewTask::new(PHOTO_DELETE_KIND, json!({ "name": name }), at);
    task.max_attempts = DELETE_ATTEMPTS;
    task.dedupe_key = Some(format!("{PHOTO_DELETE_KIND}:{why}:{name}"));
    let queued = enqueue(db, &task).await.map_err(|_| VaultError::Database)?;
    Ok(queued.is_some())
}

/// PRIV-009, D-1841: the [`PHOTO_DELETE_KIND`] handler. Deletes the `photos/` object the task
/// names unless a `privacy.photo` row still names it (a live photo); an object already gone is
/// success, so a rerun does nothing. A storage error fails the task, which the queue retries.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload, a database or a storage failure.
pub async fn run_photo_delete<S: StorageDelete + Sync>(
    pool: PgPool,
    (storage, secret): (&S, &SecretString),
    task: Claimed,
) -> Result<(), HandlerError> {
    let name = task.payload["name"].as_str().unwrap_or_default();
    if !name
        .strip_prefix(PHOTO_PREFIX)
        .is_some_and(|id| !id.is_empty())
    {
        return Err(BAD_NAME);
    }
    let live = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM privacy.photo WHERE storage_key = $1) AS "live!""#,
        name
    )
    .fetch_one(&pool)
    .await
    .map_err(|_| DB_FAILED)?;
    if live {
        return Ok(());
    }
    let key = IdempotencyKey::derive(secret, "photo.delete", &[name]);
    storage
        .delete_object(&key, name)
        .await
        .map_err(|_| STORE_FAILED)
}

/// PRIV-009, D-1793, D-1841: lists `photos/` and queues, [`ORPHAN_MIN_AGE`] after `now`, the
/// delete of every object no `privacy.photo` row names; returns how many it newly queued. The
/// delete re-checks the row, so an object whose row lands meanwhile stays.
///
/// # Errors
/// A fixed [`HandlerError`] for a storage or database failure.
pub async fn sweep_orphans<S: StorageDelete + Sync>(
    db: &mut PgConnection,
    storage: &S,
    now: Timestamp,
) -> Result<u64, HandlerError> {
    let names = storage
        .list_objects(PHOTO_PREFIX)
        .await
        .map_err(|_| STORE_FAILED)?;
    let rowed = sqlx::query_scalar!(
        "SELECT storage_key FROM privacy.photo WHERE storage_key = ANY($1)",
        &names
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| DB_FAILED)?;
    let mut queued = 0;
    for name in names.iter().filter(|name| !rowed.contains(name)) {
        let at = now + ORPHAN_MIN_AGE;
        if queue_object_delete(db, name, "orphan", at)
            .await
            .map_err(|_| DB_FAILED)?
        {
            queued += 1;
        }
    }
    Ok(queued)
}

/// Queues the next daily [`ORPHAN_SWEEP_KIND`] task, one per day (PRIV-009).
///
/// # Errors
/// [`VaultError::Database`].
pub async fn schedule_orphan_sweep(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), VaultError> {
    let run_at = now + ORPHAN_MIN_AGE;
    let mut task = NewTask::new(ORPHAN_SWEEP_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!(
        "{ORPHAN_SWEEP_KIND}:{}",
        run_at.as_second() / 86_400
    ));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| VaultError::Database)
}

/// PRIV-009, D-1841: registers [`PHOTO_DELETE_KIND`] and [`ORPHAN_SWEEP_KIND`] over the photo
/// store `storage`, deriving each delete's key from `secret`; the sweep runs at `clock`'s now and
/// then queues the next day's.
pub fn register_photo_deletes<S>(
    registry: &mut Registry,
    storage: Arc<S>,
    secret: SecretString,
    clock: Arc<dyn Clock>,
) where
    S: StorageDelete + Send + Sync + 'static,
{
    let store = Arc::clone(&storage);
    registry.register(PHOTO_DELETE_KIND, move |pool, task| {
        let (store, secret) = (Arc::clone(&store), secret.clone());
        async move { run_photo_delete(pool, (&*store, &secret), task).await }
    });
    registry.register(ORPHAN_SWEEP_KIND, move |pool, _task| {
        let (store, now) = (Arc::clone(&storage), clock.now());
        async move {
            let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
            sweep_orphans(&mut conn, &*store, now).await?;
            schedule_orphan_sweep(&mut conn, now)
                .await
                .map_err(|_| DB_FAILED)
        }
    });
}

/// PRIV-007, D-079: clears contact `id`'s name, email, address and on-site contact, its email
/// index and every name she stated in a text (`privacy.stated_name`, AI-020, D-1752), keeps the
/// sealed phone and its homeowner-phone index, and audits it once (`contact_purged`); true when
/// anything was left to clear. Inside the caller's transaction it runs as a savepoint, so the
/// retention purge decides and clears in one transaction (D-1839); its `UPDATE` takes the contact
/// row, lock (3) of the retention lock order.
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
    let stated = sqlx::query!("DELETE FROM privacy.stated_name WHERE contact_id = $1", id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    let cleared = cleared + stated;
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
/// D-079) and every photo of those jobs deleted, their sealed objects queued for deletion as in
/// [`purge_photos`]; returns [`DELETE_CONFIRMATION`]. Ledger rows hold
/// IDs only, so they stay as they are (PRIV-010). One transaction: all of it happens or none.
///
/// # Errors
/// [`VaultError::Database`], [`VaultError::Audit`].
pub async fn delete_my_data(
    db: &mut PgConnection,
    id: Uuid,
    jobs: &[Uuid],
) -> Result<&'static str, VaultError> {
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    purge_contact(&mut tx, id).await?;
    for job in jobs {
        purge_photos(&mut tx, *job).await?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(DELETE_CONFIRMATION)
}

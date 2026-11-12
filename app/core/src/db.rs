//! The database: the pool, the embedded migrations and the guarded-update outcome (JOB-004, D-092, D-491).
//!
//! `web` and `worker` connect as the app role (`askhava_app`, from `ASKHAVA_DATABASE_URL`);
//! only `server migrate` connects as the migration role, from `ASKHAVA_MIGRATION_DATABASE_URL`,
//! so the role that owns the schemas never serves a request (D-092, SEC-052). The roles
//! themselves come from `scripts/bootstrap_roles.sql`, never from a migration.
//!
//! A status change is one literal `UPDATE … WHERE id = $1 AND status = $2 RETURNING …` in the
//! module that owns the table, read with `fetch_optional` and turned into a [`Guarded`]. The
//! `UPDATE` takes the row lock; a second writer waits for it, re-checks the status and matches no
//! row, so it gets [`Guarded::MovedOn`] and changes nothing (JOB-004). Core cannot hold that SQL
//! for every module, since a query names only its own schema (D-491).

use std::str::FromStr;
use std::time::Duration;

use secrecy::SecretString;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use thiserror::Error;

use crate::settings::{Settings, expose};

/// Every file in the repository's `migrations/`, embedded at build time; `build.rs` rebuilds
/// the crate when one is added (D-488). `server migrate` and `#[sqlx::test(migrator = ...)]` use it.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// How long a request waits for a free pooled connection before it fails.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// An acquire waiting longer than this logs sqlx's one slow-acquire WARN line, which holds the
/// wait and this threshold in seconds and nothing else (D-3298).
pub const ACQUIRE_SLOW_THRESHOLD: Duration = Duration::from_millis(250);

/// The level of sqlx's per-statement and per-acquire events, which only the metrics layer's
/// filter lets through (D-3298). sqlx takes `log` levels; parsing the name keeps `log` out of
/// the direct dependencies (`kit/allowed_crates.txt`).
const METRICS_EVENT_LEVEL: &str = "debug";

/// Why the database could not be reached or migrated. No variant carries the URL or the
/// driver's message, which can quote it (SEC-001, PRIV-016).
#[derive(Debug, Error)]
pub enum DbError {
    /// The database URL does not parse.
    #[error("the database URL is not valid")]
    BadUrl,
    /// The database refused or dropped the connection.
    #[error("could not connect to the database")]
    Connect,
    /// This migration failed and was rolled back; every migration before it stays applied.
    #[error("migration {0} failed")]
    Migration(i64),
    /// Migration `{0}` was applied and its file has changed since; applied migrations are
    /// frozen (D-491).
    #[error("applied migration {0} was edited")]
    Edited(i64),
    /// Migration `{0}` is in the history table but no longer in `migrations/` (D-491).
    #[error("applied migration {0} is missing from migrations/")]
    Removed(i64),
    /// Migration `{0}` is recorded as partly applied and needs a hand fix before any other runs.
    #[error("migration {0} is partly applied")]
    Dirty(i64),
    /// The migrations could not run for another reason: the history table, a lock or the
    /// connection.
    #[error("the migrations could not run")]
    Migrate,
}

/// The outcome of a status change made only while the row is still in the expected status
/// (JOB-004).
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a MovedOn outcome means nothing changed and the caller must not act on it"]
pub enum Guarded<T> {
    /// The row was in the expected status and now holds the new one; `T` is what `RETURNING` gave.
    Applied(T),
    /// Another change got there first: the row was not in the expected status and nothing changed.
    MovedOn,
}

impl<T> Guarded<T> {
    /// Reads a guarded `UPDATE … RETURNING` fetched with `fetch_optional`: a row means applied,
    /// none means moved on (JOB-004).
    pub fn from_returning(row: Option<T>) -> Self {
        row.map_or(Self::MovedOn, Self::Applied)
    }

    /// The returned row if the change applied, `None` if things moved on (JOB-004).
    #[must_use]
    pub fn applied(self) -> Option<T> {
        match self {
            Self::Applied(row) => Some(row),
            Self::MovedOn => None,
        }
    }
}

/// The app role's pool for `web` and `worker`, `ASKHAVA_DB_POOL_MAX` connections at most, so
/// the replicas and the worker stay under the host's connection limit (postgres brain P8). The
/// pool connects on first use, so a down database fails requests, not startup. Call it inside
/// the Tokio runtime.
///
/// It also turns on sqlx's statement and acquire-time events and registers the pool with the
/// process's database metrics, so a snapshot reads its size and idle count (D-3292, D-3298); the
/// maximum and the acquire timeout are not touched by that.
///
/// # Errors
///
/// [`DbError::BadUrl`] when `ASKHAVA_DATABASE_URL` does not parse.
pub fn app_pool(settings: &Settings) -> Result<PgPool, DbError> {
    let mut options = connect_options(settings.database_url())?;
    let mut pool = PgPoolOptions::new()
        .max_connections(settings.db_pool_max())
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .acquire_slow_threshold(ACQUIRE_SLOW_THRESHOLD);
    if let Ok(level) = METRICS_EVENT_LEVEL.parse() {
        options = options.log_statements(level);
        pool = pool.acquire_time_level(level);
    }
    let pool = pool.connect_lazy_with(options);
    crate::db_metrics::global().register_pool(&pool);
    Ok(pool)
}

/// Applies every migration not yet applied, in number order, over one connection as the
/// migration role: the `server migrate` release step (D-092, D-491). Applied migrations are
/// skipped, so a second run is a no-op.
///
/// # Errors
///
/// [`DbError::BadUrl`], [`DbError::Connect`], [`DbError::Migration`], [`DbError::Edited`],
/// [`DbError::Removed`], [`DbError::Dirty`] or [`DbError::Migrate`]; none of them echoes the URL
/// or the database's message.
pub async fn migrate(migration_url: &SecretString) -> Result<(), DbError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_with(connect_options(migration_url)?)
        .await
        .map_err(|_| DbError::Connect)?;
    let outcome = MIGRATOR.run(&pool).await.map_err(migrate_error);
    pool.close().await;
    outcome
}

/// Keeps the migration number sqlx reports and drops the driver's message, which can quote the
/// URL or a value (SEC-001, PRIV-016).
fn migrate_error(err: MigrateError) -> DbError {
    match err {
        MigrateError::ExecuteMigration(_, version) => DbError::Migration(version),
        MigrateError::VersionMismatch(version) => DbError::Edited(version),
        MigrateError::VersionMissing(version) => DbError::Removed(version),
        MigrateError::Dirty(version) => DbError::Dirty(version),
        _ => DbError::Migrate,
    }
}

fn connect_options(url: &SecretString) -> Result<PgConnectOptions, DbError> {
    PgConnectOptions::from_str(expose(url)).map_err(|_| DbError::BadUrl)
}

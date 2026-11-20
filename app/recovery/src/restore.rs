//! The monthly test restore into a throwaway database (OPS-013, OPS-051; D-232, D-492, D-4293,
//! D-4835; TD-OPS-22, TD-OPS-26).
//!
//! Each [`RESTORE_KIND`] run pipes a fresh `pg_dump -Fc` of the app database (the read-only
//! `askhava_backup` role, before any encryption) into a database it creates on the separate
//! throwaway PostgreSQL 16 service behind `ASKHAVA_RESTORE_ADMIN_URL`, never the live database:
//! `scripts/bootstrap_roles.sql` first (embedded, piped into `psql`, no role password in its
//! environment, so every role stays `NOLOGIN`), then `pg_restore --single-transaction`, then the
//! ownership and append-only grant check, and the database is dropped whatever happened. It then
//! checks the newest nightly export's `recovery.backup_check` row (the object itself is
//! write-only, PRIV-029). Each result is one `recovery.backup_check` row of kind `restore` and one
//! audit entry; a failure also raises one High [`RESTORE_FAILED_ALERT`], a kind that never starts
//! safe mode. Every tool is found by [`pg_tool`] and gets only `PATH` and `PGPASSWORD`; no URL,
//! password or database message reaches an error or a log (SEC-052).

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use askhava_core::api::{Clock, Environment, HandlerError, NewTask, Registry, Settings, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::postgres::PgConnectOptions;
use sqlx::{ConnectOptions, PgConnection, PgPool};
use thiserror::Error;

use crate::backup::pg_tool;

/// The `core.task` kind of the monthly test restore (OPS-013).
pub const RESTORE_KIND: &str = "recovery.test_restore";
/// The High alert a failed test restore raises (OPS-013, D-232); not a safe-mode kind.
pub const RESTORE_FAILED_ALERT: &str = "recovery.test_restore_failed";
/// The audit action of a test restore that passed; its subject is the `recovery.backup_check`
/// row. The weekly digest reads the newest through `ops::api::latest_audit` (OPS-013).
pub const RESTORE_PASSED_ACTION: &str = "recovery.restore_passed";
/// The audit action of a test restore that failed (OPS-013); subject as above.
pub const RESTORE_FAILED_ACTION: &str = "recovery.restore_failed";
/// OPS-013: one restore's deadline, under the worker's 2-minute lease, so a slow run is recorded
/// (as `timeout`) and its tools are killed.
pub const RESTORE_DEADLINE: Duration = Duration::from_secs(100);

/// OPS-051: `scripts/bootstrap_roles.sql`, embedded because the runtime image holds no `scripts/`.
const BOOTSTRAP_ROLES: &str = include_str!("../../../scripts/bootstrap_roles.sql");
/// OPS-051, OPS-010, CON-022: the append-only tables the app role may never `UPDATE` or `DELETE`.
const APPEND_ONLY: [&str; 5] = [
    "ops.audit_log",
    "intake.consent_record",
    "messaging.optout_event",
    "quotes.job_card",
    "recovery.backup_check",
];
/// D-4835: the newest nightly export must have finished within this long, in seconds (26 hours).
const EXPORT_MAX_AGE_SECS: i64 = 26 * 3_600;
/// OPS-013: the restore's hour in UTC, [`AFTER_EXPORT_SECS`] after the nightly export's (10:00).
const RESTORE_HOUR_UTC: i64 = 12;
const AFTER_EXPORT_SECS: i64 = 2 * 3_600;
const DAY_SECS: i64 = 86_400;

/// Why a test restore failed. No variant carries a URL, a password or a database message.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum RestoreError {
    /// `ASKHAVA_BACKUP_DATABASE_URL` or `ASKHAVA_RESTORE_ADMIN_URL` is missing or invalid.
    #[error("restore settings missing or invalid")]
    Settings,
    /// No PostgreSQL 16 `pg_dump`, `psql` or `pg_restore` on `PATH`, or one did not start.
    #[error("a PostgreSQL 16 tool is missing or did not start")]
    Tool,
    /// The throwaway service could not be reached, or its database not created or dropped.
    #[error("throwaway database not created or dropped")]
    Throwaway,
    /// `scripts/bootstrap_roles.sql` failed (OPS-051).
    #[error("role bootstrap failed")]
    Bootstrap,
    /// `pg_dump` or `pg_restore --single-transaction` failed; nothing was left behind (OPS-051).
    #[error("pg_restore failed")]
    Restore,
    /// A restored object is owned by a role other than `askhava_migrator` (OPS-051).
    #[error("a restored object is not owned by the migration role")]
    Owner,
    /// `askhava_app` holds `UPDATE` or `DELETE` on an append-only table (OPS-051).
    #[error("the app role can change an append-only table")]
    Grant,
    /// The newest nightly export row is missing, failed, stale or incomplete (D-4835).
    #[error("the latest nightly export is missing, failed, stale or incomplete")]
    Export,
    /// A database read or write failed.
    #[error("database error")]
    Database,
    /// The run passed [`RESTORE_DEADLINE`]; its tools were killed.
    #[error("restore timed out")]
    Timeout,
}

impl RestoreError {
    /// The code name stored as the check's `failure` (PRIV-028: never a value).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Tool => "tool_missing",
            Self::Throwaway => "throwaway",
            Self::Bootstrap => "bootstrap",
            Self::Restore => "restore",
            Self::Owner => "owner",
            Self::Grant => "append_only_grant",
            Self::Export => "export_record",
            Self::Database => "database",
            Self::Timeout => "timeout",
        }
    }
}

/// The test restore's two databases: the app database as the read-only `askhava_backup` role, and
/// the throwaway service's admin role (OPS-051, SEC-052). Both are secrets and never logged.
#[derive(Debug, Clone)]
pub struct RestoreSettings {
    backup_url: SecretString,
    admin_url: SecretString,
}

impl RestoreSettings {
    /// The settings for a dump of `backup_url` restored on the service behind `admin_url`.
    #[must_use]
    pub fn new(backup_url: SecretString, admin_url: SecretString) -> Self {
        Self {
            backup_url,
            admin_url,
        }
    }

    /// OPS-051, SEC-052, D-4293: from `ASKHAVA_BACKUP_DATABASE_URL` and
    /// `ASKHAVA_RESTORE_ADMIN_URL`, both set on the `worker` service only.
    ///
    /// # Errors
    ///
    /// [`RestoreError::Settings`] when either key is missing.
    pub fn from_settings(settings: &Settings) -> Result<Self, RestoreError> {
        let backup = settings.backup_database_url();
        let admin = settings.restore_admin_url();
        match (backup, admin) {
            (Ok(backup), Ok(admin)) => Ok(Self::new(backup.clone(), admin.clone())),
            _ => Err(RestoreError::Settings),
        }
    }
}

/// The percent-decoded form of a URL's password.
fn percent_decode(text: &str) -> Option<String> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

/// SEC-052: the PostgreSQL 16 `tool` against `url` (its database replaced by `database` when
/// given): the password only in `PGPASSWORD`, never on the command line, nothing read from a
/// prompt, and an environment cleared to `PATH`, so no host key and no role password reaches it.
fn tool_command(
    tool: &'static str,
    url: &SecretString,
    database: Option<&str>,
) -> Result<Command, RestoreError> {
    let path = pg_tool(tool).map_err(|_| RestoreError::Tool)?.path;
    let mut parsed =
        reqwest::Url::parse(url.expose_secret()).map_err(|_| RestoreError::Settings)?;
    if !matches!(parsed.scheme(), "postgres" | "postgresql") {
        return Err(RestoreError::Settings);
    }
    let password = parsed.password().map(percent_decode);
    parsed
        .set_password(None)
        .map_err(|()| RestoreError::Settings)?;
    if let Some(database) = database {
        parsed.set_path(database);
    }
    let mut cmd = Command::new(path);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .args(["--no-password", "--dbname", parsed.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(password) = password {
        cmd.env("PGPASSWORD", password.ok_or(RestoreError::Settings)?);
    }
    Ok(cmd)
}

/// Waits for `child` until `until`, then kills and reaps it: `failed` for a non-zero exit,
/// [`RestoreError::Timeout`] past the deadline, so no tool outlives its run (OPS-013).
fn finish(child: &mut Child, until: Instant, failed: RestoreError) -> Result<(), RestoreError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(failed),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                child.kill().ok();
                child.wait().ok();
                return Err(RestoreError::Timeout);
            }
        }
    }
}

/// Pipes `script` into `psql -X -v ON_ERROR_STOP=1 -f -` against `database` on the throwaway
/// service, with `:"db"` set to `name`; `failed` when it exits non-zero.
fn psql(
    urls: &RestoreSettings,
    (database, name): (&str, &str),
    script: &str,
    (until, failed): (Instant, RestoreError),
) -> Result<(), RestoreError> {
    let mut child = tool_command("psql", &urls.admin_url, Some(database))?
        .args(["-X", "-q", "-v", "ON_ERROR_STOP=1", "-v"])
        .arg(format!("db={name}"))
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|_| RestoreError::Tool)?;
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(script.as_bytes()).is_ok());
    finish(&mut child, until, failed)?;
    if written { Ok(()) } else { Err(failed) }
}

/// OPS-051: a fresh `pg_dump -Fc` of the app database streamed into
/// `pg_restore --single-transaction` on `database`, so a failed restore leaves no object.
fn load(urls: &RestoreSettings, database: &str, until: Instant) -> Result<(), RestoreError> {
    let mut dump = tool_command("pg_dump", &urls.backup_url, None)?
        .arg("--format=custom")
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| RestoreError::Tool)?;
    let stream = dump.stdout.take().ok_or(RestoreError::Tool)?;
    let restored = tool_command("pg_restore", &urls.admin_url, Some(database)).and_then(|mut c| {
        c.args(["--single-transaction", "--exit-on-error"])
            .stdin(Stdio::from(stream))
            .spawn()
            .map_err(|_| RestoreError::Tool)
    });
    let restored = restored.and_then(|mut c| finish(&mut c, until, RestoreError::Restore));
    let dumped = finish(&mut dump, until, RestoreError::Restore);
    restored.and(dumped)
}

/// Runs blocking tool work off the async runtime.
async fn blocking<F>(work: F) -> Result<(), RestoreError>
where
    F: FnOnce() -> Result<(), RestoreError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| RestoreError::Tool)?
}

/// OPS-051: restores a fresh dump of the app database into `database`, an existing database on
/// the throwaway service, in one transaction and without the role bootstrap; [`test_restore`]
/// runs the bootstrap first. Public so TD-OPS-26 can show a restore with no roles leaves nothing.
///
/// # Errors
///
/// [`RestoreError::Restore`] when the dump or the restore fails, or another [`RestoreError`].
pub async fn load_dump(urls: &RestoreSettings, database: &str) -> Result<(), RestoreError> {
    let (urls, database) = (urls.clone(), database.to_owned());
    blocking(move || load(&urls, &database, Instant::now() + RESTORE_DEADLINE)).await
}

/// OPS-051, D-492: fails when any restored object (a relation, routine, type or schema outside
/// the system schemas) is owned by a role other than `askhava_migrator`, or when `askhava_app`
/// holds `UPDATE` or `DELETE` on an [`APPEND_ONLY`] table (a missing one fails too).
async fn check_restored(urls: &RestoreSettings, database: &str) -> Result<(), RestoreError> {
    let mut conn = PgConnectOptions::from_str(urls.admin_url.expose_secret())
        .map_err(|_| RestoreError::Settings)?
        .database(database)
        .connect()
        .await
        .map_err(|_| RestoreError::Throwaway)?;
    let strays = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM (
             SELECT c.relowner AS owner, n.nspname FROM pg_class c
               JOIN pg_namespace n ON n.oid = c.relnamespace
             UNION ALL SELECT p.proowner, n.nspname FROM pg_proc p
               JOIN pg_namespace n ON n.oid = p.pronamespace
             UNION ALL SELECT t.typowner, n.nspname FROM pg_type t
               JOIN pg_namespace n ON n.oid = t.typnamespace
             UNION ALL SELECT nspowner, nspname FROM pg_namespace WHERE nspname <> 'public') o
           WHERE o.nspname !~ '^(pg_|information_schema$)'
             AND o.owner <> 'askhava_migrator'::regrole"#
    )
    .fetch_one(&mut conn)
    .await
    .map_err(|_| RestoreError::Database)?;
    if strays > 0 {
        return Err(RestoreError::Owner);
    }
    let tables: Vec<String> = APPEND_ONLY.iter().map(|t| (*t).to_owned()).collect();
    let open = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM unnest($1::text[]) AS t(name)
           WHERE to_regclass(t.name) IS NULL
              OR has_table_privilege('askhava_app', to_regclass(t.name), 'UPDATE')
              OR has_table_privilege('askhava_app', to_regclass(t.name), 'DELETE')"#,
        &tables,
    )
    .fetch_one(&mut conn)
    .await
    .map_err(|_| RestoreError::Database)?;
    if open > 0 {
        return Err(RestoreError::Grant);
    }
    Ok(())
}

/// OPS-013, OPS-051, D-492: one test restore on the throwaway service: a database under a name
/// made here, the role bootstrap, the restore, the ownership and grant check, and the database
/// dropped whatever happened. Never touches the app database beyond the read-only dump.
///
/// # Errors
///
/// The first [`RestoreError`] met; a failed drop is [`RestoreError::Throwaway`].
pub async fn test_restore(urls: &RestoreSettings) -> Result<(), RestoreError> {
    let until = Instant::now() + RESTORE_DEADLINE;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("askhava_restore_{stamp}");
    let (u, n) = (urls.clone(), name.clone());
    let made = blocking(move || {
        let run = (until, RestoreError::Throwaway);
        psql(&u, ("postgres", &n), "CREATE DATABASE :\"db\";\n", run)?;
        psql(
            &u,
            (&n, &n),
            BOOTSTRAP_ROLES,
            (until, RestoreError::Bootstrap),
        )?;
        load(&u, &n, until)
    })
    .await;
    let result = match made {
        Ok(()) => check_restored(urls, &name).await,
        failed => failed,
    };
    let u = urls.clone();
    let dropped = blocking(move || {
        let run = (Instant::now() + RESTORE_DEADLINE, RestoreError::Throwaway);
        let drop = "DROP DATABASE IF EXISTS :\"db\" WITH (FORCE);\n";
        psql(&u, ("postgres", &name), drop, run)
    })
    .await;
    result.and(dropped)
}

/// D-4835: the newest nightly export's record is `ok`, finished within 26 hours of `now` and has
/// its size and hash; the object itself is write-only (PRIV-029), so only its record is checked.
async fn export_recorded(pool: &PgPool, now: Timestamp) -> Result<(), RestoreError> {
    let oldest = Timestamp::from_second(now.as_second() - EXPORT_MAX_AGE_SECS)
        .map_err(|_| RestoreError::Export)?;
    let fresh = sqlx::query_scalar!(
        r#"SELECT COALESCE((SELECT ok AND object_bytes IS NOT NULL AND object_sha256 IS NOT NULL
             AND finished_at >= $1 FROM recovery.backup_check WHERE kind = 'export'
             ORDER BY finished_at DESC LIMIT 1), false) AS "fresh!""#,
        oldest.to_sqlx(),
    )
    .fetch_one(pool)
    .await
    .map_err(|_| RestoreError::Database)?;
    if fresh {
        Ok(())
    } else {
        Err(RestoreError::Export)
    }
}

/// OPS-013, D-232, PRIV-028: one `recovery.backup_check` row of kind `restore`, its audit entry
/// and, for a failure, one High [`RESTORE_FAILED_ALERT`] about that row, in one transaction.
async fn record(
    pool: &PgPool,
    (started, finished): (Timestamp, Timestamp),
    result: Result<(), RestoreError>,
) -> Result<(), RestoreError> {
    let db = |_| RestoreError::Database;
    let failure = result.err().map(RestoreError::code);
    let mut tx = pool.begin().await.map_err(db)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO recovery.backup_check (kind, ok, failure, started_at, finished_at) \
         VALUES ('restore', $1, $2, $3, $4) RETURNING id",
        failure.is_none(),
        failure,
        started.to_sqlx(),
        finished.to_sqlx(),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: failure.map_or(RESTORE_PASSED_ACTION, |_| RESTORE_FAILED_ACTION),
        subject_kind: "recovery.backup_check",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| RestoreError::Database)?;
    if failure.is_some() {
        let alert = NewAlert {
            kind: RESTORE_FAILED_ALERT,
            severity: Severity::High,
            subject: Some(("recovery.backup_check", id)),
        };
        raise_alert(&mut tx, &alert)
            .await
            .map_err(|_| RestoreError::Database)?;
    }
    tx.commit().await.map_err(db)
}

/// Queues one [`RESTORE_KIND`] at `run_at`, at most one per calendar month (its dedupe key).
async fn queue(db: &mut PgConnection, run_at: Timestamp) -> Result<(), RestoreError> {
    let mut task = NewTask::new(RESTORE_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{RESTORE_KIND}:{}", run_at.strftime("%Y-%m")));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| RestoreError::Database)
}

/// OPS-013: queues the first monthly [`RESTORE_KIND`] at the first restore hour that follows the
/// next nightly export after `now`, so a fresh environment never alerts before any export row
/// exists; once per month (its dedupe key), so a restart never doubles it.
///
/// # Errors
///
/// [`RestoreError::Database`].
pub async fn schedule_restore(db: &mut PgConnection, now: Timestamp) -> Result<(), RestoreError> {
    queue(db, first_restore_at(now)).await
}

/// The first restore hour after the first export hour after `now` (OPS-013).
fn first_restore_at(now: Timestamp) -> Timestamp {
    let after = now.as_second() + AFTER_EXPORT_SECS;
    let today = after.div_euclid(DAY_SECS) * DAY_SECS + RESTORE_HOUR_UTC * 3_600;
    let first = if today > after {
        today
    } else {
        today + DAY_SECS
    };
    Timestamp::from_second(first).unwrap_or(Timestamp::MAX)
}

/// One calendar month after `at` (OPS-013: the restore is monthly).
fn month_on(at: Timestamp) -> Timestamp {
    at.to_zoned(jiff::tz::TimeZone::UTC)
        .checked_add(jiff::Span::new().months(1))
        .map_or(Timestamp::MAX, |z| z.timestamp())
}

/// OPS-013, D-232: one monthly run: queue the next a month on, restore, check the export record,
/// record the result. A failure is recorded and alerted once, so the task itself succeeds and is
/// not retried into a second alert; only a failed queue or record fails it.
async fn run_restore(
    pool: &PgPool,
    urls: &Result<RestoreSettings, RestoreError>,
    clock: &dyn Clock,
) -> Result<(), HandlerError> {
    let started = clock.now();
    let mut conn = pool.acquire().await.map_err(|_| HandlerError("database"))?;
    queue(&mut conn, month_on(started))
        .await
        .map_err(|e| HandlerError(e.code()))?;
    drop(conn);
    let mut result = match urls {
        Ok(urls) => test_restore(urls).await,
        Err(e) => Err(*e),
    };
    if result.is_ok() {
        result = export_recorded(pool, started).await;
    }
    let finished = clock.now().max(started);
    record(pool, (started, finished), result)
        .await
        .map_err(|e| HandlerError(e.code()))
}

/// OPS-013, SEC-052, D-4293: registers [`RESTORE_KIND`] (always), timed by `clock`.
///
/// # Errors
///
/// In test and live, [`RestoreError::Settings`] when `ASKHAVA_BACKUP_DATABASE_URL` or
/// `ASKHAVA_RESTORE_ADMIN_URL` is missing, which stops the worker's start-up; in dev and CI the
/// handler is registered and each run fails closed (recorded and alerted) instead.
pub fn register_restore(
    registry: &mut Registry,
    settings: &Settings,
    clock: Arc<dyn Clock>,
) -> Result<(), RestoreError> {
    let urls = RestoreSettings::from_settings(settings);
    let strict = matches!(
        settings.environment(),
        Environment::Test | Environment::Live
    );
    if let (Err(e), true) = (&urls, strict) {
        return Err(*e);
    }
    registry.register(RESTORE_KIND, move |pool, _task| {
        let (urls, clock) = (urls.clone(), Arc::clone(&clock));
        async move { run_restore(&pool, &urls, &*clock).await }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    //! The scheduler, the export-record check and the failure path (TD-OPS-22). Fake data only.

    use askhava_core::api::{ManualClock, due_or_running};
    use askhava_ops::api::{
        SAFE_MODE_ON_ACTION, alert_subject, digest_alerts, latest_audit, record_push,
        run_safe_mode_check,
    };
    use jiff::SignedDuration;

    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    /// OPS-013: whenever the worker starts, the first restore is due two hours after the next
    /// 10:00 UTC export, so an export row exists before the first check of one.
    #[test]
    fn the_first_restore_is_due_after_the_next_nightly_export() {
        for (now, due) in [
            ("2026-01-05T09:59:59Z", "2026-01-05T12:00:00Z"),
            ("2026-01-05T10:00:00Z", "2026-01-06T12:00:00Z"),
            ("2026-01-05T23:30:00Z", "2026-01-06T12:00:00Z"),
        ] {
            assert_eq!(first_restore_at(at(now)), at(due), "started at {now}");
        }
        assert_eq!(
            month_on(at("2026-01-31T12:00:00Z")),
            at("2026-02-28T12:00:00Z")
        );
    }

    async fn export_row(pool: &PgPool, ok: bool, sized: bool, finished: Timestamp) {
        let name = sized.then_some("exports/fake.age");
        sqlx::query!(
            "INSERT INTO recovery.backup_check (kind, ok, object_name, object_bytes, \
             object_sha256, failure, started_at, finished_at) \
             VALUES ('export', $1, $2, $3, $4, $5, $6, $6)",
            ok,
            name,
            name.map(|_| 7_i64),
            name.map(|_| [7_u8; 32].as_slice()),
            (!ok).then_some("dump"),
            finished.to_sqlx(),
        )
        .execute(pool)
        .await
        .unwrap();
    }

    /// D-4835: a missing, stale, failed or incomplete newest export row fails the check.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn the_latest_export_record_must_be_ok_fresh_and_complete(pool: PgPool) {
        let now = at("2026-05-02T12:00:00Z");
        let failed = Err(RestoreError::Export);
        assert_eq!(export_recorded(&pool, now).await, failed, "missing");
        export_row(&pool, true, true, at("2026-05-01T09:59:59Z")).await;
        assert_eq!(export_recorded(&pool, now).await, failed, "stale");
        export_row(&pool, true, true, at("2026-05-01T10:00:00Z")).await;
        assert_eq!(export_recorded(&pool, now).await, Ok(()));
        export_row(&pool, false, false, at("2026-05-02T10:00:00Z")).await;
        assert_eq!(export_recorded(&pool, now).await, failed, "failed");
        export_row(&pool, true, false, at("2026-05-02T10:00:01Z")).await;
        assert_eq!(export_recorded(&pool, now).await, failed, "no size or hash");
    }

    /// TD-OPS-22 (OPS-013, D-232): a failed test restore (here: no settings, as in dev) is one
    /// `restore` row, one audit entry and one High alert, queues next month's run, and its
    /// alert, even pushed and unacknowledged for 48 hours, starts no safe mode.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_ops_22_a_failed_restore_raises_one_high_alert_and_no_safe_mode(pool: PgPool) {
        let started = Timestamp::now() - SignedDuration::from_hours(49);
        let clock = ManualClock::new(started);
        let missing = Err(RestoreError::Settings);
        run_restore(&pool, &missing, &clock).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let row = sqlx::query!(
            r#"SELECT id, failure AS "failure!" FROM recovery.backup_check
               WHERE kind = 'restore' AND NOT ok"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.failure, "settings");
        let audited = latest_audit(&mut conn, RESTORE_FAILED_ACTION)
            .await
            .unwrap();
        assert_eq!(audited.map(|a| a.subject_id), Some(row.id));
        let kinds = [RESTORE_FAILED_ALERT];
        let soon = Timestamp::now() + SignedDuration::from_hours(1);
        let alerts = digest_alerts(&mut conn, &kinds, started, soon)
            .await
            .unwrap();
        assert_eq!(alerts.len(), 1, "one alert");
        let about = Some(("recovery.backup_check".to_owned(), row.id));
        assert_eq!(alerts[0].subject, about);
        let alert = alert_subject(&mut conn, alerts[0].id).await.unwrap();
        assert_eq!(alert.map(|a| a.severity).as_deref(), Some("high"));
        let key = format!("{RESTORE_KIND}:{}", month_on(started).strftime("%Y-%m"));
        let queued = due_or_running(&mut conn, RESTORE_KIND, &key).await;
        assert!(queued.unwrap(), "next month's run is queued");
        record_push(&mut conn, alerts[0].id, started).await.unwrap();
        run_safe_mode_check(&pool, Timestamp::now()).await.unwrap();
        let turned_on = latest_audit(&mut conn, SAFE_MODE_ON_ACTION).await.unwrap();
        assert_eq!(turned_on, None, "no safe mode");
    }
}

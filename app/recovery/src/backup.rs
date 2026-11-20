//! The nightly encrypted export and the PostgreSQL tool path helper (OPS-013, OPS-051, PRIV-018,
//! PRIV-024, PRIV-029; T109, D-492, D-3155, D-3883, D-4293, D-4707, D-4708).
//!
//! [`EXPORT_KIND`] runs nightly at 10:00 UTC (3 a.m. in Arizona): it queues the next night's run
//! first, then runs `pg_dump -Fc` as the read-only `askhava_backup` role (its password passed in
//! `PGPASSWORD`, never on the command line), streams the dump through `age` to the offline X25519
//! recipient (the host holds no private half), and stores it under [`EXPORT_PREFIX`] through the
//! storage adapter (Backblaze B2's write-only, prefix-scoped key with versioning and object lock in
//! Phase 2; a local folder now). Each run, stored or failed, is one `recovery.backup_check` row and
//! one `ops` audit entry in one transaction. Missing export settings fail the run closed in dev and
//! CI; in test and live [`register_export`] refuses to start. [`pg_tool`] finds `pg_dump`,
//! `pg_restore`, `psql`, `initdb` or `pg_ctl` by name on `PATH`, PostgreSQL 16 only (T206 and T648
//! reuse it). The worker drops a handler unrecorded at its 2-minute lease, so each export has its
//! own [`EXPORT_DEADLINE`] under it: past that the export is dropped, its `pg_dump` killed and
//! reaped (the [`Dump`] guard), and the run recorded as a `timeout` failure. `pg_dump` starts with
//! a cleared environment (only `PATH` and `PGPASSWORD`), so no host secret reaches it.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use askhava_adapters::api::{IdempotencyKey, StorageAdapter};
use askhava_core::api::{Clock, Environment, HandlerError, NewTask, Registry, Settings, enqueue};
use askhava_ops::api::{
    Actor, AuditEntry, BACKUP_EXPORT_FAILED_ACTION, BACKUP_EXPORTED_ACTION, Change, audit,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::{ExportSettings, RecoveryError};

/// The `core.task` kind of the nightly encrypted export (OPS-013).
pub const EXPORT_KIND: &str = "recovery.nightly_export";

/// The storage prefix of every export object, `exports/<date>/<task>.age` (PRIV-029).
pub const EXPORT_PREFIX: &str = "exports";

/// The PostgreSQL major version of the tools and the server (D-4708); never a newer one.
pub const PG_MAJOR: u32 = 16;

/// OPS-013: the export's hour in UTC, 3 a.m. in Arizona (a fixed UTC-7), outside business hours.
const EXPORT_HOUR_UTC: i64 = 10;
const DAY_SECS: i64 = 86_400;

/// OPS-013: one export's deadline, under the worker's 2-minute lease so a slow run is still
/// recorded (as `timeout`) rather than dropped by the worker unrecorded.
pub const EXPORT_DEADLINE: Duration = Duration::from_secs(100);

/// A PostgreSQL 16 binary found on `PATH` (D-4708).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgTool {
    /// The binary's full path.
    pub path: PathBuf,
    /// Its `--version` major, always [`PG_MAJOR`].
    pub major: u32,
}

/// D-4708: the first `tool` on this process's `PATH` whose `--version` major is [`PG_MAJOR`];
/// another major is skipped, never used. Blocking: call it off the async runtime.
///
/// # Errors
///
/// [`RecoveryError::ToolMissing`] naming `tool` and the `PATH` searched.
pub fn pg_tool(tool: &'static str) -> Result<PgTool, RecoveryError> {
    pg_tool_on(tool, &std::env::var_os("PATH").unwrap_or_default())
}

fn pg_tool_on(tool: &'static str, search: &OsStr) -> Result<PgTool, RecoveryError> {
    std::env::split_paths(search)
        .map(|dir| dir.join(tool))
        .filter(|path| path.is_file())
        .find_map(|path| {
            tool_major(&path)
                .filter(|major| *major == PG_MAJOR)
                .map(|major| PgTool { path, major })
        })
        .ok_or_else(|| RecoveryError::ToolMissing {
            tool,
            major: PG_MAJOR,
            searched: search.to_string_lossy().into_owned(),
        })
}

fn tool_major(path: &Path) -> Option<u32> {
    let out = Command::new(path).arg("--version").output().ok()?;
    parse_major(&String::from_utf8_lossy(&out.stdout))
}

/// The major of a `--version` line such as `pg_dump (PostgreSQL) 16.15 (Ubuntu ...)`.
fn parse_major(version: &str) -> Option<u32> {
    let (_, rest) = version.split_once("(PostgreSQL) ")?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The database URL without its password, and the decoded password for `PGPASSWORD`, so the
/// secret never shows in the process list (SEC-052).
fn split_password(url: &SecretString) -> Result<(String, Option<String>), RecoveryError> {
    let bad = RecoveryError::Settings("ASKHAVA_BACKUP_DATABASE_URL");
    let mut parsed = reqwest::Url::parse(url.expose_secret()).map_err(|_| bad.clone())?;
    if !matches!(parsed.scheme(), "postgres" | "postgresql") {
        return Err(bad);
    }
    let password = match parsed.password() {
        Some(p) => Some(percent_decode(p).ok_or_else(|| bad.clone())?),
        None => None,
    };
    parsed.set_password(None).map_err(|()| bad)?;
    Ok((parsed.into(), password))
}

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

/// OPS-051, SEC-052: the `pg_dump -Fc` command for `url`, its stdout piped, nothing read from a
/// prompt, and its environment cleared to `PATH` and `PGPASSWORD` so no host key reaches it.
fn dump_command(tool: &PgTool, url: &SecretString) -> Result<Command, RecoveryError> {
    let (dbname, password) = split_password(url)?;
    let mut cmd = Command::new(&tool.path);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.args(["--format=custom", "--no-password", "--dbname"])
        .arg(dbname)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(password) = password {
        cmd.env("PGPASSWORD", password);
    }
    Ok(cmd)
}

/// PRIV-018, PRIV-024: `input` encrypted to `recipient` as a binary `age` file.
fn encrypt_stream(
    mut input: impl Read,
    recipient: &age::x25519::Recipient,
) -> Result<Vec<u8>, RecoveryError> {
    let to = std::iter::once(recipient as &dyn age::Recipient);
    let encryptor = age::Encryptor::with_recipients(to).map_err(|_| RecoveryError::Encrypt)?;
    let mut out = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut out)
        .map_err(|_| RecoveryError::Encrypt)?;
    std::io::copy(&mut input, &mut writer).map_err(|_| RecoveryError::Encrypt)?;
    writer.finish().map_err(|_| RecoveryError::Encrypt)?;
    Ok(out)
}

/// OPS-013: a running `pg_dump`, killed and reaped when dropped before it exited (a failed
/// encryption, or the export dropped at [`EXPORT_DEADLINE`]), so no dump outlives its run.
struct Dump(std::process::Child);

impl Drop for Dump {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            self.0.kill().ok();
            self.0.wait().ok();
        }
    }
}

/// OPS-051, PRIV-024: runs the dump and encrypts it as it streams, off the async runtime.
async fn dump_encrypted(tool: &PgTool, export: ExportSettings) -> Result<Vec<u8>, RecoveryError> {
    let child = dump_command(tool, &export.backup_url)?
        .spawn()
        .map_err(|_| RecoveryError::Dump)?;
    let mut dump = Dump(child);
    let stdout = dump.0.stdout.take().ok_or(RecoveryError::Dump)?;
    let recipient = export.recipient;
    let encrypted = tokio::task::spawn_blocking(move || encrypt_stream(stdout, &recipient))
        .await
        .map_err(|_| RecoveryError::Dump)??;
    let status = tokio::task::spawn_blocking(move || dump.0.wait())
        .await
        .map_err(|_| RecoveryError::Dump)?
        .map_err(|_| RecoveryError::Dump)?;
    if !status.success() {
        return Err(RecoveryError::Dump);
    }
    Ok(encrypted)
}

/// D-3155 (b): refuses a `pg_dump` whose major is not the server's.
async fn check_server_major(pool: &PgPool, tool: &PgTool) -> Result<(), RecoveryError> {
    let server = sqlx::query_scalar!(
        r#"SELECT current_setting('server_version_num')::int / 10000 AS "major!""#
    )
    .fetch_one(pool)
    .await
    .map_err(|_| RecoveryError::Database)?;
    let server_major = u32::try_from(server).map_err(|_| RecoveryError::Database)?;
    if server_major == tool.major {
        return Ok(());
    }
    Err(RecoveryError::VersionMismatch {
        tool: "pg_dump",
        tool_major: tool.major,
        server_major,
    })
}

/// One stored export object (PRIV-029).
struct Stored {
    name: String,
    bytes: i64,
    sha256: Vec<u8>,
}

/// OPS-013, PRIV-018, PRIV-029: one dump, encrypted and stored as `name`.
async fn export_once<S: StorageAdapter>(
    pool: &PgPool,
    (store, secret): (&S, &SecretString),
    export: &Result<ExportSettings, RecoveryError>,
    name: String,
) -> Result<Stored, RecoveryError> {
    let export = export.as_ref().map_err(Clone::clone)?.clone();
    let tool = tokio::task::spawn_blocking(|| pg_tool("pg_dump"))
        .await
        .map_err(|_| RecoveryError::Dump)??;
    check_server_major(pool, &tool).await?;
    let bytes = dump_encrypted(&tool, export).await?;
    let sha256 = Sha256::digest(&bytes).to_vec();
    let size = i64::try_from(bytes.len()).map_err(|_| RecoveryError::Storage)?;
    let key = IdempotencyKey::derive(secret, EXPORT_KIND, &[&name]);
    store
        .put_object(&key, &name, bytes)
        .await
        .map_err(|_| RecoveryError::Storage)?;
    Ok(Stored {
        name,
        bytes: size,
        sha256,
    })
}

/// OPS-013, PRIV-028: one `recovery.backup_check` row for `result` and its audit entry, in one
/// transaction; returns the row's ID.
async fn record(
    pool: &PgPool,
    (started, finished): (Timestamp, Timestamp),
    result: &Result<Stored, RecoveryError>,
) -> Result<Uuid, RecoveryError> {
    let db = |_| RecoveryError::Database;
    let (stored, failure) = match result {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e.code())),
    };
    let mut tx = pool.begin().await.map_err(db)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO recovery.backup_check (kind, ok, object_name, object_bytes, object_sha256, \
         failure, started_at, finished_at) VALUES ('export', $1, $2, $3, $4, $5, $6, $7) RETURNING id",
        failure.is_none(),
        stored.map(|s| s.name.as_str()),
        stored.map(|s| s.bytes),
        stored.map(|s| s.sha256.as_slice()),
        failure,
        started.to_sqlx(),
        finished.to_sqlx(),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: stored.map_or(BACKUP_EXPORT_FAILED_ACTION, |_| BACKUP_EXPORTED_ACTION),
        subject_kind: "recovery.backup_check",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| RecoveryError::Database)?;
    tx.commit().await.map_err(db)?;
    Ok(id)
}

/// The first export hour strictly after `now` (OPS-013).
fn next_export_at(now: Timestamp) -> Timestamp {
    let today = now.as_second().div_euclid(DAY_SECS) * DAY_SECS + EXPORT_HOUR_UTC * 3_600;
    let next = if today > now.as_second() {
        today
    } else {
        today + DAY_SECS
    };
    Timestamp::from_second(next).unwrap_or(Timestamp::MAX)
}

/// OPS-013: queues the next nightly [`EXPORT_KIND`] after `now`, once per night (its dedupe key),
/// so a restart or a retry never doubles it.
///
/// # Errors
///
/// [`RecoveryError::Database`].
pub async fn schedule_export(db: &mut PgConnection, now: Timestamp) -> Result<(), RecoveryError> {
    let run_at = next_export_at(now);
    let mut task = NewTask::new(EXPORT_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{EXPORT_KIND}:{}", run_at.as_second() / DAY_SECS));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| RecoveryError::Database)
}

/// OPS-013, PRIV-018, PRIV-029: one nightly run: queue the next, export within `deadline` (past
/// it the dump is killed and the run is a `timeout` failure), record the result.
async fn run_export<S: StorageAdapter>(
    pool: &PgPool,
    parts: (&S, &SecretString, &dyn Clock),
    export: &Result<ExportSettings, RecoveryError>,
    (task, deadline): (i64, Duration),
) -> Result<(), HandlerError> {
    let (store, secret, clock) = parts;
    let started = clock.now();
    let mut conn = pool.acquire().await.map_err(|_| HandlerError("database"))?;
    schedule_export(&mut conn, started)
        .await
        .map_err(|e| HandlerError(e.code()))?;
    drop(conn);
    let name = format!(
        "{EXPORT_PREFIX}/{}/{task}.age",
        started.strftime("%Y-%m-%d")
    );
    let result = tokio::time::timeout(deadline, export_once(pool, (store, secret), export, name))
        .await
        .unwrap_or(Err(RecoveryError::Timeout));
    let finished = clock.now().max(started);
    record(pool, (started, finished), &result)
        .await
        .map_err(|e| HandlerError(e.code()))?;
    result.map(|_| ()).map_err(|e| HandlerError(e.code()))
}

/// OPS-013, PRIV-024, D-3883, D-4293: registers [`EXPORT_KIND`] (always) over `store`, deriving
/// each upload's idempotency key from `secret`, timed by `clock`.
///
/// # Errors
///
/// In test and live, [`RecoveryError::Settings`] when `ASKHAVA_EXPORT_RECIPIENT` or
/// `ASKHAVA_BACKUP_DATABASE_URL` is missing or invalid, which stops the worker's start-up; in dev
/// and CI the handler is registered and each run fails closed instead.
pub fn register_export<S>(
    registry: &mut Registry,
    settings: &Settings,
    (store, secret, clock): (Arc<S>, SecretString, Arc<dyn Clock>),
) -> Result<(), RecoveryError>
where
    S: StorageAdapter + Send + Sync + 'static,
{
    let export = ExportSettings::from_settings(settings);
    if let Err(e) = &export
        && matches!(
            settings.environment(),
            Environment::Test | Environment::Live
        )
    {
        return Err(e.clone());
    }
    let export = Arc::new(export);
    registry.register(EXPORT_KIND, move |pool, task| {
        let (store, secret) = (Arc::clone(&store), secret.clone());
        let (clock, export) = (Arc::clone(&clock), Arc::clone(&export));
        let run = (task.id, EXPORT_DEADLINE);
        async move { run_export(&pool, (&*store, &secret, &*clock), &export, run).await }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    //! TD-PRIV-09 (export half), TD-PRIV-14, TD-OPS-19 (export half) and the backup-role dump
    //! (PRIV-018, PRIV-024, PRIV-029, OPS-013, OPS-051; D-4293, D-4707, D-4708). Fake data only.

    use std::str::FromStr;

    use askhava_adapters::api::LocalStorage;
    use askhava_core::api::{SettingsError, SystemClock};
    use askhava_ops::api::latest_audit;

    use super::*;
    use crate::models::parse_recipient;

    fn pairs(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        [
            ("ASKHAVA_APP_ENV", "dev"),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/x"),
        ]
        .iter()
        .chain(extra)
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("t109-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes an executable script through a child shell, so this process never holds a write
    /// descriptor a concurrent fork could inherit (ETXTBSY on exec; D-4864).
    fn write_script(path: &Path, text: &str) {
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", r#"printf %s "$2" > "$1" && chmod 755 "$1""#, "sh"])
            .arg(path)
            .arg(text)
            .status()
            .unwrap();
        assert!(status.success(), "writing {} failed", path.display());
    }

    /// A fake binary printing `version` to `--version`.
    fn fake_tool(dir: &Path, name: &str, version: &str) {
        let text = format!("#!/bin/sh\necho '{name} (PostgreSQL) {version}'\n");
        write_script(&dir.join(name), &text);
    }

    /// A thread body spawning `/bin/true` in a loop until `stop` is set.
    fn fork_until(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> impl FnOnce() {
        move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::process::Command::new("/bin/true").status().unwrap();
            }
        }
    }

    #[test]
    fn scripts_written_while_other_threads_fork_always_run() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let forkers: Vec<_> = (0..4)
            .map(|_| std::thread::spawn(fork_until(std::sync::Arc::clone(&stop))))
            .collect();
        let path = scratch("race").join("script");
        let ran = (0..200).all(|round| {
            write_script(&path, &format!("#!/bin/sh\nexit 0\n# {round}\n"));
            std::process::Command::new(&path)
                .status()
                .is_ok_and(|s| s.success())
        });
        stop.store(true, Ordering::Relaxed);
        for forker in forkers {
            forker.join().unwrap();
        }
        assert!(ran);
    }

    #[test]
    fn the_path_helper_takes_only_postgresql_16_and_names_what_it_searched() {
        let (newer, older) = (scratch("pg18"), scratch("pg16"));
        fake_tool(&newer, "pg_dump", "18.1 (Ubuntu 18.1-1)");
        fake_tool(&older, "pg_dump", "16.15 (Ubuntu 16.15-1.pgdg24.04+2)");
        let both = std::env::join_paths([&newer, &older]).unwrap();
        let found = pg_tool_on("pg_dump", &both).unwrap();
        assert_eq!((found.path, found.major), (older.join("pg_dump"), 16));
        let only_newer = newer.clone().into_os_string();
        let err = pg_tool_on("pg_dump", &only_newer).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("pg_dump") && message.contains(newer.to_str().unwrap()));
        assert_eq!(err.code(), "tool_missing");
        assert_eq!(parse_major("psql (PostgreSQL) 16.2"), Some(16));
        assert_eq!(parse_major("no version"), None);
    }

    #[test]
    fn td_priv_09_the_recipient_is_x25519_only_and_no_private_key_is_a_setting() {
        let public = age::x25519::Identity::generate().to_public().to_string();
        let ok = Settings::from_pairs(pairs(&[("ASKHAVA_EXPORT_RECIPIENT", &public)])).unwrap();
        assert_eq!(ok.export_recipient(), Ok(public.as_str()));
        assert!(parse_recipient(&public).is_ok());
        let ssh = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeFakeFakeFakeFakeFakeFakeFakeFakeFake t@example.test";
        for bad in [ssh, "correct horse battery staple", "AGE-SECRET-KEY-1FAKE"] {
            let refused = Settings::from_pairs(pairs(&[("ASKHAVA_EXPORT_RECIPIENT", bad)]));
            assert_eq!(
                refused.unwrap_err(),
                SettingsError::Invalid("ASKHAVA_EXPORT_RECIPIENT")
            );
            assert!(parse_recipient(bad).is_err());
        }
        for key in [
            "ASKHAVA_EXPORT_IDENTITY",
            "ASKHAVA_EXPORT_PASSPHRASE",
            "ASKHAVA_EXPORT_PRIVATE_KEY",
            "ASKHAVA_EXPORT_KEY",
            "ASKHAVA_BACKUP_IDENTITY",
            "ASKHAVA_AGE_IDENTITY",
        ] {
            let refused = Settings::from_pairs(pairs(&[(key, "x")])).unwrap_err();
            assert_eq!(
                refused,
                SettingsError::Unknown(key.to_owned()),
                "{key} must not be a setting"
            );
        }
        let missing = Settings::from_pairs(pairs(&[])).unwrap();
        let err = ExportSettings::from_settings(&missing).unwrap_err();
        assert_eq!(err, RecoveryError::Settings("ASKHAVA_EXPORT_RECIPIENT"));
    }

    #[test]
    fn td_priv_14_an_export_is_unreadable_without_the_offline_key() {
        let (own, other) = (
            age::x25519::Identity::generate(),
            age::x25519::Identity::generate(),
        );
        let plain = b"PGDMP fake dump of a fake row: Pat Example".repeat(64);
        let sealed = encrypt_stream(&plain[..], &own.to_public()).unwrap();
        assert!(sealed.starts_with(b"age-encryption.org/v1"));
        assert!(!sealed.windows(11).any(|w| w == b"Pat Example"));
        assert_eq!(age::decrypt(&own, &sealed).unwrap(), plain);
        assert!(age::decrypt(&other, &sealed).is_err());
    }

    #[test]
    fn the_password_leaves_the_url_and_the_export_runs_nightly() {
        let url = SecretString::from("postgres://askhava_backup:f%40ke@localhost:5432/db");
        let (bare, password) = split_password(&url).unwrap();
        assert_eq!(
            (bare.as_str(), password.as_deref()),
            ("postgres://askhava_backup@localhost:5432/db", Some("f@ke"))
        );
        assert!(split_password(&SecretString::from("https://x/y")).is_err());
        assert_eq!(percent_decode("%zz"), None);
        let at = |t: &str| Timestamp::from_str(t).unwrap();
        assert_eq!(
            next_export_at(at("2026-11-19T09:59:59Z")),
            at("2026-11-19T10:00:00Z")
        );
        assert_eq!(
            next_export_at(at("2026-11-19T10:00:00Z")),
            at("2026-11-20T10:00:00Z")
        );
    }

    /// The superuser URL with `user`, `password` and `database` swapped in.
    fn url_for(database: &str, login: Option<(&str, &str)>) -> SecretString {
        let base = std::env::var("DATABASE_URL").expect("DATABASE_URL for #[sqlx::test]");
        let mut url = reqwest::Url::parse(&base).unwrap();
        url.set_path(database);
        if let Some((user, password)) = login {
            url.set_username(user).unwrap();
            url.set_password(Some(password)).unwrap();
        }
        SecretString::from(String::from(url))
    }

    /// OPS-051, D-4293: runs `scripts/bootstrap_roles.sql` as the superuser, `askhava_backup` with
    /// a fake password, and returns that URL.
    async fn bootstrap_backup_role(pool: &PgPool) -> SecretString {
        let db = current_db(pool).await;
        let fake: String = sqlx::query_scalar!(r#"SELECT gen_random_uuid()::text AS "p!""#)
            .fetch_one(pool)
            .await
            .unwrap();
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/bootstrap_roles.sql");
        let (admin, password) = split_password(&url_for(&db, None)).unwrap();
        let psql = pg_tool("psql").unwrap();
        let mut cmd = Command::new(psql.path);
        cmd.args(["-q", "-v", "ON_ERROR_STOP=1", "-f"])
            .arg(script)
            .arg(admin);
        cmd.env_remove("ASKHAVA_APP_PASSWORD")
            .env_remove("ASKHAVA_MIGRATOR_PASSWORD");
        cmd.env("ASKHAVA_BACKUP_PASSWORD", &fake)
            .env("PGPASSWORD", password.unwrap_or_default());
        assert!(
            cmd.status().unwrap().success(),
            "bootstrap_roles.sql failed"
        );
        url_for(&db, Some(("askhava_backup", &fake)))
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_ops_19_the_backup_role_dumps_what_the_app_role_cannot(pool: PgPool) {
        let backup_url = bootstrap_backup_role(&pool).await;
        let role = sqlx::query!(r#"SELECT rolinherit AS "inherit!", rolconnlimit AS "limit!", rolconfig::text AS "config!" FROM pg_roles WHERE rolname = 'askhava_backup'"#)
            .fetch_one(&pool).await.unwrap();
        assert!(
            !role.inherit && role.limit == 2,
            "NOINHERIT, CONNECTION LIMIT 2"
        );
        for setting in [
            "default_transaction_read_only=on",
            "statement_timeout=4h",
            "idle_in_transaction_session_timeout=60s",
        ] {
            assert!(role.config.contains(setting), "{setting}");
        }
        let members: Vec<String> = sqlx::query_scalar!(
            r#"SELECT m.rolname || '>' || g.rolname AS "m!" FROM pg_auth_members a JOIN pg_roles m ON m.oid = a.member JOIN pg_roles g ON g.oid = a.roleid WHERE g.rolname IN ('pg_read_all_data', 'askhava_backup')"#
        ).fetch_all(&pool).await.unwrap();
        assert_eq!(members, ["askhava_backup>pg_read_all_data"]);
        let owned: i64 = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM pg_class c JOIN pg_roles r ON r.oid = c.relowner WHERE r.rolname = 'askhava_backup'"#).fetch_one(&pool).await.unwrap();
        assert_eq!(owned, 0);
        let tool = pg_tool("pg_dump").unwrap();
        check_server_major(&pool, &tool).await.unwrap();
        let mut as_app = dump_command(&tool, &url_for(&current_db(&pool).await, None)).unwrap();
        let app_dump = as_app
            .arg("--role=askhava_app")
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(
            !app_dump.success(),
            "a dump as askhava_app fails on the sequences"
        );
        export_and_read_back(&pool, backup_url).await;
    }

    async fn current_db(pool: &PgPool) -> String {
        sqlx::query_scalar!(r#"SELECT current_database() AS "d!""#)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// TD-PRIV-09, TD-OPS-19: one run stores an `age` file only the test's identity opens, and
    /// records it as a check row and the audit entry the health page reads.
    async fn export_and_read_back(pool: &PgPool, backup_url: SecretString) {
        let (own, other) = (
            age::x25519::Identity::generate(),
            age::x25519::Identity::generate(),
        );
        let dir = scratch("store");
        let store = LocalStorage::new(&format!("file://{}", dir.display())).unwrap();
        let export = Ok(ExportSettings {
            recipient: own.to_public(),
            backup_url,
        });
        let secret = SecretString::from("fake-id-secret-for-tests-only-000000");
        run_export(
            pool,
            (&store, &secret, &SystemClock),
            &export,
            (7, EXPORT_DEADLINE),
        )
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let entry = latest_audit(&mut conn, BACKUP_EXPORTED_ACTION)
            .await
            .unwrap()
            .unwrap();
        let row = sqlx::query!(r#"SELECT ok, object_name AS "name!", object_bytes AS "bytes!", object_sha256 AS "sha!" FROM recovery.backup_check WHERE id = $1"#, entry.subject_id)
            .fetch_one(&mut *conn).await.unwrap();
        let sealed = store.read_object(&row.name).await.unwrap();
        assert!(row.ok && row.name.starts_with("exports/") && row.name.ends_with("/7.age"));
        assert_eq!(
            (row.bytes, row.sha),
            (
                i64::try_from(sealed.len()).unwrap(),
                Sha256::digest(&sealed).to_vec()
            )
        );
        assert!(age::decrypt(&own, &sealed).unwrap().starts_with(b"PGDMP"));
        assert!(age::decrypt(&other, &sealed).is_err());
        a_run_past_its_deadline_is_recorded(pool, (&store, &secret), &export).await;
        a_failed_run_is_recorded_and_the_next_night_queued_once(pool, &store, &secret).await;
    }

    /// OPS-013: a run without its settings fails closed, is recorded as a failure, and every run
    /// queued the same next night once.
    async fn a_failed_run_is_recorded_and_the_next_night_queued_once(
        pool: &PgPool,
        store: &LocalStorage,
        secret: &SecretString,
    ) {
        let missing = Err(RecoveryError::Settings("ASKHAVA_EXPORT_RECIPIENT"));
        let failed = run_export(
            pool,
            (store, secret, &SystemClock),
            &missing,
            (8, EXPORT_DEADLINE),
        )
        .await;
        assert_eq!(failed, Err(HandlerError("settings")));
        let mut conn = pool.acquire().await.unwrap();
        let entry = latest_audit(&mut conn, BACKUP_EXPORT_FAILED_ACTION)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.subject_kind, "recovery.backup_check");
        let next = NewTask::new(EXPORT_KIND, json!({}), next_export_at(Timestamp::now()));
        let mut again = next.clone();
        again.dedupe_key = Some(format!(
            "{EXPORT_KIND}:{}",
            next.run_at.as_second() / DAY_SECS
        ));
        let prefix = format!("{EXPORT_KIND}:");
        assert!(
            askhava_core::api::unfinished(&mut conn, EXPORT_KIND, &prefix)
                .await
                .unwrap()
        );
        let doubled = enqueue(&mut conn, &again).await.unwrap();
        assert_eq!(doubled, None, "both runs queued the same next night once");
    }

    /// OPS-013: a run past its deadline is dropped and still recorded as a `timeout` failure.
    async fn a_run_past_its_deadline_is_recorded(
        pool: &PgPool,
        (store, secret): (&LocalStorage, &SecretString),
        export: &Result<ExportSettings, RecoveryError>,
    ) {
        let run = (9, Duration::ZERO);
        let late = run_export(pool, (store, secret, &SystemClock), export, run).await;
        assert_eq!(late, Err(HandlerError("timeout")));
        let mut conn = pool.acquire().await.unwrap();
        let entry = latest_audit(&mut conn, BACKUP_EXPORT_FAILED_ACTION)
            .await
            .unwrap()
            .unwrap();
        let failure = sqlx::query_scalar!(
            "SELECT failure FROM recovery.backup_check WHERE id = $1",
            entry.subject_id
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(failure.as_deref(), Some("timeout"));
    }

    /// A fake `pg_dump` running `body`, and export settings to a fresh fake recipient.
    fn fake_dump(dir: &Path, body: &str) -> (PgTool, ExportSettings, age::x25519::Identity) {
        let path = dir.join("pg_dump");
        write_script(&path, &format!("#!/bin/sh\n{body}\n"));
        let own = age::x25519::Identity::generate();
        let export = ExportSettings {
            recipient: own.to_public(),
            backup_url: SecretString::from("postgres://fake_user:fake-pw@localhost/fake_db"),
        };
        (PgTool { path, major: 16 }, export, own)
    }

    #[tokio::test]
    async fn the_dump_sees_only_path_and_its_password() {
        let (tool, export, own) = fake_dump(&scratch("env"), "env");
        let sealed = dump_encrypted(&tool, export).await.unwrap();
        let plain = String::from_utf8(age::decrypt(&own, &sealed).unwrap()).unwrap();
        assert!(plain.lines().any(|l| l == "PGPASSWORD=fake-pw"));
        let names: Vec<&str> = plain.lines().filter_map(|l| l.split('=').next()).collect();
        let shell_own = ["PATH", "PGPASSWORD", "PWD", "OLDPWD", "SHLVL", "_"];
        for name in names {
            assert!(shell_own.contains(&name), "{name} reached pg_dump");
        }
    }

    #[tokio::test]
    async fn a_dump_dropped_at_its_deadline_is_killed() {
        let dir = scratch("slow");
        let pid_file = dir.join("pid");
        let body = format!("echo $$ > {}\nexec sleep 30", pid_file.display());
        let (tool, export, _) = fake_dump(&dir, &body);
        let started = std::time::Instant::now();
        let late = tokio::time::timeout(Duration::from_millis(500), dump_encrypted(&tool, export));
        assert!(late.await.is_err(), "the slow dump passed its deadline");
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        assert!(
            !Path::new("/proc").join(pid.trim()).exists(),
            "pg_dump {pid} still runs"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

//! The restore drill: an operator-supplied plain dump restored into a fresh database, then the
//! replay, recorded in `recovery.restore_drill` (OPS-013, OPS-018, OPS-019; D-3889, D-4932).
//!
//! The app host cannot read an export back (PRIV-029) and holds no private key (PRIV-024), so
//! nothing here reads storage or decrypts: the operator fetches the object, runs `age -d` with
//! the key kept off the host, and hands [`Drill`] the plain `pg_dump -Fc` file and the fresh
//! database's URL (its roles already made by `scripts/bootstrap_roles.sql`, OPS-051).
//! [`run_drill`] runs `pg_restore --single-transaction --exit-on-error` (found by `pg_tool`, the
//! password only in `PGPASSWORD`, SEC-052), so a failed restore leaves no object and nothing is
//! recorded, then the replay on the restored database. While a vendor cannot be reached the whole
//! replay is waited for and run again, never guessed (OPS-018): it is safe to repeat, and a
//! drill that ran out of rounds is recorded as `failed` with one High `RESTORE_FAILED_ALERT`.
//! What the replay listed for Casey in `recovery.replay_item` raises one High
//! [`REPLAY_LISTED_ALERT`] about the run (OPS-019); an unmatched charge's refund is the nightly
//! reconcile's (FEE-032, D-3889). The drill row and its alert commit together.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use askhava_adapters::api::SmsSentLog;
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::backup::pg_tool;
use crate::models::RecoveryError;
use crate::replay::{ChargeSource, ReplayReport, ReplaySources, Retry, replay};
use crate::restore::{RESTORE_FAILED_ALERT, RestoreError};

/// The High alert raised once when a replay listed anything for Casey (OPS-019); its subject is
/// the `recovery.replay_run` row whose `recovery.replay_item` rows name each one.
pub const REPLAY_LISTED_ALERT: &str = "recovery.replay_items_listed";

/// Why a drill failed: its restore step or its replay. No variant carries a value.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DrillError {
    /// The settings were refused or `pg_restore` failed; the fresh database holds nothing.
    #[error("restore failed: {0}")]
    Restore(RestoreError),
    /// The replay failed, or a vendor stayed unreachable through every round (OPS-018).
    #[error("replay failed: {0}")]
    Replay(RecoveryError),
}

/// One drill's input (D-4932): the plain dump, the export's object name, the fresh database and
/// how many whole replays are tried while a vendor is unreachable. The URL is never logged.
#[derive(Debug, Clone)]
pub struct Drill {
    dump: PathBuf,
    object: String,
    database_url: SecretString,
    rounds: Retry,
}

impl Drill {
    /// OPS-013: a drill of `dump`, the decrypted export stored as `object`, into `database_url`.
    ///
    /// # Errors
    ///
    /// [`RestoreError::Settings`] when `object` is not a storage object name (1 to 200 of
    /// `a-z0-9_./-`, the form `recovery.restore_drill` keeps).
    pub fn new(
        dump: PathBuf,
        object: &str,
        database_url: SecretString,
        rounds: Retry,
    ) -> Result<Self, DrillError> {
        let named = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_./-".contains(c);
        if object.is_empty() || object.len() > 200 || !object.chars().all(named) {
            return Err(DrillError::Restore(RestoreError::Settings));
        }
        Ok(Self {
            dump,
            object: object.to_owned(),
            database_url,
            rounds,
        })
    }
}

/// What a finished drill did: its `recovery.restore_drill` row, the seconds the restore took and
/// the replay's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillReport {
    /// The `recovery.restore_drill` row's ID.
    pub drill: Uuid,
    /// The whole seconds `pg_restore` took.
    pub restore_seconds: i32,
    /// The replay that passed.
    pub replay: ReplayReport,
}

/// A URL part with its `%XX` escapes decoded; `None` when one is malformed.
fn decoded(text: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).ok()
}

/// OPS-013, OPS-051, SEC-052: `pg_restore --single-transaction --exit-on-error` of the dump into
/// the fresh database, with an environment cleared to `PATH`, a connect timeout and the password.
fn restore(drill: &Drill) -> Result<(), RestoreError> {
    let tool = pg_tool("pg_restore").map_err(|_| RestoreError::Tool)?;
    let mut url = reqwest::Url::parse(drill.database_url.expose_secret())
        .map_err(|_| RestoreError::Settings)?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(RestoreError::Settings);
    }
    let password = url.password().map(decoded);
    url.set_password(None)
        .map_err(|()| RestoreError::Settings)?;
    let mut cmd = Command::new(tool.path);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("PGCONNECT_TIMEOUT", "10")
        .args(["--no-password", "--single-transaction", "--exit-on-error"])
        .args(["--dbname", url.as_str()])
        .arg(&drill.dump)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(password) = password {
        cmd.env("PGPASSWORD", password.ok_or(RestoreError::Settings)?);
    }
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(RestoreError::Restore),
        Err(_) => Err(RestoreError::Tool),
    }
}

/// OPS-013, OPS-018, OPS-019, D-4932: restores the drill's dump into its fresh database, replays
/// from the vendors' records as of `at`, and records the drill there.
///
/// # Errors
///
/// [`DrillError::Restore`] when the restore failed (nothing restored, nothing recorded);
/// [`DrillError::Replay`] when the replay failed, recorded as a `failed` drill.
pub async fn run_drill<S, C>(
    drill: &Drill,
    from: &ReplaySources<'_, S, C>,
    at: Timestamp,
) -> Result<DrillReport, DrillError>
where
    S: SmsSentLog + Sync,
    C: ChargeSource + Sync,
{
    let (started, job) = (Instant::now(), drill.clone());
    tokio::task::spawn_blocking(move || restore(&job))
        .await
        .map_err(|_| DrillError::Restore(RestoreError::Tool))?
        .map_err(DrillError::Restore)?;
    let seconds = i32::try_from(started.elapsed().as_secs()).unwrap_or(i32::MAX);
    let mut conn = PgConnection::connect(drill.database_url.expose_secret())
        .await
        .map_err(|_| DrillError::Replay(RecoveryError::Database))?;
    replay_recorded(&mut conn, (drill, seconds), from, at).await
}

/// OPS-018, OPS-019: the replay on the restored database, run again after `rounds.wait` while a
/// vendor is unreachable (at most `rounds.tries` replays, never a guess), then the drill's row.
pub(crate) async fn replay_recorded<S, C>(
    conn: &mut PgConnection,
    (drill, seconds): (&Drill, i32),
    from: &ReplaySources<'_, S, C>,
    at: Timestamp,
) -> Result<DrillReport, DrillError>
where
    S: SmsSentLog + Sync,
    C: ChargeSource + Sync,
{
    let mut round = 1;
    let replayed = loop {
        match replay(conn, from, at).await {
            Err(RecoveryError::Vendor) if round < drill.rounds.tries => {
                round += 1;
                tokio::time::sleep(drill.rounds.wait).await;
            }
            done => break done,
        }
    };
    let id = record(conn, (drill, seconds), replayed.as_ref().ok(), at)
        .await
        .map_err(DrillError::Replay)?;
    Ok(DrillReport {
        drill: id,
        restore_seconds: seconds,
        replay: replayed.map_err(DrillError::Replay)?,
    })
}

/// OPS-013, OPS-019: one `recovery.restore_drill` row and, in the same transaction, one High
/// alert: [`REPLAY_LISTED_ALERT`] about the run when it listed anything for Casey, or
/// `RESTORE_FAILED_ALERT` about the drill when no replay passed (`report` is `None`).
async fn record(
    conn: &mut PgConnection,
    (drill, seconds): (&Drill, i32),
    report: Option<&ReplayReport>,
    at: Timestamp,
) -> Result<Uuid, RecoveryError> {
    let db = |_| RecoveryError::Database;
    let mut tx = conn.begin().await.map_err(db)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO recovery.restore_drill \
         (ran_at, backup_object, replay_run_id, restore_seconds, outcome) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        at.to_sqlx(),
        drill.object,
        report.map(|r| r.run),
        seconds,
        report.map_or("failed", |_| "ok"),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let (kind, subject) = match report {
        None => (RESTORE_FAILED_ALERT, ("recovery.restore_drill", id)),
        Some(r) if !r.items.is_empty() => (REPLAY_LISTED_ALERT, ("recovery.replay_run", r.run)),
        Some(_) => return tx.commit().await.map(|()| id).map_err(db),
    };
    let alert = NewAlert {
        kind,
        severity: Severity::High,
        subject: Some(subject),
    };
    raise_alert(&mut tx, &alert)
        .await
        .map_err(|_| RecoveryError::Database)?;
    tx.commit().await.map(|()| id).map_err(db)
}

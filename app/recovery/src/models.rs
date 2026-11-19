//! The nightly export's settings and `recovery`'s error (OPS-013, OPS-051, PRIV-024, SEC-052;
//! D-4293, D-4707).
//!
//! [`ExportSettings`] holds the offline recipient's public key and the `askhava_backup` role's URL,
//! both parsed once from `Settings`. The recipient is X25519 only: an `ssh-` recipient or any
//! passphrase form is refused at load, and no identity, passphrase or private key is ever a
//! setting, so the host can encrypt an export but never decrypt one (PRIV-024, TD-PRIV-09).

use std::str::FromStr;

use askhava_core::api::Settings;
use secrecy::SecretString;
use thiserror::Error;

/// Why an export failed. No variant carries a URL, a password or a database message; a tool
/// error names only the binary, the `PATH` it searched and the major version it found (D-4708).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RecoveryError {
    /// The recipient key or the backup role's URL is missing or invalid (fails closed).
    #[error("export settings missing or invalid: {0}")]
    Settings(&'static str),
    /// No PostgreSQL 16 binary of that name on the `PATH` searched.
    #[error("no PostgreSQL {major} {tool} on PATH {searched}")]
    ToolMissing {
        /// The binary, such as `pg_dump`.
        tool: &'static str,
        /// The major version required.
        major: u32,
        /// The `PATH` value searched.
        searched: String,
    },
    /// The tool's major version is not the database server's.
    #[error("{tool} is PostgreSQL {tool_major}, the server is {server_major}")]
    VersionMismatch {
        /// The binary.
        tool: &'static str,
        /// The binary's major version.
        tool_major: u32,
        /// The server's major version.
        server_major: u32,
    },
    /// `pg_dump` did not run or exited non-zero.
    #[error("pg_dump failed")]
    Dump,
    /// The `age` encryption failed.
    #[error("encryption failed")]
    Encrypt,
    /// The storage adapter refused the object.
    #[error("storage write failed")]
    Storage,
    /// A database read or write failed.
    #[error("database error")]
    Database,
    /// The export passed its deadline; its `pg_dump` was killed (OPS-013).
    #[error("export timed out")]
    Timeout,
}

impl RecoveryError {
    /// The code name stored as the check's `failure` (PRIV-028: never a value).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Settings(_) => "settings",
            Self::ToolMissing { .. } => "tool_missing",
            Self::VersionMismatch { .. } => "version_mismatch",
            Self::Dump => "dump",
            Self::Encrypt => "encrypt",
            Self::Storage => "storage",
            Self::Database => "database",
            Self::Timeout => "timeout",
        }
    }
}

/// The nightly export's recipient and source database (PRIV-024, OPS-051).
#[derive(Clone)]
pub struct ExportSettings {
    pub(crate) recipient: age::x25519::Recipient,
    pub(crate) backup_url: SecretString,
}

impl std::fmt::Debug for ExportSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExportSettings([redacted])")
    }
}

impl ExportSettings {
    /// PRIV-024, D-4293, D-4707: the export settings from `ASKHAVA_EXPORT_RECIPIENT` (an X25519
    /// `age1` recipient only) and `ASKHAVA_BACKUP_DATABASE_URL`.
    ///
    /// # Errors
    ///
    /// [`RecoveryError::Settings`] naming the missing or invalid key.
    pub fn from_settings(settings: &Settings) -> Result<Self, RecoveryError> {
        let recipient = settings
            .export_recipient()
            .map_err(|_| RecoveryError::Settings("ASKHAVA_EXPORT_RECIPIENT"))?;
        let backup_url = settings
            .backup_database_url()
            .map_err(|_| RecoveryError::Settings("ASKHAVA_BACKUP_DATABASE_URL"))?
            .clone();
        Ok(Self {
            recipient: parse_recipient(recipient)?,
            backup_url,
        })
    }
}

/// PRIV-024: an X25519 recipient, or [`RecoveryError::Settings`] for anything else.
pub(crate) fn parse_recipient(raw: &str) -> Result<age::x25519::Recipient, RecoveryError> {
    age::x25519::Recipient::from_str(raw.trim())
        .map_err(|_| RecoveryError::Settings("ASKHAVA_EXPORT_RECIPIENT"))
}

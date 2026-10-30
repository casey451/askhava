//! The storage adapter: write-once objects behind one trait (PRIV-023,
//! PRIV-029, OPS-010, SEC-018).
//!
//! Holds the off-database stream, the nightly encrypted export and the
//! app-encrypted photos; callers pass bytes already encrypted. Objects are
//! never overwritten: a second put of the same name is a no-op, so a retried
//! write under the same key cannot change what was stored. The Phase 1 client
//! is a local directory (`file://` base URL); the Backblaze B2 client with a
//! write-only, prefix-scoped key is Phase 2's (D-492, D-726).

use std::future::Future;
use std::path::PathBuf;

use crate::base::{AdapterError, IdempotencyKey, is_safe_name};

/// Writing and reading stored objects (SEC-018 on every write).
pub trait StorageAdapter {
    /// Stores `bytes` as object `name` (`a/b/c`, each part a safe name) once.
    fn put_object(
        &self,
        idempotency: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;

    /// Reads object `name` back (restore and export).
    fn read_object(&self, name: &str)
    -> impl Future<Output = Result<Vec<u8>, AdapterError>> + Send;
}

/// The local-directory store of the build (Phase 1 stand-in).
#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    /// A store under the `file://` directory `base_url`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] unless `base_url` is a `file://` directory URL.
    pub fn new(base_url: &str) -> Result<Self, AdapterError> {
        let url = reqwest::Url::parse(base_url).map_err(|_| AdapterError::BadConfig)?;
        let root = (url.scheme() == "file")
            .then(|| url.to_file_path().ok())
            .flatten();
        Ok(Self {
            root: root.ok_or(AdapterError::BadConfig)?,
        })
    }

    fn path(&self, name: &str) -> Result<PathBuf, AdapterError> {
        let mut path = self.root.clone();
        for part in name.split('/') {
            if !is_safe_name(part) {
                return Err(AdapterError::InvalidId);
            }
            path.push(part);
        }
        Ok(path)
    }
}

/// Writes `bytes` to a temporary file named by the key, then renames it into
/// place unless the object already exists (write-once, SEC-018).
fn write_once(path: &std::path::Path, key: &str, bytes: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{key}.part"));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

impl StorageAdapter for LocalStorage {
    async fn put_object(
        &self,
        idempotency: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        let path = self.path(name)?;
        let key = idempotency.as_str().to_owned();
        let written = tokio::task::spawn_blocking(move || write_once(&path, &key, &bytes)).await;
        written
            .map_err(|_| AdapterError::OutcomeUnknown)?
            .map_err(|_| AdapterError::NotSent)
    }

    async fn read_object(&self, name: &str) -> Result<Vec<u8>, AdapterError> {
        let path = self.path(name)?;
        let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
        let missing = AdapterError::Vendor {
            status: 404,
            code: None,
        };
        read.map_err(|_| AdapterError::OutcomeUnknown)?
            .map_err(|_| missing)
    }
}

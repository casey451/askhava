//! The storage adapter: write-once objects behind one trait (PRIV-023,
//! PRIV-029, OPS-010, SEC-018).
//!
//! Holds the off-database stream, the nightly encrypted export and the
//! app-encrypted photos; callers pass bytes already encrypted. Objects are
//! never overwritten: a second put of the same name and bytes is a no-op (a
//! retry), and one with other bytes is refused with `ObjectExists`, so no
//! write can change what was stored. The Phase 1 client
//! is a local directory (`file://` base URL); the Backblaze B2 client with a
//! write-only, prefix-scoped key is Phase 2's (D-492, D-726). Each write
//! attempt publishes its own fully written temporary file by a hard link, so
//! a racing retry can never empty or change a published object (D-1841);
//! [`StorageDelete`] adds delete and list for the photo purge (PRIV-009).

use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};

use rand::TryRng;
use rand::rngs::SysRng;

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

/// Deleting and listing stored objects, for the photo purge and its orphan
/// sweep (PRIV-009, D-1803, D-1841). A separate trait so no existing
/// [`StorageAdapter`] fake changes (D-1753). In Phase 2 only the `photos/`
/// prefix's own key may read, list and delete; the audit-stream and export
/// prefixes keep PRIV-029's write-only, prefix-scoped key with versioning and
/// object lock, so nothing there is ever deleted through this trait.
pub trait StorageDelete: StorageAdapter {
    /// Deletes object `name`; an object already absent is success, so a
    /// repeated purge is a no-op (PRIV-009, SEC-018).
    fn delete_object(
        &self,
        idempotency: &IdempotencyKey,
        name: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;

    /// The names (`a/b/c`) of every object whose name starts with `prefix`,
    /// sorted; temporary files of writes in progress are never listed.
    fn list_objects(
        &self,
        prefix: &str,
    ) -> impl Future<Output = Result<Vec<String>, AdapterError>> + Send;
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

/// One step of a write attempt, reported to the observer of
/// [`write_once_observed`] so the competing-writer tests can stop each attempt
/// between steps (TD-SEC-39).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The attempt's own temporary file was created exclusively.
    Created,
    /// The bytes were written, synced and the file closed.
    Written,
    /// The hard link into place was tried (made, or refused by an existing object).
    Linked,
    /// The attempt removed its own temporary file.
    Removed,
}

/// Write-once publication of `bytes` at `path` (SEC-018, D-728, D-1841).
fn write_once(path: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    write_once_observed(path, bytes, &mut |_, _| {})
}

/// Each attempt creates its own temporary file `.<name>.<random>.part`
/// exclusively beside the object (dot-led, so never an object name), writes
/// it fully, syncs and closes it, then hard-links it into place: the link
/// never replaces an existing object, so a published object is always
/// complete and no racing writer can empty or change it. Bytes are compared
/// only with an object at `path`, which exists only once its publication is
/// complete; a same-bytes object is a retry and succeeds, other bytes are
/// [`AdapterError::ObjectExists`]. The attempt removes only its own temporary
/// file, and after a new link it syncs the parent directory so the name
/// survives a crash; a failure after the link is `OutcomeUnknown`, since the
/// object is published and a retry compares (SEC-018, D-728, D-1841). `observe` sees each [`Step`]
/// with the temporary path.
pub(crate) fn write_once_observed(
    path: &Path,
    bytes: &[u8],
    observe: &mut dyn FnMut(Step, &Path),
) -> Result<(), AdapterError> {
    if path.exists() {
        return same_bytes(path, bytes);
    }
    let parent = path.parent().ok_or(AdapterError::InvalidId)?;
    std::fs::create_dir_all(parent).map_err(|_| AdapterError::NotSent)?;
    let (temporary, file) = create_temporary(path)?;
    observe(Step::Created, &temporary);
    let written = fill(file, bytes);
    observe(Step::Written, &temporary);
    let linked = written.and_then(|()| std::fs::hard_link(&temporary, path));
    observe(Step::Linked, &temporary);
    let removed = std::fs::remove_file(&temporary);
    observe(Step::Removed, &temporary);
    match linked {
        Ok(()) => removed
            .map_err(|_| AdapterError::OutcomeUnknown)
            .and_then(|()| sync_directory(parent)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => removed
            .map_err(|_| AdapterError::NotSent)
            .and_then(|()| same_bytes(path, bytes)),
        Err(_) => Err(AdapterError::NotSent),
    }
}

/// How many random names one attempt tries before it gives up.
const TEMPORARY_TRIES: usize = 4;

/// Creates this attempt's own temporary file beside `path` with `create_new`
/// and a 128-bit random suffix from the OS source, so no other attempt can
/// open, truncate or remove it (D-1841).
fn create_temporary(path: &Path) -> Result<(PathBuf, File), AdapterError> {
    let name = path.file_name().ok_or(AdapterError::InvalidId)?;
    for _ in 0..TEMPORARY_TRIES {
        let mut suffix = [0u8; 16];
        SysRng
            .try_fill_bytes(&mut suffix)
            .map_err(|_| AdapterError::NotSent)?;
        let hex: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(name);
        temporary_name.push(format!(".{hex}.part"));
        let temporary = path.with_file_name(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(AdapterError::NotSent),
        }
    }
    Err(AdapterError::NotSent)
}

/// Writes all of `bytes`, syncs data and metadata, and closes the file.
fn fill(mut file: File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

/// Syncs `directory` so a new link in it survives a crash (D-1841).
fn sync_directory(directory: &Path) -> Result<(), AdapterError> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|_| AdapterError::OutcomeUnknown)
}

/// `Ok` when the stored object at `path` holds exactly `bytes`.
fn same_bytes(path: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    let stored = std::fs::read(path).map_err(|_| AdapterError::NotSent)?;
    if stored == bytes {
        Ok(())
    } else {
        Err(AdapterError::ObjectExists)
    }
}

impl StorageAdapter for LocalStorage {
    /// The name is the write-once key here, so the derived key adds nothing (SEC-018).
    async fn put_object(
        &self,
        _idempotency: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        let path = self.path(name)?;
        let written = tokio::task::spawn_blocking(move || write_once(&path, &bytes)).await;
        written.map_err(|_| AdapterError::OutcomeUnknown)?
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

impl StorageDelete for LocalStorage {
    /// The name is the key here; deleting twice deletes once (SEC-018, PRIV-009).
    async fn delete_object(
        &self,
        _idempotency: &IdempotencyKey,
        name: &str,
    ) -> Result<(), AdapterError> {
        let path = self.path(name)?;
        let removed = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
        match removed.map_err(|_| AdapterError::OutcomeUnknown)? {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(AdapterError::NotSent)
            }
            _ => Ok(()),
        }
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>, AdapterError> {
        let (start, folders) = self.prefix_start(prefix)?;
        let prefix = prefix.to_owned();
        let listed = tokio::task::spawn_blocking(move || {
            let mut names = Vec::new();
            list_under(&start, &folders, &mut names).map(|()| names)
        })
        .await;
        let mut names = listed
            .map_err(|_| AdapterError::OutcomeUnknown)?
            .map_err(|_| AdapterError::NotSent)?;
        names.retain(|name| name.starts_with(&prefix));
        names.sort();
        Ok(names)
    }
}

impl LocalStorage {
    /// The directory a listing of `prefix` starts in and its name so far:
    /// every complete part of `prefix` (before its last `/`) must be a safe
    /// name, and the last, partial part only safe characters (SEC-032).
    fn prefix_start(&self, prefix: &str) -> Result<(PathBuf, String), AdapterError> {
        let (folders, partial) = prefix.rsplit_once('/').unwrap_or(("", prefix));
        if !partial.is_empty() && !is_safe_name(partial) {
            return Err(AdapterError::InvalidId);
        }
        if folders.is_empty() {
            return Ok((self.root.clone(), String::new()));
        }
        Ok((self.path(folders)?, format!("{folders}/")))
    }
}

/// Adds to `names` every object file under `directory`, named `<folders><part>/…`;
/// an absent directory holds nothing, and dot-led entries (temporary files)
/// and symbolic links are skipped.
fn list_under(directory: &Path, folders: &str, names: &mut Vec<String>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    for entry in entries {
        let entry = entry?;
        let Some(part) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_safe_name(&part) {
            continue;
        }
        let kind = entry.file_type()?;
        let name = format!("{folders}{part}");
        if kind.is_dir() {
            list_under(&entry.path(), &format!("{name}/"), names)?;
        } else if kind.is_file() {
            names.push(name);
        }
    }
    Ok(())
}

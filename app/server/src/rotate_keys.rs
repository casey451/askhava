//! `server rotate-keys` and `server retire-index-version <version>`: the vault key rotation run to
//! the end, for the runbook's rotation step (PRIV-024, D-4418, D-4748, D-5412, D-5420).
//!
//! `rotate-keys` builds the vault from `VAULT_KEY` and `VAULT_INDEX_KEY` as the server does and the
//! photo keys from `VAULT_KEY`, `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET` as
//! `homeowner_portal::photo_parts` does, then runs `privacy`'s contact, photo and business batches
//! and `messaging`'s opt-out re-keying batch, each until it returns 0, printing one count line per
//! batch: rows moved and rows skipped (the backlog left once the batch returns 0). Without the
//! photo settings it refuses, before any batch, while a photo sits under an older `VAULT_KEY`
//! version (dropping that version would strand it). Each loop is capped at its starting backlog
//! divided by the batch size plus 2 passes (D-753). A batch error stops the run with the error's
//! fixed text; each batch commits per row, so running it again resumes. A skipped row ends the
//! run non-zero. `retire-index-version` removes an index version's opt-out rows once every one is
//! listed under the active version. Neither runs `check_index_keys` first (this is how a refused
//! start is cleared), and neither prints a key, digest, phone, address or contact ID (PRIV-016).

use std::fmt;
use std::io::{self, Write};

use askhava_adapters::api::LocalStorage;
use askhava_core::api::{IdSecret, Settings, check_base_url};
use askhava_messaging::api::{rekey_suppression_batch, retire_index_version, suppression_backlog};
use askhava_privacy::api::{
    PhotoSeal, Vault, rotate_batch, rotate_business_batch, rotate_photos, rotation_backlog,
};
use secrecy::SecretString;
use sqlx::PgConnection;

/// The rows each batch call may move: small enough that one pass is a short run of transactions.
pub const BATCH_SIZE: i64 = 100;

/// The photo storage setting, as `homeowner_portal` reads it.
const STORAGE_BASE_URL: &str = "ASKHAVA_STORAGE_BASE_URL";

/// The ID secret setting the photo moves derive their storage idempotency keys from.
const ID_SECRET: &str = "ASKHAVA_ID_SECRET";

/// Why a batch loop stopped before its batch returned 0 (PRIV-024, D-753).
#[derive(Debug, PartialEq, Eq)]
pub enum BatchStop {
    /// The batch still moved rows after the loop's pass cap.
    Capped,
    /// The batch failed; the batch error's fixed text, which names no value.
    Failed(String),
}

impl fmt::Display for BatchStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capped => f.write_str(
                "the batch still moved rows after its pass cap; nothing is lost, \
                 find why it never reaches 0 before running it again (D-753)",
            ),
            Self::Failed(error) => write!(f, "the batch stopped: {error}; run it again to resume"),
        }
    }
}

/// Runs `step` (one batch of at most `size` rows, answering how many it moved) until it answers 0
/// and returns the rows moved; capped at `backlog / size + 2` passes, `backlog` being the rows the
/// batch had to move when the loop began (PRIV-024, D-753, D-5412).
///
/// # Errors
///
/// [`BatchStop::Capped`] past the cap, [`BatchStop::Failed`] on the first batch error.
pub async fn run_batches<F>(
    db: &mut PgConnection,
    size: i64,
    backlog: u64,
    mut step: F,
) -> Result<u64, BatchStop>
where
    F: AsyncFnMut(&mut PgConnection, i64) -> Result<u64, String>,
{
    let cap = backlog / size.max(1).cast_unsigned() + 2;
    let mut moved = 0;
    for _ in 0..cap {
        let rows = step(&mut *db, size).await.map_err(BatchStop::Failed)?;
        if rows == 0 {
            return Ok(moved);
        }
        moved += rows;
    }
    Err(BatchStop::Capped)
}

/// Runs `server rotate-keys` or `server retire-index-version <version>` (`args` from the command
/// word on) over `db`, an app-role connection outside any transaction, with the settings in
/// `pairs`, writing its count lines to `out` (PRIV-024, D-5412).
///
/// # Errors
///
/// A wrong argument, no vault keys, a photo stranded without the photo settings, a capped or
/// failed batch, a skipped row or a refused retirement; each names counts, versions and setting
/// names only.
pub async fn run(
    settings: &Settings,
    pairs: Vec<(String, String)>,
    db: &mut PgConnection,
    args: &[String],
    out: &mut dyn Write,
) -> io::Result<()> {
    let vault = || {
        crate::vault_from(settings, pairs.clone())?
            .ok_or_else(|| io::Error::other("this command needs VAULT_KEY and VAULT_INDEX_KEY"))
    };
    match args {
        [word] if word == "rotate-keys" => {
            let photos = photo_keys(settings, &pairs)?;
            rotate_keys(db, &vault()?, photos.as_ref(), out).await
        }
        [word, version] if word == "retire-index-version" => {
            let version: u16 = version.trim().parse().map_err(|_| usage())?;
            retire(db, &vault()?, version, out).await
        }
        _ => Err(usage()),
    }
}

fn usage() -> io::Error {
    io::Error::other("usage: server rotate-keys | server retire-index-version <version>")
}

/// The photo seal, storage and ID secret, built from the settings as `photo_parts` builds them.
struct PhotoKeys {
    seal: PhotoSeal,
    storage: LocalStorage,
    id_secret: SecretString,
}

/// The photo keys from `VAULT_KEY`, `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET` (a blank
/// value counts as unset); `None` when any is absent (PRIV-023, PRIV-024).
fn photo_keys(settings: &Settings, pairs: &[(String, String)]) -> io::Result<Option<PhotoKeys>> {
    let value = |name: &str| {
        pairs
            .iter()
            .find(|(key, value)| key == name && !value.trim().is_empty())
            .map(|(_, value)| value.trim().to_owned())
    };
    let (Some(key), Some(base), Some(secret)) = (
        value("VAULT_KEY"),
        value(STORAGE_BASE_URL),
        value(ID_SECRET),
    ) else {
        return Ok(None);
    };
    check_base_url(settings.environment(), STORAGE_BASE_URL, &base).map_err(io::Error::other)?;
    let storage = LocalStorage::new(&base)
        .map_err(|_| io::Error::other("invalid ASKHAVA_STORAGE_BASE_URL"))?;
    let seal = PhotoSeal::new(&SecretString::from(key)).map_err(io::Error::other)?;
    let id_secret = SecretString::from(secret);
    IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    Ok(Some(PhotoKeys {
        seal,
        storage,
        id_secret,
    }))
}

/// Every batch to 0 in turn, one count line each; refuses first while a photo would be stranded
/// (PRIV-023, PRIV-024, D-5412).
async fn rotate_keys(
    db: &mut PgConnection,
    vault: &Vault,
    photos: Option<&PhotoKeys>,
    out: &mut dyn Write,
) -> io::Result<()> {
    let backlog = rotation_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    if photos.is_none() && backlog.photos > 0 {
        return Err(io::Error::other(format!(
            "{} photos are under an older VAULT_KEY version and cannot be moved without \
             ASKHAVA_STORAGE_BASE_URL and ASKHAVA_ID_SECRET; set both and run again before \
             dropping that version",
            backlog.photos
        )));
    }
    let mut skipped = rotate_vault(db, vault, backlog.vault, out).await?;
    match photos {
        Some(keys) => skipped += rotate_photo_rows(db, vault, keys, backlog.photos, out).await?,
        None => writeln!(out, "photos: no photo needs moving")?,
    }
    skipped += rotate_business(db, vault, backlog.business, out).await?;
    skipped += rekey_optouts(db, vault, out).await?;
    if skipped > 0 {
        return Err(io::Error::other(format!(
            "{skipped} rows were skipped and stay under an older key version; \
             keep that version until they are moved"
        )));
    }
    Ok(())
}

/// Writes `label`'s count line and returns the skipped count.
fn report(out: &mut dyn Write, label: &str, moved: u64, skipped: u64) -> io::Result<u64> {
    writeln!(out, "{label}: {moved} moved, {skipped} skipped")?;
    Ok(skipped)
}

/// The error for `label`'s stopped loop.
fn stopped(label: &'static str) -> impl Fn(BatchStop) -> io::Error {
    move |stop| io::Error::other(format!("{label}: {stop}"))
}

/// Contacts and stated names to 0 (`rotate_batch`); skipped is the vault backlog left.
async fn rotate_vault(
    db: &mut PgConnection,
    vault: &Vault,
    backlog: u64,
    out: &mut dyn Write,
) -> io::Result<u64> {
    let label = "contacts and stated names";
    let batch = async |db: &mut PgConnection, n| {
        rotate_batch(db, vault, n).await.map_err(|e| e.to_string())
    };
    let moved = run_batches(db, BATCH_SIZE, backlog, batch)
        .await
        .map_err(stopped(label))?;
    let left = rotation_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    report(out, label, moved, left.vault)
}

/// Photos to 0 (`rotate_photos`); skipped is the photo backlog left.
async fn rotate_photo_rows(
    db: &mut PgConnection,
    vault: &Vault,
    keys: &PhotoKeys,
    backlog: u64,
    out: &mut dyn Write,
) -> io::Result<u64> {
    let label = "photos";
    let parts = (&keys.seal, &keys.storage, &keys.id_secret);
    let batch = async |db: &mut PgConnection, n| {
        rotate_photos(db, parts, n).await.map_err(|e| e.to_string())
    };
    let moved = run_batches(db, BATCH_SIZE, backlog, batch)
        .await
        .map_err(stopped(label))?;
    let left = rotation_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    report(out, label, moved, left.photos)
}

/// Staff phones and mailboxes to 0 (`rotate_business_batch`); skipped is the business backlog left.
async fn rotate_business(
    db: &mut PgConnection,
    vault: &Vault,
    backlog: u64,
    out: &mut dyn Write,
) -> io::Result<u64> {
    let label = "staff phones and mailboxes";
    let batch = async |db: &mut PgConnection, n| {
        rotate_business_batch(db, vault, n)
            .await
            .map_err(|e| e.to_string())
    };
    let moved = run_batches(db, BATCH_SIZE, backlog, batch)
        .await
        .map_err(stopped(label))?;
    let left = rotation_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    report(out, label, moved, left.business)
}

/// Opt-out listings to 0 (`rekey_suppression_batch`); skipped is the suppression backlog left: the
/// listings no contact's phone matches (D-4748).
async fn rekey_optouts(
    db: &mut PgConnection,
    vault: &Vault,
    out: &mut dyn Write,
) -> io::Result<u64> {
    let label = "opt-outs";
    let backlog = suppression_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    let batch = async |db: &mut PgConnection, n: i64| {
        let limit = usize::try_from(n).map_err(|e| e.to_string())?;
        let moved = rekey_suppression_batch(db, vault, limit)
            .await
            .map_err(|e| e.to_string())?;
        u64::try_from(moved).map_err(|e| e.to_string())
    };
    let moved = run_batches(db, BATCH_SIZE, backlog, batch)
        .await
        .map_err(stopped(label))?;
    let left = suppression_backlog(db, vault)
        .await
        .map_err(io::Error::other)?;
    report(out, label, moved, left)
}

/// Retires index key `version` and prints the opt-out rows removed (PRIV-024, D-4748).
async fn retire(
    db: &mut PgConnection,
    vault: &Vault,
    version: u16,
    out: &mut dyn Write,
) -> io::Result<()> {
    let removed = retire_index_version(db, vault, version)
        .await
        .map_err(io::Error::other)?;
    writeln!(
        out,
        "VAULT_INDEX_KEY version {version} retired: {removed} opt-out rows removed; \
         drop it from VAULT_INDEX_KEY now"
    )
}

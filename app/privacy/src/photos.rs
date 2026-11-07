//! Homeowner photos: checked by their bytes, decoded under limits, re-encoded with no metadata,
//! sealed under the vault key and served only through short-lived links (PRIV-013, PRIV-014,
//! PRIV-023, PRIV-025, PRIV-028; D-383, D-454, D-480).
//!
//! The one way in is [`clean`]: [`sniff`] reads the type and size from the bytes (never the name)
//! and refuses HEIC before any decoder runs; the decode runs behind a two-permit semaphore in a
//! timed `spawn_blocking` under [`MAX_SIDE`], [`MAX_PIXELS`] and [`MAX_ALLOC`]; only the primary
//! frame is kept, the orientation is applied, and the pixels are written out as a fresh JPEG, so
//! EXIF, GPS, XMP, IPTC, maker notes and anything appended after the image are gone. Only a
//! [`CleanPhoto`] is screened, stored, sent to AI or put on a card. [`store_photo`] seals it under
//! `VAULT_KEY` with associated data naming its job and photo ID before the `storage` adapter writes
//! it; [`issue_link`] gives a token for the path `/photos/<token>` that [`open_link`] honours for
//! [`LINK_TTL_SECS`]. The photo-ask wording and the texting provider's media delete are T212's.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::time::Duration;

use aes_gcm::aead::{Aead, Generate, KeyInit, Nonce, Payload};
use aes_gcm::{Aes256Gcm, Key};
use askhava_adapters::api::{IdempotencyKey, StorageAdapter};
use askhava_core::api::{IdSecret, LinkKind, LinkSalt, LinkToken, link_token};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::types::uuid::Builder;
use sqlx::{Connection, PgConnection};
use tokio::sync::Semaphore;

use crate::models::PhotoError;

/// The largest upload accepted, in bytes; the photo route's body limit is this too (PRIV-013).
pub const MAX_PHOTO_BYTES: usize = 15 * 1024 * 1024;
/// The longest side a photo may have, in pixels (PRIV-025).
pub const MAX_SIDE: u32 = 8_000;
/// The most pixels a photo may have: 40 megapixels (PRIV-025).
pub const MAX_PIXELS: u64 = 40_000_000;
/// The most memory one decode may allocate: 256 MiB (PRIV-025).
pub const MAX_ALLOC: u64 = 256 * 1024 * 1024;
/// How many decodes run at once; with [`MAX_ALLOC`] it caps decode memory at 512 MiB (PRIV-025).
pub const DECODE_PERMITS: usize = 2;
/// How long a decode and re-encode may take before the upload is refused (PRIV-025).
pub const DECODE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a photo link works after it is issued, in seconds (PRIV-014, D-454).
pub const LINK_TTL_SECS: i64 = 600;
/// The messaging template a HEIC upload is answered with: send a JPEG or PNG (PRIV-025).
pub const HEIC_REPLY_TEMPLATE: &str = "photo_heic_reply";

const JPEG_QUALITY: u8 = 85;
const VERSION_LEN: usize = 2;
const NONCE_LEN: usize = 12;
const HEIC_BRANDS: [&[u8; 4]; 8] = [
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1",
];

static DECODES: Semaphore = Semaphore::const_new(DECODE_PERMITS);

/// A photo type the app decodes, as read from the file's first bytes (PRIV-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoKind {
    /// Starts `FF D8 FF`.
    Jpeg,
    /// Starts with the 8-byte PNG signature.
    Png,
    /// A RIFF file of form `WEBP`; an animated one keeps its first frame only.
    WebP,
}

impl PhotoKind {
    fn format(self) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::WebP => ImageFormat::WebP,
        }
    }
}

/// A photo after [`clean`]: a fresh JPEG with no metadata and nothing after its end marker. The
/// field is private and only [`clean`] or [`open_link`] makes one; `Debug` shows the length only
/// (PRIV-014, PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct CleanPhoto(Vec<u8>);

impl std::fmt::Debug for CleanPhoto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CleanPhoto({} bytes)", self.0.len())
    }
}

impl CleanPhoto {
    /// The JPEG bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The kind of an upload from its bytes, after the size check: a HEIC file is
/// [`PhotoError::Heic`] and is never decoded; anything else unknown is
/// [`PhotoError::NotAPhoto`], whatever its name says (PRIV-013, PRIV-025).
///
/// # Errors
///
/// [`PhotoError::TooLarge`], [`PhotoError::Heic`] or [`PhotoError::NotAPhoto`].
pub fn sniff(bytes: &[u8]) -> Result<PhotoKind, PhotoError> {
    if bytes.len() > MAX_PHOTO_BYTES {
        return Err(PhotoError::TooLarge);
    }
    let head = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or_default();
    if head(0..3) == [0xFF, 0xD8, 0xFF] {
        Ok(PhotoKind::Jpeg)
    } else if head(0..8) == b"\x89PNG\r\n\x1a\n" {
        Ok(PhotoKind::Png)
    } else if head(0..4) == b"RIFF" && head(8..12) == b"WEBP" {
        Ok(PhotoKind::WebP)
    } else if head(4..8) == b"ftyp" && HEIC_BRANDS.iter().any(|b| head(8..12) == b.as_slice()) {
        Err(PhotoError::Heic)
    } else {
        Err(PhotoError::NotAPhoto)
    }
}

/// Checks, decodes and re-encodes an upload off the runtime, at most [`DECODE_PERMITS`] at once
/// and within [`DECODE_TIMEOUT`]; the result is the only form any later step sees (PRIV-013,
/// PRIV-014, PRIV-025). The uploaded bytes are dropped here and never stored.
///
/// # Errors
///
/// [`sniff`]'s errors, then [`PhotoError::TooManyPixels`], [`PhotoError::Undecodable`] or
/// [`PhotoError::Busy`] when the decode ran out of time.
pub async fn clean(upload: Vec<u8>) -> Result<CleanPhoto, PhotoError> {
    clean_within(upload, DECODE_TIMEOUT).await
}

/// [`clean`] with the decode time limit `within`, so a test can run out the clock.
pub(crate) async fn clean_within(
    upload: Vec<u8>,
    within: Duration,
) -> Result<CleanPhoto, PhotoError> {
    let kind = sniff(&upload)?;
    let permit = DECODES.acquire().await.map_err(|_| PhotoError::Busy)?;
    let work = tokio::task::spawn_blocking(move || {
        let _held = permit;
        reencode(&upload, kind)
    });
    let joined = tokio::time::timeout(within, work)
        .await
        .map_err(|_| PhotoError::Busy)?;
    joined.map_err(|_| PhotoError::Undecodable)?.map(CleanPhoto)
}

/// The decode limits of PRIV-025.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

/// Decodes the primary frame of `bytes` as `kind` under [`limits`], refusing more than
/// [`MAX_PIXELS`] from the header before any pixel is allocated, applies the orientation, and
/// writes the pixels as a new JPEG with no metadata (PRIV-014, PRIV-025).
pub(crate) fn reencode(bytes: &[u8], kind: PhotoKind) -> Result<Vec<u8>, PhotoError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), kind.format());
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(too_big_or_bad)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(PhotoError::TooManyPixels);
    }
    let orientation = decoder.orientation().map_err(|_| PhotoError::Undecodable)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(too_big_or_bad)?;
    image.apply_orientation(orientation);
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&image.to_rgb8())
        .map_err(|_| PhotoError::Undecodable)?;
    Ok(out)
}

/// A decoder error as a photo error: a limit hit is [`PhotoError::TooManyPixels`].
fn too_big_or_bad(error: image::ImageError) -> PhotoError {
    match error {
        image::ImageError::Limits(_) => PhotoError::TooManyPixels,
        _ => PhotoError::Undecodable,
    }
}

/// The photo cipher: every version of `VAULT_KEY`, the highest active (PRIV-023). It has no
/// `Debug`, so no key can reach a log (PRIV-016).
pub struct PhotoSeal {
    keys: BTreeMap<u16, Aes256Gcm>,
}

impl PhotoSeal {
    /// Builds the cipher from `VAULT_KEY` (`<version>:<base64 of 32 bytes>`, comma-separated), the
    /// same value [`crate::api::Vault::new`] reads (PRIV-023, D-454).
    ///
    /// # Errors
    ///
    /// [`PhotoError::Keys`], never naming the value.
    pub fn new(vault_key: &SecretString) -> Result<Self, PhotoError> {
        let mut keys = BTreeMap::new();
        for entry in vault_key.expose_secret().split(',').map(str::trim) {
            let (version, b64) = entry.split_once(':').ok_or(PhotoError::Keys)?;
            let version: u16 = version.parse().map_err(|_| PhotoError::Keys)?;
            let bytes =
                zeroize::Zeroizing::new(STANDARD.decode(b64).map_err(|_| PhotoError::Keys)?);
            let key = Key::<Aes256Gcm>::try_from(bytes.as_slice()).map_err(|_| PhotoError::Keys)?;
            if version == 0 || keys.insert(version, Aes256Gcm::new(&key)).is_some() {
                return Err(PhotoError::Keys);
            }
        }
        Ok(Self { keys })
    }

    /// Seals `photo` of `job` under the active key: key version, random nonce, ciphertext.
    pub(crate) fn seal(&self, job: Uuid, photo: Uuid, plain: &[u8]) -> Result<Vec<u8>, PhotoError> {
        let (version, cipher) = self.keys.last_key_value().ok_or(PhotoError::Sealed)?;
        let nonce = Nonce::<Aes256Gcm>::generate();
        let aad = associated_data(job, photo, *version);
        let sealed = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plain,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PhotoError::Sealed)?;
        Ok([&version.to_be_bytes()[..], &nonce[..], &sealed].concat())
    }

    /// Opens a photo sealed for `photo` of `job`; any other job or photo ID fails (PRIV-023).
    pub(crate) fn open(
        &self,
        job: Uuid,
        photo: Uuid,
        sealed: &[u8],
    ) -> Result<Vec<u8>, PhotoError> {
        let (head, body) = sealed
            .split_at_checked(VERSION_LEN + NONCE_LEN)
            .ok_or(PhotoError::Sealed)?;
        let version = u16::from_be_bytes([head[0], head[1]]);
        let cipher = self.keys.get(&version).ok_or(PhotoError::Sealed)?;
        let nonce =
            Nonce::<Aes256Gcm>::try_from(&head[VERSION_LEN..]).map_err(|_| PhotoError::Sealed)?;
        let aad = associated_data(job, photo, version);
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: body,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PhotoError::Sealed)
    }
}

/// The associated data binding a sealed photo to its job, photo ID and key version (PRIV-023).
fn associated_data(job: Uuid, photo: Uuid, version: u16) -> String {
    format!("privacy.photo|{job}|{photo}|v{version}")
}

/// Seals `photo` for `job` under a new random photo ID, writes it through `storage` as
/// `photos/<id>`, then records the job, key version and storage name in `privacy.photo` with an
/// audit row, and returns the ID. The bucket never holds a readable photo (PRIV-014, PRIV-023).
///
/// # Errors
///
/// [`PhotoError::Sealed`], [`PhotoError::Storage`] or [`PhotoError::Database`].
pub async fn store_photo<S: StorageAdapter>(
    db: &mut PgConnection,
    (seal, storage, id_secret): (&PhotoSeal, &S, &SecretString),
    job: Uuid,
    photo: &CleanPhoto,
    at: Timestamp,
) -> Result<Uuid, PhotoError> {
    let random = LinkSalt::generate().map_err(|_| PhotoError::Sealed)?;
    let id = Builder::from_random_bytes(*random.as_bytes()).into_uuid();
    let sealed = seal.seal(job, id, photo.as_bytes())?;
    let version = u16::from_be_bytes([sealed[0], sealed[1]]);
    let name = format!("photos/{id}");
    let key = IdempotencyKey::derive(id_secret, "photo.put", &[&name]);
    storage
        .put_object(&key, &name, sealed)
        .await
        .map_err(|_| PhotoError::Storage)?;
    let mut tx = db.begin().await.map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at) \
         VALUES ($1, $2, $3, $4, $5)",
        id,
        job,
        i32::from(version),
        name,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    record(&mut tx, id, "privacy.photo_stored").await?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

/// Issues a link token for `photo`, live for [`LINK_TTL_SECS`] from `at`; only its SHA256 is
/// stored. Put it in the path with [`photo_path`], never a query string (PRIV-014, PRIV-028).
///
/// # Errors
///
/// [`PhotoError::Sealed`] without a random source, [`PhotoError::NotFound`] for an unknown photo,
/// or [`PhotoError::Database`].
pub async fn issue_link(
    db: &mut PgConnection,
    secret: &IdSecret,
    photo: Uuid,
    at: Timestamp,
) -> Result<LinkToken, PhotoError> {
    let salt = LinkSalt::generate().map_err(|_| PhotoError::Sealed)?;
    let token = link_token(secret, LinkKind::Photo, photo, &salt);
    let expires = at
        .checked_add(SignedDuration::from_secs(LINK_TTL_SECS))
        .map_err(|_| PhotoError::Database)?;
    let hash = token.hash();
    let mut tx = db.begin().await.map_err(db_err)?;
    let added = sqlx::query!(
        "INSERT INTO privacy.photo_link (token_hash, photo_id, issued_at, expires_at) \
         SELECT $1, id, $3, $4 FROM privacy.photo WHERE id = $2",
        hash.as_slice(),
        photo,
        at.to_sqlx() as _,
        expires.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if added.rows_affected() == 0 {
        return Err(PhotoError::NotFound);
    }
    record(&mut tx, photo, "privacy.photo_link_issued").await?;
    tx.commit().await.map_err(db_err)?;
    Ok(token)
}

/// The app's photo route path for `token`: the token in the path, never the query (PRIV-028).
#[must_use]
pub fn photo_path(token: &LinkToken) -> String {
    format!("/photos/{}", token.as_str())
}

/// The photo a live link opens at `now`, read from storage and opened, with an audit row; an
/// unknown or expired token is [`PhotoError::LinkDead`], which the route answers with 404
/// (PRIV-014, PRIV-023, D-454).
///
/// # Errors
///
/// [`PhotoError::LinkDead`], [`PhotoError::Storage`], [`PhotoError::Sealed`] or
/// [`PhotoError::Database`].
pub async fn open_link<S: StorageAdapter>(
    db: &mut PgConnection,
    (seal, storage): (&PhotoSeal, &S),
    token: &LinkToken,
    now: Timestamp,
) -> Result<CleanPhoto, PhotoError> {
    let hash = token.hash();
    let row = sqlx::query!(
        "SELECT p.id, p.job_id, p.storage_key FROM privacy.photo_link l \
         JOIN privacy.photo p ON p.id = l.photo_id \
         WHERE l.token_hash = $1 AND l.expires_at > $2",
        hash.as_slice(),
        now.to_sqlx() as _,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(db_err)?
    .ok_or(PhotoError::LinkDead)?;
    let sealed = storage
        .read_object(&row.storage_key)
        .await
        .map_err(|_| PhotoError::Storage)?;
    let plain = seal.open(row.job_id, row.id, &sealed)?;
    record(db, row.id, "privacy.photo_opened").await?;
    Ok(CleanPhoto(plain))
}

/// The audit row for `action` on `photo`, by the system; it names the photo ID only (PRIV-028).
async fn record(
    db: &mut PgConnection,
    photo: Uuid,
    action: &'static str,
) -> Result<(), PhotoError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "privacy.photo",
        subject_id: photo,
        change: Change::Action,
    };
    audit(db, &entry).await.map(|_| ()).map_err(db_err)
}

/// Any database or audit failure, carrying no message (PRIV-016).
fn db_err<E>(_: E) -> PhotoError {
    PhotoError::Database
}

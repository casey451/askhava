//! Photos into storage from a picture text or the upload page, and the concept image (INT-044,
//! INT-046, SEC-041, PRIV-014, PRIV-025; D-432, D-433, D-491, D-492).
//!
//! [`take_upload`] is the one way a homeowner photo is kept: [`clean`] checks the format and size,
//! strips metadata and re-encodes on Ask Hava's own server (no outside AI); a file that will not
//! decode, or whose shorter side is under the setting (default [`DEFAULT_MIN_SHORT_SIDE`]), is
//! [`Taken::ReAsk`]; a HEIC file is [`Taken::Heic`] (the PRIV-025 reply); only then is it sealed
//! and stored. [`take_picture_text`] first fetches the media through a [`PictureTextMedia`] whose
//! fetch is [`fetch_twilio_media`] (SEC-041: Twilio's hosts only, re-checked redirects, public
//! addresses, size cap, timeout), then takes it like an upload and deletes it at the texting
//! provider. [`make_concept`] asks a [`ConceptImageAdapter`] for one concept image from the written
//! [`ProjectFile`] only (its type holds scrubbed words, never a photo), while the "Concept images"
//! switch is on, for the first image and at most [`CONCEPT_CHANGES`] change rounds; the image is
//! cleaned and stored like a photo and carries [`CONCEPT_LABEL`].

use std::future::Future;
use std::io::Cursor;

use askhava_adapters::api::{
    AdapterError, FetchError, FetchPolicy, IdempotencyKey, StorageAdapter, fetch_untrusted,
};
use image::{ImageFormat, ImageReader};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PhotoError;
use crate::photos::{CleanPhoto, PhotoSeal, clean, store_photo};
use crate::scrub::ScrubbedText;

/// The shorter side, in pixels, under which a photo gets the one re-ask; a setting (INT-044).
pub const DEFAULT_MIN_SHORT_SIDE: u32 = 320;
/// The label on and beside every concept image (INT-046).
pub const CONCEPT_LABEL: &str = "AI-generated, not exact, may not be feasible";
/// The change rounds a concept image allows after the first one (INT-046).
pub const CONCEPT_CHANGES: u8 = 3;
/// The longest project file sent for a concept image, in characters (INT-045).
pub const MAX_PROJECT_FILE: usize = 600;

/// The keys a stored photo needs: the seal, the storage adapter and the ID secret for keys.
pub type PhotoKeys<'a, S> = (&'a PhotoSeal, &'a S, &'a SecretString);

/// What became of one photo (INT-044, PRIV-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Checked, stripped, re-encoded and stored under this photo ID.
    Stored(Uuid),
    /// It did not decode or its shorter side is under the setting: one re-ask.
    ReAsk,
    /// A HEIC file: the plain reply asking for a JPEG or PNG (PRIV-025).
    Heic,
    /// Over the size or pixel limits, or the fetch was refused (SEC-041): left off, no re-ask.
    Refused,
}

/// A picture text after [`take_picture_text`]: the photo and whether the provider's copy is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureText {
    /// What became of the photo.
    pub taken: Taken,
    /// The media was deleted at the texting provider; `false` means the caller retries the delete.
    pub media_deleted: bool,
}

/// A picture text's media at the texting provider (INT-044, SEC-041, SEC-018, D-491).
pub trait PictureTextMedia {
    /// The media bytes at `url`; the live and stand-in fetch is [`fetch_twilio_media`].
    fn fetch_media(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
    /// Deletes the media at `url` at the provider, under `idempotency` (SEC-018).
    fn delete_media(
        &self,
        idempotency: &IdempotencyKey,
        url: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;
}

/// SEC-041: the guarded fetch of a picture text's media from Twilio's hosts only.
///
/// # Errors
///
/// The [`FetchError`] naming the rule that stopped it.
pub async fn fetch_twilio_media(url: &str) -> Result<Vec<u8>, FetchError> {
    fetch_untrusted(&FetchPolicy::twilio_media(), url).await
}

/// The shorter side of a cleaned photo, read from its header only (INT-044).
#[must_use]
pub fn shorter_side(photo: &CleanPhoto) -> Option<u32> {
    ImageReader::with_format(Cursor::new(photo.as_bytes()), ImageFormat::Jpeg)
        .into_dimensions()
        .ok()
        .map(|(w, h)| w.min(h))
}

/// INT-044, PRIV-014, PRIV-025: checks an upload on the server and stores it for `job` when it
/// passes; `min_side` is the re-ask setting ([`DEFAULT_MIN_SHORT_SIDE`]).
///
/// # Errors
///
/// [`PhotoError::Busy`], [`PhotoError::Sealed`], [`PhotoError::Storage`] or
/// [`PhotoError::Database`].
pub async fn take_upload<S: StorageAdapter>(
    db: &mut PgConnection,
    keys: PhotoKeys<'_, S>,
    (job, upload): (Uuid, Vec<u8>),
    min_side: u32,
    at: Timestamp,
) -> Result<Taken, PhotoError> {
    let photo = match clean(upload).await {
        Ok(photo) => photo,
        Err(PhotoError::Undecodable | PhotoError::NotAPhoto) => return Ok(Taken::ReAsk),
        Err(PhotoError::Heic) => return Ok(Taken::Heic),
        Err(PhotoError::TooLarge | PhotoError::TooManyPixels) => return Ok(Taken::Refused),
        Err(other) => return Err(other),
    };
    if shorter_side(&photo).is_none_or(|side| side < min_side) {
        return Ok(Taken::ReAsk);
    }
    store_photo(db, keys, job, &photo, at)
        .await
        .map(Taken::Stored)
}

/// INT-044, SEC-041, D-491: fetches a picture text's media at `url`, takes it as [`take_upload`]
/// does, then deletes it at the provider, so no copy stays there once it is stored or refused. A
/// refused fetch keeps nothing and deletes nothing.
///
/// # Errors
///
/// [`take_upload`]'s errors; the media is then left for the caller's retry.
pub async fn take_picture_text<S: StorageAdapter, M: PictureTextMedia>(
    db: &mut PgConnection,
    keys: PhotoKeys<'_, S>,
    media: &M,
    (job, url): (Uuid, &str),
    (min_side, at): (u32, Timestamp),
) -> Result<PictureText, PhotoError> {
    let Ok(bytes) = media.fetch_media(url).await else {
        let taken = Taken::Refused;
        return Ok(PictureText {
            taken,
            media_deleted: false,
        });
    };
    let taken = take_upload(db, keys, (job, bytes), min_side, at).await?;
    let job_id = job.simple().to_string();
    let key = IdempotencyKey::derive(keys.2, "picture_text.media_delete", &[&job_id, url]);
    let media_deleted = media.delete_media(&key, url).await.is_ok();
    Ok(PictureText {
        taken,
        media_deleted,
    })
}

/// The written project file a concept image is made from: size, layout, style and finishes in
/// scrubbed words only (INT-045, AI-004). It holds no photo and no contact detail; `Debug` shows
/// the length only (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct ProjectFile(String);

impl std::fmt::Debug for ProjectFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProjectFile({} chars)", self.0.chars().count())
    }
}

impl ProjectFile {
    /// The project file from her scrubbed words, cut to [`MAX_PROJECT_FILE`] characters; `None`
    /// when nothing is left (INT-045).
    #[must_use]
    pub fn from_scrubbed(words: &ScrubbedText) -> Option<Self> {
        let text: String = words
            .as_str()
            .trim()
            .chars()
            .take(MAX_PROJECT_FILE)
            .collect();
        (!text.is_empty()).then_some(Self(text))
    }

    /// The words.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The image maker behind the "Concept images" switch: the stand-in until the account checkpoint
/// chooses a provider (INT-046, D-492).
pub trait ConceptImageAdapter {
    /// Makes one image from `request` (the project file and the label) under `idempotency`.
    fn make_image(
        &self,
        idempotency: &IdempotencyKey,
        request: &serde_json::Value,
    ) -> impl Future<Output = Result<Vec<u8>, AdapterError>> + Send;
}

/// What [`make_concept`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concept {
    /// The switch is off: no call was made.
    Off,
    /// The rounds are used up: no call was made.
    RoundsUsed,
    /// Made and stored under this photo ID, labeled [`CONCEPT_LABEL`].
    Made(Uuid),
    /// The call failed or the image did not pass the photo checks: no image this round.
    Failed,
}

/// INT-046, D-492: one concept image for `job` from `project` only, while `switch_on`, for
/// `changes` change rounds so far (0 for the first image, at most [`CONCEPT_CHANGES`]). The
/// request carries the project file and the label to draw on the image, never a photo.
///
/// # Errors
///
/// [`PhotoError::Sealed`], [`PhotoError::Storage`] or [`PhotoError::Database`].
pub async fn make_concept<S: StorageAdapter, A: ConceptImageAdapter>(
    db: &mut PgConnection,
    (keys, adapter): (PhotoKeys<'_, S>, &A),
    (job, changes, switch_on): (Uuid, u8, bool),
    project: &ProjectFile,
    at: Timestamp,
) -> Result<Concept, PhotoError> {
    if !switch_on {
        return Ok(Concept::Off);
    }
    if changes > CONCEPT_CHANGES {
        return Ok(Concept::RoundsUsed);
    }
    let request = json!({ "project_file": project.as_str(), "label": CONCEPT_LABEL });
    let ids = [job.simple().to_string(), changes.to_string()];
    let key = IdempotencyKey::derive(keys.2, "concept.image", &[&ids[0], &ids[1]]);
    let Ok(bytes) = adapter.make_image(&key, &request).await else {
        return Ok(Concept::Failed);
    };
    let Ok(image) = clean(bytes).await else {
        return Ok(Concept::Failed);
    };
    store_photo(db, keys, job, &image, at)
        .await
        .map(Concept::Made)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn side_of(width: u32, height: u32) -> Option<u32> {
        let mut png = Vec::new();
        image::RgbImage::new(width, height)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .expect("png");
        shorter_side(&clean(png).await.expect("clean"))
    }

    /// TD-INT-27, INT-044: the shorter side is read after the re-encode, so 319 and 320 differ.
    #[tokio::test]
    async fn td_int_27_the_shorter_side_is_read_from_the_clean_photo() {
        assert_eq!(side_of(400, 319).await, Some(319));
        assert_eq!(side_of(320, 500).await, Some(320));
        assert!(side_of(319, 400).await < Some(DEFAULT_MIN_SHORT_SIDE));
    }
}

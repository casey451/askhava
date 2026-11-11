//! The homeowner's photo upload page on her portal (INT-044, PRIV-015, A11Y-027, PRIV-025; D-432,
//! D-490, D-491).
//!
//! [`photo_upload_page`] renders `templates/photo_upload.html`: INT-044's "No photo is fine too"
//! words, the job type's photo asks, PRIV-015's line and one native file input with a visible
//! label (no drag-and-drop, so it works with JavaScript off). After an upload the caller takes
//! the file with `privacy::api::take_upload` (format and size checks, metadata stripped and
//! re-encoded on the server) and passes what happened to [`upload_screen`]: a stored photo shows
//! as it will appear on the card with "Send this photo" and "Take it off" (INT-044, CARD-013);
//! a file that did not decode or is too small, a HEIC file or one over the limits shows the form
//! again with an "Error: " title and a focused summary (A11Y-030). The route is T306's: a `GET`
//! and a multipart `POST` with `MAX_PHOTO_BYTES` as its body limit, `refuse_cross_site`, the CSRF
//! token and `no_store` (SEC-034).

use askama::Template;
use askhava_intake::api::{LEAVE_OUT_LINE, MAX_PHOTO_ASKS, NO_PHOTO_LINE};
use askhava_privacy::api::PhotoTaken;

/// The page's path under her portal; `{job}` is the request number (T306 serves it).
pub const PHOTO_UPLOAD_ROUTE: &str = "/h/requests/{job}/photos";
/// The error for a file that did not decode or is too small (INT-044).
pub const REASK_ERROR: &str = "We couldn't open that photo, or it's too small to see. Choose another photo, or skip it. No photo is fine too.";
/// The PRIV-025 error for a HEIC file.
pub const HEIC_ERROR: &str = "We can't open that kind of photo. Choose a JPEG or PNG photo.";
/// The error for a file over the byte limit (PRIV-025).
pub const TOO_BIG_ERROR: &str = "That photo is too big. Choose a photo under 15 MB.";
/// The error for a file over the pixel limits: 8,000 pixels a side, 40 megapixels (PRIV-025).
pub const TOO_MANY_PIXELS_ERROR: &str =
    "That photo has too many pixels. Choose a smaller photo, under 8,000 pixels on each side.";

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadScreen {
    /// The form, with the error of a refused upload.
    Ask(Option<&'static str>),
    /// One stored photo through its `/photos/<token>` link, before she confirms it.
    Preview(String),
}

/// INT-044, PRIV-025: the screen after an upload; `link` makes the preview of a stored photo
/// (from `privacy::api::issue_link` and `photo_path`).
#[must_use]
pub fn upload_screen(taken: PhotoTaken, link: impl FnOnce() -> String) -> UploadScreen {
    match taken {
        PhotoTaken::Stored(_) => UploadScreen::Preview(link()),
        PhotoTaken::ReAsk => UploadScreen::Ask(Some(REASK_ERROR)),
        PhotoTaken::Heic => UploadScreen::Ask(Some(HEIC_ERROR)),
        PhotoTaken::Refused => UploadScreen::Ask(Some(TOO_BIG_ERROR)),
        PhotoTaken::TooManyPixels => UploadScreen::Ask(Some(TOO_MANY_PIXELS_ERROR)),
    }
}

#[derive(Template)]
#[template(path = "photo_upload.html")]
struct UploadView<'a> {
    job_number: &'a str,
    asks: &'a [String],
    no_photo: &'static str,
    leave_out: &'static str,
    error: Option<&'static str>,
    preview: Option<&'a str>,
    csrf: &'a str,
}

/// INT-044, PRIV-015, A11Y-027: the upload page for request `job_number`, with the set's first
/// [`MAX_PHOTO_ASKS`] photo asks, in `screen`.
///
/// # Errors
/// The template's render error.
pub fn photo_upload_page(
    job_number: &str,
    asks: &[String],
    screen: &UploadScreen,
    csrf: &str,
) -> Result<String, askama::Error> {
    let (error, preview) = match screen {
        UploadScreen::Ask(error) => (*error, None),
        UploadScreen::Preview(link) => (None, Some(link.as_str())),
    };
    UploadView {
        job_number,
        asks: &asks[..asks.len().min(MAX_PHOTO_ASKS)],
        no_photo: NO_PHOTO_LINE,
        leave_out: LEAVE_OUT_LINE,
        error,
        preview,
        csrf,
    }
    .render()
}

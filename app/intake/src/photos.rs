//! The photo asks of a request (INT-044, PRIV-015, PRIV-025, CARD-013; D-432, D-491).
//!
//! After the trade questions the request gets one [`PHOTO_ASK_TEMPLATE`] text: "A photo or two
//! helps businesses quote. No photo is fine too.", the job type's photo asks (the set's `photos`,
//! at most [`MAX_PHOTO_ASKS`], in order), the upload link on her portal page and PRIV-015's line;
//! never while a 911 text waits for SAFE. Each photo is taken by `privacy::api::take_upload` or
//! `take_picture_text` (the on-server checks); [`after_photo`] gives the one re-ask for a file that
//! did not decode or is too small, the PRIV-025 reply for HEIC, else the preview text
//! ([`PHOTO_CHECK_TEMPLATE`]) showing it as it will appear on the card. Her YES confirms it shows no
//! person and no house number; REMOVE takes it off; no answer leaves it off with a note
//! ([`photo_answer`], [`left_off_note`]). A request with no photo goes on the same way.
//! [`photo_text`] also sends the design texts of `design.rs` (INT-045, INT-046).

use askhava_core::api::brand;
use askhava_messaging::api::Language;
use askhava_privacy::api::PhotoTaken;
use askhava_quotes::api::{HomeownerAnswer, LeftOff};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::design::{CONCEPT_OFFER_TEMPLATE, LOOK_ASK_TEMPLATE, LOOK_DESCRIBE_TEMPLATE};
use crate::handlers::queue_body;
use crate::models::{self, IntakeError};
use crate::service::{Queued, TextIn, safe_hold};

/// The photo ask (INT-044, PRIV-015): the job type's asks and the upload page link.
pub const PHOTO_ASK_TEMPLATE: &str = "photo_ask";
/// The one re-ask for a file that did not decode or is too small (INT-044).
pub const PHOTO_REASK_TEMPLATE: &str = "photo_reask";
/// The preview: the link shows the photo as it will appear on the card (INT-044, CARD-013).
pub const PHOTO_CHECK_TEMPLATE: &str = "photo_check";
/// The PRIV-025 reply to a HEIC photo, asking for a JPEG or PNG (D-1792).
pub const PHOTO_HEIC_TEMPLATE: &str = "photo_heic";
/// INT-044's words in the photo ask.
pub const NO_PHOTO_LINE: &str = "A photo or two helps businesses quote. No photo is fine too.";
/// PRIV-015's line, in the photo ask and on the upload page.
pub const LEAVE_OUT_LINE: &str =
    "Please leave people, house numbers and license plates out of photos.";
/// The most photo asks in one request (INT-044).
pub const MAX_PHOTO_ASKS: usize = 3;

/// One photo text of a request.
#[derive(Debug, Clone, Copy)]
pub enum PhotoText<'a> {
    /// The photo ask with the job type's asks and the upload link.
    Ask {
        /// The set's photo asks in order; only the first [`MAX_PHOTO_ASKS`] are sent.
        asks: &'a [String],
        /// Her portal page's upload link.
        link: &'a str,
    },
    /// The one re-ask.
    ReAsk,
    /// The PRIV-025 HEIC reply.
    Heic,
    /// The preview of one photo through its short-lived link.
    Check {
        /// The photo's `/photos/<token>` link on the brand's domain.
        link: &'a str,
    },
    /// The look-pictures ask of a design category (INT-045).
    LookAsk,
    /// The look in a few words, before the AI may read photos (INT-045).
    DescribeLook,
    /// One concept image with its label and INT-046's question.
    Concept {
        /// The image's `/photos/<token>` link.
        link: &'a str,
    },
}

/// What to do after one photo was taken (INT-044).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoStep {
    /// Stored: show her the preview.
    Preview(Uuid),
    /// Ask once more.
    ReAsk,
    /// Send the HEIC reply.
    Heic,
    /// Leave it off; go on without it.
    LeftOff,
}

/// INT-044: a file that did not decode or is under the size setting gets one re-ask, and only
/// when none was sent for this photo ask before (`reasked`).
#[must_use]
pub fn after_photo(taken: PhotoTaken, reasked: bool) -> PhotoStep {
    match taken {
        PhotoTaken::Stored(id) => PhotoStep::Preview(id),
        PhotoTaken::ReAsk if !reasked => PhotoStep::ReAsk,
        PhotoTaken::Heic => PhotoStep::Heic,
        PhotoTaken::ReAsk | PhotoTaken::Refused | PhotoTaken::TooManyPixels => PhotoStep::LeftOff,
    }
}

/// INT-044, CARD-013: her reply to a preview: YES (or SÍ) confirms it shows no person and no house
/// number, REMOVE (or QUITAR) removes it; anything else is no confirmation.
#[must_use]
pub fn photo_answer(words: &str) -> HomeownerAnswer {
    let word = crate::rules::fold(words.trim().trim_end_matches(['.', '!']));
    match word.as_str() {
        "yes" | "si" => HomeownerAnswer::Confirmed,
        "remove" | "quitar" => HomeownerAnswer::Removed,
        _ => HomeownerAnswer::NotConfirmed,
    }
}

/// The note shown with her card when a photo is left off (INT-044, D-432).
#[must_use]
pub const fn left_off_note(why: LeftOff) -> &'static str {
    match why {
        LeftOff::Removed => "You took a photo off. It won't go to businesses.",
        LeftOff::NotConfirmed => "We didn't send one photo because we didn't get your OK.",
        LeftOff::Flagged | LeftOff::NotScreened => {
            "We didn't send one photo because it may show a person or a house number."
        }
    }
}

/// The English and draft Spanish words of each photo and design text after the opening, as
/// (template ID, English, Spanish); `{asks}` and `{link}` are filled by [`photo_body`]. These
/// texts are fixed here, like the question bank's, until T383 lets the template files take new
/// keys with brand tokens (D-2871).
const TEXTS: [(&str, &str, &str); 7] = [
    (
        PHOTO_ASK_TEMPLATE,
        "A photo or two helps businesses quote. No photo is fine too. {asks} Text the photos here, or add them on your page: {link} Please leave people, house numbers and license plates out of photos.",
        "Una o dos fotos ayudan a los negocios a cotizar. Si no tiene foto, está bien. {asks} Mande las fotos aquí, o súbalas en su página: {link} Por favor no incluya personas, números de casa ni placas en las fotos.",
    ),
    (
        PHOTO_REASK_TEMPLATE,
        "We couldn't open that photo, or it's too small to see. Can you send it again? No photo is fine too.",
        "No pudimos abrir esa foto, o es muy pequeña. ¿La puede mandar otra vez? Si no tiene foto, está bien.",
    ),
    (
        PHOTO_CHECK_TEMPLATE,
        "Here is your photo the way businesses will see it: {link} Check that it shows no people, house numbers or license plates. Reply YES to send it, or REMOVE to take it off.",
        "Así verán su foto los negocios: {link} Revise que no muestre personas, números de casa ni placas. Responda SI para mandarla, o QUITAR para quitarla.",
    ),
    (
        PHOTO_HEIC_TEMPLATE,
        "We can't open that kind of photo. Please send it again as a JPEG or PNG. No photo is fine too.",
        "No podemos abrir ese tipo de foto. Por favor mándela otra vez como JPEG o PNG. Si no tiene foto, está bien.",
    ),
    (
        LOOK_ASK_TEMPLATE,
        "Have any pictures of the look you want? Screenshots are fine.",
        "¿Tiene fotos del estilo que quiere? Las capturas de pantalla también sirven.",
    ),
    (
        LOOK_DESCRIBE_TEMPLATE,
        "In a few words, what look do you want? For example: light gray, modern, matte. Reply SKIP to skip this.",
        "En pocas palabras, ¿qué estilo quiere? Por ejemplo: gris claro, moderno, mate. Responda OMITIR para saltar esto.",
    ),
    (
        CONCEPT_OFFER_TEMPLATE,
        "Is this the kind of thing you want? Reply YES, or tell us what to change. Example picture, AI-generated, not exact, may not be feasible: {link} Or reply SKIP if you don't want a picture.",
        "¿Es algo así lo que quiere? Responda SI, o díganos qué cambiar. Imagen de ejemplo, hecha con IA, no es exacta, puede que no se pueda hacer: {link} O responda OMITIR si no quiere una imagen.",
    ),
];

/// The whole text of `text` in `language` (Spanish only while Spanish conversations are on,
/// TXT-039), opening with the brand's short name (WORD-008, D-1932), with its template ID.
#[must_use]
pub fn photo_body(
    text: PhotoText<'_>,
    language: Language,
    spanish_on: bool,
) -> (&'static str, String) {
    let (id, asks, link) = match text {
        PhotoText::Ask { asks, link } => {
            let asks = asks[..asks.len().min(MAX_PHOTO_ASKS)].join(" ");
            (PHOTO_ASK_TEMPLATE, asks, link)
        }
        PhotoText::ReAsk => (PHOTO_REASK_TEMPLATE, String::new(), ""),
        PhotoText::Heic => (PHOTO_HEIC_TEMPLATE, String::new(), ""),
        PhotoText::Check { link } => (PHOTO_CHECK_TEMPLATE, String::new(), link),
        PhotoText::LookAsk => (LOOK_ASK_TEMPLATE, String::new(), ""),
        PhotoText::DescribeLook => (LOOK_DESCRIBE_TEMPLATE, String::new(), ""),
        PhotoText::Concept { link } => (CONCEPT_OFFER_TEMPLATE, String::new(), link),
    };
    let spanish = language == Language::Es && spanish_on;
    let words = TEXTS
        .iter()
        .find(|(key, _, _)| *key == id)
        .map_or("", |(_, en, es)| if spanish { es } else { en });
    let opening = if spanish {
        brand().es().short_name()
    } else {
        brand().short_name()
    };
    let words = words.replace("{asks}", &asks).replace("{link}", link);
    (id, format!("{opening}: {words}"))
}

/// INT-044 to INT-046, PRIV-015, PRIV-025: queues one photo or design text to the request's
/// homeowner in its language; `None`, sending nothing, while a 911 text waits for SAFE or once the
/// request is closed.
///
/// # Errors
///
/// [`IntakeError::Outbox`] or [`IntakeError::Database`].
pub async fn photo_text(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    text: PhotoText<'_>,
    spanish_on: bool,
) -> Result<Option<Queued>, IntakeError> {
    if safe_hold(db, conversation).await? {
        return Ok(None);
    }
    let Some(row) = models::conversation(db, conversation).await? else {
        return Ok(None);
    };
    if row.closed_at.is_some() {
        return Ok(None);
    }
    let language = crate::rules::language_from_code(&row.language);
    let (template, body) = photo_body(text, language, spanish_on);
    let subject = [conversation, msg.inbox_id];
    queue_body(db, secret, (msg.contact, template), &subject, &body)
        .await
        .map(Some)
}

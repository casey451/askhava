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
//!
//! Her portal page's buttons (T674, D-4360, D-5082): [`press_photo`] takes "Send this photo" and
//! "Take it off" about the job's newest photo with no [`PHOTO_OK`] row. A confirmation is one
//! `intake.intake_answer` row (`photo_ok`, the photo's ID in its 32-digit form as the choice); a
//! photo with no such row never goes on the card (INT-044, CARD-013). A removal is
//! `privacy::api::delete_photo`, of her own job's photo only. Both, and an upload, are refused
//! once her card was shown ([`card_went_out`]): a shown card keeps its photo IDs.
//! [`press_photo`] checks and writes in one transaction under `models::lock_homeowner`, and
//! [`confirmed_photos`] names only photos stored on the job asked about (D-5084).

use askhava_catalog::api::photo_asks;
use askhava_core::api::{IdSecret, brand};
use askhava_jobs::api::job_number;
use askhava_messaging::api::Language;
use askhava_privacy::api::{PhotoTaken, delete_photo, issue_link, job_photos, photo_path};
use askhava_quotes::api::{HomeownerAnswer, LeftOff};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

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

/// The question ID of her photo confirmation rows (INT-044, CARD-013); the choice is the photo's
/// ID in its 32-digit form.
pub const PHOTO_OK: &str = "photo_ok";

/// What her press on the upload page's preview did (INT-044).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoPress {
    /// "Send this photo": the confirmation row was recorded.
    Confirmed,
    /// "Take it off": the photo was deleted.
    Removed,
    /// No photo waits for her answer (a second press), or the answer was neither: nothing changed.
    Nothing,
    /// Her card was shown already: nothing changed.
    CardOut,
}

/// The request that opened `job` (as its first job or through a `job.` link row, like
/// `models::request_jobs`), of the homeowner whose vault contact is `contact` when one is given.
async fn opener(
    db: &mut PgConnection,
    job: Uuid,
    contact: Option<Uuid>,
) -> Result<Option<Uuid>, IntakeError> {
    sqlx::query_scalar!(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE ($3::uuid IS NULL OR h.contact_id = $3) AND (c.job_id = $1 OR EXISTS \
         (SELECT 1 FROM intake.intake_answer a WHERE a.conversation_id = c.id \
          AND a.question_id LIKE 'job.%' AND a.choice = $2)) ORDER BY c.opened_at, c.id LIMIT 1",
        job,
        job.simple().to_string(),
        contact
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// SEC-019 (T674, D-5083): whether `job` was opened by a request of the homeowner whose vault
/// contact is `contact`, as its first job or a linked one; her upload page serves only these.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn photo_job_is_hers(
    db: &mut PgConnection,
    contact: Uuid,
    job: Uuid,
) -> Result<bool, IntakeError> {
    Ok(opener(db, job, Some(contact)).await?.is_some())
}

/// The request that opened `job` and whether its card was shown (a `card.<job>` state row).
async fn request_of(db: &mut PgConnection, job: Uuid) -> Result<Option<(Uuid, bool)>, IntakeError> {
    let Some(conversation) = opener(db, job, None).await? else {
        return Ok(None);
    };
    let card = format!("card.{}", job.simple());
    let shown = models::latest_answer(db, conversation, &card).await?;
    Ok(Some((conversation, shown.is_some())))
}

/// INT-044, CARD-013 (D-5082): whether `job`'s card was shown to her, after which its photos no
/// longer change; `false` for a job no request opened.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn card_went_out(db: &mut PgConnection, job: Uuid) -> Result<bool, IntakeError> {
    Ok(request_of(db, job).await?.is_some_and(|(_, shown)| shown))
}

/// The photo IDs of `conversation`'s [`PHOTO_OK`] rows.
async fn confirmed(db: &mut PgConnection, conversation: Uuid) -> Result<Vec<Uuid>, IntakeError> {
    let codes = sqlx::query_scalar!(
        r#"SELECT choice AS "choice!" FROM intake.intake_answer
           WHERE conversation_id = $1 AND question_id = $2 AND choice IS NOT NULL"#,
        conversation,
        PHOTO_OK
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(codes
        .iter()
        .filter_map(|c| Uuid::parse_str(c).ok())
        .collect())
}

/// INT-044, CARD-013 (T674, D-4360, D-5084): the photos of `job` itself she confirmed show no
/// person and no house number, each once, newest first; only these may go on its card. A photo
/// with no row never does, and neither does a confirmed photo of another job of the same request.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn confirmed_photos(db: &mut PgConnection, job: Uuid) -> Result<Vec<Uuid>, IntakeError> {
    let Some((conversation, _)) = request_of(db, job).await? else {
        return Ok(Vec::new());
    };
    let done = confirmed(db, conversation).await?;
    let photos = job_photos(db, job).await;
    let photos = photos.map_err(|_| IntakeError::Database)?;
    Ok(photos.into_iter().filter(|id| done.contains(id)).collect())
}

/// INT-044, CARD-013, PRIV-009 (T674, D-4360, D-5084): her `answer` about `job`'s newest photo
/// that has no [`PHOTO_OK`] row, only before her card was shown. Confirmed records one `photo_ok`
/// row (a second press finds no waiting photo and records nothing); Removed deletes the photo
/// through `privacy::api::delete_photo`, which never removes another job's photo. The card check,
/// the read of her confirmations and the write are one transaction under
/// `models::lock_homeowner`, so two presses at once record one row and none lands after the card
/// step marked her card shown.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn press_photo(
    db: &mut PgConnection,
    job: Uuid,
    answer: HomeownerAnswer,
    at: Timestamp,
) -> Result<PhotoPress, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let Some(conversation) = opener(&mut tx, job, None).await? else {
        return Ok(PhotoPress::Nothing);
    };
    let Some((homeowner, _)) = models::conversation_state(&mut tx, conversation).await? else {
        return Ok(PhotoPress::Nothing);
    };
    models::lock_homeowner(&mut tx, homeowner).await?;
    let pressed = pressed(&mut tx, (conversation, job), answer, at).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(pressed)
}

/// [`press_photo`]'s check and write, inside its transaction and under its homeowner lock.
async fn pressed(
    db: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
    answer: HomeownerAnswer,
    at: Timestamp,
) -> Result<PhotoPress, IntakeError> {
    let card = format!("card.{}", job.simple());
    if models::latest_answer(db, conversation, &card)
        .await?
        .is_some()
    {
        return Ok(PhotoPress::CardOut);
    }
    let done = confirmed(db, conversation).await?;
    let photos = job_photos(db, job).await;
    let photos = photos.map_err(|_| IntakeError::Database)?;
    let Some(photo) = photos.into_iter().find(|id| !done.contains(id)) else {
        return Ok(PhotoPress::Nothing);
    };
    match answer {
        HomeownerAnswer::Confirmed => {
            let code = photo.simple().to_string();
            models::insert_plain(db, conversation, (PHOTO_OK, &code), at).await?;
            Ok(PhotoPress::Confirmed)
        }
        HomeownerAnswer::Removed => {
            let gone = delete_photo(db, (job, photo), at).await;
            gone.map_err(|_| IntakeError::Database)?;
            Ok(PhotoPress::Removed)
        }
        HomeownerAnswer::NotConfirmed => Ok(PhotoPress::Nothing),
    }
}

/// The step marker of a picture text (T675, D-4360, D-5251): one row per homeowner text with
/// media, its choice the text's inbox ID in its 32-digit form, written by the inbound route. At
/// steps 9 and 10 such a text never moves the order on; the picture task's photo step does.
pub const PHOTO_PENDING: &str = "photo_pending";
/// Step 9's marker: the photo ask was sent (INT-044).
const PHOTO_ASKED: &str = "photo_ask";
/// Step 9 is over: "no", any other text, or the photo step of a picture task that ended.
const PHOTO_DONE: &str = "photo_done";
/// Step 10's look-pictures question was sent (INT-045).
const LOOK_ASKED: &str = "look_ask";
/// Step 10's look pictures are over.
const LOOK_DONE: &str = "look_done";
/// Step 10's describe-the-look question was sent (INT-045, before the AI may read photos).
const DESCRIBE_ASKED: &str = "look_describe";
/// Step 10's describe-the-look question was answered or skipped.
const DESCRIBED: &str = "look_described";
/// A photo was shown for her YES or REMOVE; the choice is the photo's 32-digit ID.
const PHOTO_SHOWN: &str = "photo_shown";
/// Her answer to a shown photo came (YES, REMOVE or anything else); the choice is the photo's ID.
const PHOTO_ANSWERED: &str = "photo_answered";
/// The one re-ask was sent (INT-044).
const PHOTO_REASKED: &str = "photo_reask";
/// The choice of a step-9 or step-10 marker row (a choice is lowercase letters and digits).
const MARKED: &str = "yes";
/// Every step-9 and step-10 marker [`stage`] reads.
const STAGE_MARKERS: [&str; 6] = [
    PHOTO_ASKED,
    PHOTO_DONE,
    LOOK_ASKED,
    LOOK_DONE,
    DESCRIBE_ASKED,
    DESCRIBED,
];
/// Her upload page's path, from T674's route text (`portals::api::PHOTO_UPLOAD_ROUTE`; `intake`
/// may not depend on `portals`), with `{job}` the job number (INT-044).
pub const UPLOAD_PATH: &str = "/h/requests/{job}/photos";
/// The words that skip the describe-the-look question (INT-045).
const SKIP_WORDS: [&str; 2] = ["skip", "omitir"];

/// The request's first job a photo step works on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PhotoJob<'a> {
    /// The job's trade code, which is also its question set's ID.
    pub trade: &'a str,
    /// The job.
    pub job: Uuid,
    /// "Spanish conversations" (TXT-039).
    pub spanish_on: bool,
}

/// Where steps 9 and 10 stand, read from the markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Ask,
    Photos,
    AskLook,
    Look,
    AskDescribe,
    Describe,
    Done,
}

/// INT-044, INT-045: the stage of `conversation`; step 10 only in a `design: true` category.
async fn stage(
    db: &mut PgConnection,
    conversation: Uuid,
    trade: &str,
) -> Result<Stage, IntakeError> {
    let markers: Vec<String> = STAGE_MARKERS.iter().map(|m| (*m).to_owned()).collect();
    let have = sqlx::query_scalar!(
        r#"SELECT ARRAY(SELECT DISTINCT question_id FROM intake.intake_answer
             WHERE conversation_id = $1 AND question_id = ANY($2)) AS "have!: Vec<String>""#,
        conversation,
        &markers
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let has = |m: &str| have.iter().any(|h| h == m);
    let design = crate::design::is_design_category(trade);
    Ok(match () {
        () if !has(PHOTO_ASKED) => Stage::Ask,
        () if !has(PHOTO_DONE) => Stage::Photos,
        () if !design => Stage::Done,
        () if !has(LOOK_ASKED) => Stage::AskLook,
        () if !has(LOOK_DONE) => Stage::Look,
        () if !has(DESCRIBE_ASKED) => Stage::AskDescribe,
        () if !has(DESCRIBED) => Stage::Describe,
        () => Stage::Done,
    })
}

/// The choices of `conversation`'s `question` rows.
async fn choices(
    db: &mut PgConnection,
    conversation: Uuid,
    question: &str,
) -> Result<Vec<String>, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT choice AS "choice!" FROM intake.intake_answer
           WHERE conversation_id = $1 AND question_id = $2 AND choice IS NOT NULL
           ORDER BY answered_at, id"#,
        conversation,
        question
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// The job's photos, oldest first, split into the one shown and waiting for her answer and those
/// never shown; a confirmed photo (a [`PHOTO_OK`] row, also from her upload page) is neither.
async fn photo_queue(
    db: &mut PgConnection,
    conversation: Uuid,
    job: Uuid,
) -> Result<(Option<Uuid>, Vec<Uuid>), IntakeError> {
    let mut photos = job_photos(db, job)
        .await
        .map_err(|_| IntakeError::Database)?;
    photos.reverse();
    let shown = choices(db, conversation, PHOTO_SHOWN).await?;
    let answered = choices(db, conversation, PHOTO_ANSWERED).await?;
    let ok = choices(db, conversation, PHOTO_OK).await?;
    let code = |id: &Uuid| id.simple().to_string();
    photos.retain(|id| !ok.contains(&code(id)));
    let waiting = photos
        .iter()
        .find(|id| shown.contains(&code(id)) && !answered.contains(&code(id)))
        .copied();
    photos.retain(|id| !shown.contains(&code(id)));
    Ok((waiting, photos))
}

/// INT-044, CARD-013, PRIV-014: shows `photo` as it will appear on the card through a
/// short-lived `/photos/<token>` link (`privacy::api::issue_link`) and marks it shown.
async fn show(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    (photo, spanish_on): (Uuid, bool),
) -> Result<Vec<Queued>, IntakeError> {
    let at = request.0.received_at;
    let key = IdSecret::new(1, secret.clone()).map_err(|_| IntakeError::Job)?;
    let token = issue_link(db, &key, photo, at)
        .await
        .map_err(|_| IntakeError::Database)?;
    let link = format!("https://{}{}", brand().domain(), photo_path(&token));
    let check = PhotoText::Check { link: &link };
    let Some(queued) = photo_text(db, secret, request, check, spanish_on).await? else {
        return Ok(Vec::new());
    };
    let code = photo.simple().to_string();
    models::insert_plain(db, request.1, (PHOTO_SHOWN, &code), at).await?;
    Ok(vec![queued])
}

/// INT-044, PRIV-015: step 9's photo ask: the job type's asks (`catalog::api::photo_asks` of its
/// set, none in Spanish, TXT-039), her upload page link and PRIV-015's line; then the first
/// photo stored before step 9 is shown. Nothing, and no marker, while a 911 text waits for SAFE.
async fn ask_photos(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    about: PhotoJob<'_>,
) -> Result<Vec<Queued>, IntakeError> {
    let language = crate::service::request_language(db, request.1).await?;
    let spanish = about.spanish_on && language == Language::Es;
    let asks = if spanish {
        Vec::new()
    } else {
        photo_asks(about.trade)
    };
    let number = job_number(db, about.job)
        .await
        .map_err(|_| IntakeError::Job)?;
    let path = UPLOAD_PATH.replace("{job}", &number.to_string());
    let link = format!("https://{}{path}", brand().domain());
    let ask = PhotoText::Ask {
        asks: &asks,
        link: &link,
    };
    let Some(queued) = photo_text(db, secret, request, ask, about.spanish_on).await? else {
        return Ok(Vec::new());
    };
    let at = request.0.received_at;
    models::insert_plain(db, request.1, (PHOTO_ASKED, MARKED), at).await?;
    let mut texts = vec![queued];
    if let (_, unshown) = photo_queue(db, request.1, about.job).await?
        && let Some(first) = unshown.first()
    {
        texts.extend(show(db, secret, request, (*first, about.spanish_on)).await?);
    }
    Ok(texts)
}

/// INT-045: one step-10 question (`text`) and its marker; nothing during a SAFE hold.
async fn ask_marked(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    (text, marker): (PhotoText<'_>, &str),
    spanish_on: bool,
) -> Result<Vec<Queued>, IntakeError> {
    let Some(queued) = photo_text(db, secret, request, text, spanish_on).await? else {
        return Ok(Vec::new());
    };
    let at = request.0.received_at;
    models::insert_plain(db, request.1, (marker, MARKED), at).await?;
    Ok(vec![queued])
}

/// INT-043 to INT-045, INT-027: asks what the stage needs next: the photo ask (step 9), the
/// look-pictures or describe-the-look question (step 10, design categories only; no concept
/// image, D-5251), else step 11 (`order::ask_last_question`, once per request).
async fn advance(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    about: PhotoJob<'_>,
) -> Result<Vec<Queued>, IntakeError> {
    let on = about.spanish_on;
    match stage(db, request.1, about.trade).await? {
        Stage::Ask => ask_photos(db, secret, request, about).await,
        Stage::AskLook => {
            ask_marked(db, secret, request, (PhotoText::LookAsk, LOOK_ASKED), on).await
        }
        Stage::AskDescribe => {
            let text = (PhotoText::DescribeLook, DESCRIBE_ASKED);
            ask_marked(db, secret, request, text, on).await
        }
        Stage::Done => Ok(crate::order::ask_last_question(db, secret, request, on)
            .await?
            .into_iter()
            .collect()),
        Stage::Photos | Stage::Look | Stage::Describe => Ok(Vec::new()),
    }
}

/// Ends the open photo stage (step 9's photos or step 10's look pictures) and asks what follows.
async fn end_photos(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    (about, now): (PhotoJob<'_>, Stage),
) -> Result<Vec<Queued>, IntakeError> {
    let marker = if now == Stage::Look {
        LOOK_DONE
    } else {
        PHOTO_DONE
    };
    models::insert_plain(db, request.1, (marker, MARKED), request.0.received_at).await?;
    advance(db, secret, request, about).await
}

/// Whether her text `msg` is a picture text of `conversation` ([`PHOTO_PENDING`]).
async fn is_picture_text(
    db: &mut PgConnection,
    (msg, conversation): (&TextIn<'_>, Uuid),
) -> Result<bool, IntakeError> {
    let code = msg.inbox_id.simple().to_string();
    Ok(choices(db, conversation, PHOTO_PENDING)
        .await?
        .contains(&code))
}

/// INT-043 to INT-045, INT-027, CARD-013 (T675, D-4360): steps 9 and 10 once the trade questions
/// are done, called by the order walker; `hear` when her text answers the open question. A picture
/// text never moves them on (the picture task's [`picture_step`] does). Her reply to a shown photo
/// is YES (a [`PHOTO_OK`] row, as T674's "Send this photo"), REMOVE or QUITAR
/// (`privacy::api::delete_photo`), or anything else (left off, INT-044); the next photo not yet
/// shown is shown, else "no" or any other text ends the stage. Step 10's look described is kept
/// as her words (`order::keep_words`, PRIV-008) unless she replies SKIP or OMITIR.
///
/// # Errors
///
/// The step functions' errors or [`IntakeError::Database`].
pub(crate) async fn photo_steps(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    about: PhotoJob<'_>,
    hear: bool,
) -> Result<Vec<Queued>, IntakeError> {
    let now = stage(db, request.1, about.trade).await?;
    let open = matches!(now, Stage::Photos | Stage::Look | Stage::Describe);
    if !hear || !open {
        return advance(db, secret, request, about).await;
    }
    if is_picture_text(db, request).await? {
        return Ok(Vec::new());
    }
    let (msg, conversation) = request;
    if now == Stage::Describe {
        let word = crate::rules::fold(msg.inbound.words.trim());
        if !SKIP_WORDS.contains(&word.as_str()) {
            let words = (msg.inbound.words, msg.received_at);
            crate::order::keep_words(db, (conversation, about.job), msg.inbox_id, words).await?;
        }
        models::insert_plain(db, conversation, (DESCRIBED, MARKED), msg.received_at).await?;
        return advance(db, secret, request, about).await;
    }
    let (waiting, unshown) = photo_queue(db, conversation, about.job).await?;
    if let Some(photo) = waiting {
        answer_shown(db, (conversation, about.job), photo, msg).await?;
    }
    if let Some(next) = unshown.first() {
        return show(db, secret, request, (*next, about.spanish_on)).await;
    }
    end_photos(db, secret, request, (about, now)).await
}

/// INT-044, CARD-013: her reply `msg` to shown `photo`: YES records [`PHOTO_OK`], REMOVE deletes
/// it (`privacy::api::delete_photo`, of this job only), anything else leaves it off the card.
async fn answer_shown(
    db: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
    photo: Uuid,
    msg: &TextIn<'_>,
) -> Result<(), IntakeError> {
    let (code, at) = (photo.simple().to_string(), msg.received_at);
    models::insert_plain(db, conversation, (PHOTO_ANSWERED, &code), at).await?;
    match photo_answer(msg.inbound.words) {
        HomeownerAnswer::Confirmed => {
            models::insert_plain(db, conversation, (PHOTO_OK, &code), at).await
        }
        HomeownerAnswer::Removed => delete_photo(db, (job, photo), at)
            .await
            .map(|_| ())
            .map_err(|_| IntakeError::Database),
        HomeownerAnswer::NotConfirmed => Ok(()),
    }
}

/// INT-044, PRIV-025 (T675, D-4360): the picture task's photo step for `request`, with what
/// became of each of its media (`steps`, from [`after_photo`]). Only while step 9's photos or
/// step 10's look pictures are open; before step 9 a stored photo waits for it. With a photo
/// shown and waiting, nothing; else the first photo not yet shown is shown, else the one re-ask
/// (once per request), else the HEIC reply, else (left off, refused or failed) the stage ends and
/// the order moves on, so step 9 never waits on a task that has ended.
///
/// # Errors
///
/// The step functions' errors or [`IntakeError::Database`].
pub(crate) async fn picture_step(
    db: &mut PgConnection,
    secret: &SecretString,
    request: (&TextIn<'_>, Uuid),
    about: PhotoJob<'_>,
    steps: &[PhotoStep],
) -> Result<Vec<Queued>, IntakeError> {
    let now = stage(db, request.1, about.trade).await?;
    if !matches!(now, Stage::Photos | Stage::Look) {
        return Ok(Vec::new());
    }
    let (waiting, unshown) = photo_queue(db, request.1, about.job).await?;
    if waiting.is_some() {
        return Ok(Vec::new());
    }
    if let Some(next) = unshown.first() {
        return show(db, secret, request, (*next, about.spanish_on)).await;
    }
    let on = about.spanish_on;
    if steps.contains(&PhotoStep::ReAsk) {
        let marked = (PhotoText::ReAsk, PHOTO_REASKED);
        return ask_marked(db, secret, request, marked, on).await;
    }
    if steps.contains(&PhotoStep::Heic) {
        return Ok(photo_text(db, secret, request, PhotoText::Heic, on)
            .await?
            .into_iter()
            .collect());
    }
    end_photos(db, secret, request, (about, now)).await
}

/// Whether request `conversation` already sent its one re-ask (INT-044).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub(crate) async fn reasked(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<bool, IntakeError> {
    Ok(models::latest_answer_time(db, conversation, PHOTO_REASKED)
        .await?
        .is_some())
}

/// INT-044 (T675, D-4360, D-5251): marks homeowner `contact`'s text `inbox` a picture text of her
/// open request (one [`PHOTO_PENDING`] row, its choice the inbox ID's 32-digit form, at `at`), in
/// the inbound route's transaction, so the order never moves on at steps 9 and 10 for its body.
/// The route calls it before `receive_raw_reply` and again after it (for a request the text
/// opened); a second call writes nothing. Returns whether the text is marked.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn note_picture_text(
    db: &mut PgConnection,
    contact: Uuid,
    inbox: Uuid,
    at: Timestamp,
) -> Result<bool, IntakeError> {
    let Some(conversation) = models::open_conversation(db, contact).await? else {
        return Ok(false);
    };
    let code = inbox.simple().to_string();
    if !choices(db, conversation, PHOTO_PENDING)
        .await?
        .contains(&code)
    {
        models::insert_plain(db, conversation, (PHOTO_PENDING, &code), at).await?;
    }
    Ok(true)
}

/// The request picture text `inbox` was marked in and the marker's time (the text's), if any.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub(crate) async fn picture_request(
    db: &mut PgConnection,
    inbox: Uuid,
) -> Result<Option<(Uuid, Timestamp)>, IntakeError> {
    let row = sqlx::query!(
        "SELECT conversation_id, answered_at FROM intake.intake_answer \
         WHERE question_id = $1 AND choice = $2 ORDER BY answered_at, id LIMIT 1",
        PHOTO_PENDING,
        inbox.simple().to_string()
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(row.map(|r| (r.conversation_id, r.answered_at.to_jiff())))
}

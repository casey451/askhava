//! The ZIP and area check, step 4 of the intake order (INT-049, INT-048, INT-022, INT-019,
//! PRIV-028; D-1521, T251).
//!
//! [`ask_zip`] texts `zip_question` (English or Spanish, draft copy for the copy panel, WORD-028)
//! and records that it asked. The two texts are fixed here ([`zip_body`]), as the photo texts
//! are, until the template files can take new keys without a done test's list changing (D-3179,
//! D-2871). [`answer_zip`] reads the reply: one 5-digit ZIP ([`read_zip`]) that
//! `catalog::api::town_for_zip` maps to a service-area town (a reviewed extra ZIP with no centroid
//! included; its job then has no location and cards match by towns only) is stored on the
//! property with that town ([`ZipStep::Stored`]); a ZIP in the reviewed Arizona centroid table that
//! belongs to no service-area town closes the request through `routing::apply`'s out-of-area close
//! ([`ZipStep::OutOfArea`]); any other reply (no ZIP, two ZIPs, a well-formed ZIP Ask Hava does not
//! know) is asked once more with `zip_again` ([`ZipStep::AskedAgain`]) and after that waited on
//! ([`ZipStep::Waiting`]): nothing closes, INT-019's no-reply wait still applies.
//!
//! The ZIP is stored only in `intake.property` (with the vault contact ID and the town, never an
//! address). It goes into no AI call, card, log line, audit value or error. T162's intake order
//! calls this step; this file adds the step only.

use askhava_catalog::api::{town_for_zip, zip_location};
use askhava_core::api::brand;
use askhava_messaging::api::Language;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::handlers::queue_body;
use crate::models::{self, IntakeError};
use crate::routing::{Outcome, Request, Routing, apply};
use crate::rules::{Settings, language_from_code};
use crate::service::{Queued, TextIn};

/// INT-049: the ZIP question's template ID.
pub const ZIP_QUESTION_TEMPLATE: &str = "zip_question";
/// INT-049: the one re-ask's template ID (from the twin's re-ask line).
pub const ZIP_AGAIN_TEMPLATE: &str = "zip_again";
/// The English and draft Spanish words of each ZIP text after the opening, as (template ID,
/// English, Spanish): the cold read's and the copy panel's wording (WORD-028, WORD-029, D-3179).
const TEXTS: [(&str, &str, &str); 2] = [
    (
        ZIP_QUESTION_TEMPLATE,
        "What's the ZIP code where the work is? We use it to find businesses that serve your area.",
        "¿Cuál es el código postal de donde es el trabajo? Lo usamos para encontrar negocios que atienden su zona.",
    ),
    (
        ZIP_AGAIN_TEMPLATE,
        "Sorry, we didn't catch a ZIP code. Just the 5 numbers, like 86403.",
        "Perdón, no entendimos el código postal. Solo los 5 números, por ejemplo 86403.",
    ),
];
/// The `intake_answer` question ID that records the asks: `asked`, then `again`; never the ZIP.
const ZIP_ASKS: &str = "zip";
/// The town code `routing::apply` is given for an out-of-area ZIP: it has no service-area town,
/// and an out-of-area close writes no demand row, so this code is checked and never stored.
const NO_TOWN: &str = "out_of_area";

/// Where a ZIP lies (INT-048, INT-049).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipPlace {
    /// A service-area town's ZIP: the town code and whether the ZIP has a centroid location.
    Town {
        /// The catalog town code.
        town: String,
        /// The ZIP has a reviewed centroid; a reviewed extra ZIP has none.
        located: bool,
    },
    /// In the reviewed Arizona centroid table but in no service-area town (INT-022).
    OutOfArea,
    /// Not a ZIP Ask Hava knows.
    Unknown,
}

/// What [`answer_zip`] did (INT-049).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipStep {
    /// The ZIP and its town are stored on the property; the trade check follows.
    Stored(ZipPlace),
    /// The request closed early as out of area, with the INT-022 reply (none if already closed).
    OutOfArea(Option<Queued>),
    /// The reply was unusable; the ZIP question was asked once more.
    AskedAgain(Queued),
    /// The reply was unusable again; nothing is sent and nothing closes (INT-019's wait applies).
    Waiting,
}

/// INT-049: the one 5-digit ZIP in `words`, or `None` for none or more than one. A ZIP is five
/// digits standing alone (a ZIP+4's first five count); a longer digit run is not one.
#[must_use]
pub fn read_zip(words: &str) -> Option<&str> {
    let mut found = words
        .split(|c: char| !c.is_ascii_digit())
        .filter(|run| run.len() == 5);
    let first = found.next()?;
    found.all(|other| other == first).then_some(first)
}

/// INT-048, INT-049, INT-022: where `zip` lies in the compiled-in markets and centroid table.
///
/// # Errors
///
/// [`IntakeError::Catalog`] if the compiled-in market data cannot be read.
pub fn place_zip(zip: &str) -> Result<ZipPlace, IntakeError> {
    let located = zip_location(zip)
        .map_err(|_| IntakeError::Catalog)?
        .is_some();
    Ok(match town_for_zip(zip).map_err(|_| IntakeError::Catalog)? {
        Some(town) => ZipPlace::Town {
            town: town.code,
            located,
        },
        None if located => ZipPlace::OutOfArea,
        None => ZipPlace::Unknown,
    })
}

/// INT-049: texts the ZIP question in the request's language and records that it was asked.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Outbox`] or [`IntakeError::Database`]; nothing
/// is kept on an error.
pub async fn ask_zip(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<Queued, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let queued = ask(
        &mut tx,
        secret,
        (msg, conversation),
        settings,
        ZIP_QUESTION_TEMPLATE,
    )
    .await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(queued)
}

/// INT-049, INT-022, INT-019, PRIV-028: takes the homeowner's reply to the ZIP question, as the
/// module comment says. A stored property holds the vault contact ID, the town and the ZIP.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Catalog`], [`IntakeError::Template`],
/// [`IntakeError::Outbox`], [`IntakeError::Job`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`]; nothing is kept on an error.
pub async fn answer_zip(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<ZipStep, IntakeError> {
    let place = match read_zip(msg.inbound.words) {
        Some(zip) => (zip, place_zip(zip)?),
        None => ("", ZipPlace::Unknown),
    };
    match place {
        (zip, ZipPlace::Town { town, located }) => {
            store(db, (msg, conversation), (&town, zip)).await?;
            Ok(ZipStep::Stored(ZipPlace::Town { town, located }))
        }
        (_, ZipPlace::OutOfArea) => close(db, secret, (msg, conversation), settings).await,
        (_, ZipPlace::Unknown) => again(db, secret, (msg, conversation), settings).await,
    }
}

/// Stores the property under the homeowner lock, in one transaction.
async fn store(
    db: &mut PgConnection,
    (msg, conversation): (&TextIn<'_>, Uuid),
    (town, zip): (&str, &str),
) -> Result<(), IntakeError> {
    let (homeowner, _) = models::conversation_state(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let ids = (homeowner, msg.contact);
    models::insert_property(&mut tx, ids, (town, zip), msg.received_at).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)
}

/// INT-022: the out-of-area close, with its reply in the request's language.
async fn close(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<ZipStep, IntakeError> {
    let language = language(db, conversation).await?;
    let req = Request {
        town: NO_TOWN,
        area: &[],
        trade: "",
        trade_open: false,
        words: "",
        move_states: None,
        job_types: &[],
    };
    let routing = Routing {
        conversation,
        msg,
        language,
        spanish_on: settings.spanish_conversations_on,
        label: "",
        job_type: None,
    };
    let queued = apply(db, secret, (&req, Outcome::OutOfArea), &routing).await?;
    Ok(ZipStep::OutOfArea(queued))
}

/// INT-049, INT-019: asks once more after the first ask, then waits.
async fn again(
    db: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
) -> Result<ZipStep, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    if models::latest_answer(&mut tx, conversation, ZIP_ASKS).await? != Some("asked".to_owned()) {
        return Ok(ZipStep::Waiting);
    }
    let queued = ask(
        &mut tx,
        secret,
        (msg, conversation),
        settings,
        ZIP_AGAIN_TEMPLATE,
    )
    .await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(ZipStep::AskedAgain(queued))
}

/// Queues `template` to the sender and records the ask (`asked` or `again`) in `tx`.
async fn ask(
    tx: &mut PgConnection,
    secret: &SecretString,
    (msg, conversation): (&TextIn<'_>, Uuid),
    settings: Settings,
    template: &'static str,
) -> Result<Queued, IntakeError> {
    let language = language(tx, conversation).await?;
    let body = zip_body(template, language, settings.spanish_conversations_on);
    let subject = [conversation, msg.inbox_id];
    let queued = queue_body(tx, secret, (msg.contact, template), &subject, &body).await?;
    let choice = if template == ZIP_QUESTION_TEMPLATE {
        "asked"
    } else {
        "again"
    };
    let ids = (conversation, msg.inbox_id);
    models::insert_answer(tx, ids, (ZIP_ASKS, choice), msg.received_at).await?;
    Ok(queued)
}

/// INT-049, WORD-008, TXT-039: the whole text of ZIP text `template` ([`ZIP_QUESTION_TEMPLATE`] or
/// [`ZIP_AGAIN_TEMPLATE`]) in `language` (Spanish only while Spanish conversations are on),
/// opening with the brand's short name; empty words for any other ID.
#[must_use]
pub fn zip_body(template: &str, language: Language, spanish_on: bool) -> String {
    let spanish = language == Language::Es && spanish_on;
    let words = TEXTS
        .iter()
        .find(|(id, _, _)| *id == template)
        .map_or("", |(_, en, es)| if spanish { es } else { en });
    let opening = if spanish {
        brand().es().short_name()
    } else {
        brand().short_name()
    };
    format!("{opening}: {words}")
}

/// The request's language (CON-026).
async fn language(db: &mut PgConnection, conversation: Uuid) -> Result<Language, IntakeError> {
    let row = models::conversation(db, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    Ok(language_from_code(&row.language))
}

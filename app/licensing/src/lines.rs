//! The license lines on a quote card: the record line with its check date and link, the
//! "license since" year, the city-license line for a no-license business and its business-reported
//! insurance (LIC-026, LIC-032, LIC-034, LIC-037, LIC-038, WORD-006, WORD-016).
//!
//! Pure functions over facts the caller read from the state record, the city list and the
//! business's own sign-up; the words are `templates/license_lines.json`, English and Spanish. A
//! card line states only a public record or the business's own answer: nothing here can produce a
//! rating, review count, hire count or ranking (WORD-016). The pest-management card's LIC-047 line
//! is not built here: a self-attested license has no check date, so [`licensed_lines`] gives it
//! no lines. The "About us" line (BIZ-022) and the card order on the quote page are other tasks'.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use jiff::civil::Date;
use serde::Deserialize;
use thiserror::Error;

use crate::handyman::HANDYMAN_CARD_LINE;
use crate::models::Agency;

/// The compiled-in line wording and agency links.
const LINES_FILE: &str = include_str!("../templates/license_lines.json");

/// Why a card line could not be built. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinesError {
    /// The compiled-in template file is malformed or lacks a line.
    #[error("the license line templates are not readable")]
    TemplatesUnreadable,
}

/// The language a card is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardLanguage {
    /// English.
    English,
    /// Spanish.
    Spanish,
}

/// One line on a quote card, with the link a homeowner follows to check the record yourself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    /// The words shown.
    pub text: String,
    /// The agency's website, for the record-check line only.
    pub link: Option<String>,
}

impl CardLine {
    fn plain(text: String) -> Self {
        Self { text, link: None }
    }
}

/// Who issued a licensed-trade card's license.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issuer {
    /// The Registrar of Contractors (LIC-026).
    Roc,
    /// Another state agency (LIC-034).
    Agency(Agency),
}

/// A license on a licensed-trade card, as the daily check and the state record gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseFacts {
    /// Who issued it.
    pub issuer: Issuer,
    /// The license number as the record writes it.
    pub number: String,
    /// The day of the check that showed it Active.
    pub checked_on: Date,
    /// The year of the original issue date on the state record, when the record has one.
    pub since_year: Option<i16>,
}

/// A no-license card's facts (LIC-038, LIC-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoLicenseFacts {
    /// The business is signed up as a handyman, so the LIC-024 line comes first.
    pub handyman: bool,
    /// The year on the city list, when the list carries a date for the business.
    pub city_since_year: Option<i16>,
    /// What the business reported about insurance; `None` when it recorded nothing.
    pub insurance: Option<bool>,
}

/// One language's words.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Words {
    agencies: BTreeMap<String, String>,
    months: Vec<String>,
    date: String,
    roc_checked: String,
    agency_checked: String,
    roc_since: String,
    agency_since: String,
    city_since: String,
    city: String,
    insurance_yes: String,
    insurance_no: String,
}

/// The template file's shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Templates {
    links: BTreeMap<String, String>,
    en: Words,
    es: Words,
}

static TEMPLATES: LazyLock<Option<Templates>> = LazyLock::new(|| parse_templates(LINES_FILE));

/// Reads a template file; `None` when malformed or when a month list is not twelve long.
pub(crate) fn parse_templates(text: &str) -> Option<Templates> {
    let file: Templates = serde_saphyr::from_str(text).ok()?;
    (file.en.months.len() == 12 && file.es.months.len() == 12).then_some(file)
}

fn words(language: CardLanguage) -> Result<(&'static Templates, &'static Words), LinesError> {
    let all = TEMPLATES.as_ref().ok_or(LinesError::TemplatesUnreadable)?;
    Ok(match language {
        CardLanguage::English => (all, &all.en),
        CardLanguage::Spanish => (all, &all.es),
    })
}

fn format_date(words: &Words, date: Date) -> Result<String, LinesError> {
    let index = usize::try_from(date.month() - 1).map_err(|_| LinesError::TemplatesUnreadable)?;
    let month = words
        .months
        .get(index)
        .ok_or(LinesError::TemplatesUnreadable)?;
    Ok(words
        .date
        .replace("{month}", month)
        .replace("{day}", &date.day().to_string())
        .replace("{year}", &date.year().to_string()))
}

/// The record-check line and the since-line of a licensed-trade card (LIC-026, LIC-034, LIC-037).
///
/// The check line carries the agency's link. The since-line appears only when the state record has
/// an original issue year, so a record with none gets no such line. A pest-management license is
/// self-attested (LIC-047) and gets no lines here.
///
/// # Errors
///
/// [`LinesError::TemplatesUnreadable`] when the template file is malformed.
pub fn licensed_lines(
    facts: &LicenseFacts,
    language: CardLanguage,
) -> Result<Vec<CardLine>, LinesError> {
    let (all, words) = words(language)?;
    let (link_key, checked, since, agency) = match facts.issuer {
        Issuer::Roc => ("roc", &words.roc_checked, &words.roc_since, String::new()),
        Issuer::Agency(Agency::PestManagement) => return Ok(Vec::new()),
        Issuer::Agency(a) => {
            let name = words
                .agencies
                .get(a.code())
                .ok_or(LinesError::TemplatesUnreadable)?;
            (
                a.code(),
                &words.agency_checked,
                &words.agency_since,
                name.clone(),
            )
        }
    };
    let date = format_date(words, facts.checked_on)?;
    let text = checked
        .replace("{agency}", &agency)
        .replace("{number}", &facts.number)
        .replace("{date}", &date);
    let link = all
        .links
        .get(link_key)
        .ok_or(LinesError::TemplatesUnreadable)?;
    let mut lines = vec![CardLine {
        text,
        link: Some(link.clone()),
    }];
    if let Some(year) = facts.since_year {
        let text = since
            .replace("{agency}", &agency)
            .replace("{year}", &year.to_string());
        lines.push(CardLine::plain(text));
    }
    Ok(lines)
}

/// The lines of a no-license card, in order: the LIC-024 handyman line when it applies, then the
/// city-license line, then the business-reported insurance (LIC-038, LIC-032, WORD-006).
///
/// The city line says "since <year>" only when the city list gave a date. Insurance is shown
/// as the business reported it and is never required; with no answer there is no line (D-061).
/// The handyman line is LIC-024's English words in both languages: the spec fixes no Spanish.
///
/// # Errors
///
/// [`LinesError::TemplatesUnreadable`] when the template file is malformed.
pub fn no_license_lines(
    facts: &NoLicenseFacts,
    language: CardLanguage,
) -> Result<Vec<CardLine>, LinesError> {
    let (_, words) = words(language)?;
    let mut lines = Vec::new();
    if facts.handyman {
        lines.push(CardLine::plain(HANDYMAN_CARD_LINE.to_string()));
    }
    lines.push(CardLine::plain(match facts.city_since_year {
        Some(year) => words.city_since.replace("{year}", &year.to_string()),
        None => words.city.clone(),
    }));
    match facts.insurance {
        Some(true) => lines.push(CardLine::plain(words.insurance_yes.clone())),
        Some(false) => lines.push(CardLine::plain(words.insurance_no.clone())),
        None => {}
    }
    Ok(lines)
}

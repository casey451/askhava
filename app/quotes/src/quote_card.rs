//! The quote card a homeowner reads, and the business sees exactly as she does (PICK-003, PICK-004,
//! PICK-005, PICK-040, PICK-041, PICK-044; D-1111 to D-1113).
//!
//! [`QuoteCard::lines`] is pure and takes no viewer, so the business preview and the homeowner's
//! card can't differ (PICK-005). Line order: the business's public name, its license record lines
//! (LIC-026, LIC-034, LIC-047, built by `licensing`), the Google and Yelp link slot (T165, D-491),
//! the "check it yourself" line, WORD-014's "Independent business" line, "Gets emergency requests
//! at night" for a business that ticked after-hours emergencies (BIZ-007), the six quote facts in
//! their fixed order, the format's plain definition (and "Free estimate" for no trip fee, never on
//! a visit-fee quote), then the business's own words and, only when the "AI quote summaries"
//! setting is on and the summary passed [`check_summary`], the summary with its label. Nothing
//! here holds a phone, an address or the "minutes this quote took" field.

use askhava_licensing::api::{CardLanguage, CardLine};
use jiff::civil::Date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::{DayPart, Format};
use crate::models::{self, QuotesError};

/// "Trip fee counts toward the repair" (PICK-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripFee {
    /// It counts toward the repair.
    Counts,
    /// It does not.
    DoesNotCount,
    /// There is no trip fee: the card adds the "Free estimate" definition (D-283).
    None,
}

/// "Parts and materials" (PICK-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parts {
    /// Included in the price.
    Included,
    /// Charged on top.
    Extra,
    /// Estimated on site.
    OnSite,
}

/// "Permit" (PICK-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permit {
    /// Included in the price.
    Included,
    /// Not included.
    NotIncluded,
    /// The job needs none.
    NotNeeded,
}

/// "Speaks Spanish" (PICK-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spanish {
    /// On the phone.
    Phone,
    /// On site.
    OnSite,
    /// On the phone and on site.
    Both,
    /// No.
    No,
}

/// The business's own statements on one quote, never set or checked by Ask Hava (PICK-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteFacts {
    /// Trip fee counts toward the repair.
    pub trip_fee: TripFee,
    /// Parts and materials.
    pub parts: Parts,
    /// Permit.
    pub permit: Permit,
    /// Price may change after seeing the job.
    pub may_change: bool,
    /// Earliest visit: a date and a part of the day.
    pub earliest_visit: (Date, DayPart),
    /// Speaks Spanish.
    pub spanish: Spanish,
}

/// The "AI quote summaries" owner-portal setting: off by default and at launch (PICK-003, D-373).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SummarySetting {
    /// No summary is made, stored or shown.
    #[default]
    Off,
    /// Summaries that pass [`check_summary`] show under the business's own words.
    On,
}

/// The AI-018 privacy-policy sentence that must be live before summaries switch on (PICK-003).
pub const POLICY_SUMMARY_SENTENCE: &str =
    "AI may also shorten a business's quote into a summary shown next to the business's own words.";

/// Why the setting stays off (PICK-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchRefused {
    /// No recorded decision of Casey's (a `D-<n>` id) was given.
    NoDecision,
    /// The policy version in force today lacks the summary sentence, or is not yet published.
    PolicyLacksSentence,
}

/// PICK-003, D-373: switches summaries on only with Casey's recorded `decision` (`D-<n>`) and on a
/// `today` when the published policy version (`policy_text`, published on `published_on`) holds
/// [`POLICY_SUMMARY_SENTENCE`].
///
/// # Errors
///
/// [`SwitchRefused`]; the setting then stays [`SummarySetting::Off`].
pub fn switch_on(
    decision: Option<&str>,
    (policy_text, published_on): (&str, Date),
    today: Date,
) -> Result<SummarySetting, SwitchRefused> {
    let recorded = decision
        .and_then(|d| d.strip_prefix("D-"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if !recorded {
        return Err(SwitchRefused::NoDecision);
    }
    if published_on > today || !policy_text.contains(POLICY_SUMMARY_SENTENCE) {
        return Err(SwitchRefused::PolicyLacksSentence);
    }
    Ok(SummarySetting::On)
}

/// A summary that adds nothing to the business's words; only [`check_summary`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary(String);

/// Words a summary may use that the business did not write: they carry no price, scope or claim.
const LINKING_WORDS: &[&str] = &[
    "a", "an", "and", "the", "of", "to", "for", "in", "on", "at", "with", "is", "it", "this",
    "will", "be", "by", "or", "y", "el", "la", "los", "las", "de", "del", "en", "un", "una", "con",
    "por", "para", "o", "es",
];

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// PICK-003, PICK-004: the summary to show, or none: none while the setting is off, and none when
/// any word or number of it is not in `original` (a price, scope, warranty or other claim the
/// business did not write) beyond [`LINKING_WORDS`] (D-1112).
#[must_use]
pub fn check_summary(setting: SummarySetting, original: &str, summary: &str) -> Option<Summary> {
    if setting == SummarySetting::Off || summary.trim().is_empty() {
        return None;
    }
    let own: Vec<String> = words(original).collect();
    words(summary)
        .all(|w| LINKING_WORDS.contains(&w.as_str()) || own.contains(&w))
        .then(|| Summary(summary.trim().to_owned()))
}

/// Everything one quote card shows (PICK-040, PICK-041, PICK-044).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteCard {
    /// The business's public business name (PICK-041).
    pub business_name: String,
    /// Its license record lines with their links, from `licensing::api` (LIC-026, LIC-034, LIC-047).
    pub license_lines: Vec<CardLine>,
    /// It ticked after-hours emergencies (BIZ-007, PICK-044).
    pub after_hours: bool,
    /// The quote's format (PICK-053).
    pub format: Format,
    /// The quote facts (PICK-040).
    pub facts: QuoteFacts,
    /// The business's original wording, always shown (PICK-003).
    pub original: String,
    /// A checked summary, present only while summaries are on (PICK-003, PICK-004).
    pub summary: Option<Summary>,
}

impl QuoteCard {
    /// PICK-005, PICK-040, PICK-041, PICK-044: the card's lines in `language`; the same for the
    /// homeowner and for the business's preview.
    #[must_use]
    pub fn lines(&self, language: CardLanguage) -> Vec<CardLine> {
        let w = match language {
            CardLanguage::English => &EN,
            CardLanguage::Spanish => &ES,
        };
        let plain = |text: String| CardLine { text, link: None };
        let mut out = vec![plain(self.business_name.clone())];
        out.extend(self.license_lines.iter().cloned());
        out.push(plain(w.check_yourself.to_owned()));
        out.push(plain(w.independent.to_owned()));
        if self.after_hours {
            out.push(plain(w.after_hours.to_owned()));
        }
        out.extend(fact_lines(w, &self.facts).into_iter().map(plain));
        out.push(plain(w.definition(self.format).to_owned()));
        if self.facts.trip_fee == TripFee::None && self.format != Format::DiagnosticVisit {
            out.push(plain(w.free_estimate.to_owned()));
        }
        out.push(plain(format!("{} {}", w.own_words, self.original)));
        if let Some(Summary(text)) = &self.summary {
            out.push(plain(format!("{} {}", w.summary_label, text)));
        }
        out
    }
}

/// What the caller brings to a stored quote's card: the business's public name and license lines
/// (`businesses`, `licensing`, T165) and its quote facts (T148's form, D-1049). A stored earliest
/// visit replaces `facts.earliest_visit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSource {
    /// The business's public business name.
    pub business_name: String,
    /// Its license record lines.
    pub license_lines: Vec<CardLine>,
    /// Its quote facts.
    pub facts: QuoteFacts,
}

/// PICK-003, PICK-005, PICK-044: the card of `business`'s shown quote on `job`, or none when it has
/// no confirmed, unwithdrawn quote. The original wording is read from the stored quote; no summary
/// is stored, so none is shown while summaries are off (D-373); the after-hours tick is read from
/// `businesses::api` (BIZ-007).
///
/// # Errors
///
/// [`QuotesError::Database`] or [`QuotesError::Business`].
pub async fn quote_card(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    source: CardSource,
) -> Result<Option<QuoteCard>, QuotesError> {
    let Some(stored) = models::stored_card(db, job, business).await? else {
        return Ok(None);
    };
    let record = askhava_businesses::api::business(db, business)
        .await
        .map_err(|_| QuotesError::Business)?;
    let mut facts = source.facts;
    facts.earliest_visit = stored.earliest_visit.unwrap_or(facts.earliest_visit);
    Ok(Some(QuoteCard {
        business_name: source.business_name,
        license_lines: source.license_lines,
        after_hours: record.after_hours,
        format: stored.format,
        facts,
        original: stored.original,
        summary: None,
    }))
}

/// The `i`th word of a fixed list (the enums' order matches each list).
fn pick(i: usize, set: &[&'static str]) -> &'static str {
    set.get(i).copied().unwrap_or_default()
}

/// The six facts in PICK-040's fixed order.
fn fact_lines(w: &Words, f: &QuoteFacts) -> [String; 6] {
    let (date, part) = f.earliest_visit;
    let month = usize::try_from(date.month() - 1).unwrap_or_default();
    let day = w
        .date
        .replace("{month}", pick(month, &w.months))
        .replace("{day}", &date.day().to_string());
    [
        format!("{} {}", w.facts[0], pick(f.trip_fee as usize, &w.trip_fee)),
        format!("{} {}", w.facts[1], pick(f.parts as usize, &w.parts)),
        format!("{} {}", w.facts[2], pick(f.permit as usize, &w.permit)),
        format!("{} {}", w.facts[3], w.yes_no[usize::from(!f.may_change)]),
        format!(
            "{} {}",
            w.facts[4],
            pick(part as usize, &w.day_part).replace("{day}", &day)
        ),
        format!("{} {}", w.facts[5], pick(f.spanish as usize, &w.spanish)),
    ]
}

/// One language's card words (D-1113).
struct Words {
    check_yourself: &'static str,
    independent: &'static str,
    after_hours: &'static str,
    facts: [&'static str; 6],
    trip_fee: [&'static str; 3],
    parts: [&'static str; 3],
    permit: [&'static str; 3],
    yes_no: [&'static str; 2],
    day_part: [&'static str; 4],
    spanish: [&'static str; 4],
    months: [&'static str; 12],
    date: &'static str,
    definitions: [&'static str; 5],
    free_estimate: &'static str,
    own_words: &'static str,
    summary_label: &'static str,
}

impl Words {
    /// PICK-040, PICK-053: the plain definition of a quote format.
    fn definition(&self, format: Format) -> &'static str {
        let i = match format {
            Format::Firm => 0,
            Format::Range => 1,
            Format::Monthly => 2,
            Format::Tiers => 3,
            Format::DiagnosticVisit => 4,
        };
        self.definitions[i]
    }
}

const EN: Words = Words {
    check_yourself: "Ask Hava doesn't rank businesses; check it yourself.",
    independent: "Independent business, not affiliated with Ask Hava.",
    after_hours: "Gets emergency requests at night",
    facts: [
        "Trip fee counts toward the repair:",
        "Parts and materials:",
        "Permit:",
        "Price may change after seeing the job:",
        "Earliest visit:",
        "Speaks Spanish:",
    ],
    trip_fee: ["Yes", "No", "No trip fee"],
    parts: ["Included", "Extra", "Estimated on site"],
    permit: ["Included", "Not included", "Not needed"],
    yes_no: ["Yes", "No"],
    day_part: [
        "{day}, morning",
        "{day}, afternoon",
        "{day}, evening",
        "the night of {day}",
    ],
    spanish: [
        "On the phone",
        "In person",
        "On the phone and in person",
        "No",
    ],
    months: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    date: "{month} {day}",
    definitions: [
        "Firm price: the business's stated price for the work described. Ask for any change in writing before work starts.",
        "Price range: the lowest and highest the business expects to charge for this job. Ask what would push it to the high end.",
        "Recurring price: what the business charges each visit, week or month (the quote says which). Ask what each visit covers.",
        "Packages: options the business offers, each with its own price. You pick one.",
        "Visit fee: what the business charges to come out and look at the job. While there, the business gives you a price for the work.",
    ],
    free_estimate: "Free estimate: the business comes out and prices the job on site.",
    own_words: "The business's own words:",
    summary_label: "Summary by Ask Hava's software. The business's own words above control.",
};

const ES: Words = Words {
    check_yourself: "Ask Hava no califica ni ordena a los negocios; revise este negocio usted mismo.",
    independent: "Negocio independiente, sin relación con Ask Hava.",
    after_hours: "Recibe solicitudes de emergencia de noche",
    facts: [
        "El cargo por venir se descuenta de la reparación:",
        "Piezas y materiales:",
        "Permiso:",
        "El precio puede cambiar después de que el negocio vea el trabajo:",
        "Lo más pronto que puede venir:",
        "Habla español:",
    ],
    trip_fee: ["Sí", "No", "No cobra por venir"],
    parts: ["Incluidos", "Se cobran aparte", "Se calculan en el lugar"],
    permit: ["Incluido", "No incluido", "No hace falta"],
    yes_no: ["Sí", "No"],
    day_part: [
        "{day}, en la mañana",
        "{day}, en la tarde",
        "{day}, en la noche",
        "la noche del {day}",
    ],
    spanish: [
        "Por teléfono",
        "En persona",
        "Por teléfono y en persona",
        "No",
    ],
    months: [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ],
    date: "{day} de {month}",
    definitions: [
        "Precio fijo: el precio que el negocio da por el trabajo descrito. Pida por escrito cualquier cambio antes de que empiece el trabajo.",
        "Rango de precio: el precio más bajo y el más alto que el negocio cobraría por este trabajo. Pregunte qué haría que cueste lo más alto.",
        "Servicio regular: lo que el negocio cobra por cada visita, semana o mes (la cotización dice cuál). Pregunte qué incluye cada visita.",
        "Paquetes: hasta 3 opciones que ofrece el negocio, cada una con su precio. Usted elige una.",
        "Cargo por revisión: lo que el negocio cobra por venir a revisar el trabajo. Ahí mismo, el negocio le da el precio del trabajo.",
    ],
    free_estimate: "Cotización gratis: el negocio viene y le da el precio del trabajo en el lugar.",
    own_words: "Palabras del negocio:",
    summary_label: "Resumen hecho por el software de Ask Hava. Las palabras del negocio arriba son las que valen.",
};

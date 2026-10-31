//! The handyman limits: the $1,000 construction limit judged on the top of any range, the
//! certification box, the gas, smoke-alarm, sprinkler and permit-type block that overrides the AI,
//! the split-job check and the handyman card line (AI-012, LIC-019 to LIC-025, LIC-036).
//!
//! Pure functions, no database. `quotes` calls [`check_quote`] and [`split_job`] when a quote is
//! submitted and [`never_handyman`] (through `rules::eligibility_for_request`) when it chooses
//! recipients. Every refusal is a [`QuoteRefusal`] variant; the words a business reads are the
//! portal's (T148). Amounts are whole cents; nothing here holds a homeowner's detail.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::models::LicenseKind;

/// The handyman limit in cents: a quote must be under $1,000.00 (LIC-019, D-035).
pub const HANDYMAN_LIMIT_CENTS: u64 = 100_000;

/// The split-job window's default, in days; the caller passes the setting (LIC-023, D-027).
pub const SPLIT_JOB_WINDOW_DAYS: i64 = 30;

/// The line every handyman quote card shows, word for word from LIC-024.
pub const HANDYMAN_CARD_LINE: &str =
    "Not a licensed contractor. Only for jobs under $1,000 total with no permit.";

/// Job types that are never handyman work at any price: hardwired smoke alarms and fire
/// sprinklers (LIC-021), matched as words inside a candidate job type.
const NEVER_AT_ANY_PRICE: [(&str, NeverHandyman); 6] = [
    ("smoke alarm", NeverHandyman::SmokeAlarm),
    ("smoke alarms", NeverHandyman::SmokeAlarm),
    ("smoke detector", NeverHandyman::SmokeAlarm),
    ("smoke detectors", NeverHandyman::SmokeAlarm),
    ("fire sprinkler", NeverHandyman::Sprinkler),
    ("fire sprinklers", NeverHandyman::Sprinkler),
];

/// Permit-type job types (LIC-022): the catalog's water heater replacement, panel upgrade, new
/// circuit and re-roof, and the handyman category's refused wall removal.
const PERMIT_TYPE: [&str; 6] = [
    "water heater replacement",
    "panel upgrade",
    "panel",
    "new circuit",
    "re-roof",
    "wall removal",
];

/// The compiled-in gas keyword list (AI-012, D-157).
const GAS_FILE: &str = include_str!("../../../docs/catalog/gas_keywords.yaml");

/// The gas keyword file's shape: its decision and the English and Spanish words.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GasFile {
    /// The decision that set the list.
    #[serde(rename = "decision")]
    _decision: String,
    en: Vec<String>,
    es: Vec<String>,
}

/// Each gas phrase as words; `None` when the file does not parse, which blocks every handyman job.
static GAS_PHRASES: LazyLock<Option<Vec<Vec<String>>>> = LazyLock::new(|| parse_gas(GAS_FILE));

/// Reads a gas keyword file into phrases of words; `None` when it is malformed or empty (AI-012).
pub(crate) fn parse_gas(text: &str) -> Option<Vec<Vec<String>>> {
    let file: GasFile = serde_saphyr::from_str(text).ok()?;
    let phrases: Vec<Vec<String>> = file
        .en
        .iter()
        .chain(&file.es)
        .map(|phrase| words(phrase))
        .filter(|phrase| !phrase.is_empty())
        .collect();
    (!phrases.is_empty()).then_some(phrases)
}

/// Why a job never reaches a handyman, whatever the AI says (AI-012, LIC-021, LIC-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeverHandyman {
    /// A gas keyword in the request or a candidate job type (AI-012, LIC-021).
    Gas,
    /// A hardwired smoke alarm (LIC-021).
    SmokeAlarm,
    /// Fire sprinklers (LIC-021).
    Sprinkler,
    /// A permit-type job (LIC-022).
    PermitType,
}

/// What intake knows of a request: the homeowner's scrubbed words and the candidate job types.
#[derive(Debug, Clone, Copy)]
pub struct JobRequest<'a> {
    /// The scrubbed request text and answers (AI-004).
    pub text: &'a str,
    /// Every candidate job type, from the AI and the keyword rules alike (AI-011).
    pub candidates: &'a [String],
}

/// AI-012, LIC-021, LIC-022: whether a request is work no handyman may take, read from the
/// request text and every candidate job type, never from the AI's handyman reading. Gas comes
/// first; when the gas list cannot be read every request counts as gas (fail closed).
#[must_use]
pub fn never_handyman(request: &JobRequest<'_>) -> Option<NeverHandyman> {
    let texts: Vec<Vec<String>> = std::iter::once(request.text)
        .chain(request.candidates.iter().map(String::as_str))
        .map(words)
        .collect();
    let Some(gas) = GAS_PHRASES.as_ref() else {
        return Some(NeverHandyman::Gas);
    };
    if texts
        .iter()
        .any(|text| gas.iter().any(|p| contains(text, p)))
    {
        return Some(NeverHandyman::Gas);
    }
    let candidates = &texts[1..];
    let at_any_price = NEVER_AT_ANY_PRICE.iter().find(|(phrase, _)| {
        let phrase = words(phrase);
        candidates.iter().any(|c| contains(c, &phrase))
    });
    if let Some((_, reason)) = at_any_price {
        return Some(*reason);
    }
    let permit = request.candidates.iter().any(|c| {
        let c = words(c).join(" ");
        PERMIT_TYPE.iter().any(|p| words(p).join(" ") == c)
    });
    permit.then_some(NeverHandyman::PermitType)
}

/// A quote's price as the business entered it, in cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotePrice {
    /// One price for the whole job.
    Fixed(u64),
    /// A range; judged by its top (LIC-019).
    Range {
        /// The low end.
        low: u64,
        /// The high end.
        high: u64,
    },
    /// A diagnostic-visit quote: no full-job price to test, the certification covers it (D-035).
    TripFee,
}

impl QuotePrice {
    /// The amount the limits judge: the price, or the top of a range (LIC-019); none for a trip fee.
    #[must_use]
    pub fn top(self) -> Option<u64> {
        match self {
            Self::Fixed(amount) => Some(amount),
            Self::Range { low, high } => Some(low.max(high)),
            Self::TripFee => None,
        }
    }
}

/// A quote as submitted on the portal quote form (LIC-025, LIC-036).
#[derive(Debug, Clone, Copy)]
pub struct QuoteSubmission {
    /// The category's `construction_type` flag (LIC-019, LIC-020).
    pub construction_type: bool,
    /// The category's license rule.
    pub license: LicenseKind,
    /// Whether the quoting business works under the handyman exemption.
    pub handyman: bool,
    /// The price entered.
    pub price: QuotePrice,
    /// Whether the certification box on the price screen was ticked (LIC-025, LIC-036).
    pub certified: bool,
}

/// Why a submitted quote is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteRefusal {
    /// A handyman quote without the certification box ticked (LIC-025, LIC-036, D-186).
    NotCertified,
    /// $1,000.00 or more, top of any range, for construction work under the exemption: the portal
    /// explains Arizona's handyman limit (LIC-019, D-035).
    OverHandymanLimit,
    /// Another handyman job of the same business at the same address within the window shares a
    /// job type or together reaches $1,000 (LIC-023).
    SplitJob,
}

/// LIC-019, LIC-020, LIC-024, LIC-025, LIC-036, D-035, D-186: a handyman quote needs the
/// certification box ticked; a construction-type quote from a handyman, or from any business in a
/// no-license construction-type category, is refused at $1,000.00 or more, top of any range. A
/// non-construction category has no limit (a $1,200 car detail, W29).
///
/// # Errors
///
/// [`QuoteRefusal::NotCertified`] or [`QuoteRefusal::OverHandymanLimit`].
pub fn check_quote(quote: &QuoteSubmission) -> Result<(), QuoteRefusal> {
    if quote.handyman && !quote.certified {
        return Err(QuoteRefusal::NotCertified);
    }
    let limited = quote.construction_type && (quote.handyman || quote.license == LicenseKind::None);
    match quote.price.top() {
        Some(top) if limited && top >= HANDYMAN_LIMIT_CENTS => Err(QuoteRefusal::OverHandymanLimit),
        Some(_) | None => Ok(()),
    }
}

/// One handyman job of a business at an address, for the split-job check (LIC-023).
#[derive(Debug, Clone)]
pub struct HandymanJob {
    /// Its job types.
    pub job_types: BTreeSet<String>,
    /// Its price; a trip fee counts as nothing toward the $1,000.
    pub price: QuotePrice,
    /// When its quote was submitted, in seconds since 1970 (UTC).
    pub submitted_at: i64,
}

/// LIC-023, D-027, D-081, D-154: run when `new` is submitted, over the same business's other
/// handyman jobs at the same address (`quotes` selects them). Blocked when one lies less than
/// `window_days` away and shares a job type, or the two tops together reach $1,000.
///
/// # Errors
///
/// [`QuoteRefusal::SplitJob`] when an earlier job blocks it.
pub fn split_job(
    new: &HandymanJob,
    others: &[HandymanJob],
    window_days: i64,
) -> Result<(), QuoteRefusal> {
    let window = window_days.saturating_mul(86_400);
    let total = |job: &HandymanJob| job.price.top().unwrap_or(0);
    let blocks = |other: &HandymanJob| {
        let near = new.submitted_at.abs_diff(other.submitted_at) < window.unsigned_abs();
        let shared = !new.job_types.is_disjoint(&other.job_types);
        near && (shared || total(new).saturating_add(total(other)) >= HANDYMAN_LIMIT_CENTS)
    };
    if others.iter().any(blocks) {
        Err(QuoteRefusal::SplitJob)
    } else {
        Ok(())
    }
}

/// Lower-case words, curly apostrophes read as straight ones; an apostrophe inside a word stays.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .map(|word| word.trim_matches(|c| c == '\'' || c == '-'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `phrase` appears in `text` as consecutive whole words.
fn contains(text: &[String], phrase: &[String]) -> bool {
    !phrase.is_empty() && text.windows(phrase.len()).any(|window| window == phrase)
}

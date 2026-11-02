//! What `ai` sends and what it accepts back (AI-005, AI-010, AI-021, AI-022, AI-025).
//!
//! The intake reading's shape comes from one file, `schemas/intake_reading.json` (T167's, D-493):
//! the request's `output_config` sends it as committed, [`FIELDS`] lists it for the Rust type and
//! `tests/schemas.rs` fails on any difference among the three (AI-022). A reply is used only when
//! it parses into exactly those fields (serde `deny_unknown_fields`), its confidences lie in 0 to
//! 1, and its trimmed, lower-cased trade and job type are open in the catalog (AI-005). Every
//! AI-written text passes [`screen_text`] before use, or the caller's fixed template goes instead
//! (AI-021). Homeowner and quote text go inside one fixed tag with that tag's name escaped in
//! the text (AI-025).

use std::collections::{BTreeMap, BTreeSet};

use askhava_catalog::api::restricted_match;
use askhava_privacy::api::is_format;
use serde::Deserialize;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use unicode_normalization::UnicodeNormalization;

/// The committed schema file, compiled in (AI-022).
pub(crate) const SCHEMA_FILE: &str = include_str!("../../../schemas/intake_reading.json");

/// The schema sent as `output_config.format.schema`: the file as committed (AI-022, D-812).
pub(crate) fn intake_schema() -> Value {
    serde_json::from_str(SCHEMA_FILE).unwrap_or(Value::Null)
}

/// The JSON-schema shape of one field of the Rust reading type (AI-022).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Text,
    Confidence,
    Flag,
    Language,
    TextList,
}

#[cfg(test)]
impl FieldKind {
    /// The field's schema, as the file must hold it (AI-022).
    pub(crate) fn schema(self) -> Value {
        match self {
            Self::Text => json!({"type": "string"}),
            Self::Confidence => json!({"type": "number", "minimum": 0, "maximum": 1}),
            Self::Flag => json!({"type": "boolean"}),
            Self::Language => json!({"type": "string", "enum": ["en", "es"]}),
            Self::TextList => json!({"type": "array", "items": {"type": "string"}}),
        }
    }
}

/// The fields of [`RawReading`], in the file's order (AI-010, AI-022); `tests/schemas.rs`
/// checks them against the file and the type.
#[cfg(test)]
pub(crate) const FIELDS: [(&str, FieldKind); 9] = [
    ("trade", FieldKind::Text),
    ("trade_confidence", FieldKind::Confidence),
    ("job_type", FieldKind::Text),
    ("job_type_confidence", FieldKind::Confidence),
    ("emergency", FieldKind::Flag),
    ("urgent", FieldKind::Flag),
    ("language", FieldKind::Language),
    ("major", FieldKind::Flag),
    ("missing_info", FieldKind::TextList),
];

/// The reply as the model wrote it; any missing or extra field fails (AI-005, AI-022).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawReading {
    trade: String,
    trade_confidence: f64,
    job_type: String,
    job_type_confidence: f64,
    emergency: bool,
    urgent: bool,
    language: String,
    major: bool,
    missing_info: Vec<String>,
}

/// The language a request was written in (AI-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// English.
    En,
    /// Spanish.
    Es,
}

/// A checked intake reading (AI-005, AI-010). `emergency` and `urgent` are recorded only and
/// change nothing (INT-026). Built only by this crate.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct IntakeReading {
    /// The category code, open in the catalog.
    pub trade: String,
    /// 0 to 1.
    pub trade_confidence: f64,
    /// A job type of `trade`, lower-case.
    pub job_type: String,
    /// 0 to 1.
    pub job_type_confidence: f64,
    /// The AI's emergency reading, recorded only (INT-026).
    pub emergency: bool,
    /// The AI's urgency reading, recorded only (INT-026).
    pub urgent: bool,
    /// The request's language.
    pub language: Language,
    /// Whether the job reads as major work.
    pub major: bool,
    /// What the request leaves out: at most [`MAX_MISSING_INFO`] items, each through [`screen_text`]
    /// (an item that fails is dropped), trimmed, lower-case, at most 64 characters. For records
    /// and question choice only: never sent to a person or put into a template (D-814).
    pub missing_info: Vec<String>,
}

/// The categories open in the catalog with their job types, lower-case (AI-005).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenTrades(BTreeMap<String, BTreeSet<String>>);

impl OpenTrades {
    /// The open trades from `(category code, job type names)` pairs, normalized as replies are (AI-005).
    pub fn new<I: IntoIterator<Item = (String, Vec<String>)>>(entries: I) -> Self {
        Self(
            entries
                .into_iter()
                .map(|(trade, jobs)| {
                    (
                        normalize(&trade),
                        jobs.iter().map(|j| normalize(j)).collect(),
                    )
                })
                .collect(),
        )
    }

    fn allows(&self, trade: &str, job_type: &str) -> bool {
        self.0
            .get(trade)
            .is_some_and(|jobs| jobs.contains(job_type))
    }
}

/// Why a reply was thrown away (AI-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Discard {
    /// It broke the schema: a missing, extra or mistyped field, or a value out of range.
    Schema,
    /// Its trade is not open, or its job type is not under that trade.
    Catalog,
}

/// Trims and lower-cases an enum value before matching (AI-005).
pub(crate) fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

/// The most `missing_info` items a reading may hold; more discards it (AI-005, D-814).
pub(crate) const MAX_MISSING_INFO: usize = 8;

/// Parses the model's JSON text into a checked reading (AI-005, AI-010, AI-022).
pub(crate) fn parse_reading(text: &str, open: &OpenTrades) -> Result<IntakeReading, Discard> {
    let raw: RawReading = serde_json::from_str(text).map_err(|_| Discard::Schema)?;
    let in_range = |c: f64| (0.0..=1.0).contains(&c);
    let language = match normalize(&raw.language).as_str() {
        "en" => Language::En,
        "es" => Language::Es,
        _ => return Err(Discard::Schema),
    };
    let missing_info: Vec<String> = raw
        .missing_info
        .iter()
        .filter_map(|m| match screen_text(TextKind::Question, m) {
            Screened::Use(text) => Some(normalize(&text)),
            Screened::Fallback(_) => None,
        })
        .collect();
    if !in_range(raw.trade_confidence)
        || !in_range(raw.job_type_confidence)
        || raw.missing_info.len() > MAX_MISSING_INFO
        || missing_info.iter().any(|m| m.chars().count() > 64)
    {
        return Err(Discard::Schema);
    }
    let (trade, job_type) = (normalize(&raw.trade), normalize(&raw.job_type));
    if !open.allows(&trade, &job_type) {
        return Err(Discard::Catalog);
    }
    Ok(IntakeReading {
        trade,
        trade_confidence: raw.trade_confidence,
        job_type,
        job_type_confidence: raw.job_type_confidence,
        emergency: raw.emergency,
        urgent: raw.urgent,
        language,
        major: raw.major,
        missing_info,
    })
}

/// The fixed tag a text is sent inside (AI-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// `<homeowner_message>`, for homeowner text.
    HomeownerMessage,
    /// `<business_quote>`, for quote text.
    BusinessQuote,
}

impl Tag {
    /// The tag's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::HomeownerMessage => "homeowner_message",
            Self::BusinessQuote => "business_quote",
        }
    }
}

/// `text` inside one `tag`, after `&`, `<` and `>` become entities and every occurrence of the
/// tag's name, in any case, has its `_` written as `&#95;`, so the text can neither close the tag
/// nor name it (AI-025, AI-007).
pub(crate) fn wrap(tag: Tag, text: &str) -> String {
    let name = tag.name();
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let lower = escaped.to_ascii_lowercase();
    let mut out = String::with_capacity(escaped.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(name) {
        let start = at + found;
        out.push_str(&escaped[at..start]);
        out.push_str(&escaped[start..start + name.len()].replace('_', "&#95;"));
        at = start + name.len();
    }
    out.push_str(&escaped[at..]);
    format!("<{name}>\n{out}\n</{name}>")
}

/// Which AI-written text is screened, setting its length cap (AI-021, D-811).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// A reworded bank question or the one follow-up: at most 320 characters (two text segments).
    Question,
    /// The job card text: at most 1,200 characters.
    JobCard,
    /// The project file: at most 4,000 characters.
    ProjectFile,
}

impl TextKind {
    /// The length cap in characters (AI-021, D-811).
    #[must_use]
    pub fn cap(self) -> usize {
        match self {
            Self::Question => 320,
            Self::JobCard => 1200,
            Self::ProjectFile => 4000,
        }
    }
}

/// Why an AI-written text was refused (AI-021). Names the list entry, never the text (D-485).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenHit {
    /// A carrier-restricted phrase (CARD-010), by its list entry.
    Restricted(&'static str),
    /// A link or bare domain.
    Url,
    /// Longer than the kind's cap.
    TooLong,
}

/// The result of the AI-021 screen: the AI's text, or the hit that sends the fixed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screened {
    /// The text passed and may be used.
    Use(String),
    /// The text failed; the caller sends its fixed template.
    Fallback(ScreenHit),
}

impl Screened {
    /// The text to send: the AI's when it passed, `template` otherwise (AI-021).
    #[must_use]
    pub fn or_template(self, template: &str) -> String {
        match self {
            Self::Use(text) => text,
            Self::Fallback(_) => template.to_owned(),
        }
    }
}

/// Screens one AI-written text for the CARD-010 list, links and the length cap (AI-021). The
/// screen runs on a normalized copy (NFKC, format characters dropped, ideographic full stops as
/// `.`), and that copy is the text returned, so what was checked is what is sent (D-814).
#[must_use]
pub fn screen_text(kind: TextKind, text: &str) -> Screened {
    let text = screen_normalize(text);
    if text.chars().count() > kind.cap() {
        return Screened::Fallback(ScreenHit::TooLong);
    }
    if let Some(entry) = restricted_match(&text) {
        return Screened::Fallback(ScreenHit::Restricted(entry));
    }
    if text.split_whitespace().any(is_link) {
        return Screened::Fallback(ScreenHit::Url);
    }
    Screened::Use(text)
}

/// NFKC, Unicode format characters (category Cf) dropped and ideographic full stops written
/// `.`, as `privacy`'s scrubber normalizes, with its `is_format` list (AI-020, AI-021, D-814).
fn screen_normalize(text: &str) -> String {
    text.nfkc()
        .filter(|c| !is_format(*c))
        .map(|c| if c == '\u{3002}' { '.' } else { c })
        .collect()
}

/// A word that is or holds a link: a scheme, `www.`, or a bare domain (last label 2 to 24
/// letters) in any piece of it cut at `/`, `?`, `#`, `:`, `@` or `,`, so `evil.com/pay` and
/// `bit.ly?x=1` are caught.
fn is_link(word: &str) -> bool {
    let word = word.to_lowercase();
    if word.contains("://") || word.contains("www.") {
        return true;
    }
    word.split(['/', '?', '#', ':', '@', ','])
        .any(|piece| is_domain(piece.trim_matches(|c: char| !c.is_alphanumeric())))
}

fn is_domain(piece: &str) -> bool {
    let labels: Vec<&str> = piece.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty())
        && labels.last().is_some_and(|tld| {
            (2..=24).contains(&tld.len()) && tld.bytes().all(|b| b.is_ascii_lowercase())
        })
}

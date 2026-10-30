//! The template registry: fixed English and Spanish texts chosen by ID and language (TXT-001 to
//! TXT-004, TXT-039, BIZ-018, WORD-008).
//!
//! The two files `templates/en.json` and `templates/es.json` are compiled in. Each key is a template
//! ID; each entry holds the text, the audience, the `segments` count recorded for TXT-044 and, in
//! Spanish, the draft mark and whether the text stays on while Spanish conversations are off. Later
//! tasks add only their own keys, in both files. Placeholders are `{name}`. Parsing refuses a registry
//! where a kind lacks a language, a text does not begin "Ask Hava:" or a Spanish text is not a draft.
//! The link, emoji and wording scan is `tests/docs/test_template_scan.py`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// Every text the app composes begins with this (WORD-008, D-064).
const PREFIX: &str = "Ask Hava:";

/// A template language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// English, the default for every person (TXT-039).
    En,
    /// Spanish, a draft until the native-speaker review (TXT-003).
    Es,
}

/// Who a template is for.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    /// A homeowner text.
    Homeowner,
    /// A business notice (English unless that person picked Spanish, BIZ-018).
    Business,
}

/// One template as stored in the JSON files.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    audience: Audience,
    segments: usize,
    text: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    spanish_stays_on: bool,
}

/// A rendered text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// The template ID.
    pub id: String,
    /// The language actually used.
    pub language: Language,
    /// The text to send.
    pub body: String,
    /// True for a Spanish template awaiting native-speaker review (TXT-003).
    pub draft: bool,
}

/// How a text is encoded on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// The GSM 03.38 alphabet: 160 characters in one segment, 153 in each of several.
    Gsm7,
    /// Unicode: 70 characters in one segment, 67 in each of several.
    Ucs2,
}

/// The cost of a text (TXT-044).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segments {
    /// The encoding the text needs.
    pub encoding: Encoding,
    /// The number of segments billed.
    pub segments: usize,
}

/// Why a template could not be used.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum TemplateError {
    /// No template has this ID.
    #[error("no template with ID {0}")]
    Unknown(String),
    /// A placeholder had no value.
    #[error("template {id} needs a value for {name}")]
    MissingValue {
        /// The template ID.
        id: String,
        /// The placeholder name.
        name: String,
    },
    /// The template files break a rule.
    #[error("template files invalid: {0}")]
    Invalid(String),
}

/// The English and Spanish templates, checked.
#[derive(Debug)]
pub struct Registry {
    en: BTreeMap<String, Entry>,
    es: BTreeMap<String, Entry>,
}

static EMBEDDED: OnceLock<Result<Registry, TemplateError>> = OnceLock::new();

impl Registry {
    /// The registry compiled into the build.
    ///
    /// # Errors
    ///
    /// [`TemplateError::Invalid`] if a rule of [`Registry::parse`] is broken.
    pub fn embedded() -> Result<&'static Registry, TemplateError> {
        EMBEDDED
            .get_or_init(|| {
                Registry::parse(
                    include_str!("../templates/en.json"),
                    include_str!("../templates/es.json"),
                )
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Parses and checks the two files: the same IDs in both (TXT-001), every text begins "Ask Hava:"
    /// (WORD-008), every Spanish template is a draft (TXT-003) and no English one is.
    ///
    /// # Errors
    ///
    /// [`TemplateError::Invalid`] naming the first broken rule.
    pub fn parse(en_json: &str, es_json: &str) -> Result<Registry, TemplateError> {
        let read = |json: &str, which: &str| {
            serde_json::from_str::<BTreeMap<String, Entry>>(json)
                .map_err(|e| TemplateError::Invalid(format!("{which}: {e}")))
        };
        let registry = Registry {
            en: read(en_json, "en.json")?,
            es: read(es_json, "es.json")?,
        };
        registry.check().map_err(TemplateError::Invalid)?;
        Ok(registry)
    }

    fn check(&self) -> Result<(), String> {
        if let Some(id) = self.en.keys().find(|id| !self.es.contains_key(*id)) {
            return Err(format!("{id} has no Spanish template"));
        }
        for (id, es) in &self.es {
            let en = self
                .en
                .get(id)
                .ok_or_else(|| format!("{id} has no English template"))?;
            if en.draft || !es.draft {
                return Err(format!("{id}: Spanish must be a draft and English not"));
            }
            if en.audience != es.audience {
                return Err(format!("{id}: audience differs between languages"));
            }
        }
        match self
            .en
            .values()
            .chain(self.es.values())
            .find(|e| !e.text.starts_with(PREFIX))
        {
            Some(e) => Err(format!("text does not begin \"{PREFIX}\": {}", e.text)),
            None => Ok(()),
        }
    }

    /// Every template ID, sorted.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.en.keys().cloned().collect()
    }

    /// The audience of a template.
    #[must_use]
    pub fn audience(&self, id: &str) -> Option<Audience> {
        self.en.get(id).map(|e| e.audience)
    }

    /// The raw template text and its recorded segment count (TXT-044).
    #[must_use]
    pub fn template(&self, id: &str, language: Language) -> Option<(&str, usize)> {
        let map = if language == Language::Es {
            &self.es
        } else {
            &self.en
        };
        map.get(id).map(|e| (e.text.as_str(), e.segments))
    }

    /// The language a person gets: Spanish only when wanted and either Spanish conversations are on
    /// or the Spanish template stays on (TXT-039); English otherwise, including for every business
    /// notice while the setting is off (BIZ-018).
    ///
    /// # Errors
    ///
    /// [`TemplateError::Unknown`] for an ID not in the registry.
    pub fn language_for(
        &self,
        id: &str,
        wanted: Language,
        spanish_on: bool,
    ) -> Result<Language, TemplateError> {
        let es = self
            .es
            .get(id)
            .ok_or_else(|| TemplateError::Unknown(id.to_owned()))?;
        let spanish = wanted == Language::Es && (spanish_on || es.spanish_stays_on);
        Ok(if spanish { Language::Es } else { Language::En })
    }

    /// Renders a template (TXT-004); see [`crate::api::render`].
    ///
    /// # Errors
    ///
    /// [`TemplateError::Unknown`] or [`TemplateError::MissingValue`].
    pub fn render(
        &self,
        id: &str,
        wanted: Language,
        spanish_on: bool,
        values: &[(&str, &str)],
    ) -> Result<Text, TemplateError> {
        let language = self.language_for(id, wanted, spanish_on)?;
        let (raw, _) = self
            .template(id, language)
            .ok_or_else(|| TemplateError::Unknown(id.to_owned()))?;
        let body = fill(id, raw, values)?;
        Ok(Text {
            id: id.to_owned(),
            language,
            body,
            draft: language == Language::Es,
        })
    }
}

/// Replaces each `{name}` once, in a single pass, so a value containing braces is never expanded.
fn fill(id: &str, raw: &str, values: &[(&str, &str)]) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((before, after)) = rest.split_once('{') {
        let Some((name, tail)) = after.split_once('}') else {
            break;
        };
        let value = values.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        let missing = || TemplateError::MissingValue {
            id: id.to_owned(),
            name: name.to_owned(),
        };
        out.push_str(before);
        out.push_str(value.ok_or_else(missing)?);
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// Units a character takes in GSM-7: 1 for the basic set, 2 for the extension table, `None` outside it.
fn gsm_units(c: char) -> Option<usize> {
    const BASIC: &str = "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡\
                         ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà";
    const EXTENDED: &str = "^{}\\[~]|€\u{c}";
    if BASIC.contains(c) {
        Some(1)
    } else if EXTENDED.contains(c) {
        Some(2)
    } else {
        None
    }
}

/// Counts the encoding and segments of a text with a GSM-7 counter (TXT-044, CON-031).
#[must_use]
pub fn count_segments(text: &str) -> Segments {
    let gsm: Option<usize> = text.chars().map(gsm_units).sum();
    let (encoding, units, single, multi) = match gsm {
        Some(n) => (Encoding::Gsm7, n, 160, 153),
        None => (Encoding::Ucs2, text.encode_utf16().count(), 70, 67),
    };
    let segments = if units <= single {
        1
    } else {
        units.div_ceil(multi)
    };
    Segments { encoding, segments }
}

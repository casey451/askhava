//! Reads and checks `docs/catalog/question_bank.yaml`, each category's versioned intake question set
//! (LIC-040, TD-LIC-24).
//!
//! The file is compiled in. Parsing is pure and refuses the same malformed sets that
//! `scripts/check_catalog.py` rejects: a set outside 3 to 8 questions, a question missing either
//! language, unequal English and Spanish option lists, a free-text card field, a size band without one
//! `band_floor` per option, a repeated question id, a condition on a question that does not come
//! earlier or names an answer number beyond the options (plus the extra "Not sure" answer), more than 3
//! photo asks, photos without a `kb_trade`, and a set whose id covers a warranty or guarantee
//! (LIC-042). The `kb_trade` file and photo-id checks stay in the script, which can read the trade files.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::loader::covers_warranty;
use crate::models::{Catalog, CatalogError};

/// The reviewed question bank, compiled in.
const EMBEDDED: &str = include_str!("../../../docs/catalog/question_bank.yaml");

/// The fewest and most questions a set may hold (LIC-040).
const QUESTIONS: std::ops::RangeInclusive<usize> = 3..=8;
/// The most photo asks a set may list (LIC-040).
const MAX_PHOTOS: usize = 3;

/// A condition on an earlier answer: ask only when that question got one of these answer numbers.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AskIf {
    /// The id of an earlier question in the same set.
    pub question: String,
    /// The answer numbers (1-based) that make this question appear.
    pub answers: Vec<u32>,
}

/// One intake question (LIC-040): a multiple-choice question carries both option lists; a free-text
/// question carries neither and can never be a card field.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Question {
    /// Unique within the set.
    pub id: String,
    /// The English template.
    pub en: String,
    /// The Spanish draft, marked for native-speaker review (TXT-003).
    pub es: String,
    /// English answers; absent for a free-text question.
    pub options_en: Option<Vec<String>>,
    /// Spanish answers, the same count as the English ones.
    pub options_es: Option<Vec<String>>,
    /// True when the answer goes on the job card (CARD-007); multiple choice only.
    #[serde(default)]
    pub card_field: bool,
    /// True when the answer is a size or budget band (BIZ-049).
    #[serde(default)]
    pub size_band: bool,
    /// For a size band, the lowest dollar amount of each option as decimal text, one per option (BIZ-049).
    #[serde(default)]
    pub band_floor: Vec<String>,
    /// True when asked even on an emergency job (INT-039).
    #[serde(default)]
    pub emergency: bool,
    /// The condition on an earlier answer, if any.
    pub ask_if: Option<AskIf>,
}

/// One category's versioned set (LIC-040).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct QuestionSet {
    /// The set's version.
    pub version: u32,
    /// The knowledge-base trade it draws on, where one exists.
    pub kb_trade: Option<String>,
    /// Photo asks, at most 3, asked after the questions.
    #[serde(default)]
    pub photos: Vec<String>,
    /// The questions in the order they are asked.
    pub questions: Vec<Question>,
}

/// The whole file: the sets by category code (LIC-040).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct QuestionBank {
    /// The bank's version.
    pub version: u32,
    /// The sets, keyed by category code.
    pub sets: BTreeMap<String, QuestionSet>,
}

/// The bank compiled into the build, checked against `catalog` (LIC-040).
///
/// # Errors
///
/// [`CatalogError::Invalid`] if the compiled-in file breaks a rule of [`parse_bank`].
pub fn embedded_bank(catalog: &Catalog) -> Result<QuestionBank, CatalogError> {
    parse_bank(EMBEDDED, catalog)
}

/// Parses and checks a question bank against the catalog: every set is well formed, names a category
/// of the catalog, and every enabled category has a set (LIC-040, LIC-042).
///
/// # Errors
///
/// [`CatalogError::Invalid`] for a file that does not parse or breaks a rule above.
pub fn parse_bank(text: &str, catalog: &Catalog) -> Result<QuestionBank, CatalogError> {
    let bank: QuestionBank = serde_saphyr::from_str(text).map_err(|_| CatalogError::Invalid)?;
    let known: BTreeSet<&str> = catalog.categories.iter().map(|c| c.id.as_str()).collect();
    let all_known = bank.sets.keys().all(|k| known.contains(k.as_str()));
    let all_enabled_have_sets = catalog
        .categories
        .iter()
        .filter(|c| c.enabled)
        .all(|c| bank.sets.contains_key(&c.id));
    let sets_ok = bank
        .sets
        .iter()
        .all(|(code, set)| !covers_warranty(code) && set_ok(set));
    if all_known && all_enabled_have_sets && sets_ok {
        Ok(bank)
    } else {
        Err(CatalogError::Invalid)
    }
}

/// One set's shape (LIC-040).
fn set_ok(set: &QuestionSet) -> bool {
    QUESTIONS.contains(&set.questions.len())
        && set.photos.len() <= MAX_PHOTOS
        && set.version >= 1
        && (set.kb_trade.is_some() || set.photos.is_empty())
        && set.questions.iter().enumerate().all(|(i, q)| {
            let earlier = &set.questions[..i];
            question_ok(q) && ask_if_ok(q, earlier) && earlier.iter().all(|e| e.id != q.id)
        })
}

/// One question's languages and options: both languages present, option lists equal and non-empty,
/// and a card field only when multiple choice (LIC-040, CARD-007).
fn question_ok(q: &Question) -> bool {
    let options = match (&q.options_en, &q.options_es) {
        (Some(en), Some(es)) => {
            en.len() == es.len()
                && !en.is_empty()
                && filled(en)
                && filled(es)
                && (!q.size_band || q.band_floor.len() == en.len())
        }
        (None, None) => !q.card_field && !q.size_band,
        _ => false,
    };
    options && !q.id.trim().is_empty() && !q.en.trim().is_empty() && !q.es.trim().is_empty()
}

/// True when no entry is blank.
fn filled(items: &[String]) -> bool {
    items.iter().all(|s| !s.trim().is_empty())
}

/// A condition must name an earlier question and at least one of its answer numbers; the numbers run
/// one past the options because the app adds a "Not sure" answer (D-754).
fn ask_if_ok(q: &Question, earlier: &[Question]) -> bool {
    q.ask_if.as_ref().is_none_or(|c| {
        !c.answers.is_empty()
            && earlier.iter().any(|e| {
                e.id == c.question
                    && e.options_en.as_ref().is_some_and(|o| {
                        c.answers
                            .iter()
                            .all(|&a| a >= 1 && a as usize <= o.len() + 1)
                    })
            })
    })
}

//! The AI-019 code checks every AI-written question, job card and project file passes (AI-019,
//! AI-021, CARD-008, WORD-003, WORD-022, D-492, D-493).
//!
//! [`check_output`] runs, in order: the AI-021 screen (length cap, CARD-010 list, URL ban), then
//! on a folded copy (accents dropped, lower-cased, spaces collapsed) dollar amounts and price
//! words, the English and Spanish cause words, the WORD-022 phrases of
//! `data/word_022_phrases.json` (kept equal to `scripts/trace_spec.py` by
//! `tests/docs/test_word_022_phrases.py`), the WORD-003 claims, the business names the caller
//! passes (D-493: `ai` cannot call `businesses`), a fact absent from her own words (a number or a
//! capitalized name), and last the CARD-008 contact shapes. Any hit refuses the whole text; the
//! writer then uses the fixed template or her scrubbed words (D-988).

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::schemas::{ScreenHit, Screened, TextKind, screen_text};

/// The WORD-022 phrase file, compiled in (WORD-022, D-493).
const WORD_022_FILE: &str = include_str!("../data/word_022_phrases.json");

/// Dollar and price words, folded (AI-019).
const PRICE_WORDS: &[&str] = &[
    "usd",
    "dollar",
    "dollars",
    "buck",
    "bucks",
    "dolar",
    "dolares",
    "price",
    "prices",
    "priced",
    "pricing",
    "cost",
    "costs",
    "costing",
    "estimate",
    "estimates",
    "estimated",
    "cheap",
    "expensive",
    "precio",
    "precios",
    "costo",
    "costos",
    "cuesta",
    "cuestan",
    "barato",
];
/// The AI-019 cause words, English and Spanish, folded (AI-019, D-492).
const CAUSE_WORDS: &[&str] = &[
    "probably",
    "likely",
    "sounds like",
    "needs a new",
    "the problem is",
    "probablemente",
    "parece que",
    "seguramente",
    "suena a",
    "el problema es",
    "necesita un nuevo",
    "necesita una nueva",
];
/// The WORD-003 claims, folded (WORD-003).
const CLAIM_WORDS: &[&str] = &[
    "vetted",
    "background-checked",
    "background checked",
    "best",
    "top-rated",
    "top rated",
    "el mejor",
    "la mejor",
    "los mejores",
    "las mejores",
];
/// CARD-008 contact phrases, folded (CARD-008).
const CONTACT_PHRASES: &[&str] = &[
    "text me",
    "call me",
    "email me",
    "message me",
    "dm me",
    "reach me",
    "contact me",
    "ask for",
    "my number",
    "my phone",
    "my cell",
    "my email",
    "whatsapp",
    "llamame",
    "mandame",
    "escribeme",
    "mi numero",
    "mi telefono",
    "mi celular",
    "mi correo",
    "pregunte por",
    "pregunta por",
];
/// Number words a phone number can be spelled in, folded (CARD-008).
const DIGIT_WORDS: &[&str] = &[
    "zero", "oh", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "cero",
    "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve",
];
/// A run of this many number words is a contact detail (CARD-008, D-988).
const DIGIT_WORD_RUN: usize = 3;
/// This many digits in a run, separators allowed, is a contact detail (CARD-008).
const DIGIT_RUN: usize = 7;

/// Why an AI-written text was refused (AI-019, AI-021). Names the check, never the text (D-485).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckHit {
    /// The AI-021 screen: length cap, CARD-010 list or link.
    Screen(ScreenHit),
    /// A dollar amount or a price word.
    Price,
    /// A cause word, English or Spanish.
    Cause,
    /// A WORD-022 phrase (or the phrase file failed to load: fail closed).
    NotAContractor,
    /// A WORD-003 claim.
    Claim,
    /// A business name the caller passed.
    BusinessName,
    /// A number or capitalized name absent from her words, answers and photo notes.
    AddedFact,
    /// A CARD-008 contact shape: 7 or more digits, number words, an email sign or a contact phrase.
    Contact,
}

/// What an AI-written text is checked against: everything she gave and the names it must not hold.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Against<'a> {
    pub(crate) sources: &'a [&'a str],
    pub(crate) business_names: &'a [String],
}

/// Runs every AI-019 and AI-021 check on `text`; returns the screened text or the first hit.
pub(crate) fn check_output(
    kind: TextKind,
    text: &str,
    against: &Against<'_>,
) -> Result<String, CheckHit> {
    let text = match screen_text(kind, text) {
        Screened::Use(text) => text,
        Screened::Fallback(hit) => return Err(CheckHit::Screen(hit)),
    };
    let folded = fold(&text);
    let any = |list: &[&str]| list.iter().any(|p| has_phrase(&folded, p));
    if text.contains('$') || any(PRICE_WORDS) {
        return Err(CheckHit::Price);
    }
    if any(CAUSE_WORDS) {
        return Err(CheckHit::Cause);
    }
    if word_022().is_none_or(|list| list.iter().any(|p| has_phrase(&folded, p))) {
        return Err(CheckHit::NotAContractor);
    }
    if any(CLAIM_WORDS) {
        return Err(CheckHit::Claim);
    }
    let names = against.business_names.iter().map(|n| fold(n));
    if names
        .filter(|n| !n.is_empty())
        .any(|n| has_phrase(&folded, &n))
    {
        return Err(CheckHit::BusinessName);
    }
    if adds_fact(&text, against.sources) {
        return Err(CheckHit::AddedFact);
    }
    if is_contact(&text, &folded) {
        return Err(CheckHit::Contact);
    }
    Ok(text)
}

/// Accents dropped, lower-cased, curly apostrophes straightened and whitespace collapsed, so
/// "PROBABLEMENTE", "Probablemente" and "próbablemente" fold alike (AI-019, D-492).
pub(crate) fn fold(text: &str) -> String {
    let plain: String = text
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .flat_map(char::to_lowercase)
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `phrase` occurs in `hay` between word edges; a trailing `*` lets word letters follow.
fn has_phrase(hay: &str, phrase: &str) -> bool {
    let (stem, wild) = phrase
        .strip_suffix('*')
        .map_or((phrase, false), |s| (s, true));
    hay.match_indices(stem).any(|(at, _)| {
        let before = hay[..at].chars().next_back().is_none_or(|c| !is_word(c));
        let after = hay[at + stem.len()..].chars().next();
        before && (wild || after.is_none_or(|c| !is_word(c)))
    })
}

#[derive(Deserialize)]
struct PhraseFile {
    patterns: Vec<PhrasePattern>,
}

#[derive(Deserialize)]
struct PhrasePattern {
    slots: Vec<Vec<String>>,
}

/// The WORD-022 phrases, expanded from their slots and folded once; `None` if the file is broken.
pub(crate) fn word_022() -> Option<&'static [String]> {
    static PHRASES: OnceLock<Option<Vec<String>>> = OnceLock::new();
    PHRASES
        .get_or_init(|| {
            let file: PhraseFile = serde_json::from_str(WORD_022_FILE).ok()?;
            let mut out = Vec::new();
            for pattern in file.patterns {
                let mut phrases = vec![String::new()];
                for slot in &pattern.slots {
                    phrases = phrases
                        .iter()
                        .flat_map(|p| slot.iter().map(move |alt| format!("{p}{alt}")))
                        .collect();
                }
                out.extend(phrases.iter().map(|p| fold(p)));
            }
            (!out.is_empty()).then_some(out)
        })
        .as_deref()
}

/// AI-019's "adds no fact": a digit run, or a capitalized word not starting a sentence, that her
/// words, answers and photo notes do not hold (D-988).
fn adds_fact(text: &str, sources: &[&str]) -> bool {
    let known: HashSet<String> = sources
        .iter()
        .map(|s| fold(s))
        .flat_map(|s| {
            let words = s.split(|c| !is_word(c));
            let digits = s.split(|c: char| !c.is_ascii_digit());
            words.chain(digits).map(str::to_owned).collect::<Vec<_>>()
        })
        .collect();
    let unknown = |w: &str| !w.is_empty() && !known.contains(&fold(w));
    let numbers = text.split(|c: char| !c.is_ascii_digit());
    if numbers.filter(|n| !n.is_empty()).any(unknown) {
        return true;
    }
    text.lines().any(|line| {
        let mut starts = true;
        line.split_whitespace().any(|token| {
            let word = token.trim_matches(|c: char| !is_word(c));
            let named = !starts
                && word.chars().next().is_some_and(char::is_uppercase)
                && !["I", "Ask", "Hava"].contains(&word);
            starts = token.ends_with(['.', '!', '?', ':']);
            named && unknown(word)
        })
    })
}

/// CARD-008's shapes: 7 or more digits (spaces, dashes, dots and brackets between), a run of
/// number words, an email sign or a contact phrase.
fn is_contact(text: &str, folded: &str) -> bool {
    let mut run = 0;
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
        } else if !" -.()".contains(c) {
            run = 0;
        }
        if run >= DIGIT_RUN {
            return true;
        }
    }
    let mut words = 0;
    for word in folded
        .split(|c: char| !is_word(c))
        .filter(|w| !w.is_empty())
    {
        words = if DIGIT_WORDS.contains(&word) {
            words + 1
        } else {
            0
        };
        if words >= DIGIT_WORD_RUN {
            return true;
        }
    }
    text.contains('@') || CONTACT_PHRASES.iter().any(|p| has_phrase(folded, p))
}

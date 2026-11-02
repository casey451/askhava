//! The TXT-040 check on the homeowner's update texts after intake: short sentences, the link to
//! her private quote page (PICK-010), and no quote details outside the LIST reply (TXT-031) and
//! the lapsed-pick text (PICK-046) (WORD-024, D-438, D-477, D-478, D-482).
//!
//! [`UPDATE_TEXTS`] names every homeowner update template after intake that tells her about her
//! quotes, with its [`Shape`]. A sentence no requirement fixes has at most [`SHORT`] words; every
//! sentence, fixed or not, at most [`LIMIT`]. A sentence ends at ".", "?" or "!" before a space or
//! the end, or at a line break; the "Ask Hava:" opening (WORD-008) is not counted and a `{name}`
//! placeholder or a link is one word. [`check`] judges one text and [`scan`] every listed template
//! in both languages; `src/tests/update_texts.rs` runs both (TD-TXT-25).
//!
//! The lapsed-pick text (PICK-046) has no template yet; its owner adds it here as a
//! [`Shape::List`] entry, since it lists the other quotes.

use askhava_messaging::api::{Language, Registry};

use crate::models::QuotesError;

/// The most words in a sentence no requirement fixes (WORD-024, TXT-040).
pub const SHORT: usize = 15;
/// The most words in any sentence (WORD-029's 20-word limit, D-482).
pub const LIMIT: usize = 20;
/// The opening every text has (WORD-008); it is not part of a sentence's count.
const OPENING: &str = "Ask Hava:";
/// The placeholders an update text may fill: the request number, the counts and her page link.
/// Anything else (an amount, a business, a date) is a quote detail (TXT-040).
const PLAIN_VALUES: [&str; 4] = ["job", "quoted", "count", "link"];

/// How a template is held to TXT-040.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Wording of our own (TXT-017 gives only an example): [`SHORT`] words a sentence, the
    /// portal link, no quote details.
    Update,
    /// Wording a requirement fixes (TXT-016, TXT-042): [`LIMIT`] words a sentence, the portal
    /// link, no quote details; TXT-016's fee, LIST and Terms lines are part of its fixed wording.
    Fixed,
    /// The LIST reply (TXT-031): fixed wording, [`LIMIT`] words a sentence, the quotes themselves
    /// in place of the link.
    List,
}

impl Shape {
    fn limit(self) -> usize {
        match self {
            Shape::Update => SHORT,
            Shape::Fixed | Shape::List => LIMIT,
        }
    }

    fn needs_link(self) -> bool {
        matches!(self, Shape::Update | Shape::Fixed)
    }

    fn plain(self) -> bool {
        matches!(self, Shape::Update | Shape::Fixed)
    }
}

/// TXT-040: every homeowner update template after intake about her quotes, and its shape.
pub const UPDATE_TEXTS: [(&str, Shape); 5] = [
    ("first_quote", Shape::Fixed),
    ("quote_update", Shape::Update),
    ("all_quotes_in", Shape::Fixed),
    ("quote_list", Shape::List),
    ("quote_list_end", Shape::List),
];

/// One way a text breaks TXT-040.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Sentence `sentence` (from 0) has `words` words, over its shape's limit.
    Long {
        /// The sentence's place in the text.
        sentence: usize,
        /// Its word count.
        words: usize,
    },
    /// No link to her private quote page (PICK-010).
    NoLink,
    /// A quote detail (an amount or a value other than the request number, counts and link).
    Details,
}

/// The text's sentences, the "Ask Hava:" opening left out (WORD-008).
#[must_use]
pub fn sentences(text: &str) -> Vec<String> {
    let body = text.strip_prefix(OPENING).unwrap_or(text);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        let ends = matches!(c, '.' | '?' | '!') && chars.peek().is_none_or(|n| n.is_whitespace());
        if c != '\n' {
            current.push(c);
        }
        if ends || c == '\n' {
            out.push(std::mem::take(&mut current));
        }
    }
    out.push(current);
    out.into_iter().filter(|s| words(s) > 0).collect()
}

/// The words in a sentence; a `{name}` placeholder or a link is one word (WORD-024).
#[must_use]
pub fn words(sentence: &str) -> usize {
    sentence
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric() || c == '{'))
        .count()
}

/// The `{name}` placeholders in a template.
fn placeholders(text: &str) -> Vec<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .collect()
}

/// TXT-040, WORD-024: every way `text` (a template or a rendered text) breaks its `shape`: a
/// sentence over [`SHORT`] or [`LIMIT`] words, no portal link (`{link}` or an askhava.com page
/// link), or a quote detail in a text that may not list one. An empty list passes.
#[must_use]
pub fn check(text: &str, shape: Shape) -> Vec<Problem> {
    let mut found: Vec<Problem> = sentences(text)
        .iter()
        .enumerate()
        .map(|(sentence, s)| (sentence, words(s)))
        .filter(|(_, n)| *n > shape.limit())
        .map(|(sentence, words)| Problem::Long { sentence, words })
        .collect();
    let linked = text.contains("{link}") || text.contains("https://askhava.com/");
    if shape.needs_link() && !linked {
        found.push(Problem::NoLink);
    }
    let detail = text.contains('$') || placeholders(text).iter().any(|p| !PLAIN_VALUES.contains(p));
    if shape.plain() && detail {
        found.push(Problem::Details);
    }
    found
}

/// TD-TXT-25: [`check`] on every [`UPDATE_TEXTS`] template in English and Spanish, as the
/// compiled-in registry holds them. An empty list passes.
///
/// # Errors
///
/// [`QuotesError::Text`] when the registry does not load or a listed template is missing.
pub fn scan() -> Result<Vec<(&'static str, Language, Problem)>, QuotesError> {
    let registry = Registry::embedded().map_err(|_| QuotesError::Text)?;
    let mut found = Vec::new();
    for (id, shape) in UPDATE_TEXTS {
        for language in [Language::En, Language::Es] {
            let (text, _) = registry.template(id, language).ok_or(QuotesError::Text)?;
            found.extend(check(text, shape).into_iter().map(|p| (id, language, p)));
        }
    }
    Ok(found)
}

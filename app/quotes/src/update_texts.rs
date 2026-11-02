//! The TXT-040 check on the homeowner's update texts after intake: short sentences, the link to
//! her private quote page (PICK-010), and no quote details outside the LIST reply (TXT-031) and
//! the lapsed-pick text (PICK-046) (WORD-024, D-438, D-477, D-478, D-482).
//!
//! [`UPDATE_TEXTS`] names every homeowner update template after intake that tells her about her
//! quotes, with its [`Shape`], and the other texts after intake whose wording other requirements
//! fix ([`Shape::Capped`], the 20-word limit only). Every other homeowner template is named in
//! [`NOT_UPDATE_TEXTS`], so a new one is listed in one or the other before the tests pass. A
//! sentence no requirement fixes has at most [`SHORT`] words; every sentence, fixed or not, at most
//! [`LIMIT`]. A sentence ends at ".", "?" or "!" before a space or
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
/// Her private quote page's address (PICK-010, `/q/{token}`); a rendered text links here, never
/// only to the Terms or help pages.
const PAGE_LINK: &str = "https://askhava.com/q/";

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
    /// A homeowner text after intake that is not a quote update, whose wording another
    /// requirement fixes (TXT-032, JOB-010, JOB-011, PICK-015): [`LIMIT`] words a sentence only,
    /// no link required and no quote-detail check (D-1291, D-1293).
    Capped,
}

impl Shape {
    fn limit(self) -> usize {
        match self {
            Shape::Update => SHORT,
            Shape::Fixed | Shape::List | Shape::Capped => LIMIT,
        }
    }

    /// A quote update pointing to her page: it needs the page link and lists no quote details.
    fn points_to_page(self) -> bool {
        matches!(self, Shape::Update | Shape::Fixed)
    }
}

/// TXT-040: every homeowner update template after intake about her quotes, and its shape, then
/// the other texts after intake held to the 20-word limit only (D-1293).
pub const UPDATE_TEXTS: [(&str, Shape); 16] = [
    ("first_quote", Shape::Fixed),
    ("quote_update", Shape::Update),
    ("all_quotes_in", Shape::Fixed),
    ("quote_list", Shape::List),
    ("quote_list_end", Shape::List),
    ("request_out", Shape::Capped),
    ("request_out_held", Shape::Capped),
    ("request_out_today", Shape::Capped),
    ("request_out_slow", Shape::Capped),
    ("no_quotes_yet", Shape::Capped),
    ("closed_no_quotes", Shape::Capped),
    ("closed_no_pick", Shape::Capped),
    ("closed_no_quote_left", Shape::Capped),
    ("text_pick_terms", Shape::Capped),
    ("text_pick_which", Shape::Capped),
    ("text_pick_done", Shape::Capped),
];

/// The homeowner templates outside TXT-040's scan, each for a reason (D-1293): the consent,
/// opt-out, HELP and safety texts (fixed carrier or safety wording),
/// the intake questions and replies before her request goes out (TXT-040 covers texts after
/// intake), the card review texts (INT-041), and the command replies that are not updates
/// (CLOSE, LIST-which, the other `text_pick_*` replies, the missed-call note). A template is
/// in this list or in [`UPDATE_TEXTS`], never both (`src/tests/update_texts.rs`).
pub const NOT_UPDATE_TEXTS: [&str; 43] = [
    "consent",
    "consent_safety_call",
    "who_is_this",
    "opt_out_confirmation",
    "start_reply",
    "help_reply",
    "help_reply_ca",
    "help_reply_away",
    "help_reply_away_ca",
    "emergency_911",
    "spanish_not_yet",
    "under_18",
    "caller_missed",
    "problem_reply",
    "close_howto",
    "cost_answer",
    "beep_question",
    "how_soon",
    "needed_by_day",
    "emergency_choice",
    "night_count_many",
    "night_count_one",
    "night_count_none",
    "name_address",
    "not_covered",
    "out_of_area",
    "coverage_count_many",
    "coverage_count_one",
    "split_offer",
    "quote_list_which",
    "intake_reminder",
    "intake_closed_no_reply",
    "close_which",
    "close_mention",
    "job_closed",
    "jobs_closed",
    "text_pick_no_quote",
    "text_pick_confirmed",
    "text_pick_waiting",
    "text_pick_none",
    "card_shown",
    "card_link",
    "card_edit",
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
pub(crate) fn sentences(text: &str) -> Vec<String> {
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
pub(crate) fn words(sentence: &str) -> usize {
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
/// sentence over [`SHORT`] or [`LIMIT`] words, no portal link (`{link}` or a link to her quote
/// page, `https://askhava.com/q/<token>`; a Terms or help link does not count), or a quote detail
/// in a text that may not list one. An empty list passes.
#[must_use]
pub fn check(text: &str, shape: Shape) -> Vec<Problem> {
    let mut found: Vec<Problem> = sentences(text)
        .iter()
        .enumerate()
        .map(|(sentence, s)| (sentence, words(s)))
        .filter(|(_, n)| *n > shape.limit())
        .map(|(sentence, words)| Problem::Long { sentence, words })
        .collect();
    if !shape.points_to_page() {
        return found;
    }
    if !text.contains("{link}") && !links_page(text) {
        found.push(Problem::NoLink);
    }
    if text.contains('$') || placeholders(text).iter().any(|p| !PLAIN_VALUES.contains(p)) {
        found.push(Problem::Details);
    }
    found
}

/// Whether a word of `text` is a link to her quote page with a token after [`PAGE_LINK`].
fn links_page(text: &str) -> bool {
    text.split_whitespace()
        .filter_map(|w| w.strip_prefix(PAGE_LINK))
        .any(|token| !token.is_empty() && !token.contains('/'))
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

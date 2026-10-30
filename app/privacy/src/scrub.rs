//! The plain-code scrubber and `ScrubbedText` (AI-002, AI-003, AI-004, AI-020; D-146, D-373, D-800).
//!
//! Every text bound for the AI, homeowner or business (quote wording too, D-373), passes
//! [`scrub`] first. It matches on a normalized copy: NFKC, Unicode format characters (category Cf,
//! the tag block included) removed, Arabic-Indic and Devanagari digits read as ASCII digits, curly
//! apostrophes made straight (AI-020). It then removes, each run replaced by [`REMOVED`]: up to two
//! words after "this is", "my name is", "I'm", "im", "it's", "soy", "me llamo" or "mi nombre es"
//! unless the first is a common word (D-146); any phone-shaped run of 7 or more digits, digits and
//! English or Spanish number words mixed ("nine two eight", "9two8", "twenty-eight", "double
//! five", "555-O1l2"); emails, "at"/"dot" spelled emails, @handles, links and bare domains; street
//! numbers with their street names or a capitalised name ("2150 Mesquite"); gate codes;
//! sign-off names; and the vault name and every name the person stated earlier (AI-003, AI-020).
//! [`ScrubbedText`] has a private field and no public constructor, so only this crate can make one
//! and `ai` never receives unscrubbed text (AI-004; `tests/security/ui/forge_scrubbed_text.rs`).

use std::collections::HashSet;
use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use unicode_normalization::UnicodeNormalization;

/// What replaces each removed run of words.
pub const REMOVED: &str = "[removed]";

/// The default common-words list (AI-002, D-146, D-800): a word after "I'm" and the like that is
/// on it is kept, so "I'm having a leak" keeps every word. The owner's setting replaces it.
pub const DEFAULT_COMMON_WORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "or",
    "but",
    "not",
    "no",
    "so",
    "very",
    "really",
    "just",
    "also",
    "still",
    "here",
    "there",
    "home",
    "back",
    "out",
    "in",
    "on",
    "at",
    "of",
    "to",
    "for",
    "with",
    "from",
    "about",
    "by",
    "up",
    "down",
    "off",
    "over",
    "sorry",
    "sure",
    "ok",
    "okay",
    "fine",
    "good",
    "great",
    "glad",
    "happy",
    "ready",
    "available",
    "interested",
    "looking",
    "having",
    "getting",
    "going",
    "trying",
    "wondering",
    "calling",
    "texting",
    "writing",
    "asking",
    "needing",
    "hoping",
    "leaving",
    "moving",
    "selling",
    "buying",
    "renting",
    "new",
    "owner",
    "homeowner",
    "renter",
    "tenant",
    "landlord",
    "broken",
    "leaking",
    "dripping",
    "running",
    "stuck",
    "making",
    "been",
    "done",
    "gone",
    "hot",
    "cold",
    "warm",
    "too",
    "again",
    "now",
    "today",
    "tomorrow",
    "tonight",
    "late",
    "early",
    "my",
    "your",
    "our",
    "his",
    "her",
    "their",
    "it",
    "its",
    "this",
    "that",
    "is",
    "was",
    "be",
    "me",
    "you",
    "he",
    "she",
    "we",
    "they",
    "what",
    "when",
    "where",
    "why",
    "how",
    "all",
    "some",
    "any",
    "probably",
    "maybe",
    "actually",
    "currently",
    "never",
    "always",
    "urgent",
    "emergency",
    "worried",
    "afraid",
    "concerned",
    "free",
    "away",
    "busy",
    "outside",
    "inside",
    "upstairs",
    "downstairs",
    "like",
    "call",
    "please",
    "thanks",
    "el",
    "la",
    "los",
    "las",
    "un",
    "una",
    "de",
    "del",
    "y",
    "o",
    "que",
    "en",
    "con",
    "por",
    "para",
    "muy",
    "si",
    "sí",
    "aquí",
    "dueño",
    "dueña",
    "inquilino",
    "inquilina",
    "nuevo",
    "nueva",
    "propietario",
    "propietaria",
    "bien",
    "mal",
    "tan",
];

const TRIGGERS: &[&[&str]] = &[
    &["this", "is"],
    &["my", "name", "is"],
    &["my", "name's"],
    &["i'm"],
    &["im"],
    &["i", "am"],
    &["it's"],
    &["soy"],
    &["me", "llamo"],
    &["mi", "nombre", "es"],
];
const SIGN_OFFS: &[&str] = &[
    "thanks",
    "thx",
    "regards",
    "cheers",
    "gracias",
    "saludos",
    "atentamente",
    "sincerely",
];
/// Number words and the digits each spells; a tens word ("twenty", "veinte") spells two digits,
/// its zero taken by a unit word joined to it ("twenty-eight" is 28).
const NUMBER_WORDS: &[(&str, &str)] = &[
    ("zero", "0"),
    ("cero", "0"),
    ("oh", "0"),
    ("one", "1"),
    ("uno", "1"),
    ("two", "2"),
    ("dos", "2"),
    ("three", "3"),
    ("tres", "3"),
    ("four", "4"),
    ("cuatro", "4"),
    ("five", "5"),
    ("cinco", "5"),
    ("six", "6"),
    ("seis", "6"),
    ("seven", "7"),
    ("siete", "7"),
    ("eight", "8"),
    ("ocho", "8"),
    ("nine", "9"),
    ("nueve", "9"),
    ("ten", "10"),
    ("diez", "10"),
    ("eleven", "11"),
    ("once", "11"),
    ("twelve", "12"),
    ("doce", "12"),
    ("thirteen", "13"),
    ("trece", "13"),
    ("fourteen", "14"),
    ("catorce", "14"),
    ("fifteen", "15"),
    ("quince", "15"),
    ("sixteen", "16"),
    ("seventeen", "17"),
    ("eighteen", "18"),
    ("nineteen", "19"),
    ("twenty", "20"),
    ("veinte", "20"),
    ("veinti", "20"),
    ("thirty", "30"),
    ("treinta", "30"),
    ("forty", "40"),
    ("cuarenta", "40"),
    ("fifty", "50"),
    ("cincuenta", "50"),
    ("sixty", "60"),
    ("sesenta", "60"),
    ("seventy", "70"),
    ("setenta", "70"),
    ("eighty", "80"),
    ("ochenta", "80"),
    ("ninety", "90"),
    ("noventa", "90"),
];
/// "double five" is 55, "triple oh" is 000 (AI-020).
const MULTIPLIERS: &[(&str, usize)] = &[("double", 2), ("doble", 2), ("triple", 3)];
const TLDS: &[&str] = &[
    "com", "net", "org", "edu", "gov", "us", "io", "co", "mx", "info", "biz", "me", "app", "dev",
];
const STREET_SUFFIXES: &[&str] = &[
    "st",
    "street",
    "ave",
    "avenue",
    "way",
    "dr",
    "drive",
    "rd",
    "road",
    "ln",
    "lane",
    "blvd",
    "boulevard",
    "ct",
    "court",
    "pl",
    "place",
    "cir",
    "circle",
    "ter",
    "terrace",
    "pkwy",
    "parkway",
    "hwy",
    "highway",
    "trl",
    "trail",
    "loop",
];
const STREET_PREFIXES: &[&str] = &["calle", "avenida", "camino"];
const CODE_CUES: &[&str] = &["code", "codigo", "código", "gate", "pin", "clave", "portón"];
const AT_WORDS: &[&str] = &["at", "arroba"];
const DOT_WORDS: &[&str] = &["dot", "punto"];

/// Text that passed [`scrub`]; the only text type `ai` accepts (AI-004). Its field is private and
/// it has no public constructor, so no other crate can build one (`forge_scrubbed_text.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrubbedText {
    text: String,
}

impl ScrubbedText {
    /// The scrubbed words, for the AI request body or `messaging.inbox`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// The words a stated name is checked against (AI-002, D-146), lower-cased.
#[derive(Debug, Clone)]
pub struct CommonWords(HashSet<String>);

impl CommonWords {
    /// The list from the owner's setting.
    pub fn new<'a>(words: impl IntoIterator<Item = &'a str>) -> Self {
        Self(words.into_iter().map(str::to_lowercase).collect())
    }

    fn has(&self, word: &str) -> bool {
        self.0.contains(word)
    }
}

impl Default for CommonWords {
    fn default() -> Self {
        Self::new(DEFAULT_COMMON_WORDS.iter().copied())
    }
}

/// The names a person stated earlier in this conversation, carried from turn to turn by the
/// caller without being readable outside `privacy` (AI-020). `Debug` shows only a count.
#[derive(Default)]
pub struct NameMemory {
    names: Vec<SecretString>,
}

impl fmt::Debug for NameMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NameMemory({} redacted)", self.names.len())
    }
}

/// What the scrubber knows about the person besides the text (AI-003, AI-020).
#[derive(Debug)]
pub struct Known<'a> {
    /// The common-words list (AI-002).
    pub common: &'a CommonWords,
    /// The homeowner's name from the vault, if any (AI-003).
    pub vault_name: Option<&'a SecretString>,
}

struct Token {
    text: String,
    key: String,
}

/// Scrubs `raw` and remembers any name it states in `memory` (AI-002, AI-003, AI-020). Removes
/// stated names, every vault and remembered name, phone-shaped numbers in any spelling, emails,
/// handles, links, bare domains, street numbers and gate codes.
pub fn scrub(raw: &str, known: &Known<'_>, memory: &mut NameMemory) -> ScrubbedText {
    let tokens: Vec<Token> = normalize(raw)
        .split_whitespace()
        .map(|t| Token {
            text: t.to_owned(),
            key: key_of(t),
        })
        .collect();
    let mut cut = vec![false; tokens.len()];
    mark_contacts(&tokens, &mut cut);
    mark_digit_runs(&tokens, &mut cut);
    mark_spelled_emails(&tokens, &mut cut);
    mark_streets(&tokens, known.common, &mut cut);
    mark_bare_houses(&tokens, known.common, &mut cut);
    mark_codes(&tokens, &mut cut);
    let stated = mark_stated(&tokens, known.common, &mut cut);
    for name in stated {
        if !memory.names.iter().any(|n| n.expose_secret() == name) {
            memory.names.push(SecretString::from(name));
        }
    }
    mark_known(&tokens, known, memory, &mut cut);
    ScrubbedText {
        text: render(&tokens, &cut),
    }
}

/// NFKC, Cf characters dropped, other-script digits as ASCII, curly apostrophes straight (AI-020).
pub(crate) fn normalize(raw: &str) -> String {
    raw.nfkc()
        .filter(|c| !is_format(*c))
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => shift(c, 0x0660),
            '\u{06F0}'..='\u{06F9}' => shift(c, 0x06F0),
            '\u{0966}'..='\u{096F}' => shift(c, 0x0966),
            '\u{2018}' | '\u{2019}' | '\u{02BC}' => '\'',
            _ => c,
        })
        .collect()
}

fn shift(c: char, zero: u32) -> char {
    char::from_u32(u32::from(c) - zero + u32::from('0')).unwrap_or(c)
}

/// Unicode general category Cf (format characters), including the tag block (AI-020).
fn is_format(c: char) -> bool {
    matches!(u32::from(c),
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF
        | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// The lower-cased token without edge punctuation (a leading `@` kept).
fn key_of(token: &str) -> String {
    let lower = token.to_lowercase();
    lower
        .trim_matches(|c: char| !(c.is_alphanumeric() || c == '@'))
        .to_owned()
}

fn render(tokens: &[Token], cut: &[bool]) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(tokens.len());
    for (token, &gone) in tokens.iter().zip(cut) {
        match (gone, out.last()) {
            (true, Some(&REMOVED)) => {}
            (true, _) => out.push(REMOVED),
            (false, _) => out.push(&token.text),
        }
    }
    out.join(" ")
}

/// Emails, @handles, links, bare domains and any token holding 7 or more digits (AI-002, AI-020).
fn mark_contacts(tokens: &[Token], cut: &mut [bool]) {
    for (i, t) in tokens.iter().enumerate() {
        let k = t.key.as_str();
        let email = k.find('@').is_some_and(|at| at > 0 || k.len() > 1);
        let digits = k.chars().filter(char::is_ascii_digit).count();
        if email || is_link(k) || digits >= 7 {
            cut[i] = true;
        }
    }
}

fn is_link(key: &str) -> bool {
    key.contains("://") || key.starts_with("www.") || is_domain(key)
}

/// A bare domain such as `example.com/x`: dotted labels ending in a known top-level domain.
fn is_domain(key: &str) -> bool {
    let host = key.split('/').next().unwrap_or_default();
    let labels: Vec<&str> = host.split('.').collect();
    let well_formed = labels
        .iter()
        .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'));
    labels.len() >= 2
        && well_formed
        && labels[0].chars().any(char::is_alphabetic)
        && labels.last().is_some_and(|tld| TLDS.contains(tld))
}

/// The digits a token spells, mixing digits, separators, number words ("9two8", "twenty-eight")
/// and, beside a digit, "o" for zero and "l" for one ("555-O1l2"), if it spells nothing else (AI-020).
fn digit_form(key: &str) -> Option<String> {
    let lookalikes = key.len() == 1 || key.contains(|c: char| c.is_ascii_digit());
    let (mut rest, mut out, mut tens) = (key, String::new(), false);
    while let Some(c) = rest.chars().next() {
        if let Some((word, digits)) = number_word(rest) {
            if tens && digits.len() == 1 {
                out.pop();
            }
            out.push_str(digits);
            tens = digits.len() == 2 && digits.ends_with('0') && digits != "10";
            rest = &rest[word.len()..];
            continue;
        }
        let digit = match c {
            '0'..='9' => Some(c),
            'o' if lookalikes => Some('0'),
            'l' if lookalikes => Some('1'),
            _ if ".-()/+#".contains(c) => None,
            _ => return None,
        };
        if let Some(d) = digit {
            out.push(d);
            tens = false;
        }
        rest = &rest[c.len_utf8()..];
    }
    (!out.is_empty()).then_some(out)
}

/// The longest number word `rest` starts with ("seventeen" before "seven").
fn number_word(rest: &str) -> Option<(&'static str, &'static str)> {
    NUMBER_WORDS
        .iter()
        .filter(|(w, _)| rest.starts_with(w))
        .max_by_key(|(w, _)| w.len())
        .copied()
}

/// Runs of number-spelling tokens (separator-only tokens and "double"/"triple" between them
/// allowed) that together hold 7 or more digits: a phone number in any spelling (AI-002, AI-020).
fn mark_digit_runs(tokens: &[Token], cut: &mut [bool]) {
    let mut start = 0;
    while start < tokens.len() {
        let (mut end, mut digits, mut last, mut times) = (start, 0, None, 1);
        while end < tokens.len() {
            let key = tokens[end].key.as_str();
            let repeat = MULTIPLIERS.iter().find(|(w, _)| *w == key);
            match (repeat, digit_form(key)) {
                (Some(&(_, n)), _) => times = n,
                (None, Some(d)) => (digits, last, times) = (digits + d.len() * times, Some(end), 1),
                (None, None) if key.is_empty() && last.is_some() => {}
                (None, None) => break,
            }
            end += 1;
        }
        if let Some(last) = last.filter(|_| digits >= 7) {
            cut[start..=last].fill(true);
        }
        start = end.max(start + 1);
    }
}

/// "steve at gmail dot com", "steve at gmail.com", "gmail dot com" (AI-020).
fn mark_spelled_emails(tokens: &[Token], cut: &mut [bool]) {
    let key = |i: usize| tokens.get(i).map_or("", |t| t.key.as_str());
    for i in 0..tokens.len() {
        let dotted = DOT_WORDS.contains(&key(i)) && TLDS.contains(&key(i + 1)) && i > 0;
        if dotted {
            cut[i - 1..=i + 1].fill(true);
            let mut j = i - 1;
            while j >= 2 && (DOT_WORDS.contains(&key(j - 1)) || AT_WORDS.contains(&key(j - 1))) {
                cut[j - 2..j].fill(true);
                j -= 2;
            }
        }
        if AT_WORDS.contains(&key(i)) && i > 0 && is_domain(key(i + 1)) {
            cut[i - 1..=i + 1].fill(true);
        }
    }
}

fn is_wordish(key: &str, common: &CommonWords) -> bool {
    !key.is_empty()
        && !common.has(key)
        && key.chars().all(|c| c.is_alphabetic() || "-'".contains(c))
}

fn is_house_number(key: &str) -> bool {
    let digits = key.chars().take_while(char::is_ascii_digit).count();
    (1..=6).contains(&digits) && key.len() <= digits + 1
}

/// A street suffix with up to three name words before it and a house number before those, and
/// "calle", "avenida" or "camino" with the words and number after it (AI-002).
fn mark_streets(tokens: &[Token], common: &CommonWords, cut: &mut [bool]) {
    for (s, t) in tokens.iter().enumerate() {
        if STREET_SUFFIXES.contains(&t.key.as_str()) {
            let named = (s.saturating_sub(3)..s)
                .rev()
                .take_while(|&j| is_wordish(&tokens[j].key, common) || ordinal(&tokens[j].key))
                .last();
            let Some(first) = named else { continue };
            cut[first..=s].fill(true);
            if first > 0 && is_house_number(&tokens[first - 1].key) {
                cut[first - 1] = true;
            }
        }
        if STREET_PREFIXES.contains(&t.key.as_str()) {
            for j in s + 1..tokens.len().min(s + 5) {
                let k = tokens[j].key.as_str();
                cut[j] |= is_wordish(k, common) || is_house_number(k);
            }
        }
    }
}

/// A house number of 3 to 6 digits followed by one or two capitalised names and no suffix
/// ("I'm at 2150 Mesquite") (AI-002).
fn mark_bare_houses(tokens: &[Token], common: &CommonWords, cut: &mut [bool]) {
    for (i, t) in tokens.iter().enumerate() {
        let digits = t.key.chars().take_while(char::is_ascii_digit).count();
        if digits < 3 || !is_house_number(&t.key) {
            continue;
        }
        let names = (i + 1..tokens.len().min(i + 3))
            .take_while(|&j| titled(&tokens[j], common))
            .count();
        if names > 0 {
            cut[i..=i + names].fill(true);
        }
    }
}

/// A capitalised, not all-capitals, uncommon word ("Mesquite", not "YES" or "The").
fn titled(t: &Token, common: &CommonWords) -> bool {
    let mut chars = t
        .text
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .chars();
    chars.next().is_some_and(char::is_uppercase)
        && chars.any(char::is_lowercase)
        && is_wordish(&t.key, common)
}

fn ordinal(key: &str) -> bool {
    let digits = key.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && ["st", "nd", "rd", "th"].contains(&&key[digits..])
}

/// The number after "gate code", "code", "pin", "código" or "clave", within three words.
fn mark_codes(tokens: &[Token], cut: &mut [bool]) {
    for (c, t) in tokens.iter().enumerate() {
        if !CODE_CUES.contains(&t.key.as_str()) {
            continue;
        }
        let near = c + 1..tokens.len().min(c + 4);
        if let Some(j) = near
            .into_iter()
            .find(|&j| digit_form(&tokens[j].key).is_some_and(|d| d.len() >= 3))
        {
            cut[j] = true;
        }
    }
}

/// Names after the AI-002 phrases (up to two words, the first not a common word, D-146) and after
/// a sign-off at the message's end (AI-020); returns every word removed, lower-cased.
fn mark_stated(tokens: &[Token], common: &CommonWords, cut: &mut [bool]) -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..tokens.len() {
        let after = TRIGGERS.iter().find_map(|phrase| {
            let end = i + phrase.len();
            let hit = tokens
                .get(i..end)?
                .iter()
                .zip(*phrase)
                .all(|(t, w)| t.key == *w);
            hit.then_some(end)
        });
        let after = after.or_else(|| sign_off(tokens, i));
        let Some(n) = after.filter(|&n| n < tokens.len()) else {
            continue;
        };
        if !is_wordish(&tokens[n].key, common) {
            continue;
        }
        cut[n] = true;
        names.push(tokens[n].key.clone());
        let ends_clause = tokens[n].text.ends_with(|c: char| !c.is_alphanumeric());
        if !ends_clause
            && tokens
                .get(n + 1)
                .is_some_and(|t| is_wordish(&t.key, common))
        {
            cut[n + 1] = true;
            names.push(tokens[n + 1].key.clone());
        }
    }
    names
}

/// The index after a sign-off ("Thanks, Steve", "thank you - Ana Test", "— Steve") when one or two
/// words end the message.
fn sign_off(tokens: &[Token], i: usize) -> Option<usize> {
    let key = tokens[i].key.as_str();
    let cue = SIGN_OFFS.contains(&key)
        || (key == "you" && i > 0 && tokens[i - 1].key == "thank")
        || tokens[i].text.chars().all(|c| "-—–~".contains(c));
    let left = tokens.len() - i - 1;
    (cue && (1..=2).contains(&left)).then_some(i + 1)
}

/// Every word of the vault name, common words too ("Joy Early"), and of each remembered name
/// (AI-003, AI-020).
fn mark_known(tokens: &[Token], known: &Known<'_>, memory: &NameMemory, cut: &mut [bool]) {
    let vault = known.vault_name.map(|n| n.expose_secret().to_owned());
    let mut names: Vec<String> = vault
        .iter()
        .flat_map(|n| {
            normalize(n)
                .split_whitespace()
                .map(key_of)
                .collect::<Vec<_>>()
        })
        .collect();
    names.extend(memory.names.iter().map(|n| n.expose_secret().to_owned()));
    names.retain(|n| n.chars().count() >= 2);
    for (i, t) in tokens.iter().enumerate() {
        let bare = t.key.strip_suffix("'s").unwrap_or(&t.key);
        if names.iter().any(|n| n == bare) {
            cut[i] = true;
        }
    }
}

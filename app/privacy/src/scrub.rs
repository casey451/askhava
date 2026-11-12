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
//! No word of `messaging`'s opt-out, HELP and START lists ([`NEVER_A_NAME`]) is ever cut, kept or
//! removed as a name, so an opt-out after a name is still heard (CON-010, D-2621).
//! [`ScrubbedText`] has a private field and no public constructor, so only this crate can make one
//! and `ai` never receives unscrubbed text (AI-004; `tests/security/ui/forge_scrubbed_text.rs`).

use std::collections::HashSet;
use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

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
    &["it", "is"],
    &["its"],
    &["call", "me"],
    &["soy"],
    &["me", "llamo"],
    &["mi", "nombre", "es"],
];
/// Words never cut as a stated or sign-off name, remembered as one or removed as one, a vault name
/// word included (CON-010, CON-013, CON-035, AI-020, D-2621): every word of `messaging`'s opt-out
/// phrases and of its sole opt-out, HELP and START words, English and Spanish, folded (lower case,
/// no accents, no apostrophes). `privacy` may not depend on `messaging`, so the list is kept here;
/// `tests/integration/tests/optout_after_name.rs` fails when a word of messaging's lists is missing.
pub const NEVER_A_NAME: &[&str] = &[
    "stop",
    "stopall",
    "unsubscribe",
    "cancel",
    "end",
    "revoke",
    "optout",
    "quit",
    "parar",
    "alto",
    "cancelar",
    "baja",
    "opt",
    "out",
    "start",
    "unstop",
    "help",
    "info",
    "ayuda",
    "texting",
    "messaging",
    "sending",
    "dont",
    "do",
    "not",
    "text",
    "texts",
    "me",
    "no",
    "more",
    "remove",
    "take",
    "off",
    "escriban",
    "manden",
    "mensajes",
    "ya",
    "quiero",
    "dejen",
    "de",
    "escribirme",
    "quitenme",
    "la",
    "lista",
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
/// Sign-offs that are also everyday words ("best price"): a cue only with a comma after them.
const COMMA_SIGN_OFFS: &[&str] = &["best"];
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
/// Words that join a spoken digit run ("928 dash 555 dash 0134", D-2622).
const SPOKEN_SEPARATORS: &[&str] = &["dash", "guion", "guión", "space", "espacio"];
/// Words before a bare house number that put an address next ("i live at 2150 mesquite").
const HOUSE_CUES: &[&str] = &["at", "live", "address", "is"];
/// Measures after a number that make it a size, not a house ("is 1500 square feet").
const MEASURES: &[&str] = &[
    "square", "sq", "sqft", "feet", "foot", "ft", "inch", "inches", "gallon", "gallons", "gal",
    "dollars", "bucks", "watts", "volts", "amps", "btu", "pounds", "lbs", "miles", "years",
    "degrees", "psi", "cfm", "tons", "yards", "meters", "metros", "pies", "pesos", "dolares",
];
/// How many words after a code cue its digits may start, and how far a spaced run may reach.
const CODE_REACH: usize = 3;
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

/// Unicode general category Cf (format characters), including the tag block (AI-020). The one
/// list: `ai`'s screen drops the same characters before it checks AI text (AI-021, D-814, D-1362).
#[must_use]
pub fn is_format(c: char) -> bool {
    matches!(u32::from(c),
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF
        | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// The lower-cased token without edge punctuation (a leading `@` kept), `(at)`, `[at]`, `(dot)`
/// and `[dot]` read as `@` and `.` ("steve(at)gmail(dot)com", AI-020, D-2622).
fn key_of(token: &str) -> String {
    let mut lower = token.to_lowercase();
    for (spelled, mark) in [("(at)", "@"), ("[at]", "@"), ("(dot)", "."), ("[dot]", ".")] {
        lower = lower.replace(spelled, mark);
    }
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

/// Runs of number-spelling tokens (separator-only tokens, a spoken "dash", "guion" or "space" and
/// "double"/"triple" between them allowed) that together hold 7 or more digits: a phone number in
/// any spelling (AI-002, AI-020, D-2622).
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
                (None, None) if spoken_gap(key) && last.is_some() => {}
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

/// A token that may sit inside a digit run: separators only, or a spoken separator (D-2622).
fn spoken_gap(key: &str) -> bool {
    key.is_empty() || SPOKEN_SEPARATORS.contains(&key)
}

/// "steve at gmail dot com", "steve at gmail.com", "gmail dot com", and a spaced `@` with both
/// its sides ("steve @ gmail.com", "steve @gmail.com") (AI-020, D-2622).
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
        let spaced_at = key(i) == "@" || key(i).strip_prefix('@').is_some_and(is_domain);
        if spaced_at && i > 0 {
            cut[i - 1..=(i + 1).min(tokens.len() - 1)].fill(true);
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
/// ("I'm at 2150 Mesquite"), or, after "at", "live", "address" or "is", by one or two uncommon
/// words in any case that are not measures ("i live at 2150 mesquite") (AI-002, D-2622).
fn mark_bare_houses(tokens: &[Token], common: &CommonWords, cut: &mut [bool]) {
    for (i, t) in tokens.iter().enumerate() {
        let digits = t.key.chars().take_while(char::is_ascii_digit).count();
        if digits < 3 || !is_house_number(&t.key) {
            continue;
        }
        let cued = i > 0 && HOUSE_CUES.contains(&tokens[i - 1].key.as_str());
        let street_word = |j: &usize| {
            let token = &tokens[*j];
            let plain = cued && is_wordish(&token.key, common);
            titled(token, common) || (plain && !MEASURES.contains(&token.key.as_str()))
        };
        let names = (i + 1..tokens.len().min(i + 3))
            .take_while(street_word)
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

/// The number after "gate code", "code", "pin", "código" or "clave": the first digit-spelling
/// token within three words and every one right after it, when the run holds 3 or more digits
/// ("Gate code is 4 4 1 2", "gate code 44 12"; "error code 41" stays) (AI-002, D-2622).
fn mark_codes(tokens: &[Token], cut: &mut [bool]) {
    let digits_of = |j: usize| digit_form(&tokens[j].key).map_or(0, |d| d.len());
    for (c, t) in tokens.iter().enumerate() {
        if !CODE_CUES.contains(&t.key.as_str()) {
            continue;
        }
        let near = c + 1..tokens.len().min(c + 1 + CODE_REACH);
        let Some(first) = near.into_iter().find(|&j| digits_of(j) > 0) else {
            continue;
        };
        let end = (first..tokens.len())
            .find(|&j| digits_of(j) == 0)
            .unwrap_or(tokens.len());
        if (first..end).map(digits_of).sum::<usize>() >= 3 {
            cut[first..end].fill(true);
        }
    }
}

/// Names after the AI-002 phrases (up to two words, the first not a common word, D-146), after
/// a sign-off at the message's end and before "here" at a sentence's start (AI-020, D-2622); a
/// [`NEVER_A_NAME`] word is never taken, but a first one does not stop the second ("This is Do
/// Nguyen" still cuts "Nguyen", D-3277). Returns every word removed, lower-cased.
fn mark_stated(tokens: &[Token], common: &CommonWords, cut: &mut [bool]) -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..tokens.len() {
        let after = trigger_end(tokens, i)
            .or_else(|| sign_off(tokens, i, cut))
            .or_else(|| here_start(tokens, i));
        let Some(n) = after.filter(|&n| n < tokens.len()) else {
            continue;
        };
        if !is_wordish(&tokens[n].key, common) {
            continue;
        }
        let nameish = |key: &str| is_wordish(key, common) && !never_a_name(key);
        if nameish(&tokens[n].key) {
            cut[n] = true;
            names.push(tokens[n].key.clone());
        }
        let ends_clause = tokens[n].text.ends_with(|c: char| !c.is_alphanumeric());
        if !ends_clause && tokens.get(n + 1).is_some_and(|t| nameish(&t.key)) {
            cut[n + 1] = true;
            names.push(tokens[n + 1].key.clone());
        }
    }
    names
}

/// The index after an AI-002 phrase starting at word `i` ("it is", "its" and "call me" too, D-2622).
fn trigger_end(tokens: &[Token], i: usize) -> Option<usize> {
    TRIGGERS.iter().find_map(|phrase| {
        let end = i + phrase.len();
        let hit = tokens
            .get(i..end)?
            .iter()
            .zip(*phrase)
            .all(|(t, w)| t.key == *w);
        hit.then_some(end)
    })
}

/// The index after a sign-off ("Thanks, Steve", "thank you - Ana Test", "— Steve", "Best, Steve")
/// when one or two words end the message, contact tokens already cut after them aside ("Thanks,
/// Steve Smith 928-555-0134", D-2622). "best" is a sign-off only with a comma ("best price").
fn sign_off(tokens: &[Token], i: usize, cut: &[bool]) -> Option<usize> {
    let key = tokens[i].key.as_str();
    let cue = SIGN_OFFS.contains(&key)
        || (COMMA_SIGN_OFFS.contains(&key) && tokens[i].text.ends_with(','))
        || (key == "you" && i > 0 && tokens[i - 1].key == "thank")
        || tokens[i].text.chars().all(|c| "-—–~".contains(c));
    let end = cut
        .iter()
        .rposition(|&gone| !gone)
        .map_or(0, |last| last + 1);
    let left = end.saturating_sub(i + 1);
    (cue && (1..=2).contains(&left)).then_some(i + 1)
}

/// Word `i` when one or two words starting there, at the message's or a sentence's start, come
/// before "here" ("Steve Smith here", "Hi, Steve here") (AI-020, D-2622).
fn here_start(tokens: &[Token], i: usize) -> Option<usize> {
    let starts = i == 0 || tokens[i - 1].text.ends_with(['.', '!', '?', ',']);
    let here = |j: usize| tokens.get(j).is_some_and(|t| t.key == "here");
    let joined = !tokens[i].text.ends_with(|c: char| !c.is_alphanumeric());
    (starts && (here(i + 1) || (joined && here(i + 2)))).then_some(i)
}

/// Whether `key` (a scrubber key or a remembered name), folded like `messaging`'s detector (lower
/// case, accents off by NFD and dropping combining marks, apostrophes dropped, other marks
/// splitting it), is made only of [`NEVER_A_NAME`] words, so "stop", "Don't" and "stop-texting"
/// are never names while "Ya-Ting" and "Ortiz-de-la-Rosa" still are (CON-010, AI-020, D-2621,
/// D-3277).
fn never_a_name(key: &str) -> bool {
    let folded: String = key
        .to_lowercase()
        .nfd()
        .filter(|&c| c != '\'' && !is_combining_mark(c))
        .collect();
    let mut parts = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .peekable();
    parts.peek().is_some() && parts.all(|part| NEVER_A_NAME.contains(&part))
}

/// Every word of the vault name, common words too ("Joy Early"), and of each remembered name, but
/// never a [`NEVER_A_NAME`] word, so a name stored before D-2621 that is one is ignored (AI-003,
/// AI-020, CON-010).
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
    names.retain(|n| n.chars().count() >= 2 && !never_a_name(n));
    for (i, t) in tokens.iter().enumerate() {
        let bare = t.key.strip_suffix("'s").unwrap_or(&t.key);
        if names.iter().any(|n| n == bare) {
            cut[i] = true;
        }
    }
}

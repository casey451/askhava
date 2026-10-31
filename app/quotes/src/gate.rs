//! The final plain-code gate every job card passes last (CARD-008; CARD-005, PRIV-006, D-202).
//!
//! The text was already scrubbed (AI-002) or written by the AI from scrubbed words (AI-019); this
//! gate is the last, plain-code check before a business sees it. In each sentence, the first word
//! shaped like a contact removes that word and the rest of the sentence: a name after "ask for",
//! "talk to", "I'm", "this is" or a contact verb; a contact phrase ("text me", "my number"); an
//! email, link, domain or spelled email; a street number; three number words in a row; or 7 or
//! more digits, however spaced. Lines stay lines, so each card field is gated on its own line.
//! The gate repeats until nothing changes, so running it twice changes nothing (TD-CARD-11).
//!
//! Each pass first removes every phrase on `catalog`'s carrier-restricted list (whole words, any
//! case, read through `catalog::api`, CARD-010, D-492) and logs each removal with the list entry
//! that matched, never the text around it (D-485).

use askhava_catalog::api::RESTRICTED_WORDS;

/// Cues that put a name or on-site contact next: fire unless an everyday word follows.
const NAME_CUES: &str = "ask for|talk to|speak to|speak with|meet|pregunte por";
/// Words skipped after a cue before the word that decides ("ask for the owner").
const ARTICLES: &str = "a an the some";
/// Words after a [`NAME_CUES`] cue (and any article) that are not a name ("ask for a quote").
const EVERYDAY: &str = "help quote quotes price prices estimate estimates bid bids it them details photos pictures someone somebody more";
/// Self-introductions: fire unless a [`COMMON`] word follows ("I'm worried", never "im pat").
const SELF_CUES: &str = "i'm|im|i am|this is";
/// Words after a [`SELF_CUES`] cue that are not a name.
const COMMON: &str = "a an the not so very really just also still too in at on by from with about for to of out up back home here there away my our your his her its this that what why how when because sure afraid worried unsure glad happy sorry ready busy free available flexible open ok okay fine new moving renting selling retired interested concerned hoping trying looking getting going having leaking broken urgent important bad old told always never only usually currently";
/// Cues that fire only before a capitalised word ("Mrs. Lee", never "it's broken").
const CAP_CUES: &str = "it's|mr|mrs|ms|dr|sr|sra";
/// Contact phrases: each fires wherever it appears.
const CONTACT_CUES: &str =
    "contact|my name|name is|phone number|cell number|whatsapp|me llamo|llame|llámame";
/// Contact verbs: fire before "me", "us", a role word or a capitalised word ("Text Pat").
const VERBS: &str = "call text email e-mail reach message dm ring";
const MINE: &str = "my our mi";
/// After "my" or "our": a contact detail (CARD-005); a [`ROLES`] word fires too.
const MY_THINGS: &str = "number cell phone email address numero número";
/// On-site people: after "my", "our" or a contact verb they are a contact (CARD-005).
const ROLES: &str =
    "owner manager tenant husband wife son daughter neighbor neighbour roommate landlord caretaker";
const STREET_WORDS: &str = "st street ave avenue dr drive rd road ln lane blvd way ct court pl place cir circle pkwy hwy highway trl trail ter loop box calle avenida camino";
const NUMBER_WORDS: &str = "zero oh o one two three four five six seven eight nine ten eleven twelve twenty thirty forty fifty sixty seventy eighty ninety hundred thousand double triple cero uno dos tres cuatro cinco seis siete ocho nueve diez";
/// Words after which a `.` never ends a sentence: titles and street directions ("1234 N. Lake").
const NO_STOP: &str = "mr mrs ms dr sr sra n s e w ne nw se sw";

/// Runs the CARD-008 gate on `text`, line by line and sentence by sentence (CARD-005, PRIV-006).
#[must_use]
pub fn final_gate(text: &str) -> String {
    let mut out = gate_once(text);
    // A pass that changes the text makes it shorter, so these passes always reach the fixed point.
    for _ in 0..=text.len() {
        let next = gate_once(&out);
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// One pass of the gate over every line and sentence (CARD-008, CARD-010).
fn gate_once(text: &str) -> String {
    let lines = text.lines().map(|line| {
        let line = strip_restricted(line);
        let kept: Vec<String> = sentences(&line)
            .into_iter()
            .filter_map(gate_sentence)
            .collect();
        kept.join(" ")
    });
    lines
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes every carrier-restricted phrase from `line` as whole words, any case, logging each
/// removal with the list entry only (CARD-010, D-485). A line holding none comes back unchanged.
fn strip_restricted(line: &str) -> String {
    let spans = word_spans(line);
    let lower: Vec<String> = spans
        .iter()
        .map(|&(s, e)| line[s..e].to_lowercase())
        .collect();
    let mut cut = vec![false; spans.len()];
    let mut entries = RESTRICTED_WORDS.to_vec();
    // Longest phrases first, so "neo nazi" is one removal, logged once, not also "nazi".
    entries.sort_by_key(|e| std::cmp::Reverse(e.split(' ').count()));
    for entry in &entries {
        let phrase: Vec<&str> = entry.split(' ').collect();
        for (i, window) in lower.windows(phrase.len()).enumerate() {
            let free = !cut[i..i + phrase.len()].contains(&true);
            if free && window.iter().zip(&phrase).all(|(w, p)| w == p) {
                cut[i..i + phrase.len()].fill(true);
                tracing::info!(entry = *entry, "restricted phrase removed from a job card");
            }
        }
    }
    if !cut.contains(&true) {
        return line.to_owned();
    }
    let mut out = String::new();
    let mut from = 0;
    for (&(start, end), _) in spans.iter().zip(&cut).filter(|(_, c)| **c) {
        out.push_str(&line[from..start]);
        from = end;
    }
    out.push_str(&line[from..]);
    tidy(&out)
}

/// The byte ranges of the runs of letters and digits in `line`, the words the list matches.
fn word_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, c) in line.char_indices().chain([(line.len(), ' ')]) {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    spans
}

/// Drops the punctuation a removal left alone, keeping a sentence's last stop on the word before.
fn tidy(text: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        if token.chars().any(char::is_alphanumeric) {
            kept.push(token.to_owned());
        } else if let (Some(stop), Some(last)) =
            (token.chars().rfind(|c| ".!?".contains(*c)), kept.last_mut())
            && !last.ends_with(|c: char| c.is_ascii_punctuation())
        {
            last.push(stop);
        }
    }
    kept.join(" ")
}

/// The words the gate counts for CARD-009: tokens holding a letter or digit.
#[must_use]
pub fn word_count(text: &str) -> usize {
    let words = text.split_whitespace();
    words
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// Splits a line after `!`, `?` or a `.` followed by a space, except after a digit, a single
/// letter, a title or a street direction.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (n, &(i, c)) in chars.iter().enumerate() {
        let spaced = chars
            .get(n + 1)
            .is_none_or(|(_, next)| next.is_whitespace());
        let stop = match c {
            '!' | '?' => true,
            '.' => spaced && !no_stop_before(&line[start..i]),
            _ => false,
        };
        if stop {
            out.push(&line[start..=i]);
            start = i + 1;
        }
    }
    out.push(&line[start..]);
    out
}

fn no_stop_before(before: &str) -> bool {
    before.ends_with(|c: char| c.is_ascii_digit())
        || before.split_whitespace().last().is_some_and(|w| {
            let k = key(w);
            (k.chars().count() == 1 && k.chars().all(char::is_alphabetic)) || listed(NO_STOP, &k)
        })
}

/// Keeps a sentence whole, or the words before its first contact-shaped word with a full stop.
fn gate_sentence(sentence: &str) -> Option<String> {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let keys: Vec<String> = words.iter().map(|w| key(w)).collect();
    let Some(cut) = (0..words.len()).find(|&i| hit(&words, &keys, i)) else {
        return Some(sentence.trim().to_owned()).filter(|s| !s.is_empty());
    };
    let kept = words[..cut].join(" ");
    let kept = kept.trim_end_matches(|c: char| !c.is_alphanumeric() && c != ')');
    (word_count(kept) > 0).then(|| format!("{kept}."))
}

/// A word lower-cased, curly apostrophes made straight, outer punctuation off.
fn key(word: &str) -> String {
    let word = word.replace('\u{2019}', "'").to_lowercase();
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_owned()
}

/// Whether `word` is one of the space-separated words of `list`.
fn listed(list: &str, word: &str) -> bool {
    list.split(' ').any(|w| w == word)
}

/// The word count of the first `|`-separated phrase of `cues` that starts at word `i`.
fn cue_at(keys: &[String], i: usize, cues: &str) -> Option<usize> {
    let starts = |cue: &&str| {
        cue.split(' ')
            .enumerate()
            .all(|(n, w)| keys.get(i + n).is_some_and(|k| k == w))
    };
    cues.split('|')
        .find(starts)
        .map(|cue| cue.split(' ').count())
}

/// Whether a contact-shaped run starts at word `i` (CARD-008).
fn hit(words: &[&str], keys: &[String], i: usize) -> bool {
    let k = keys[i].as_str();
    let next = |len: usize| keys.get(i + len).map(String::as_str);
    let mine = listed(MINE, k) && next(1).is_some_and(|n| listed(MY_THINGS, n) || listed(ROLES, n));
    let web = words[i].contains('@') || ["http", "www"].iter().any(|p| k.starts_with(p));
    let spelled = next(1).is_some_and(|n| n == "at" || n == "arroba")
        && (next(2).is_some_and(domain)
            || (2..=4).any(|n| next(n).is_some_and(|d| d == "dot" || d == "punto")));
    named(words, keys, i)
        || cue_at(keys, i, CONTACT_CUES).is_some()
        || mine
        || web
        || domain(k)
        || spelled
        || street(words, keys, i)
        || number_run(keys, i)
}

/// A name after a name cue, a self-introduction, a title or a contact verb (CARD-005).
fn named(words: &[&str], keys: &[String], i: usize) -> bool {
    let after = |len: usize| {
        let rest = keys.get(i + len..).unwrap_or_default();
        let skip = rest.iter().take_while(|k| listed(ARTICLES, k)).count();
        keys.get(i + len + skip).map(String::as_str)
    };
    let capital = |len: usize| {
        words
            .get(i + len)
            .is_some_and(|w| *w != "I" && w.starts_with(char::is_uppercase))
    };
    let cue = |cues: &str| cue_at(keys, i, cues);
    let asked = cue(NAME_CUES)
        .and_then(after)
        .is_some_and(|k| !listed(EVERYDAY, k));
    let intro =
        cue(SELF_CUES).is_some_and(|len| keys.get(i + len).is_some_and(|k| !listed(COMMON, k)));
    let titled = cue(CAP_CUES).is_some_and(capital);
    let verb = listed(VERBS, &keys[i])
        && (capital(1) || after(1).is_some_and(|n| n == "me" || n == "us" || listed(ROLES, n)));
    asked || intro || titled || verb
}

/// A street number: up to 6 digits before a street word, or 3 to 6 before a capitalised word.
fn street(words: &[&str], keys: &[String], i: usize) -> bool {
    let k = keys[i].as_str();
    let digits = !k.is_empty() && k.len() <= 6 && k.chars().all(|c| c.is_ascii_digit());
    let worded = (1..=4).any(|n| keys.get(i + n).is_some_and(|s| listed(STREET_WORDS, s)));
    let named = k.len() >= 3
        && words
            .get(i + 1)
            .is_some_and(|w| w.starts_with(char::is_uppercase));
    digits && (worded || named)
}

/// A domain-shaped word ("example.com", "pat.lee"): 2 or more letters or digits, a dot, then 2
/// or more letters; "a.m.", "e.g." and "3.5" are not domains (CARD-008).
fn domain(key: &str) -> bool {
    let host = key.split('/').next().unwrap_or_default();
    let parts: Vec<&str> = host.split('.').collect();
    parts.windows(2).any(|p| {
        p[0].chars().filter(|c| c.is_alphanumeric()).count() >= 2
            && p[1].chars().take(2).filter(|c| c.is_alphabetic()).count() == 2
    })
}

/// A digit, or the letter O typed for a zero ("555 O142").
fn digit_like(c: char) -> bool {
    c.is_ascii_digit() || c == 'o'
}

/// Three number words in a row, or 7 or more digits and number words, from word `i` (CARD-008).
fn number_run(keys: &[String], i: usize) -> bool {
    let (mut words, mut digits) = (0, 0);
    for k in &keys[i..] {
        let parts: Vec<&str> = k.split('-').collect();
        if parts.iter().all(|p| listed(NUMBER_WORDS, p)) {
            words += parts.len();
        } else if k.chars().any(|c| c.is_ascii_digit())
            && k.chars().all(|c| digit_like(c) || "()-+./#".contains(c))
        {
            digits += k.chars().filter(|&c| digit_like(c)).count();
        } else if k.chars().filter(char::is_ascii_digit).count() >= 7 {
            return true;
        } else if !k.is_empty() {
            break;
        }
        if words >= 3 || words + digits >= 7 {
            return true;
        }
    }
    false
}

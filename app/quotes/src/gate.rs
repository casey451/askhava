//! The final plain-code gate every job card passes last (CARD-008; CARD-005, PRIV-006, D-202).
//!
//! The text was already scrubbed (AI-002) or written by the AI from scrubbed words (AI-019); this
//! gate is the last, plain-code check before a business sees it. In each sentence, the first word
//! shaped like a contact removes that word and the rest of the sentence: a name after "ask for",
//! "talk to" or "I'm"; a contact phrase ("text me", "my number"); an email, link or spelled email;
//! a street number before a street word; three number words in a row; or 7 or more digits,
//! however spaced. Lines stay lines, so each card field is gated on its own line. Running the gate
//! twice changes nothing (TD-CARD-11).

/// Cues that put a name or on-site contact next: fire unless the next word is an everyday word.
const NAME_CUES: &str = "ask for|talk to|speak to|speak with|meet|pregunte por";
/// Words after a [`NAME_CUES`] cue that are not a name ("ask for a quote").
const EVERYDAY: &str = "a an the some more help quote quotes price prices estimate estimates it them details photos someone somebody";
/// Cues that fire only before a capitalised word ("I'm Pat", "Mrs. Lee", never "I'm worried").
const SELF_CUES: &str = "i'm|im|i am|this is|it's|mr|mrs|ms|dr|sr|sra";
/// Contact phrases: each fires wherever it appears.
const CONTACT_CUES: &str =
    "contact|my name|name is|phone number|cell number|whatsapp|me llamo|llame|llámame";
const VERBS: &str = "call text email e-mail reach message dm ring";
const MINE: &str = "my our mi";
/// After "my" or "our": a contact detail or an on-site person (CARD-005).
const MY_THINGS: &str = "number cell phone email address numero número husband wife son daughter tenant neighbor neighbour roommate manager";
const STREET_WORDS: &str = "st street ave avenue dr drive rd road ln lane blvd way ct court pl place cir circle pkwy hwy highway trl trail ter loop box calle avenida camino";
const NUMBER_WORDS: &str = "zero oh o one two three four five six seven eight nine ten eleven twelve twenty thirty forty fifty sixty seventy eighty ninety hundred thousand double triple cero uno dos tres cuatro cinco seis siete ocho nueve diez";
const TITLES: &str = "mr mrs ms dr sr sra";

/// Runs the CARD-008 gate on `text`, line by line and sentence by sentence (CARD-005, PRIV-006).
#[must_use]
pub fn final_gate(text: &str) -> String {
    let lines = text.lines().map(|line| {
        let kept: Vec<String> = sentences(line)
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

/// The words the gate counts for CARD-009: tokens holding a letter or digit.
#[must_use]
pub fn word_count(text: &str) -> usize {
    let words = text.split_whitespace();
    words
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// Splits a line after `!`, `?` or a `.` followed by a space, except after a digit or a title.
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
            '.' => spaced && !after_digit_or_title(&line[start..i]),
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

fn after_digit_or_title(before: &str) -> bool {
    before.ends_with(|c: char| c.is_ascii_digit())
        || before
            .split_whitespace()
            .last()
            .is_some_and(|w| listed(TITLES, &key(w)))
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
    let next = |len: usize| keys.get(i + len);
    let cue = |cues: &str| cue_at(keys, i, cues);
    let named = cue(NAME_CUES)
        .and_then(next)
        .is_some_and(|k| !listed(EVERYDAY, k));
    let stated = cue(SELF_CUES).is_some_and(|len| {
        words
            .get(i + len)
            .is_some_and(|w| *w != "I" && w.starts_with(char::is_uppercase))
    });
    let k = keys[i].as_str();
    let verb = listed(VERBS, k) && next(1).is_some_and(|n| n == "me" || n == "us");
    let mine = listed(MINE, k) && next(1).is_some_and(|n| listed(MY_THINGS, n));
    let web = words[i].contains('@') || ["http", "www"].iter().any(|p| k.starts_with(p));
    let spelled = next(1).is_some_and(|n| n == "at" || n == "arroba")
        && (2..=4).any(|n| next(n).is_some_and(|d| d == "dot" || d == "punto"));
    let street = !k.is_empty()
        && k.len() <= 6
        && k.chars().all(|c| c.is_ascii_digit())
        && (1..=4).any(|n| next(n).is_some_and(|s| listed(STREET_WORDS, s)));
    named
        || stated
        || cue(CONTACT_CUES).is_some()
        || verb
        || mine
        || web
        || spelled
        || street
        || number_run(keys, i)
}

/// Three number words in a row, or 7 or more digits and number words, from word `i` (CARD-008).
fn number_run(keys: &[String], i: usize) -> bool {
    let (mut words, mut digits) = (0, 0);
    for k in &keys[i..] {
        let parts: Vec<&str> = k.split('-').collect();
        if parts.iter().all(|p| listed(NUMBER_WORDS, p)) {
            words += parts.len();
        } else if k.chars().any(|c| c.is_ascii_digit())
            && k.chars()
                .all(|c| c.is_ascii_digit() || "()-+./#".contains(c))
        {
            digits += k.chars().filter(char::is_ascii_digit).count();
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

//! The 911-list stem matcher and the pure answers of the emergency flow (INT-002, INT-003,
//! INT-025, INT-030, INT-037, D-137, D-165, D-361, D-436).
//!
//! [`check`] runs on every inbound message before anything else (`service::receive_text`). It
//! folds the words (lower case, Spanish accents and apostrophes dropped), removes street-address
//! patterns first so "100 Smoketree Test Ave" is not smoke (D-165), and never looks at the answer to the
//! name-and-address question. It holds only the INT-004 911 list: a gas or propane smell or hiss, a
//! CO alarm or detector going off, smoke, fire or flames, a sparking, smoking or burning outlet,
//! plug, cord, switch, breaker or panel, and a downed power line. Floods, heat, no heat, symptoms
//! and a detector that chirps are not on it (D-436); a smoke or CO alarm said only to beep gets the
//! INT-037 question instead. The AI's emergency field never stands in for this match (INT-026).
//!
//! [`is_safe_reply`], [`urgency`], [`choice`] and [`beep_is_alarm`] read the homeowner's answers
//! to the SAFE request, "How soon?", the emergency-or-regular choice and the INT-037 question.

/// One entry of the 911 list (INT-002); [`Hazard::code`] is stored in `intake.emergency_event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hazard {
    /// A gas or propane smell, leak or hiss.
    Gas,
    /// A CO (or any) alarm or detector going off.
    CoAlarm,
    /// Smoke, fire or flames.
    SmokeFire,
    /// A sparking, smoking or burning outlet, plug, cord, switch, breaker or panel.
    Electrical,
    /// A downed or sparking power line.
    PowerLine,
}

impl Hazard {
    /// The stored code of the entry (`intake.emergency_event.matched`).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Gas => "gas",
            Self::CoAlarm => "co_alarm",
            Self::SmokeFire => "smoke_fire",
            Self::Electrical => "electrical",
            Self::PowerLine => "power_line",
        }
    }
}

/// What [`check`] found in one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// A 911-list match: the INT-004 text goes and cards wait for SAFE (INT-004, INT-005).
    Match(Hazard),
    /// A smoke or CO alarm said only to beep: ask the INT-037 question.
    BeepQuestion,
    /// Nothing on the list.
    Clear,
}

const GAS_WORDS: &[&str] = &["gas", "propane", "propano"];
const GAS_SIGNS: &[&str] = &[
    "leak", "hiss", "whistl", "smell", "odor", "odour", "stink", "fume",
];
const GAS_PHRASES: &[&str] = &[
    "rotten egg",
    "sulfur",
    "sulphur",
    "fuga de gas",
    "escape de gas",
    "huele a gas",
    "olor a gas",
    "sale gas",
    "huevo podrido",
];
const ALARM_WORDS: &[&str] = &["alarm", "detector", "alarma"];
const GOING_OFF: &[&str] = &[
    "going off",
    "goes off",
    "went off",
    "sounding",
    "ringing",
    "wont stop",
    "suena",
    "sonando",
];
const CHIRP_WORDS: &[&str] = &["chirp"];
const BEEP_WORDS: &[&str] = &["beep", "pita", "pitando"];
const BATTERY_WORDS: &[&str] = &[
    "battery",
    "batteri",
    "bateria",
    "low",
    "every minute",
    "once a minute",
    "cada minuto",
];
/// Phrases that never count as smoke or fire by themselves (INT-002).
const NOT_SMOKE: &[&str] = &[
    "smoke detector",
    "smoke alarm",
    "detector de humo",
    "alarma de humo",
    "fire pit",
    "fire table",
    "fire sprinkler",
];
const FIRE_PHRASES: &[&str] = &["en llamas"];
const FIRE_STEMS: &[&str] = &["smok", "flame", "humo", "fuego", "incendi"];
const ELECTRIC_PARTS: &[&str] = &[
    "outlet",
    "plug",
    "cord",
    "switch",
    "breaker",
    "panel",
    "enchufe",
    "toma",
    "cable",
    "interruptor",
];
const ELECTRIC_SIGNS: &[&str] = &["spark", "smok", "burn", "chispa", "quema"];
const POWER_LINE: &[&str] = &[
    "power line down",
    "power lines down",
    "line down",
    "lines down",
    "line is down",
    "lines are down",
    "wire down",
    "wires down",
    "downed power line",
    "downed line",
    "power line sparking",
    "cable caido",
];
/// Street suffixes whose name before them is removed before matching (D-165).
const STREET_SUFFIXES: &[&str] = &[
    "ave", "avenue", "st", "street", "ln", "lane", "rd", "road", "dr", "drive", "blvd", "ct",
    "court", "pl", "cir", "circle", "trl", "trail", "pkwy", "hwy", "loop",
];

/// The 911-list check of one message (INT-002, INT-037, D-165, D-361, D-436): `address_answer`
/// is true for the answer to the name-and-address question, which is never checked.
#[must_use]
pub fn check(words: &str, address_answer: bool) -> Check {
    if address_answer {
        return Check::Clear;
    }
    let text = Folded::new(&strip_addresses(words));
    if let Some(hazard) = hazard(&text) {
        return Check::Match(hazard);
    }
    let beeps = text.any_stem(BEEP_WORDS) && text.any_stem(ALARM_WORDS);
    if beeps && !text.any_stem(CHIRP_WORDS) && !text.any_phrase(BATTERY_WORDS) {
        Check::BeepQuestion
    } else {
        Check::Clear
    }
}

/// The first 911-list entry the folded message names (INT-002).
fn hazard(text: &Folded) -> Option<Hazard> {
    if text.any_phrase(POWER_LINE) {
        return Some(Hazard::PowerLine);
    }
    let gas = text.any_word(GAS_WORDS) && text.any_stem(GAS_SIGNS);
    if gas || text.any_phrase(GAS_PHRASES) {
        return Some(Hazard::Gas);
    }
    if text.any_stem(ELECTRIC_PARTS) && text.any_stem(ELECTRIC_SIGNS) {
        return Some(Hazard::Electrical);
    }
    let going_off = text.any_phrase(GOING_OFF)
        || (text.any_stem(&["loud"]) && !text.any_stem(CHIRP_WORDS) && !text.any_stem(BEEP_WORDS));
    if text.any_stem(ALARM_WORDS) && going_off {
        return Some(Hazard::CoAlarm);
    }
    let fire = Folded::new(&text.without(NOT_SMOKE));
    let burning = fire.any_word(&["fire", "fires"]) || fire.any_phrase(FIRE_PHRASES);
    if burning || fire.any_stem(FIRE_STEMS) {
        return Some(Hazard::SmokeFire);
    }
    None
}

/// Removes street-address patterns (D-165): a street suffix and the capitalized name words (and a
/// house number) right before it, so "100 Smoketree Test Ave" or "Burst Test Ln" is never a hazard.
fn strip_addresses(words: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for token in words.split_whitespace() {
        let bare = token.trim_matches(|c: char| !c.is_alphanumeric());
        let is_suffix = STREET_SUFFIXES.contains(&bare.to_lowercase().as_str());
        let named = kept.last().is_some_and(|w| capitalized(w));
        if !(is_suffix && named) {
            kept.push(token);
            continue;
        }
        while kept.last().is_some_and(|w| capitalized(w)) {
            kept.pop();
        }
        if kept
            .last()
            .is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()))
        {
            kept.pop();
        }
    }
    kept.join(" ")
}

/// Whether a word starts with a capital letter (a street name word).
fn capitalized(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

/// A message folded for matching: lower case, accents folded, apostrophes dropped, every other
/// non-letter a space, padded with one space each side.
struct Folded(String);

impl Folded {
    fn new(words: &str) -> Self {
        let mapped: String = words
            .to_lowercase()
            .chars()
            .filter(|c| !matches!(c, '\'' | '\u{2019}'))
            .map(|c| match c {
                'á' => 'a',
                'é' => 'e',
                'í' => 'i',
                'ó' => 'o',
                'ú' | 'ü' => 'u',
                'ñ' => 'n',
                c if c.is_alphanumeric() => c,
                _ => ' ',
            })
            .collect();
        Self(format!(
            " {} ",
            mapped.split_whitespace().collect::<Vec<_>>().join(" ")
        ))
    }

    fn words(&self) -> impl Iterator<Item = &str> {
        self.0.split_whitespace()
    }

    /// A whole word from `list`.
    fn any_word(&self, list: &[&str]) -> bool {
        self.words().any(|w| list.contains(&w))
    }

    /// A word beginning with a stem from `list`.
    fn any_stem(&self, list: &[&str]) -> bool {
        self.words().any(|w| list.iter().any(|s| w.starts_with(s)))
    }

    /// A run of whole words from `list` (a phrase may also be the start of the last word).
    fn any_phrase(&self, list: &[&str]) -> bool {
        list.iter().any(|p| self.0.contains(&format!(" {p}")))
    }

    /// The text with every phrase of `list` removed.
    fn without(&self, list: &[&str]) -> String {
        list.iter().fold(self.0.clone(), |text, p| {
            text.replace(&format!(" {p}"), " ")
        })
    }
}

/// Whether a reply is the safe reply (INT-025, D-128, D-419): safe, seguro or segura, "we're
/// safe", "we're ok" or "we're fine", or a plain OK or Yes; a "not" or "no" just before the safe
/// word means not safe.
#[must_use]
pub fn is_safe_reply(words: &str) -> bool {
    let text = Folded::new(words);
    let all: Vec<&str> = text.words().collect();
    if matches!(
        all.as_slice(),
        ["ok" | "okay" | "yes" | "si" | "yep" | "yeah"]
    ) {
        return true;
    }
    if text.any_phrase(&["were ok", "were fine", "we are ok", "we are fine"]) {
        return true;
    }
    all.iter().enumerate().any(|(i, w)| {
        let safe = matches!(*w, "safe" | "seguro" | "segura" | "seguros" | "seguras");
        let negated = all[i.saturating_sub(2)..i]
            .iter()
            .any(|n| matches!(*n, "not" | "no" | "isnt" | "arent" | "never"));
        safe && !negated
    })
}

/// An answer to "How soon do you need this?" (INT-030, D-208, D-292).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// 1) Today, it's urgent: the urgent flag, then the emergency-or-regular choice (INT-006).
    Today,
    /// 2) In the next few days.
    FewDays,
    /// 3) No rush.
    NoRush,
    /// 4) By a certain day: the day is asked next; otherwise handled as answer 2.
    ByDay,
}

impl Urgency {
    /// The stored option code (`intake.intake_answer.choice`, CARD-007).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::FewDays => "few_days",
            Self::NoRush => "no_rush",
            Self::ByDay => "by_day",
        }
    }
}

/// The "How soon?" answer in a reply: its number, or the option's own words; `None` otherwise
/// (INT-030). Other word answers are T040's.
#[must_use]
pub fn urgency(words: &str) -> Option<Urgency> {
    let text = Folded::new(words);
    let pick = |n: &str, words: &[&str]| text.any_word(&[n]) || text.any_phrase(words);
    if pick("1", &["today", "urgent", "hoy", "urgente"]) {
        Some(Urgency::Today)
    } else if pick("2", &["next few days", "few days", "unos dias"]) {
        Some(Urgency::FewDays)
    } else if pick("3", &["no rush", "sin prisa"]) {
        Some(Urgency::NoRush)
    } else if pick("4", &["certain day", "by a day", "un dia"]) {
        Some(Urgency::ByDay)
    } else {
        None
    }
}

/// The emergency-or-regular choice (INT-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Cards go at once, at night to after-hours businesses (INT-029, D-054).
    Emergency,
    /// The job runs as a regular one.
    Regular,
}

impl Choice {
    /// The stored option code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Emergency => "emergency",
            Self::Regular => "regular",
        }
    }
}

/// The choice in a reply to the INT-006 text: 1 or "emergency", 2 or "regular"; `None` otherwise.
#[must_use]
pub fn choice(words: &str) -> Option<Choice> {
    let text = Folded::new(words);
    if text.any_word(&["1"]) || text.any_stem(&["emergenc"]) {
        Some(Choice::Emergency)
    } else if text.any_word(&["2", "regular", "normal"]) {
        Some(Choice::Regular)
    } else {
        None
    }
}

/// Whether a reply to the INT-037 question says an alarm that keeps going off (answer 2); any
/// other answer is a chirp and the job continues as regular (INT-037, D-361).
#[must_use]
pub fn beep_is_alarm(words: &str) -> bool {
    let text = Folded::new(words);
    text.any_word(&["2"]) || text.any_phrase(&["keeps going", "going off", "alarm"])
}

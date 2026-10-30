//! Property tests for the scrubber (TD-AI-01 property, TD-AI-09; AI-002, AI-003, AI-020).
//!
//! Generated names after each AI-002 phrase in any case, phone numbers with any mix of spaces,
//! dots, dashes and brackets, street numbers and emails never survive [`scrub`]. Generated names
//! are fake letter strings kept off the common-words list.

use proptest::prelude::*;

use crate::api::{CommonWords, Known, NameMemory, scrub};

const PHRASES: &[&str] = &[
    "this is",
    "my name is",
    "I'm",
    "im",
    "it's",
    "soy",
    "me llamo",
    "mi nombre es",
];
const SEPARATORS: &[&str] = &["", " ", ".", "-", " - ", ") ", "  "];

fn scrubbed(text: &str) -> String {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(text, &known, &mut NameMemory::default())
        .as_str()
        .to_lowercase()
}

/// A random-case version of `s`, one flag per character.
fn recase(s: &str, upper: &[bool]) -> String {
    s.chars()
        .zip(upper.iter().cycle())
        .map(|(c, &u)| if u { c.to_ascii_uppercase() } else { c })
        .collect()
}

fn name() -> impl Strategy<Value = String> {
    "zq[a-z]{3,8}"
}

proptest! {
    /// TD-AI-01: a name after any AI-002 phrase, the phrase in any case, never survives.
    #[test]
    fn names_after_a_phrase_never_survive(
        phrase in 0..PHRASES.len(),
        caps in prop::collection::vec(any::<bool>(), 1..12),
        first in name(),
        lead in "(my sink leaks|hola|ok)",
    ) {
        let text = format!("{lead} {} {first}", recase(PHRASES[phrase], &caps));
        prop_assert!(!scrubbed(&text).contains(&first));
    }

    /// TD-AI-01: a 10-digit phone with any separators between its groups never survives.
    #[test]
    fn phones_never_survive(
        area in 200u32..999,
        line in 0u32..9999,
        seps in prop::collection::vec(0..SEPARATORS.len(), 2),
        bracket in any::<bool>(),
    ) {
        let area_text = if bracket { format!("({area})") } else { area.to_string() };
        let (a, b) = (SEPARATORS[seps[0]], SEPARATORS[seps[1]]);
        let line_text = format!("{line:04}");
        let text = format!("call {area_text}{a}555{b}{line_text} tonight");
        let out = scrubbed(&text);
        prop_assert!(!out.contains(&line_text), "{text} -> {out}");
    }

    /// TD-AI-01: a street number and street name never survive.
    #[test]
    fn street_numbers_never_survive(
        number in 1u32..99_999,
        street in name(),
        suffix in "(St|Ave|Way|Dr|Rd|Ln|Blvd|Ct)",
    ) {
        let text = format!("water heater out at {number} {street} {suffix} please");
        let out = scrubbed(&text);
        prop_assert!(!out.contains(&street) && !out.contains(&number.to_string()), "{out}");
    }

    /// TD-AI-01: an email, written plainly or with "at" and "dot", never survives.
    #[test]
    fn emails_never_survive(user in name(), host in name(), spelled in any::<bool>()) {
        let email = if spelled {
            format!("{user} at {host} dot com")
        } else {
            format!("{user}@{host}.com")
        };
        let out = scrubbed(&format!("reach me {email} thanks"));
        prop_assert!(!out.contains(&user) && !out.contains(&host), "{out}");
    }
}

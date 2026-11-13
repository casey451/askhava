//! TD-INT-40 (INT-002, D-165, D-982, D-2202): street-address removal does not depend on capitals.
//! A lower-case, capitalized or mixed-case address is never a hazard, a hazard beside a street
//! still matches in every case form, and no street name is written into the matcher's word lists.
//! Street names here are fake test fixtures.

use crate::emergency::{Check, Hazard, WORD_LISTS, check};

/// The D-165 address answers, in their lower-case form.
const ADDRESSES: &[&str] = &["100 smoketree test ave", "burst test ln"];

/// Hazards that name a street, in their lower-case form, with the entry each must match.
const HAZARDS: &[(&str, Hazard)] = &[
    ("gas leak at house on main st", Hazard::Gas),
    ("smoke coming from the outlet by 5th st", Hazard::Electrical),
];

/// The street names of the fixtures (and of `tests/emergency.rs`), none of which may sit in a list.
const FIXTURE_STREETS: &[&str] = &[
    "smoketree",
    "smoketre",
    "smoketree test",
    "burst",
    "burst test",
    "test",
    "main",
    "5th",
    "elm",
    "palm",
];

/// The lower-case, all-capitals, word-capitalized and mixed-case forms of a message.
fn case_forms(lower: &str) -> Vec<String> {
    let title = lower
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mixed = lower
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    vec![lower.to_owned(), lower.to_uppercase(), title, mixed]
}

/// TD-INT-40: the D-165 address answers are no hazard in any case form, alone or after a name.
#[test]
fn td_int_40_addresses_are_not_hazards_in_any_case() {
    for address in ADDRESSES {
        for form in case_forms(address) {
            assert_eq!(check(&form, false), Check::Clear, "{form}");
            let named = format!("pat test, {form}");
            assert_eq!(check(&named, false), Check::Clear, "{named}");
        }
    }
}

/// TD-INT-40: a hazard that names a street still matches in every case form.
#[test]
fn td_int_40_hazards_beside_a_street_match_in_any_case() {
    for (lower, hazard) in HAZARDS {
        for form in case_forms(lower) {
            assert_eq!(check(&form, false), Check::Match(*hazard), "{form}");
        }
    }
}

/// TD-INT-40: a lower-case hazard before a lower-case address keeps its hazard words.
#[test]
fn td_int_40_lower_case_hazard_beside_an_address_matches() {
    assert_eq!(
        check("i smell gas at 100 smoketree test ave", false),
        Check::Match(Hazard::Gas)
    );
    assert_eq!(
        check("outlet sparking at 12 palm ln", false),
        Check::Match(Hazard::Electrical)
    );
    assert_eq!(
        check("leaking faucet on burst test ln", false),
        Check::Clear
    );
}

/// TD-INT-40 (D-2202): no street name of any market is written in the matcher's word lists.
#[test]
fn td_int_40_no_street_name_in_the_word_lists() {
    for list in WORD_LISTS {
        for word in *list {
            assert!(
                !FIXTURE_STREETS.contains(word),
                "street name {word:?} written in a word list"
            );
        }
    }
}

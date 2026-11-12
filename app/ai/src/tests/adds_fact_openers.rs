//! TD-AI-26 (AI-019, T323, D-2209, D-3083): a capitalized word that starts a sentence, or a label
//! ending in a colon, is an added fact when her words never named it, unless it is an ordinary
//! opener of `data/sentence_openers.json`; the bank's and the card's fixed texts never trip it.
//! Fake data only.

use askhava_catalog::api::{question_bank, service_area_towns};
use askhava_core::api::brand;

use super::schemas::scrubbed;
use crate::output_checks::{CheckHit, is_opener};
use crate::writer::{Document, WriteSources, accept_document, accept_question};

const WITHOUT: &str = "the water heater is leaking and it broke, so the unit drips";
const WITH: &str = "the rheem water heater is leaking and it broke, so the unit drips";

/// The job card's fixed lines (`quotes::cards`), which the AI may echo.
const CARD_LINES: [&str; 9] = [
    "Homeowner prefers Spanish.",
    "No introduction fee for this job.",
    "Sent to 1 business.",
    "Sent to 3 businesses.",
    "How soon: Today, it's urgent",
    "How soon: In the next few days",
    "How soon: No rush",
    "How soon: Needed by Friday",
    "Example picture: kitchen sink",
];

/// The hit of `text` as a job card and as a follow-up question (no bank wording), against `words`.
fn hits(text: &str, words: &str) -> [Option<CheckHit>; 2] {
    let words = scrubbed(words);
    let sources = WriteSources {
        words: &words,
        answers: &[],
        photo_notes: &[],
        business_names: &[],
    };
    [
        accept_document(Document::JobCard, text, &sources).hit(),
        accept_question(text, "", &sources).hit(),
    ]
}

/// Every word of `text` except a capitalized one starting a sentence: the words a check of the
/// sentence starts alone must not need.
fn mid_sentence_words(text: &str) -> String {
    let mut starts = true;
    let mut kept = Vec::new();
    for token in text.split_whitespace() {
        let capital = token
            .chars()
            .find(|c| c.is_alphanumeric())
            .is_some_and(char::is_uppercase);
        if !(starts && capital) {
            kept.push(token);
        }
        starts = token.ends_with(['.', '!', '?', ':']);
    }
    kept.join(" ")
}

/// TD-AI-26 (AI-019, D-2209): a new name starting a sentence is an added fact; named by her, it passes.
#[test]
fn a_new_name_starting_a_sentence_adds_a_fact() {
    for text in [
        "Rheem unit is leaking.",
        "Water heater broke. Rheem says so.",
    ] {
        assert_eq!(
            hits(text, WITHOUT),
            [Some(CheckHit::AddedFact); 2],
            "{text}"
        );
        assert_eq!(hits(text, WITH), [None; 2], "{text}");
    }
}

/// TD-AI-26 (AI-019, D-3083): a label not on the openers list counts like any sentence-start word;
/// "Note:" is an opener.
#[test]
fn a_label_counts_unless_it_is_an_opener() {
    let text = "Rheem: unit is leaking.";
    assert_eq!(hits(text, WITHOUT), [Some(CheckHit::AddedFact); 2]);
    assert_eq!(hits(text, WITH), [None; 2]);
    assert_eq!(
        hits("Note: the water heater is leaking.", WITHOUT),
        [None; 2]
    );
}

/// TD-AI-26 (AI-019): ordinary English and Spanish openers add no fact.
#[test]
fn ordinary_openers_pass() {
    for text in ["The water heater is leaking.", "¿Cuándo empezó?"] {
        assert_eq!(hits(text, WITHOUT), [None; 2], "{text}");
    }
}

/// TD-AI-26 (AI-019, D-2209): every question of the bank, English and Spanish, and every fixed card
/// line adds no fact with only its own mid-sentence words given, so no sentence start of them trips
/// it (a bank question naming a price word is refused by that earlier check, never as a fact).
#[test]
fn fixed_texts_never_trip_it() {
    let bank = question_bank().unwrap();
    let questions = bank.sets.values().flat_map(|set| &set.questions);
    let texts = questions
        .flat_map(|q| [q.en.as_str(), q.es.as_str()])
        .chain(CARD_LINES);
    for text in texts {
        let added = hits(text, &mid_sentence_words(text)).contains(&Some(CheckHit::AddedFact));
        assert!(!added, "{text}");
    }
}

/// TD-AI-26 (AI-019, D-2209): the openers list holds no brand or market name.
#[test]
fn openers_hold_no_brand_or_market_name() {
    assert!(is_opener("Note") && is_opener("NOTA") && is_opener("cuando"));
    let names = brand();
    let brand_words = [names.display_name(), names.short_name(), names.legal_name()];
    let towns = service_area_towns().unwrap();
    let places = towns
        .iter()
        .flat_map(|t| [t.name.as_str(), t.market.as_str()]);
    for name in brand_words.into_iter().chain(places) {
        let words = name.split(|c: char| !c.is_alphanumeric());
        for word in words.filter(|w| w.chars().count() > 3) {
            assert!(!is_opener(word), "{word}");
        }
    }
}

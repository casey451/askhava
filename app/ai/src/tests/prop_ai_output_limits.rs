//! The AI-019 output checks and their fallbacks, over generated stand-in outputs (TD-AI-08,
//! TD-AI-10; AI-019, AI-021, CARD-008, CARD-010, WORD-003, WORD-022, D-492, D-493). Fake data only.

use askhava_catalog::api::RESTRICTED_WORDS;
use proptest::prelude::*;

use super::schemas::scrubbed;
use crate::output_checks::{CheckHit, word_022};
use crate::schemas::ScreenHit;
use crate::writer::{Document, WriteSources, accept_document, accept_question};

const WORDS: &str =
    "kitchen sink drips under the cabinet, water pools on the floor, it started Monday";
const TEMPLATE: &str = "Where is the leak? Reply 1 Kitchen, 2 Bathroom, 3 Outside or Not sure.";
const CLEAN: [&str; 4] = [
    "Kitchen sink drips under the cabinet.",
    "Water pools on the floor.",
    "It started Monday.",
    "The sink drips on the floor under the cabinet.",
];
const BUSINESS: &str = "Desert Fake Plumbing";
const PLANTED: [&str; 22] = [
    "$150",
    "150 dollars",
    "probably",
    "likely",
    "sounds like",
    "needs a new",
    "the problem is",
    "probablemente",
    "parece que",
    "seguramente",
    "suena a",
    "el problema es",
    "necesita un nuevo",
    "necesita una nueva",
    "PARECE QUE",
    "Suena á",
    "best",
    "vetted",
    "Desert Fake Plumbing",
    "928 555 0142",
    "nine two eight five",
    "text me",
];

fn with<T>(f: impl FnOnce(&WriteSources<'_>) -> T) -> T {
    let words = scrubbed(WORDS);
    let answers = ["Kitchen".to_owned()];
    let names = [BUSINESS.to_owned()];
    f(&WriteSources {
        words: &words,
        answers: &answers,
        photo_notes: &[],
        business_names: &names,
    })
}

fn word_022_phrase(index: prop::sample::Index) -> String {
    let list = word_022().unwrap();
    list[index.index(list.len())].replace('*', "ed")
}

fn base(picks: &[usize]) -> Vec<String> {
    picks.iter().map(|&i| CLEAN[i].to_owned()).collect()
}

/// Every generated output with a planted phrase falls back for all three kinds of text.
fn assert_all_fall_back(text: &str) {
    with(|s| {
        let question = accept_question(text, TEMPLATE, s);
        assert!(question.hit().is_some(), "{text}");
        assert_eq!(question.text(), TEMPLATE);
        for doc in [Document::JobCard, Document::ProjectFile] {
            let written = accept_document(doc, text, s);
            assert!(written.hit().is_some(), "{text}");
            assert_eq!(written.text(), s.words.as_str());
        }
    });
}

proptest! {
    /// TD-AI-08 (AI-019): an output built only from her words passes and is used as written.
    #[test]
    fn outputs_from_her_words_pass(picks in prop::collection::vec(0..CLEAN.len(), 1..4)) {
        let text = base(&picks).join(" ");
        with(|s| {
            prop_assert_eq!(accept_question(&text, TEMPLATE, s).hit(), None);
            let card = accept_document(Document::JobCard, &text, s);
            prop_assert_eq!(card.hit(), None);
            prop_assert_eq!(card.text(), text.as_str());
            Ok(())
        })?;
    }

    /// TD-AI-08 (AI-019, D-492, D-493): a dollar amount, a cause word (English or Spanish, any
    /// case, accent or none), a claim, a business name or a contact detail planted anywhere is
    /// rejected and the template or her scrubbed words used instead.
    #[test]
    fn a_planted_limit_breach_falls_back(
        picks in prop::collection::vec(0..CLEAN.len(), 1..4),
        planted in 0..PLANTED.len(),
        at in any::<prop::sample::Index>(),
        upper in any::<bool>(),
    ) {
        let mut sentences = base(&picks);
        let phrase = if upper { PLANTED[planted].to_uppercase() } else { PLANTED[planted].to_owned() };
        sentences.insert(at.index(sentences.len() + 1), format!("Note: {phrase}."));
        assert_all_fall_back(&sentences.join(" "));
    }

    /// TD-AI-08 (AI-019, WORD-022): every phrase of the WORD-022 list is rejected.
    #[test]
    fn a_word_022_phrase_falls_back(index in any::<prop::sample::Index>()) {
        assert_all_fall_back(&format!("Kitchen sink drips. {}.", word_022_phrase(index)));
    }
}

/// TD-AI-08 (AI-019, D-492): each check names its own hit; Spanish cause words in a Spanish card.
#[test]
fn each_check_names_its_hit() {
    let cases = [
        ("The sink drips, about $90.", CheckHit::Price),
        ("The sink probably drips.", CheckHit::Cause),
        ("La tubería probablemente gotea.", CheckHit::Cause),
        ("Parece que el fregadero gotea.", CheckHit::Cause),
        ("Seguramente gotea. Suena a una fuga.", CheckHit::Cause),
        ("El problema es la llave.", CheckHit::Cause),
        ("El fregadero necesita un nuevo sello.", CheckHit::Cause),
        ("La cocina necesita una nueva llave.", CheckHit::Cause),
        ("We will send a plumber.", CheckHit::NotAContractor),
        ("The best kitchen sink.", CheckHit::Claim),
        (
            "Ask desert fake plumbing about the sink.",
            CheckHit::BusinessName,
        ),
        ("The sink is 15 years old.", CheckHit::AddedFact),
        ("The sink is a Rheem model.", CheckHit::AddedFact),
        ("Water pools on the floor, ask for pat.", CheckHit::Contact),
        (
            "Water pools on the floor, nine two eight.",
            CheckHit::Contact,
        ),
    ];
    with(|s| {
        for (text, hit) in cases {
            assert_eq!(
                accept_document(Document::JobCard, text, s).hit(),
                Some(hit),
                "{text}"
            );
        }
    });
}

/// TD-AI-08 (AI-019): an approved card holds no fact absent from her words, answers and photo
/// notes: a number or a name she gave passes, one she did not falls back.
#[test]
fn a_card_holds_only_facts_she_gave() {
    let words = scrubbed("the water heater is 15 years old and leaks");
    let notes = ["A Rheem label on the tank".to_owned()];
    let sources = WriteSources {
        words: &words,
        answers: &[],
        photo_notes: &notes,
        business_names: &[],
    };
    let card = "The water heater is 15 years old. It leaks and has a Rheem label.";
    assert_eq!(
        accept_document(Document::JobCard, card, &sources).hit(),
        None
    );
    let added = "The water heater is 16 years old and leaks.";
    let written = accept_document(Document::JobCard, added, &sources);
    assert_eq!(written.hit(), Some(CheckHit::AddedFact));
    assert_eq!(written.text(), words.as_str());
}

/// TD-AI-10 (AI-021, CARD-010): a planted injection that makes the stand-in return a carrier-filtered
/// word, a URL or 600 characters is rejected and the fixed template goes out.
#[test]
fn an_injected_reply_sends_the_template() {
    let filtered = format!("Where is the leak? {}", RESTRICTED_WORDS[0]);
    let long = "Where is the leak? ".repeat(32);
    let cases = [
        (filtered.as_str(), None),
        (
            "Where is the leak? See evil.example.com/pay",
            Some(ScreenHit::Url),
        ),
        (long.as_str(), Some(ScreenHit::TooLong)),
    ];
    with(|s| {
        for (text, expected) in cases {
            let written = accept_question(text, TEMPLATE, s);
            assert!(
                matches!(written.hit(), Some(CheckHit::Screen(hit)) if expected.is_none_or(|e| e == hit))
            );
            assert_eq!(written.text(), TEMPLATE);
        }
    });
    assert!(long.chars().count() >= 600);
}

//! TD-CARD-02, TD-CARD-03 and TD-CARD-07 card cases (CARD-010, CARD-012, CARD-019, FEE-045;
//! rust brain R6, R17): the gate removes every carrier-restricted phrase and logs only the list
//! entry, the Spanish line shows only to a business that speaks Spanish, and every card says how
//! many businesses got it, with the fee-free line for the catalog's fee-free job types. All
//! descriptions here are made up.

use std::cell::RefCell;
use std::io::Write;
use std::sync::Once;

use askhava_catalog::api::{RESTRICTED_WORDS, parse, restricted_match};
use proptest::prelude::*;

use crate::api::{CardLines, CardParts, DEFAULT_MIN_WORDS, Place, Timing, build_card, final_gate};

const CATALOG: &str = include_str!("../../../../docs/catalog/categories.yaml");
const FILLER: [&str; 8] = [
    "garage", "cabinet", "hinge", "broken", "loose", "door", "shelf", "since",
];

fn parts(category: &str, description: &str) -> CardParts {
    CardParts {
        category: category.to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: description.to_owned(),
        answers: vec![("Where".to_owned(), "Inside the house".to_owned())],
        timing: Some(Timing::NoRush),
        danger: None,
        photos: Vec::new(),
        concept: None,
    }
}

fn lines(sent_to: u32, prefers_spanish: bool, fee_free: bool) -> CardLines {
    CardLines {
        sent_to,
        prefers_spanish,
        fee_free,
    }
}

thread_local! {
    /// What this test thread logged while [`logged`] listens; `None` drops the lines.
    static CAPTURE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// The writer of the one process-wide subscriber: it keeps a line only on a listening thread.
struct Sink;

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        CAPTURE.with_borrow_mut(|c| c.as_mut().map(|v| v.extend_from_slice(buf)));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Runs `run` and returns what it logged on this thread. One global subscriber is installed
/// once, so the removal's log callsite is always enabled: a scoped subscriber raced other test
/// threads registering the same callsite and could miss the first event (D-1034).
fn logged(run: impl FnOnce()) -> String {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(|| Sink)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
    });
    CAPTURE.set(Some(Vec::new()));
    run();
    let bytes = CAPTURE.take().unwrap_or_default();
    String::from_utf8(bytes).expect("utf-8 log")
}

/// TD-CARD-02: a planted phrase from each CARD-010 category (sex, hate, alcohol, firearms,
/// tobacco, cannabis and other drugs) leaves the card, and the log names the list entry, never
/// the description around it (CARD-010, D-485).
#[test]
fn td_card_02_each_category_is_removed_and_logged_by_entry() {
    let planted = [
        ("Porn", "porn"),
        ("Neo-Nazi", "neo nazi"),
        ("BEER", "beer"),
        ("shotgun", "shotgun"),
        ("Cigarettes", "cigarettes"),
        ("marijuana", "marijuana"),
        ("cocaine", "cocaine"),
        ("cerveza", "cerveza"),
    ];
    for (plant, entry) in planted {
        let text = format!("The garage cabinet by the {plant} shelf has a broken hinge.");
        let mut card = None;
        let log = logged(|| card = Some(build_card(&parts("Handyman", &text), DEFAULT_MIN_WORDS)));
        let card = card.expect("card");
        assert_eq!(restricted_match(card.text()), None, "{plant} left the card");
        assert!(
            card.text()
                .contains("garage cabinet by the shelf has a broken hinge.")
        );
        assert_eq!(log.matches("restricted phrase removed").count(), 1, "{log}");
        assert!(log.contains(&format!("entry=\"{entry}\"")), "{log}");
        assert!(!log.contains("garage") && !log.contains("hinge"), "{log}");
    }
}

/// TD-CARD-02: every entry on the list is removed from a description, a card-field answer and a
/// line end alike (CARD-010).
#[test]
fn td_card_02_every_list_entry_is_removed() {
    for entry in RESTRICTED_WORDS {
        let text = format!("Fix the shelf near the {entry}. The door near the {entry} sticks");
        let gated = final_gate(&text);
        assert_eq!(restricted_match(&gated), None, "{entry}: {gated}");
        assert_eq!(
            gated, "Fix the shelf near the. The door near the sticks",
            "{entry}"
        );
    }
}

/// TD-CARD-02: a description with no listed phrase passes unchanged, everyday trade words too
/// (CARD-010, D-813).
#[test]
fn td_card_02_no_listed_phrase_passes_unchanged() {
    let texts = [
        "Pull the weed in the side yard and reseal the pipe joint, please.",
        "Fill the crack in the wall by the wine fridge in Essex style.",
        "Clean the gunk out of the nail gun and the heat gun.",
    ];
    for text in texts {
        let log = logged(|| assert_eq!(final_gate(text), text));
        assert!(log.is_empty(), "{log}");
    }
}

/// TD-CARD-02 with CARD-008: a domain holding a listed word is cut whole by the contact gate,
/// never split by the restricted removal into a shape that escapes it (PRIV-006).
#[test]
fn td_card_02_domain_with_a_listed_word_is_still_cut() {
    for domain in ["joes.beer.biz", "havasu.vape.shop"] {
        let gated = final_gate(&format!("Find us at {domain} today"));
        assert_eq!(gated, "Find.", "{domain}");
    }
}

/// TD-CARD-02: a hyphenated listed word leaves no fragment ("e-" from "e-cigarette").
#[test]
fn td_card_02_hyphen_fragment_is_dropped() {
    assert_eq!(
        final_gate("The e-cigarette shelf broke."),
        "The shelf broke."
    );
    assert_eq!(
        final_gate("Fix the shelf by the e-cigarette."),
        "Fix the shelf by the."
    );
}

proptest! {
    /// TD-CARD-02 property: whatever mix of words and list entries, no entry survives the gate,
    /// and gating again changes nothing (CARD-010, TD-CARD-11).
    #[test]
    fn td_card_02_no_entry_survives(
        words in prop::collection::vec(
            prop_oneof![
                prop::sample::select(FILLER.to_vec()),
                prop::sample::select(RESTRICTED_WORDS.to_vec()),
            ],
            1..20,
        ),
    ) {
        let gated = final_gate(&words.join(" "));
        prop_assert_eq!(restricted_match(&gated), None);
        prop_assert_eq!(final_gate(&gated), gated.clone());
    }
}

/// TD-CARD-03: "Homeowner prefers Spanish" shows only to a business that ticked "We speak
/// Spanish"; the stored text is the same for every business, and no card says own or rent or
/// "Landlord approval" (CARD-012, D-437).
#[test]
fn td_card_03_spanish_line_only_for_spanish_speakers() {
    let card = build_card(
        &parts("Plumbing", "The water heater leaks under the tank."),
        DEFAULT_MIN_WORDS,
    );
    let spanish = "Homeowner prefers Spanish";
    assert!(
        card.text_for(&lines(3, true, false), true)
            .contains(spanish)
    );
    assert!(
        !card
            .text_for(&lines(3, true, false), false)
            .contains(spanish)
    );
    assert!(
        !card
            .text_for(&lines(3, false, false), true)
            .contains(spanish)
    );
    assert!(!card.text().contains(spanish));
    for (prefers, speaks) in [(true, true), (true, false), (false, true), (false, false)] {
        let text = card
            .text_for(&lines(3, prefers, false), speaks)
            .to_lowercase();
        assert!(!text.contains("own or rent") && !text.contains("landlord approval"));
        assert!(text.starts_with(&card.text().to_lowercase()));
    }
}

/// TD-CARD-07: given N, the card says "Sent to <N> businesses." as its last line, with no cap
/// on N (CARD-019, CARD-003, D-188, D-1032).
#[test]
fn td_card_07_sent_to_line() {
    let card = build_card(
        &parts("Plumbing", "The water heater leaks under the tank."),
        DEFAULT_MIN_WORDS,
    );
    for (n, line) in [
        (3, "Sent to 3 businesses."),
        (4, "Sent to 4 businesses."),
        (1, "Sent to 1 business."),
        (5000, "Sent to 5000 businesses."),
    ] {
        let text = card.text_for(&lines(n, false, false), false);
        assert_eq!(text.lines().last(), Some(line));
        assert!(!text.contains("introduction fee"));
    }
}

/// TD-CARD-07: a buyer, pre-listing or refinance inspection or a termite report for a sale or
/// loan (the catalog's `fee_free_job_types`) says "No introduction fee for this job."; the other
/// inspection does not (FEE-045, D-403).
#[test]
fn td_card_07_fee_free_job_types_say_no_fee() {
    let catalog = parse(CATALOG).expect("catalog");
    let def = catalog
        .categories
        .iter()
        .find(|c| !c.fee_free_job_types.is_empty())
        .expect("a category with fee-free job types");
    let card = build_card(
        &parts(
            &def.name,
            "The house needs an inspection before the sale closes.",
        ),
        DEFAULT_MIN_WORDS,
    );
    let named = [
        "buyer inspection",
        "pre-listing inspection",
        "refinance inspection",
        "termite report for a sale or loan",
    ];
    for job_type in &def.job_types {
        let fee_free = def.fee_free_job_types.contains(job_type);
        assert_eq!(fee_free, named.contains(&job_type.as_str()), "{job_type}");
        let text = card.text_for(&lines(4, false, fee_free), false);
        let says = text.contains("\nNo introduction fee for this job.\nSent to 4 businesses.");
        assert_eq!(says, fee_free, "{job_type}");
    }
}

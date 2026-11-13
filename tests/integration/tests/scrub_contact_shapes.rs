//! TD-AI-27 (T351, D-2622; AI-002, AI-020, CARD-007, CARD-008, PRIV-005): contact shapes the
//! Nov 10 review found passing both the scrubber and the card's final gate are now cut, and the
//! everyday words beside them are not.
//!
//! Each shape runs through `privacy::api::scrub` with an empty context (no vault name, nothing
//! remembered) and then `quotes::api::final_gate`, the path a homeowner's words take to every
//! business's job card before any pick (CARD-007). No code digit run, house number, phone, email
//! local part or name may be left. The CARD-008 code-cue cut is also checked on raw text, as the
//! gate's own last line of defence. Every name, number and address is fictional.

use askhava_privacy::api::{CommonWords, Known, NameMemory, scrub};
use askhava_quotes::api::final_gate;

/// Each shape and the lower-cased pieces that must not survive (D-2622).
const SHAPES: &[(&str, &[&str])] = &[
    ("Gate code is 4 4 1 2", &["4 4 1 2", "4412"]),
    ("Gate code 44 12", &["44 12", "4412"]),
    ("i live at 2150 mesquite in havasu", &["2150", "mesquite"]),
    ("928 dash 555 dash 0134", &["928", "555", "0134"]),
    ("Steve Smith here", &["steve", "smith"]),
    ("Hi, it is Steve", &["steve"]),
    ("Hey its steve", &["steve"]),
    ("Hi, Steve here", &["steve"]),
    ("Sink is leaking. Best, Steve", &["steve"]),
    (
        "Thanks, Steve Smith 928-555-0134",
        &["steve", "smith", "555", "0134"],
    ),
    ("steve(at)gmail(dot)com", &["steve", "gmail"]),
    ("reach me at steve @ gmail.com", &["steve", "gmail"]),
];

/// Everyday words beside the new rules that the scrubber leaves as they are (D-2622).
const KEPT: &[&str] = &[
    "the 2 4x4 posts",
    "call me back",
    "it is leaking",
    "best price",
    "Ask for 3 quotes",
    "error code 41",
    "code E4",
    "is 1500 square feet",
    "its leaking",
    "its compressor died",
    "it is rusty",
    "it is Tuesday",
    "Mold here too",
    "Water here.",
    "Damage here, under the sink",
    "The dryer outlet is 240 volt",
    "it is 1200 sf",
];

/// The same everyday words the CARD-008 code-cue cut must not touch on a card.
const CARD_KEPT: &[&str] = &[
    "the 2 4x4 posts",
    "it is leaking",
    "best price",
    "error code 41",
    "code E4",
    "is 1500 square feet",
    "its leaking",
];

fn scrubbed(text: &str) -> String {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(text, &known, &mut NameMemory::default())
        .as_str()
        .to_owned()
}

/// Every shape whose output under `path` still holds one of its pieces, with that output.
fn kept_pieces(path: impl Fn(&str) -> String) -> Vec<(&'static str, String)> {
    let mut kept = Vec::new();
    for (shape, pieces) in SHAPES {
        let out = path(shape).to_lowercase();
        if pieces.iter().any(|piece| out.contains(piece)) {
            kept.push((*shape, out));
        }
    }
    kept
}

/// TD-AI-27: scrub then the final gate keeps no piece of any contact shape (AI-002, AI-020,
/// CARD-007, PRIV-005).
#[test]
fn td_ai_27_every_shape_is_cut_before_a_card() {
    let kept = kept_pieces(|shape| final_gate(&scrubbed(shape)));
    assert!(kept.is_empty(), "kept on a card: {kept:?}");
}

/// TD-AI-27: the scrubber alone already cuts each shape (AI-002, AI-020).
#[test]
fn td_ai_27_the_scrubber_alone_cuts_each_shape() {
    let kept = kept_pieces(scrubbed);
    assert!(kept.is_empty(), "kept by the scrubber: {kept:?}");
}

/// TD-AI-27: the everyday words beside the new rules pass the scrubber unchanged (AI-002, D-146).
#[test]
fn td_ai_27_everyday_words_stay() {
    for text in KEPT {
        assert_eq!(scrubbed(text), *text);
    }
}

/// TD-AI-27 (CARD-008): the gate on its own cuts the rest of a sentence holding a code cue and
/// a spaced run of 3 or more digits, and leaves short or lettered codes alone.
#[test]
fn td_ai_27_the_gate_cuts_a_spaced_code() {
    assert_eq!(final_gate("Gate code is 4 4 1 2"), "");
    assert_eq!(
        final_gate("Leak by the side. Gate code 44 12."),
        "Leak by the side."
    );
    assert_eq!(final_gate("Back door, pin #2468 works."), "Back door.");
    for text in CARD_KEPT {
        assert_eq!(final_gate(text), *text);
    }
}

/// TD-AI-27 (D-3317): a job word after a loose name phrase is neither cut nor remembered, so a
/// later message keeps it, while a greeted name is still cut and remembered.
#[test]
fn td_ai_27_job_words_are_not_remembered_as_names() {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let mut memory = NameMemory::default();
    for first in ["its compressor died", "Mold here too", "it is rusty"] {
        assert_eq!(scrub(first, &known, &mut memory).as_str(), first);
    }
    let later = "Can you replace the compressor? The mold is rusty";
    assert_eq!(scrub(later, &known, &mut memory).as_str(), later);
    scrub("Hey its zebulon", &known, &mut memory);
    assert_eq!(
        scrub("zebulon again", &known, &mut memory).as_str(),
        "[removed] again"
    );
}

//! Design categories: the look pictures, the project file and the concept image's rounds
//! (INT-045, INT-046; D-433, D-492).
//!
//! [`is_design_category`] reads `design: true` from `docs/catalog/categories.yaml` (compiled in),
//! for range categories only. There the request asks [`LOOK_ASK_TEMPLATE`] after the photos; her
//! look pictures are taken as photos (INT-044). Until the AI may read photos she is asked to
//! describe the look in a few words ([`LOOK_DESCRIBE_TEMPLATE`]), which becomes the project file
//! (`privacy::api::ProjectFile`). While the "Concept images" switch is on, `privacy::api::make_concept`
//! makes one image from that file only, and [`CONCEPT_OFFER_TEMPLATE`] shows it with its label and
//! INT-046's question; [`concept_step`] reads her reply: YES puts it on the card, SKIP goes on
//! without it, anything else is a change, at most `CONCEPT_CHANGES` (3), a fourth refused.

use std::sync::OnceLock;

use askhava_privacy::api::CONCEPT_CHANGES;
use serde::Deserialize;

/// "Have any pictures of the look you want? Screenshots are fine." (INT-045).
pub const LOOK_ASK_TEMPLATE: &str = "look_ask";
/// The look in a few words, before the AI may read photos (INT-045).
pub const LOOK_DESCRIBE_TEMPLATE: &str = "look_describe";
/// The concept image's link with its label and INT-046's question.
pub const CONCEPT_OFFER_TEMPLATE: &str = "concept_offer";
/// INT-046's question, word for word.
pub const CONCEPT_QUESTION: &str =
    "Is this the kind of thing you want? Reply YES, or tell us what to change.";

const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");

#[derive(Deserialize)]
struct File {
    categories: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    #[serde(default)]
    quote_format: String,
    #[serde(default)]
    design: bool,
}

/// INT-045, D-433: whether `category` is marked `design: true` with range quotes; an unknown code
/// or an unreadable file is not.
#[must_use]
pub fn is_design_category(category: &str) -> bool {
    static DESIGN: OnceLock<Vec<String>> = OnceLock::new();
    DESIGN
        .get_or_init(|| {
            serde_saphyr::from_str::<File>(CATEGORIES)
                .map(|file| file.categories)
                .unwrap_or_default()
                .into_iter()
                .filter(|row| row.design && row.quote_format == "range")
                .map(|row| row.id)
                .collect()
        })
        .iter()
        .any(|id| id == category)
}

/// What to do after her reply to a concept image (INT-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConceptStep {
    /// YES: this image goes on the card with its label.
    Accept,
    /// SKIP: go on with no concept image.
    Skip,
    /// A change: make change round `n` (1 to `CONCEPT_CHANGES`) from the updated project file.
    Remake(u8),
    /// A change after the last round: no new image; she may reply YES or SKIP.
    NoMoreChanges,
}

/// INT-046: her reply to the image of change round `changes` (0 for the first image).
#[must_use]
pub fn concept_step(words: &str, changes: u8) -> ConceptStep {
    let word = crate::rules::fold(words.trim().trim_end_matches(['.', '!']));
    match word.as_str() {
        "yes" | "si" => ConceptStep::Accept,
        "skip" | "omitir" => ConceptStep::Skip,
        _ if changes >= CONCEPT_CHANGES => ConceptStep::NoMoreChanges,
        _ => ConceptStep::Remake(changes + 1),
    }
}

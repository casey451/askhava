//! The anonymous job card: what a business sees of a job (CARD-005 to CARD-009, PRIV-006).
//!
//! A [`JobCard`] holds the town, the kind of place, the screened card photos and the card text,
//! and nothing else: no field exists for a name, phone, email, street address, on-site contact or
//! original photo. The text is the approved description (the AI card or the scrubbed words,
//! INT-041), then one line per card-field answer (LIC-040, D-202), the "How soon?" answer
//! (INT-030, D-292), the 911 line when that text was sent (D-436) and any concept image label
//! (INT-046), each field through the final gate ([`crate::gate::final_gate`], CARD-008) and the
//! whole text through it again last. A description left with fewer than the setting's words
//! (default 5) becomes "<Category> job (details on the job page)" (CARD-009, D-434).
//!
//! [`JobCard::text_for`] is the text one business gets: the stored text, then "Homeowner prefers
//! Spanish" for a business that speaks Spanish (CARD-012), "No introduction fee for this job." for
//! a fee-free job type (FEE-045) and "Sent to <N> businesses." (CARD-019), all fixed wording.

use sqlx::types::Uuid;

use crate::gate::{final_gate, word_count};
use crate::models::CardLines;

/// CARD-009's default: a gated description with fewer words than this becomes the fallback line.
/// The caller passes the setting's value to [`build_card`].
pub const DEFAULT_MIN_WORDS: usize = 5;

/// The kind of place a job is at, the only location detail besides the town (CARD-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A house.
    House,
    /// A marina.
    Marina,
    /// A storage lot.
    StorageLot,
    /// A boat ramp.
    Ramp,
}

/// The "How soon?" answer shown on the card (INT-030, D-292).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timing {
    /// 1) Today, it's urgent.
    Today,
    /// 2) In the next few days.
    NextFewDays,
    /// 3) No rush.
    NoRush,
    /// 4) By a certain day: the day as the homeowner gave it ("Wed").
    NeededBy(String),
}

/// The 911-list situation when the fixed 911 text was sent (INT-004, D-436); never her words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Danger {
    /// A gas or propane smell or hiss.
    Gas,
    /// A CO alarm going off.
    CoAlarm,
    /// Smoke, fire or flames.
    SmokeOrFire,
    /// A sparking, smoking or burning outlet, cord, switch or panel.
    Electrical,
    /// A downed or sparking power line.
    PowerLine,
}

/// What a card is built from (CARD-007). Every text here is already scrubbed or fixed wording.
/// Its `Debug` hides the description, answers and concept label, so a log never holds her words.
#[derive(Clone)]
pub struct CardParts {
    /// The category's display name, for the CARD-009 line ("Plumbing").
    pub category: String,
    /// The town's display name, an Arizona catalog town (CARD-002, CARD-006).
    pub town: String,
    /// The kind of place (CARD-006).
    pub place: Place,
    /// The approved description: the AI card (AI-019) or her scrubbed words (INT-041).
    pub description: String,
    /// The card-field answers as (question label, answer) fixed wording (LIC-040, D-202).
    pub answers: Vec<(String, String)>,
    /// The "How soon?" answer (INT-030).
    pub timing: Option<Timing>,
    /// The 911 situation, when the 911 text was sent (D-436).
    pub danger: Option<Danger>,
    /// The IDs of the screened card photos, never an original (CARD-013, CARD-014, PRIV-004).
    pub photos: Vec<Uuid>,
    /// A concept image's screened ID and its label (INT-046).
    pub concept: Option<(Uuid, String)>,
}

impl std::fmt::Debug for CardParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardParts")
            .field("category", &self.category)
            .field("town", &self.town)
            .field("place", &self.place)
            .field("description", &"<hidden>")
            .field("answers", &self.answers.len())
            .field("timing", &self.timing.is_some())
            .field("danger", &self.danger)
            .field("photos", &self.photos.len())
            .field("concept", &self.concept.is_some())
            .finish()
    }
}

/// A built job card. Its fields are private and [`build_card`] is the only way to make one, so
/// every card has passed the final gate (CARD-005, CARD-008, PRIV-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobCard {
    town: String,
    place: Place,
    photos: Vec<Uuid>,
    text: String,
}

impl JobCard {
    /// The town shown on the card (CARD-006).
    #[must_use]
    pub fn town(&self) -> &str {
        &self.town
    }

    /// The kind of place shown on the card (CARD-006).
    #[must_use]
    pub fn place(&self) -> Place {
        self.place
    }

    /// The screened card photos and any concept image (CARD-013, INT-046).
    #[must_use]
    pub fn photos(&self) -> &[Uuid] {
        &self.photos
    }

    /// The card text, stored and sent to businesses character for character (TD-CARD-11).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text sent to one business: the stored text, then "Homeowner prefers Spanish" only when
    /// she prefers it and `speaks_spanish` (the business ticked "We speak Spanish", CARD-012),
    /// "No introduction fee for this job." for a fee-free job type (FEE-045) and the
    /// "Sent to <N> businesses." line, "Sent to 1 business." for one (CARD-019, D-1032).
    #[must_use]
    pub fn text_for(&self, lines: &CardLines, speaks_spanish: bool) -> String {
        let mut out = vec![self.text.clone()];
        if lines.prefers_spanish && speaks_spanish {
            out.push("Homeowner prefers Spanish.".to_owned());
        }
        if lines.fee_free {
            out.push("No introduction fee for this job.".to_owned());
        }
        out.push(match lines.sent_to {
            1 => "Sent to 1 business.".to_owned(),
            n => format!("Sent to {n} businesses."),
        });
        out.join("\n")
    }
}

/// Builds the card from its parts, gating every field and then the whole text (CARD-005 to
/// CARD-009, PRIV-006). `min_words` is CARD-009's setting ([`DEFAULT_MIN_WORDS`]); it is checked
/// on the gate's fixed point, so the stored description is the one counted.
#[must_use]
pub fn build_card(parts: &CardParts, min_words: usize) -> JobCard {
    let description = final_gate(&parts.description);
    let mut lines = vec![if word_count(&description) < min_words {
        format!("{} job (details on the job page)", parts.category)
    } else {
        description
    }];
    for (label, answer) in &parts.answers {
        lines.push(format!("{label}: {answer}"));
    }
    lines.extend(
        parts
            .timing
            .as_ref()
            .map(|t| format!("How soon: {}", timing_text(t))),
    );
    lines.extend(
        parts
            .danger
            .map(|d| format!("911 text sent: {}", danger_text(d))),
    );
    lines.extend(
        parts
            .concept
            .as_ref()
            .map(|(_, l)| format!("Example picture: {l}")),
    );
    let gated: Vec<String> = lines.iter().map(|l| final_gate(l)).collect();
    let mut photos = parts.photos.clone();
    photos.extend(parts.concept.as_ref().map(|(id, _)| *id));
    JobCard {
        town: final_gate(&parts.town),
        place: parts.place,
        photos,
        text: final_gate(&gated.join("\n")),
    }
}

/// The card wording of each "How soon?" answer (INT-030, D-292).
fn timing_text(timing: &Timing) -> String {
    match timing {
        Timing::Today => "Today, it's urgent".to_owned(),
        Timing::NextFewDays => "In the next few days".to_owned(),
        Timing::NoRush => "No rush".to_owned(),
        Timing::NeededBy(day) => format!("Needed by {day}"),
    }
}

/// The fixed 911 wording of CARD-007 (D-436).
fn danger_text(danger: Danger) -> &'static str {
    match danger {
        Danger::Gas => "gas",
        Danger::CoAlarm => "CO alarm",
        Danger::SmokeOrFire => "smoke or fire",
        Danger::Electrical => "electrical",
        Danger::PowerLine => "power line",
    }
}

//! The one INT-004 911 text (INT-004, D-436).
//!
//! A 911-list match (INT-002, T039's stem matcher) gets exactly one safety text: the fixed
//! `emergency_911` template, in English or Spanish, sent within 5 minutes at any hour. Nothing
//! here holds a local phone number, a utility, 211 or any other safety, heat or health advice:
//! the safety-contacts file of D-350 is withdrawn (D-436), so this module loads no file and the
//! only number a homeowner is ever given is 911. `tests/docs/test_no_phone_numbers_in_templates.py`
//! fails the build on any other number in a template or page.
//!
//! [`crate::rules::plan`] puts the text first (before the consent text, D-008); this module gives
//! its words, its quiet-hours kind and its deadline to the code that queues and sends it.

use askhava_messaging::api::{Channel, Kind, Language, Outgoing, render};
use jiff::{SignedDuration, Timestamp};

use crate::models::IntakeError;
use crate::rules::SAFETY_TEMPLATE;

/// How long after the homeowner's message the INT-004 text may take to go, at any hour (INT-004).
pub const SAFETY_WITHIN: SignedDuration = SignedDuration::from_mins(5);

/// The INT-004 text in `language`, exactly as its template holds it (INT-004, D-436). The Spanish
/// template is marked to stay on, so a Spanish-language match gets the Spanish text even while
/// Spanish conversations are off (TXT-039, D-492).
///
/// # Errors
///
/// [`IntakeError::Template`] if the compiled-in templates are broken.
pub fn safety_text(language: Language) -> Result<String, IntakeError> {
    render(SAFETY_TEMPLATE, language, false, &[])
        .map(|text| text.body)
        .map_err(|_| IntakeError::Template)
}

/// The quiet-hours view of the INT-004 text to `phone` (`+1` number): kind
/// [`Kind::Safety911`], which QH-004 lets through at any hour, so the text is never held for the
/// morning (INT-004).
#[must_use]
pub fn safety_outgoing(phone: &str) -> Outgoing<'_> {
    Outgoing {
        phone,
        kind: Kind::Safety911,
        channel: Channel::Text,
        emergency_job: true,
        after_hours_business: false,
        emergency_emails: false,
    }
}

/// Whether a safety text sent at `sent_at` for a message received at `received_at` went within
/// [`SAFETY_WITHIN`] (INT-004); a late or not-yet-sent text is the caller's to alert on.
#[must_use]
pub fn sent_in_time(received_at: Timestamp, sent_at: Timestamp) -> bool {
    let took = sent_at.duration_since(received_at);
    took >= SignedDuration::ZERO && took <= SAFETY_WITHIN
}

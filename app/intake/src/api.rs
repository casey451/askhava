//! The public face of `askhava-intake`: everything other crates may call (D-481).
//!
//! The first reply to a request (CON-001 to CON-004, CON-026, CON-027, CON-031): [`on_text`] opens
//! a request with its consent text, after the 911 text on a 911-list match, and hands every later
//! reply to the conversation; the row types of the `intake` tables (D-492, D-493).

pub use crate::models::{
    ConsentRecordRow, ConversationRow, EmergencyEventRow, HomeownerRow, IntakeAnswerRow,
    IntakeError, PropertyRow, RecruitingDemandRow,
};
pub use crate::service::{Handled, Queued, TextIn, conversation, disclosure, on_text};

/// The pure first-reply rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{
        CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, DISCLOSURE_TEMPLATE, Inbound, Plan,
        SAFETY_TEMPLATE, Send, Settings, Then, asks_who, consent_language, consent_template,
        language_code, language_from_code, plan, reads_spanish, wanted_language,
    };
}

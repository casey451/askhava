//! The public face of `askhava-intake`: everything other crates may call (D-481).
//!
//! The first reply to a request (CON-001 to CON-004, CON-026, CON-027, CON-031): [`receive_text`]
//! opens a request with its consent text, after the 911 text on a 911-list match, and hands every
//! later reply to the conversation; its consent record and the gates on cards and texts (CON-005,
//! CON-007, CON-021); the homeowner terms acceptance at a pick (TERMS-001, TERMS-016, TERMS-033);
//! the row types of the `intake` tables (D-492, D-493); the one INT-004 911 text, sent at any hour
//! within [`SAFETY_WITHIN`] (D-436).

pub use crate::models::{
    ConsentRecordRow, ConversationRow, EmergencyEventRow, HomeownerRow, IntakeAnswerRow,
    IntakeError, PropertyRow, RecruitingDemandRow, TermsAcceptanceRow,
};
pub use crate::safety_text::{SAFETY_WITHIN, safety_outgoing, safety_text, sent_in_time};
/// The first-reply step without the consent record, for this crate's own tests only.
#[cfg(test)]
pub(crate) use crate::service::on_text;
pub use crate::service::{
    Handled, Queued, TermsAcceptance, TermsChannel, TextIn, card_allowed, consent_records,
    conversation, disclosure, may_text, receive_text, record_terms_acceptance, terms_acceptances,
};

/// The pure first-reply rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{
        CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, DISCLOSURE_TEMPLATE, Inbound, Plan,
        SAFETY_TEMPLATE, Send, Settings, Then, asks_who, consent_language, consent_template,
        language_code, language_from_code, plan, reads_spanish, wanted_language,
    };
}

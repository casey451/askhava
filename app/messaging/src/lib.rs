//! Messaging (`askhava-messaging`, layer 3): the text templates and quiet hours, so far (TXT-001 to
//! TXT-008, TXT-010, TXT-039, BIZ-018, WORD-008, QH-001 to QH-009, TXT-025).
//!
//! `messaging` builds every text from a fixed English or Spanish template chosen by ID and language.
//! Other crates reach it only through [`api`]. Quiet hours (`rules`) decide when a text or business email may go.
//! The outbox, inbox and opt-out list arrive in later tasks.

pub mod api;
mod rules;
mod templates;

#[cfg(test)]
mod tests;

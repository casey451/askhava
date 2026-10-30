//! Messaging (`askhava-messaging`, layer 3): the text templates, so far (TXT-001 to TXT-008, TXT-010,
//! TXT-039, BIZ-018, WORD-008).
//!
//! `messaging` builds every text from a fixed English or Spanish template chosen by ID and language.
//! Other crates reach it only through [`api`]. The outbox, inbox, opt-out list and quiet hours arrive
//! in later tasks.

pub mod api;
mod templates;

#[cfg(test)]
mod tests;

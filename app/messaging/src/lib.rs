//! Messaging (`askhava-messaging`, layer 3): the text templates, quiet hours and the outbox, so far
//! (TXT-001 to TXT-011, TXT-039, BIZ-018, WORD-008, QH-001 to QH-009, TXT-025).
//!
//! `messaging` builds every text from a fixed English or Spanish template chosen by ID and language.
//! Other crates reach it only through [`api`]. Quiet hours (`rules`) decide when a text or business email may go.
//! Every text goes through the outbox (`service`, `models`) under a derived idempotency key, never
//! to a past customer who has not written first (TXT-009, TXT-011); the sender directory maps
//! business phones to their business (D-492). The inbox and opt-out list arrive in later tasks.

pub mod api;
mod models;
mod rules;
mod service;
mod templates;

#[cfg(test)]
mod tests;

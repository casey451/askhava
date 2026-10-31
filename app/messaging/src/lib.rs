//! Messaging (`askhava-messaging`, layer 3): the text templates, quiet hours and the outbox, so far
//! (TXT-001 to TXT-013, TXT-039, BIZ-018, WORD-008, QH-001 to QH-009, TXT-025).
//!
//! `messaging` builds every text from a fixed English or Spanish template chosen by ID and language.
//! Other crates reach it only through [`api`]. Quiet hours (`rules`) decide when a text or business email may go.
//! Every text goes through the outbox (`service`, `models`) under a derived idempotency key, never
//! to a past customer who has not written first (TXT-009, TXT-011); the sender directory maps
//! business phones to their business (D-492). Twilio's inbound and status webhooks are recorded
//! once per message ID (`handlers`, TXT-012) and every text's delivery status is kept (TXT-013).
//! Opt-outs (`optout`, CON-008 to CON-017, CON-030, CON-035): the rules' detector, the one
//! suppression list every revocation channel feeds, and the checked send every text goes through.
//! The HELP, INFO and AYUDA reply (`keywords`, CON-018) is queued for any number, listed or not.

pub mod api;
mod handlers;
mod keywords;
mod models;
mod optout;
mod rules;
mod service;
mod status;
mod templates;

#[cfg(test)]
mod tests;

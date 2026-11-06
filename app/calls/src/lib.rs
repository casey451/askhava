//! Calls (`askhava-calls`, layer 5): masked numbers and masked calls (docs/architecture.md
//! section 3).
//!
//! T076 built the masked-number pool: each connected job gets a number from the fixed pool in
//! settings, never bought by the app, one no other active job of the same homeowner or business is
//! using when one is free, otherwise a shared one with PIN routing and one open number-pool alert
//! to Casey (CALL-001, CALL-002, CALL-017); and the suppression-list check before any automated
//! call (CON-035, CALL-014). Calls are built on standard programmable voice only, never Twilio
//! Proxy (CALL-021). T078: calls are never recorded, and each finished call is logged with who
//! called, who answered and its length; a person answering is talking, a machine never is
//! (CALL-006 to CALL-008). Other crates reach it only through [`api`].

pub mod api;
mod handlers;
mod models;
mod routing;
mod rules;
mod service;
#[cfg(test)]
mod tests;

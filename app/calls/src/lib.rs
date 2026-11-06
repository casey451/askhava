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
//! (CALL-006 to CALL-008). T079: lines close on cancel, no-show, complaint or after 30 days, the
//! staff-phone approval gate, the LINK request and the Ask Hava number's greeting (CALL-009 to
//! CALL-012, CALL-018, CALL-020). T080: relay-friendly PIN prompts, the homeowner's code call, the
//! safety voice call behind its switch and Casey's bridge to a homeowner (CALL-013 to CALL-016,
//! CALL-019, CALL-022, CALL-023, OPS-028). Other crates reach it only through [`api`].

pub mod api;
mod handlers;
mod lifecycle;
mod models;
mod routing;
mod rules;
mod service;
#[cfg(test)]
mod tests;
mod voice;

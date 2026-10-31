//! Intake (`askhava-intake`, layer 7): inbound requests, conversation state and homeowner consent
//! (docs/architecture.md section 3).
//!
//! T038 built the first reply: the consent text first on every path, after the 911 text on a
//! 911-list match, never asking for a reply and never sent twice for one request (CON-001 to
//! CON-004, CON-026, CON-027, CON-031), and the `intake` tables later tasks fill (D-492, D-493).
//! T152 added the one INT-004 911 text's words, quiet-hours kind and 5-minute check (D-436).
//! T039 added the 911-list stem matcher on every message, the SAFE hold, the INT-037 question,
//! "How soon?", the INT-033 night count and the emergency-or-regular choice and release.
//! T042 added the keyword second opinion, the trade question and its word answers, the INT-010
//! after-window word share and INT-032's two-trade detection (`matching`).
//! Other crates reach it only through [`api`].

pub mod api;
mod conversation;
mod emergency;
mod emergency_count;
mod matching;
mod models;
mod rules;
mod safety_text;
mod service;

#[cfg(test)]
mod tests;

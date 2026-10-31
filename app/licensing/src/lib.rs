//! Licensing (`askhava-licensing`, layer 3): which business may receive a card on its licenses, for
//! agency-licensed, no-license and boundary job types (LIC-011 to LIC-016, LIC-028 to LIC-031,
//! LIC-047).
//!
//! T035 added the handyman limits (AI-012, LIC-019 to LIC-025, LIC-036) in `handyman`.
//!
//! T036 added the daily ROC and city list imports, the daily license check and the sign-up
//! read-back (CARD-015, LIC-006, LIC-007, LIC-010, LIC-027) in `imports` and `service`.
//!
//! T180 added the live lookup at the pick and the lookup and list failure clocks (LIC-008,
//! LIC-009, LIC-039) in `handlers`.
//!
//! T037 added the quote-card license lines (LIC-026, LIC-032, LIC-034, LIC-037, LIC-038, WORD-016)
//! in `lines`.
//!
//! T153 added the license class map, the commercial-only sign-up stop, the R-61 and R-62 project
//! limits and the `launch_open` openings (LIC-035, LIC-043 to LIC-046) in `class_map`.
//!
//! Other crates reach it only through [`api`].

pub mod api;
mod class_map;
mod handlers;
mod handyman;
mod imports;
mod lines;
mod models;
mod rules;
mod service;

#[cfg(test)]
mod tests;

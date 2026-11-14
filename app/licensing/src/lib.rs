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
//! T374 added the sign-up writers for agency licenses and the pest attestation and the one read
//! of a business's holder (LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-047, LIC-048) in
//! `holder`.
//!
//! T389 added the wrong-license report that pauses a pest business and the replacement number
//! that clears it (LIC-047, D-3055) in `pest_report`.
//!
//! T385 added the sign-up read of a business's own record, its name check and the numbers it is
//! read under (BIZ-002, BIZ-016, LIC-008, LIC-010, LIC-011, LIC-048) in `own_record`.
//!
//! T372 added the class changes between recorded ROC lists that `businesses`' daily re-apply
//! reads (BIZ-050, LIC-043, D-2705) in `class_changes`.
//!
//! T371 added the worker's daily ROC and city list run, with the failing-list alerts and the
//! throttled lookups (LIC-006 to LIC-008, LIC-010, LIC-039, CARD-015, D-2704) in `daily_lists`.
//!
//! Other crates reach it only through [`api`].

pub mod api;
mod card_facts;
mod class_changes;
mod class_map;
mod daily_lists;
mod handlers;
mod handyman;
mod holder;
mod imports;
mod lines;
mod models;
mod own_record;
mod pest_report;
mod rules;
mod service;

#[cfg(test)]
mod tests;

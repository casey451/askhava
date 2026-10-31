//! Licensing (`askhava-licensing`, layer 3): which business may receive a card on its licenses, for
//! agency-licensed, no-license and boundary job types (LIC-011 to LIC-016, LIC-028 to LIC-031,
//! LIC-047).
//!
//! T035 added the handyman limits (AI-012, LIC-019 to LIC-025, LIC-036) in `handyman`.
//!
//! T036 added the daily ROC and city list imports, the daily license check and the sign-up
//! read-back (CARD-015, LIC-006, LIC-007, LIC-010, LIC-027) in `imports` and `service`.
//!
//! Other crates reach it only through [`api`]. T153 adds the license-map class coverage that fills
//! `Holder::roc_covers`.

pub mod api;
mod handyman;
mod imports;
mod models;
mod rules;
mod service;

#[cfg(test)]
mod tests;

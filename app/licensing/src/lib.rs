//! Licensing (`askhava-licensing`, layer 3): which business may receive a card on its licenses, for
//! agency-licensed, no-license and boundary job types (LIC-011 to LIC-016, LIC-028 to LIC-031,
//! LIC-047).
//!
//! T035 added the handyman limits (AI-012, LIC-019 to LIC-025, LIC-036) in `handyman`.
//!
//! Other crates reach it only through [`api`]. T036 the daily list
//! imports, T153 the license-map class coverage that fills `Holder::roc_covers`.

pub mod api;
mod handyman;
mod models;
mod rules;

#[cfg(test)]
mod tests;

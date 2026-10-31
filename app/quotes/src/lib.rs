//! Quotes (`askhava-quotes`, layer 5): who receives each job card, the cards and the quotes
//! (`docs/architecture.md` section 3).
//!
//! T044 added the matching: a card goes to every active, verified business serving one of the
//! job's Arizona offer towns whose licenses cover every candidate job type as the license map says
//! and which ticked one of them, with no cap (CARD-001 to CARD-003, LIC-035), in `models`, `rules`
//! and `service`.
//!
//! Other crates reach it only through [`api`].

pub mod api;
mod models;
mod rules;
mod service;

#[cfg(test)]
mod tests;

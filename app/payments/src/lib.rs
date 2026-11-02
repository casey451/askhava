//! Payments (`askhava-payments`, layer 3): Stripe and the money ledger
//! (docs/architecture.md section 3).
//!
//! T225 built the append-only ledger, `payments.ledger_entry`, and its one writer
//! [`api::ledger_append`] (FEE-028, FEE-032, D-092). Money moves only through this crate; other
//! crates reach it only through [`api`].
//!
//! T058 added the introduction fee by job size of a chosen one-time quote, [`api::fee_for`]
//! (FEE-001 to FEE-004, FEE-008, FEE-043, FEE-045, D-1399).
//!
//! T059 added Stripe customers made at sign-up with no card, saved cards with the FEE-015
//! authorization, and cards only (FEE-009, FEE-014, FEE-015, FEE-036, FEE-048, D-1409).

pub mod api;
mod ledger;
mod models;
mod rules;
mod service;

#[cfg(test)]
mod tests;

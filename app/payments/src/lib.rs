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

//!
//! T061 added free first wins (one per entity key, inside the 90-day window) and $0 repeat
//! customers, [`api::waive_at_confirm`] (FEE-010 to FEE-013, D-069, D-151).
//!
//! T062 added the Confirm-and-pay charge: one PaymentIntent per pick, one attempt per press with
//! its own key, the Stripe outage release, the card-fix window, the charging pause and the
//! monthly fee limit, [`api::confirm_and_pay`] (FEE-016 to FEE-020, FEE-037, FEE-038, FEE-041,
//! FEE-047).

pub mod api;
mod charge;
mod free_wins;
mod handlers;
mod ledger;
mod models;
mod rules;
mod service;
mod status;

#[cfg(test)]
mod tests;

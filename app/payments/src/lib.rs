//! Payments (`askhava-payments`, layer 3): Stripe and the money ledger
//! (docs/architecture.md section 3).
//!
//! T225 built the append-only ledger, `payments.ledger_entry`, and its one writer
//! [`api::ledger_append`] (FEE-028, FEE-032, D-092). Money moves only through this crate; other
//! crates reach it only through [`api`].

pub mod api;
mod ledger;

#[cfg(test)]
mod tests;

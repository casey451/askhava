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
//!
//! T189 added the release minter, [`api::Minter`]: a `ReleaseProof` only from a verified, re-fetched
//! Stripe success or a recorded $0 decision, and the FEE-042 evidence record of each charge
//! (PICK-022, SEC-030, SEC-039, FEE-042).
//!
//! T224 added the signature-checked Stripe webhook route, [`api::stripe_webhook_routes`], which
//! hands a verified success to T189's checks and minter (PICK-022, SEC-039, SEC-049).
//!
//! T066 added each automatic refund exactly once with its hourly retry and its block on a disputed
//! charge, [`api::request_refund`], and bank disputes with their evidence pack,
//! [`api::record_dispute`] (FEE-026, FEE-027, FEE-042, PRIV-027).
//!
//! T064 added the automatic refund, once, of a payment that succeeded after its job moved on, with
//! one alert to Casey, [`api::late_success`] (FEE-021).
//!
//! T067 added the nightly reconcile, [`api::reconcile`]: every unknown Stripe charge auto-matched
//! to its pick's job or auto-refunded, Stripe's processing fees in the ledger, and the owner money
//! view and business statements built only from the ledger, matching Stripe to the cent (FEE-029,
//! FEE-030, FEE-032, FEE-051).

pub mod api;
mod charge;
mod disputes;
mod free_wins;
mod handlers;
mod late;
mod ledger;
mod models;
mod reconcile;
mod refund_rules;
mod refunds;
mod release_proof;
mod routes;
mod rules;
mod service;
mod status;

#[cfg(test)]
mod tests;

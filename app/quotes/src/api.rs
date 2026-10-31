//! The public face of `askhava-quotes`: everything other crates may call (D-481).
//!
//! Matching (T044): [`send_cards`] chooses every business that receives a job's card and records
//! one `quotes.job_card` row each, with no cap (CARD-001 to CARD-003); [`card_holders`] reads them
//! back; [`rules::card_for`] and [`rules::recipients`] are the pure decision (LIC-035, LIC-043).

pub use crate::models::{
    CardBusiness, CardDecision, CardJob, OfferTown, QuotesError, Recipient, Skip,
};
pub use crate::service::{card_holders, send_cards};

/// The pure matching rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{card_for, recipients};
}

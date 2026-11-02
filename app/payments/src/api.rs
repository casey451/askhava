//! The public face of `askhava-payments`: everything other crates may call (D-481).
//!
//! T225: [`ledger_append`] is the only way to write a `payments.ledger_entry` row, from a checked
//! [`LedgerEntry`] of one of the FEE-028 [`EntryKind`]s (FEE-028, FEE-032, D-092).
//!
//! T058: [`fee_for`] turns a chosen one-time quote ([`FeeQuote`], [`FeeJob`]) into its
//! introduction fee by job size, [`Fee`], at one of the three [`Tier`]s or $0 for a fee-free job
//! type (FEE-001 to FEE-004, FEE-008, FEE-043, FEE-045, D-1399).

pub use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
pub use crate::rules::{Fee, FeeError, FeeJob, FeeQuote, Tier, VisitTier, fee_for};

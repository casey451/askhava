//! The public face of `askhava-payments`: everything other crates may call (D-481).
//!
//! T225: [`ledger_append`] is the only way to write a `payments.ledger_entry` row, from a checked
//! [`LedgerEntry`] of one of the FEE-028 [`EntryKind`]s (FEE-028, FEE-032, D-092).

pub use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};

//! The public face of `askhava-payments`: everything other crates may call (D-481).
//!
//! T225: [`ledger_append`] is the only way to write a `payments.ledger_entry` row, from a checked
//! [`LedgerEntry`] of one of the FEE-028 [`EntryKind`]s (FEE-028, FEE-032, D-092).
//!
//! T058: [`fee_for`] turns a chosen one-time quote ([`FeeQuote`], [`FeeJob`]) into its
//! introduction fee by job size, [`Fee`], at one of the three [`Tier`]s or $0 for a fee-free job
//! type (FEE-001 to FEE-004, FEE-008, FEE-043, FEE-045, D-1399).
//!
//! T188: [`period_fee_for`] adds the quote's [`Period`]: the 4-month rule and the
//! [`RecurringCap`] (REC-002 to REC-006, REC-010, FEE-039, D-1404), and [`price_lock_until`] with
//! [`locked_cents`] is the early joiner's price lock (FEE-040, D-1405).
//!
//! T059: [`ensure_customer`] makes a business's Stripe customer with no card (FEE-036);
//! [`save_card`] stores a [`SavedCard`] (cards only, FEE-014) with the FEE-015 authorization;
//! [`SetupIntentRequest`] is the off-session SetupIntent of a $0 Confirm page (FEE-048). Nothing
//! here takes a homeowner (FEE-009, D-1409).

pub use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
pub use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, CustomerId, SavedCard,
    SetupIntentRequest,
};
pub use crate::rules::{
    Fee, FeeError, FeeJob, FeeQuote, LockDates, Period, PeriodJob, RECURRING_MONTHS, RecurringCap,
    Service, Tier, VisitTier, fee_for, locked_cents, period_fee_for, price_lock_until,
};
pub use crate::service::{CUSTOMER_ACTION, Saved, customer_of, ensure_customer, save_card};

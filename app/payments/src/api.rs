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
//!
//! T060: [`fee_at_pick`] fixes the fee at the pick from the chosen quote alone (FEE-005);
//! [`record_fee_at_confirm`] writes it to the ledger at Confirm, never adjusted afterwards
//! (FEE-006, JOB-016); [`switch_fee_mode`] logs the owner's [`FeeMode`] switch, and
//! [`may_confirm`] holds each business to the new [`FeeTerms`] authorization (FEE-007, D-072).
//!
//! T061: [`record_entity_keys`] stores a business's [`EntityKey`]s at sign-up (FEE-011, D-151);
//! [`waive_at_confirm`] decides, before any charge, whether a pick is a repeat customer (FEE-013)
//! or the business's free first win inside the [`in_free_win_window`] (FEE-010, FEE-012), and
//! writes its $0 entry; [`Waiver::NotWaived`] means the caller charges.

pub use crate::free_wins::{
    FREE_WIN_ACTION, FREE_WIN_WINDOW_DAYS, REPEAT_ACTION, Waiver, WinCheck, in_free_win_window,
    record_entity_keys, waive_at_confirm,
};
pub use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
pub use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, CustomerId,
    ENTITY_KEY_ACTION, EntityKey, EntityKind, FreeWinError, JoinDates, SavedCard,
    SetupIntentRequest,
};
pub use crate::rules::{
    Fee, FeeError, FeeJob, FeeMode, FeeQuote, FeeTerms, LockDates, Period, PeriodJob, PickedFee,
    RECURRING_MONTHS, RecurringCap, Service, Tier, VisitTier, fee_at_pick, fee_for, flat_tier,
    locked_cents, period_fee_for, price_lock_until,
};
pub use crate::service::{
    Authorized, CONFIRM_FEE_ACTION, CUSTOMER_ACTION, ConfirmError, Confirmed, FEE_MODE_ACTION,
    FEE_MODE_ID, FEE_MODE_SUBJECT_KIND, Saved, accept_authorization, customer_of, ensure_customer,
    may_confirm, record_fee_at_confirm, save_card, switch_fee_mode,
};

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
//! [`RecurringCap`] (REC-002 to REC-006, REC-010, FEE-039, D-1404). There is no early-joiner
//! price lock: no fee function takes a join date (D-1494).
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

//!
//! T062: [`open_intent`] makes the pick's one PaymentIntent (FEE-047); [`confirm_and_pay`] is the
//! only charge, one [`Press`] at a time, each with its own key (FEE-016, FEE-017, FEE-037,
//! FEE-041), answering a [`ChargeOutcome`]; [`run_charge_check`], [`run_pay_timer`] and
//! [`sync_pause`] are the FEE-019 check, the FEE-018 and FEE-020 timers and the FEE-038 pause;
//! [`run_pause_watch`] keeps the timers in line with the pause, and [`card_fix_window`] tells
//! `picks` whether a card-fix notice is still due (FEE-020).
//!
//! T189: [`paid_release`] clears a pick only from a [`VerifiedStripeEvent`] whose re-fetched
//! intent matches the [`StripeAccount`], fee and pick, and [`no_fee_release`] only from a recorded
//! $0 decision; [`Minter`] holds the release mint `server` hands over and turns only a
//! [`Releasable`] into a `ReleaseProof` (PICK-022, SEC-030, SEC-039, SEC-049); [`IntentText`] is the
//! description and statement descriptor of a pick's PaymentIntent (FEE-049).
//!
//! T224: [`stripe_webhook_routes`] is the Stripe webhook route ([`STRIPE_WEBHOOK_PATH`]): source
//! rule, signature, then [`paid_release`] and the [`ReleaseSink`] that mints (SEC-039, SEC-049).
//!
//! T065: [`decide_unreachable`] and [`decide_cancel`] decide each automatic refund from the call
//! log ([`CallRecord`]s) as a [`RefundDecision`], once per pick under [`refund_label`]; [`no_show`]
//! says whether a no-show is recorded, never refunded (FEE-022 to FEE-026, FEE-035, CALL-007).
//!
//! T066: [`request_refund`] refunds a pick once, [`run_refund_retry`] retries a failed one hourly
//! and a disputed charge is blocked (FEE-026); [`early_fraud_warning`] refunds an unreleased warned
//! charge (FEE-052); [`record_dispute`] alerts Casey and records a dispute once with the $15 fee,
//! [`run_dispute_submit`] sends the [`EvidencePack`] 2 days before the due date when T191 called
//! [`mark_auto_submit`] (FEE-027, FEE-042, FEE-050, PRIV-027).
//!
//! T064: [`late_success`] queues the refund of a payment that succeeded after its job moved on and
//! alerts Casey, once per pick; [`run_late_refund`] sends it through [`request_refund`] (FEE-021).

pub use crate::charge::{
    ATTEMPT_ACTION, ATTEMPT_SUBJECT, IN_FLIGHT, INTENT_ACTION, IntentStripe, confirm_and_pay,
    open_intent, set_fee_limit,
};
pub use crate::disputes::{
    DISPUTE_ACTION, DISPUTE_FEE_CENTS, DisputeStripe, EVIDENCE_ACTION, SUBMIT_BEFORE, SUBMIT_KIND,
    evidence_pack, list_disputes, mark_auto_submit, record_dispute, record_owner_submission,
    run_dispute_submit,
};
pub use crate::free_wins::{
    FREE_WIN_ACTION, FREE_WIN_WINDOW_DAYS, REPEAT_ACTION, Waiver, WinCheck, in_free_win_window,
    record_entity_keys, waive_at_confirm,
};
pub use crate::handlers::{
    CARD_FIX_KIND, CARD_FIX_SECONDS, CHECK_AFTER, CHECK_KIND, CardFixWindow, EndReason,
    PAUSE_EVERY, PAUSE_KIND, PICK_ENDED_KIND, RECHECK_AFTER, STRIPE_DOWN_SECONDS, TIMER_KIND,
    card_fix_window, charges_paused, run_charge_check, run_pause_watch, run_pay_timer, sync_pause,
};
pub use crate::late::{
    LATE_KIND, LATE_SUCCESS_ALERT, LATE_TRIGGER, LateSuccess, late_success, run_late_refund,
};
pub use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
pub use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, ChargeError, ChargeOutcome,
    CustomerId, DESCRIPTOR_MAX, DisputeOpened, DisputeView, ENTITY_KEY_ACTION, EVIDENCE_PRODUCT,
    EntityKey, EntityKind, EvidencePack, FreeWinError, IntentText, JoinDates, MoneyEventError,
    PickNow, Press, RefundOutcome, RefundStep, RefundTrigger, STATEMENT_PREFIX, SavedCard,
    SetupIntentRequest, attempt_label, intent_label,
};
pub use crate::refund_rules::{
    ATTEMPT_GAP, ATTEMPT_WINDOW, Answer, CallRecord, Caller, CancelFacts, Contact, DECIDE_AFTER,
    EmailCheck, NoShow, RefundDecision, RefundReason, Unmet, UnreachableFacts, counted_attempts,
    decide_cancel, decide_unreachable, no_show, refund_label, talked,
};
pub use crate::refunds::{
    FRAUD_WARNING_ALERT, REFUND_ACTION, REFUND_BLOCKED_ALERT, REFUND_FAILED_ALERT, RETRY_EVERY,
    RETRY_KIND, early_fraud_warning, refund_failed, request_refund, run_refund_retry,
};
pub use crate::release_proof::{
    Minter, Releasable, ReleaseError, SUCCEEDED_EVENT, StripeAccount, VerifiedStripeEvent,
    no_fee_release, paid_release,
};
pub use crate::routes::{ReleaseSink, STRIPE_WEBHOOK_PATH, StripeWebhook, stripe_webhook_routes};
pub use crate::rules::{
    Fee, FeeError, FeeJob, FeeMode, FeeQuote, FeeTerms, Period, PeriodJob, PickedFee,
    RECURRING_MONTHS, RecurringCap, Service, Tier, VisitTier, fee_at_pick, fee_for, flat_tier,
    period_fee_for,
};
pub use crate::service::{
    Authorized, CONFIRM_FEE_ACTION, CUSTOMER_ACTION, ConfirmError, Confirmed, FEE_MODE_ACTION,
    FEE_MODE_ID, FEE_MODE_SUBJECT_KIND, Saved, accept_authorization, customer_of, ensure_customer,
    may_confirm, record_fee_at_confirm, save_card, switch_fee_mode,
};
pub use crate::status::{AttemptStatus, RefundStatus};

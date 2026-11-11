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
//! T287 (D-1871): [`fee_terms_in_force`] reads the stored terms; [`fix_fee_at_pick`] stores the
//! fee in the pick's transaction and [`pick_fee`] reads it, the only amount a press may charge.
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
//! T259: it also dispatches refund, dispute and early-fraud-warning events once each by event ID
//! ([`DISPUTE_CLOSED_ACTION`], [`FRAUD_WARNING_KIND`]; SEC-017, FEE-026, FEE-027, FEE-052).
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
//!
//! T258 (D-1753, D-2067): [`RefundStripe`] reads each refund as Stripe reports it ([`Refund`],
//! [`RefundState`]); only `succeeded` is refunded, a `pending` one is asked about again by
//! [`run_refund_check`] ([`REFUND_CHECK_KIND`]), and [`refund_succeeded`], [`refund_failed`] and
//! [`refund_canceled`] move it on, a failure after success appending one `refund-reversal`.
//!
//! T270 (D-1836, D-2145): [`refund_state`] reads where a pick's refund stands as a
//! [`PickRefundState`] (`NoRefund`, `InFlight`, `Succeeded`, `Failed`, `Blocked`); `picks` closes a
//! job or tells a business its fee is refunded only on `Succeeded` (FEE-022, FEE-026, FEE-052).
//!
//! T067: [`reconcile`] (the [`RECONCILE_KIND`] task, [`run_reconcile`]) reads Stripe's charges
//! through [`ReconcileStripe`] and auto-matches or auto-refunds each unknown one by
//! [`plan_charge`], records processing fees and checks the ledger against Stripe to the cent
//! ([`ReconcileReport`]); [`money_view`] and [`business_statement`] read only the ledger (FEE-029,
//! FEE-030, FEE-032, FEE-051).
//!
//! T288 (D-1872): each charge a run cannot settle is kept in `payments.reconcile_open` ([`OpenWhy`])
//! and retried every night until it settles; one open [`OPEN_ALERT_AFTER`] raises one
//! [`OPEN_CHARGE_ALERT`].
//!
//! T068: [`queue_receipt`] raises a charge's [`RECEIPT_KIND`] event (IDs only) and [`receipt_for`]
//! reads its [`ChargeReceipt`] for the "Why was I charged?" page (FEE-031, FEE-053);
//! [`tier_edge_flag`] applies the [`TierEdgeRule`] to a business's latest fee bases (FEE-033).
//!
//! T209: [`charge_off_session`] is the one off-session charge, the PICK-048 auto-confirm charge,
//! through an [`OffSessionStripe`], only when the caller found Stripe's written answer filed and
//! only with the [`saved_card_of`] the business (PICK-048, PICK-056, FEE-014, D-401).
//!
//! T228: [`remove_saved_cards`] marks a leaving business's saved cards removed (BIZ-051).

pub use crate::charge::{
    ATTEMPT_ACTION, ATTEMPT_SUBJECT, IN_FLIGHT, INTENT_ACTION, IntentStripe, confirm_and_pay,
    open_intent, set_fee_limit,
};
pub use crate::disputes::{
    DISPUTE_ACTION, DISPUTE_FEE_CENTS, DisputeStripe, EVIDENCE_ACTION, SUBMIT_BEFORE, SUBMIT_KIND,
    evidence_pack, list_disputes, mark_auto_submit, record_dispute, record_owner_submission,
    run_dispute_submit,
};
pub use crate::flags::{
    FlagError, LAST_BASES, TIER_EDGES, TierEdgeFlag, TierEdgeRule, tier_edge_flag,
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
pub use crate::ledger::{
    Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, REFUND_REVERSAL_CODE, ledger_append,
};
pub use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, ChargeError, ChargeOutcome,
    ChargePlan, CustomerId, DESCRIPTOR_MAX, DisputeOpened, DisputeView, ENTITY_KEY_ACTION,
    EVIDENCE_PRODUCT, EntityKey, EntityKind, EvidencePack, FreeWinError, IntentText, JoinDates,
    MoneyEventError, MoneyTotals, MoneyView, PickFacts, PickNow, Press, RefundOutcome, RefundStep,
    RefundTrigger, RefundWhy, SavedCard, SetupIntentRequest, StripeCharge, attempt_label,
    intent_label, plan_charge, statement_prefix,
};
pub use crate::off_session::{
    OffSessionCharge, OffSessionOutcome, OffSessionStripe, charge_off_session, saved_card_of,
};
pub use crate::pick_fee::{
    PickFeeError, StoredPickFee, fee_terms_in_force, fix_fee_at_pick, pick_fee,
};
pub use crate::purge::{DISPUTE_OPEN_AFTER_DUE, place_dispute_holds, run_dispute_holds};
pub use crate::receipts::{ChargeReceipt, RECEIPT_KIND, queue_receipt, receipt_for, tier_of_fee};
pub use crate::reconcile::{
    CHARGE_FAILED_ALERT, LOOKBACK, MISMATCH_ALERT, OPEN_ALERT_AFTER, OPEN_CHARGE_ALERT, OpenWhy,
    RECONCILE_ACTION, RECONCILE_EVERY, RECONCILE_KIND, ReconcileReport, ReconcileStripe, Statement,
    StatementLine, UNKNOWN_CHARGE_ALERT, business_statement, money_view, reconcile, run_reconcile,
};
pub use crate::refund_rules::{
    ATTEMPT_GAP, ATTEMPT_WINDOW, Answer, CallRecord, Caller, CancelFacts, Contact, DECIDE_AFTER,
    EmailCheck, NoShow, RefundDecision, RefundReason, Unmet, UnreachableFacts, counted_attempts,
    decide_cancel, decide_unreachable, no_show, refund_label, talked,
};
pub use crate::refund_state::{PickRefundState, refund_state};
pub use crate::refund_stripe::{Refund, RefundState, RefundStripe};
pub use crate::refunds::{
    FRAUD_WARNING_ALERT, REFUND_ACTION, REFUND_BLOCKED_ALERT, REFUND_CHECK_EVERY,
    REFUND_CHECK_KIND, REFUND_FAILED_ALERT, RETRY_EVERY, RETRY_KIND, early_fraud_warning,
    refund_canceled, refund_failed, refund_succeeded, request_refund, run_refund_check,
    run_refund_retry,
};
pub use crate::release_proof::{
    Minter, Releasable, ReleaseError, SUCCEEDED_EVENT, StripeAccount, VerifiedStripeEvent,
    no_fee_release, paid_release,
};
pub use crate::routes::{
    DISPUTE_CLOSED_ACTION, FRAUD_WARNING_KIND, ReleaseSink, STRIPE_WEBHOOK_PATH, StripeWebhook,
    UNREADABLE_EVENT_ALERT, stripe_webhook_routes,
};
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
pub use askhava_adapters::api::StripeRefund;

/// BIZ-051, D-446, D-3046 (T228): marks every saved card of `business` removed, at the
/// transaction's own time (this file names no date type, TD-FEE-41), inside the caller's
/// transaction, and returns their `pm_…` IDs for the caller to detach at Stripe after the commit;
/// [`saved_card_of`] then finds none, so no charge can use them. The rows, the ledger and the
/// authorizations stay.
///
/// # Errors
///
/// [`CardError::Database`].
pub async fn remove_saved_cards(
    tx: &mut sqlx::PgConnection,
    business: BusinessId,
) -> Result<Vec<String>, CardError> {
    sqlx::query_scalar!(
        "UPDATE payments.payment_method SET removed_at = now() \
         WHERE business_id = $1 AND removed_at IS NULL RETURNING stripe_payment_method_id",
        business.0,
    )
    .fetch_all(tx)
    .await
    .map_err(|_| CardError::Database)
}

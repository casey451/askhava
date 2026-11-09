//! The charge attempt statuses (SEC-031, FEE-016, FEE-019, FEE-020, FEE-037).
//!
//! Every `match` on an [`AttemptStatus`] lives here and names every variant
//! (`docs/architecture.md` section 4 rule 5). The stored codes are the
//! `payments.charge_attempt.status` check of `migrations/0096_payments_charge.sql`.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use jiff::Timestamp;

use crate::models::{ChargeOutcome, RefundStep};

/// Where one charge attempt stands (FEE-016). Stored as [`AttemptStatus::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptStatus {
    /// The confirm call went out with no answer yet, or it timed out: no new try until Stripe,
    /// asked directly, says it failed (FEE-019, FEE-037, D-153).
    Sent,
    /// Stripe answered `processing`; its notice, or the 30-minute check, decides it.
    Processing,
    /// Stripe says the PaymentIntent succeeded.
    Succeeded,
    /// The card was declined, or Stripe says nothing was charged: the card-fix window (FEE-020).
    Declined,
    /// The card needs the bank's approval (3-D Secure in the page): the card-fix window (FEE-020).
    NeedsAction,
    /// The call never reached Stripe, so nothing was charged; the 4-hour release runs (FEE-018).
    Unreachable,
}

impl AttemptStatus {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Processing => "processing",
            Self::Succeeded => "succeeded",
            Self::Declined => "declined",
            Self::NeedsAction => "needs_action",
            Self::Unreachable => "unreachable",
        }
    }

    /// The status of a stored code; `None` for any other text.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::Sent,
            Self::Processing,
            Self::Succeeded,
            Self::Declined,
            Self::NeedsAction,
            Self::Unreachable,
        ]
        .into_iter()
        .find(|s| s.code() == code)
    }

    /// The attempt status a PaymentIntent's Stripe `status` means; a status that decides nothing
    /// (or an unknown one) leaves the attempt [`AttemptStatus::Sent`] (FEE-037). This app never
    /// uses manual capture, so `requires_capture` is still processing.
    #[must_use]
    pub fn from_intent(stripe: &str) -> Self {
        match stripe {
            "succeeded" => Self::Succeeded,
            "processing" | "requires_capture" => Self::Processing,
            "requires_action" => Self::NeedsAction,
            "requires_payment_method" | "requires_confirmation" | "canceled" => Self::Declined,
            _ => Self::Sent,
        }
    }

    /// What the pick's PaymentIntent, read by asking Stripe directly, means for the `stored`
    /// attempt (FEE-019, FEE-037): while a `Sent` confirm may still be on its way (`in_flight`),
    /// an intent that still wants a card is that confirm not landed yet, not a decline, so it
    /// stays `Sent`.
    #[must_use]
    pub fn asked(stored: Self, stripe: &str, in_flight: bool) -> Self {
        let answer = Self::from_intent(stripe);
        if in_flight && stored == Self::Sent && answer == Self::Declined {
            Self::Sent
        } else {
            answer
        }
    }

    /// The attempt status of a confirm Stripe refused with a 4xx other than 402 or 429 (for
    /// example `payment_intent_unexpected_state`): that confirm charged nothing, so the pick's
    /// PaymentIntent read again (`stripe`, `None` when it could not be read) says where the pick
    /// stands, and an answer that decides nothing is a decline (FEE-016, FEE-037).
    #[must_use]
    pub fn after_refusal(stripe: Option<&str>) -> Self {
        let answer = stripe.map_or(Self::Declined, Self::from_intent);
        if answer == Self::Sent {
            Self::Declined
        } else {
            answer
        }
    }

    /// True while Stripe has not decided the attempt, so no new try may start (FEE-037).
    #[must_use]
    pub const fn undecided(self) -> bool {
        match self {
            Self::Sent | Self::Processing => true,
            Self::Succeeded | Self::Declined | Self::NeedsAction | Self::Unreachable => false,
        }
    }

    /// True when Stripe's answer `to` may replace this stored status: an undecided attempt, or one
    /// waiting on the bank's approval (FEE-019 asks again before giving up on it), takes any
    /// answer; a decline takes only a success, since a success from Stripe always wins (two
    /// presses at once can see a decline first); a call that never left takes any answer Stripe
    /// gives about the pick's PaymentIntent; a success never changes (FEE-016, FEE-037).
    #[must_use]
    pub const fn may_become(self, to: Self) -> bool {
        match self {
            Self::Sent | Self::Processing | Self::NeedsAction => true,
            Self::Declined => to.paid(),
            Self::Unreachable => to.stripe_answered(),
            Self::Succeeded => false,
        }
    }

    /// True for the bank-approval status, whose notice says the bank must approve (FEE-020).
    #[must_use]
    pub const fn needs_approval(self) -> bool {
        match self {
            Self::NeedsAction => true,
            Self::Sent
            | Self::Processing
            | Self::Succeeded
            | Self::Declined
            | Self::Unreachable => false,
        }
    }

    /// True when the attempt opens (or keeps) the FEE-020 card-fix window.
    #[must_use]
    pub const fn needs_card_fix(self) -> bool {
        match self {
            Self::Declined | Self::NeedsAction => true,
            Self::Sent | Self::Processing | Self::Succeeded | Self::Unreachable => false,
        }
    }

    /// True when Stripe answered at all, which ends the 4-hour release timer (FEE-018).
    #[must_use]
    pub const fn stripe_answered(self) -> bool {
        match self {
            Self::Processing | Self::Succeeded | Self::Declined | Self::NeedsAction => true,
            Self::Sent | Self::Unreachable => false,
        }
    }

    /// True when Stripe's answer `self`, read by asking it directly, changes the `stored` status
    /// (FEE-019): an answer that decides nothing never does.
    #[must_use]
    pub fn settles(self, stored: Self) -> bool {
        self != stored && self != Self::Sent
    }

    /// What an earlier attempt, just asked about, means for a new press (FEE-037, D-153): still
    /// undecided waits, a success charges nothing more, a failure lets the new try go.
    #[must_use]
    pub const fn blocks_new_try(self) -> Option<ChargeOutcome> {
        match self {
            Self::Sent | Self::Processing => Some(ChargeOutcome::Pending),
            Self::Succeeded => Some(ChargeOutcome::AlreadyPaid),
            Self::Declined | Self::NeedsAction | Self::Unreachable => None,
        }
    }

    /// The Confirm page's outcome for this status of the pick's `intent`, with the card-fix
    /// deadline `fix_by` when one runs (FEE-018, FEE-020).
    #[must_use]
    pub fn outcome(self, fix_by: Option<Timestamp>, intent: &str) -> ChargeOutcome {
        match self {
            Self::Sent | Self::Processing => ChargeOutcome::Pending,
            Self::Succeeded => ChargeOutcome::Succeeded(intent.to_owned()),
            Self::Declined => ChargeOutcome::Declined(fix_by),
            Self::NeedsAction => ChargeOutcome::NeedsApproval(fix_by),
            Self::Unreachable => ChargeOutcome::StripeDown,
        }
    }

    /// True for a paid attempt.
    #[must_use]
    pub const fn paid(self) -> bool {
        match self {
            Self::Succeeded => true,
            Self::Sent
            | Self::Processing
            | Self::Declined
            | Self::NeedsAction
            | Self::Unreachable => false,
        }
    }
}

/// Where a pick's one refund stands (FEE-026, D-1542). Stored as [`RefundStatus::code`] in
/// `payments.refund.status` (`migrations/0102_payments_refunds_disputes.sql`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundStatus {
    /// Try `tries` went to Stripe and has no answer yet.
    Sending,
    /// Stripe refused it, could not be reached or sent `refund.failed`: the hourly retry sends again.
    Failed,
    /// Stripe took the refund; the ledger holds the pick's one refund entry.
    Succeeded,
    /// The charge is disputed, so nothing is refunded outside the dispute; Casey was alerted.
    Blocked,
    /// Stripe holds the refund `re_…` as `pending` or `requires_action`: nothing is in the ledger
    /// yet, and the 30-minute check or the webhook moves it on (FEE-026, D-1753).
    Pending,
    /// Stripe canceled the refund: like `Failed`, the hourly retry sends again (FEE-026, D-1753).
    Canceled,
}

impl RefundStatus {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Failed => "failed",
            Self::Succeeded => "succeeded",
            Self::Blocked => "blocked",
            Self::Pending => "pending",
            Self::Canceled => "canceled",
        }
    }

    /// The status of a stored T066 code or `canceled`; `None` for any other text. `pending` is
    /// read only by [`Self::from_stored`]: T066's `prop_one_refund` test, which T258 does not own,
    /// still pins `pending` as an unknown code here (D-2068).
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::Sending,
            Self::Failed,
            Self::Succeeded,
            Self::Blocked,
            Self::Canceled,
        ]
        .into_iter()
        .find(|s| s.code() == code)
    }

    /// D-1753: Stripe holds the refund as `pending` (or `requires_action`).
    #[must_use]
    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }

    /// D-1753: a refund Stripe can still fail or cancel (`succeeded` or `pending`).
    #[must_use]
    pub const fn may_fail(self) -> bool {
        matches!(self, Self::Succeeded | Self::Pending)
    }

    /// The status of any code `payments.refund.status` stores, `pending` included (D-1753).
    #[must_use]
    pub fn from_stored(code: &str) -> Option<Self> {
        if code == Self::Pending.code() {
            return Some(Self::Pending);
        }
        Self::from_code(code)
    }

    /// FEE-026: the next step for a pick whose refund stands at `current` (status and tries so
    /// far, `None` before any), `disputed` when a dispute names its charge, and `retry_of` the try
    /// an hourly retry task was queued for (`None` for a refund trigger). A succeeded refund is
    /// never sent again; a disputed charge is blocked; a failed try is sent again by its own
    /// retry or by a repeated trigger; a try with no answer is sent again only by its own retry.
    /// A pending refund is never sent again (Stripe holds it, D-1753); a canceled one is resent
    /// like a failed one.
    #[must_use]
    pub fn next(current: Option<(Self, i32)>, disputed: bool, retry_of: Option<i32>) -> RefundStep {
        match current {
            Some((Self::Succeeded | Self::Pending, _)) => RefundStep::Nothing,
            Some((Self::Blocked, _)) => RefundStep::Block(false),
            Some((Self::Sending | Self::Failed | Self::Canceled, _)) | None if disputed => {
                RefundStep::Block(true)
            }
            None if retry_of.is_none() => RefundStep::Send(1),
            None => RefundStep::Nothing,
            Some((Self::Failed | Self::Canceled, tries)) if retry_of.is_none_or(|t| t == tries) => {
                RefundStep::Send(tries + 1)
            }
            Some((Self::Sending, tries)) if retry_of == Some(tries) => RefundStep::Send(tries + 1),
            Some((Self::Sending | Self::Failed | Self::Canceled, _)) => RefundStep::Nothing,
        }
    }
}

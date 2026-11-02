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

use crate::models::ChargeOutcome;

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
            Self::NeedsAction => "needs-action",
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

    /// True while Stripe has not decided the attempt, so no new try may start (FEE-037).
    #[must_use]
    pub const fn undecided(self) -> bool {
        match self {
            Self::Sent | Self::Processing => true,
            Self::Succeeded | Self::Declined | Self::NeedsAction | Self::Unreachable => false,
        }
    }

    /// True while Stripe's answer may still move the attempt: undecided, or waiting on the bank's
    /// approval (FEE-019 asks again before giving up on it).
    #[must_use]
    pub const fn may_change(self) -> bool {
        match self {
            Self::Sent | Self::Processing | Self::NeedsAction => true,
            Self::Succeeded | Self::Declined | Self::Unreachable => false,
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

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

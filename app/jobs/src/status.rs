//! The twelve job statuses and the arrows between them (JOB-001, JOB-002, SEC-031).
//!
//! Every `match` on a status lives here and names every variant, so a new status does not build
//! until each place handles it (`docs/architecture.md` section 4 rule 5). The arrows are the
//! diagram of `docs/architecture.md` section 6, and nothing else may move a job.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use crate::rules::{self, Wait};

/// A job's one status (JOB-001). Stored in `jobs.job.status` as [`JobStatus::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobStatus {
    /// Answering the intake questions.
    Intake,
    /// Cards are out; waiting for the first quote.
    Open,
    /// No quote by 6 p.m. the next day.
    NoQuotesYet,
    /// At least one quote; waiting for a pick.
    QuotesReady,
    /// Picked; waiting for the business to confirm.
    AwaitingConfirm,
    /// Confirmed; waiting for the charge.
    PaymentPending,
    /// Contact released; waiting for the work and the check-ins.
    Connected,
    /// The homeowner said the work is done (final).
    Done,
    /// Closed by a timeout, a close request or a reply (final).
    Closed,
    /// Closed during intake (final).
    ClosedEarly,
    /// No open category or active business covers it (final).
    NotCovered,
    /// The homeowner cancelled (final).
    Cancelled,
}

impl JobStatus {
    /// All twelve statuses (JOB-001).
    pub const ALL: [Self; 12] = [
        Self::Intake,
        Self::Open,
        Self::NoQuotesYet,
        Self::QuotesReady,
        Self::AwaitingConfirm,
        Self::PaymentPending,
        Self::Connected,
        Self::Done,
        Self::Closed,
        Self::ClosedEarly,
        Self::NotCovered,
        Self::Cancelled,
    ];

    /// The stored code name, as in `jobs.job.status` and the audit row (JOB-001, PRIV-028).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Intake => "intake",
            Self::Open => "open",
            Self::NoQuotesYet => "no_quotes_yet",
            Self::QuotesReady => "quotes_ready",
            Self::AwaitingConfirm => "awaiting_confirm",
            Self::PaymentPending => "payment_pending",
            Self::Connected => "connected",
            Self::Done => "done",
            Self::Closed => "closed",
            Self::ClosedEarly => "closed_early",
            Self::NotCovered => "not_covered",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status a stored code names, if any.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.code() == code)
    }

    /// The statuses this one may move to: the arrows of `docs/architecture.md` section 6, with
    /// every non-final status able to move to Cancelled (JOB-002, JOB-018). Final statuses have none.
    #[must_use]
    pub const fn targets(self) -> &'static [Self] {
        use JobStatus::{
            AwaitingConfirm, Cancelled, Closed, ClosedEarly, Connected, Done, NoQuotesYet,
            NotCovered, Open, PaymentPending, QuotesReady,
        };
        match self {
            Self::Intake => &[Open, NotCovered, ClosedEarly, Cancelled],
            Self::Open => &[QuotesReady, NoQuotesYet, NotCovered, Cancelled],
            Self::NoQuotesYet => &[QuotesReady, Closed, Cancelled],
            Self::QuotesReady => &[AwaitingConfirm, Closed, Cancelled],
            Self::AwaitingConfirm => &[PaymentPending, QuotesReady, Cancelled],
            Self::PaymentPending => &[Connected, QuotesReady, Cancelled],
            Self::Connected => &[Done, QuotesReady, Closed, Cancelled],
            Self::Done | Self::Closed | Self::ClosedEarly | Self::NotCovered | Self::Cancelled => {
                &[]
            }
        }
    }

    /// True when the arrow from `self` to `to` exists (JOB-002).
    #[must_use]
    pub fn can_move_to(self, to: Self) -> bool {
        self.targets().contains(&to)
    }

    /// What this status waits for before it times out, and where it then moves; none for a final
    /// status (JOB-003, JOB-010 to JOB-014, JOB-026; `docs/architecture.md` section 6).
    #[must_use]
    pub const fn wait(self) -> Option<(Wait, Self)> {
        Some(match self {
            Self::Intake => (Wait::Hours(rules::INTAKE_HOURS), Self::ClosedEarly),
            Self::Open => (Wait::NextDayAt(rules::NO_QUOTES_HOUR), Self::NoQuotesYet),
            Self::NoQuotesYet => (Wait::Days(rules::NO_QUOTES_DAYS), Self::Closed),
            Self::QuotesReady => (Wait::Days(rules::NO_PICK_DAYS), Self::Closed),
            Self::AwaitingConfirm => (
                Wait::BusinessMinutes(rules::CONFIRM_BUSINESS_MINUTES),
                Self::QuotesReady,
            ),
            Self::PaymentPending => (
                Wait::BusinessMinutes(rules::PAYMENT_BUSINESS_MINUTES),
                Self::QuotesReady,
            ),
            Self::Connected => (Wait::Days(rules::CONNECTED_DAYS), Self::Closed),
            Self::Done | Self::Closed | Self::ClosedEarly | Self::NotCovered | Self::Cancelled => {
                return None;
            }
        })
    }

    /// The homeowner text, by template ID, that this status's timeout sends with its move: the
    /// "no quotes yet" text at 6 p.m. (JOB-010, D-030) and the two closing notes (JOB-011,
    /// JOB-012, D-461). The Intake close text is INT-019's own task's, and a return to Quotes
    /// ready is told by `picks` (PICK-045); Connected closes with no text and no review request
    /// (JOB-026, D-046).
    #[must_use]
    pub const fn timeout_text(self) -> Option<&'static str> {
        match self {
            Self::Open => Some("no_quotes_yet"),
            Self::NoQuotesYet => Some("closed_no_quotes"),
            Self::QuotesReady => Some("closed_no_pick"),
            Self::Intake
            | Self::AwaitingConfirm
            | Self::PaymentPending
            | Self::Connected
            | Self::Done
            | Self::Closed
            | Self::ClosedEarly
            | Self::NotCovered
            | Self::Cancelled => None,
        }
    }

    /// True for Quotes ready, the only status the all-quotes-in text and the every-window-ended
    /// close apply to (TXT-042, JOB-025).
    #[must_use]
    pub const fn awaits_pick(self) -> bool {
        matches!(self, Self::QuotesReady)
    }

    /// True for Open and No quotes yet, the statuses in which a business that becomes eligible
    /// still gets the job card, with its own 24 hours (CARD-018, D-042).
    #[must_use]
    pub const fn takes_late_cards(self) -> bool {
        matches!(self, Self::Open | Self::NoQuotesYet)
    }

    /// True for Open, No quotes yet, Quotes ready and Awaiting confirm, the statuses in which a
    /// business may still quote inside its own 24 hours; once the picked business taps Confirm the
    /// job has left them (PICK-001, D-043).
    #[must_use]
    pub const fn takes_quotes(self) -> bool {
        matches!(
            self,
            Self::Open | Self::NoQuotesYet | Self::QuotesReady | Self::AwaitingConfirm
        )
    }

    /// True for Open, No quotes yet and Quotes ready, the statuses in which a business may still
    /// edit its quote: from Awaiting confirm on the job has had a pick (PICK-051, D-043). A job
    /// back in Quotes ready after a failed pick is caught by `quotes`' unavailable quote.
    #[must_use]
    pub const fn takes_quote_edits(self) -> bool {
        matches!(self, Self::Open | Self::NoQuotesYet | Self::QuotesReady)
    }

    /// True for the five final statuses, which wait for nothing (JOB-001, JOB-003).
    #[must_use]
    pub const fn is_final(self) -> bool {
        self.targets().is_empty()
    }
}

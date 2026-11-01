//! The pick statuses and the job stage a text pick reads (SEC-031, PICK-031, PICK-032, PICK-036).
//!
//! Every `match` on a [`PickStatus`] or on `jobs`' `JobStatus` in this crate lives here and names
//! every variant (`docs/architecture.md` section 4 rule 5). The stored codes are the
//! `picks.pick.status` check of `migrations/0079_picks_code.sql`; later tasks add the variants for
//! needs card, charging, paid and the rest with their arms.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use askhava_jobs::api::JobStatus;

use crate::rules::Stage;

/// A pick's status (`docs/architecture.md` section 6, FEE-052). Stored as [`PickStatus::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickStatus {
    /// Picked; waiting for the business's Confirm.
    AwaitingConfirm,
    /// Ended before a Confirm: replaced by another pick (PICK-032) or withdrawn by a pause
    /// (CON-029).
    Cancelled,
}

impl PickStatus {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AwaitingConfirm => "awaiting_confirm",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The text-pick stage of a job status (PICK-031, PICK-032, PICK-036).
#[must_use]
pub const fn stage(status: JobStatus) -> Stage {
    match status {
        JobStatus::Intake | JobStatus::Open | JobStatus::NoQuotesYet => Stage::Collecting,
        JobStatus::QuotesReady => Stage::Ready,
        JobStatus::AwaitingConfirm => Stage::Waiting,
        JobStatus::PaymentPending | JobStatus::Connected => Stage::Accepted,
        JobStatus::Done
        | JobStatus::Closed
        | JobStatus::ClosedEarly
        | JobStatus::NotCovered
        | JobStatus::Cancelled => Stage::Final,
    }
}

/// The status a text pick moves a job in `stage` from, to Awaiting confirm: Quotes ready only; a
/// job already Awaiting confirm (a replacement, or a pick after a pause withdrew one) stays put.
#[must_use]
pub const fn pick_move(stage: Stage) -> Option<(JobStatus, JobStatus)> {
    match stage {
        Stage::Ready => Some((JobStatus::QuotesReady, JobStatus::AwaitingConfirm)),
        Stage::Collecting | Stage::Waiting | Stage::Accepted | Stage::Final => None,
    }
}

//! The pick statuses, the job stage a text pick reads and the Confirm window's status checks
//! (SEC-031, PICK-019, PICK-020, PICK-031, PICK-032, PICK-036), and `jobs`' Awaiting confirm
//! backstop length.
//!
//! Every `match` on a [`PickStatus`] or on `jobs`' `JobStatus` in this crate lives here and names
//! every variant (`docs/architecture.md` section 4 rule 5). The stored codes are the
//! `picks.pick.status` check of `migrations/0079_picks_code.sql`; later tasks add the variants for
//! needs card, charging and the rest with their arms (T069 added paid and free for the release).
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use askhava_businesses::api::AccountStatus;
use askhava_jobs::api::JobStatus;
use askhava_jobs::api::rules::Wait;
use askhava_privacy::api::ReleaseBasis;

use crate::rules::Stage;

/// A pick's status (`docs/architecture.md` section 6, FEE-052). Stored as [`PickStatus::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickStatus {
    /// Picked; waiting for the business's Confirm.
    AwaitingConfirm,
    /// The Confirm window ended with no Confirm (PICK-019, PICK-035).
    Expired,
    /// Ended before a Confirm: replaced by another pick (PICK-032) or withdrawn by a pause
    /// (CON-029).
    Cancelled,
    /// The charge succeeded; the details may be released (PICK-022).
    Paid,
    /// A $0 fee decision was recorded; the details may be released (PICK-022, D-477).
    Free,
}

impl PickStatus {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AwaitingConfirm => "awaiting_confirm",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Paid => "paid",
            Self::Free => "free",
        }
    }
}

/// The only pick status a release proof of `basis` may release from: paid for a paid charge, free
/// for a $0 fee decision (PICK-022, PRIV-005, D-1564); every other status releases nothing.
#[must_use]
pub const fn release_status(basis: ReleaseBasis) -> PickStatus {
    match basis {
        ReleaseBasis::PaidCharge => PickStatus::Paid,
        ReleaseBasis::NoFeeDue => PickStatus::Free,
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

/// Whether an account in `status` may Confirm a pick: Active only (PICK-020, D-056).
#[must_use]
pub const fn account_confirms(status: AccountStatus) -> bool {
    match status {
        AccountStatus::Active => true,
        AccountStatus::Pending
        | AccountStatus::Paused
        | AccountStatus::Waitlist
        | AccountStatus::NeedsReview
        | AccountStatus::Removed => false,
    }
}

/// Whether a job in `status` still waits for the business's Confirm (PICK-019).
#[must_use]
pub const fn awaits_confirm(status: JobStatus) -> bool {
    match status {
        JobStatus::AwaitingConfirm => true,
        JobStatus::Intake
        | JobStatus::Open
        | JobStatus::NoQuotesYet
        | JobStatus::QuotesReady
        | JobStatus::PaymentPending
        | JobStatus::Connected
        | JobStatus::Done
        | JobStatus::Closed
        | JobStatus::ClosedEarly
        | JobStatus::NotCovered
        | JobStatus::Cancelled => false,
    }
}

/// Whether a waiting pick is dead because its job in `status` no longer waits for this pick's
/// Confirm and no Confirm moved it on (PICK-019, PICK-044, D-1503): `jobs`' backstop or an
/// emergency's 60 minutes returned it to Quotes ready, or it closed or was cancelled. Payment
/// pending, Connected and Done follow a Confirm, so they are not a dead pick.
#[must_use]
pub const fn pick_lost(status: JobStatus) -> bool {
    match status {
        JobStatus::Intake
        | JobStatus::Open
        | JobStatus::NoQuotesYet
        | JobStatus::QuotesReady
        | JobStatus::Closed
        | JobStatus::ClosedEarly
        | JobStatus::NotCovered
        | JobStatus::Cancelled => true,
        JobStatus::AwaitingConfirm
        | JobStatus::PaymentPending
        | JobStatus::Connected
        | JobStatus::Done => false,
    }
}

/// `jobs`' Awaiting confirm backstop in business minutes, read from `jobs`' own wait (JOB-013,
/// D-805, D-1503), so the Confirm window's restart follows any change there; zero if `jobs` ever
/// stops counting it in business minutes (the restart then starts at the window's end).
#[must_use]
pub const fn confirm_backstop_minutes() -> u32 {
    match JobStatus::AwaitingConfirm.wait() {
        Some((Wait::BusinessMinutes(minutes), _)) => minutes,
        Some((Wait::Hours(_) | Wait::Days(_) | Wait::NextDayAt(_), _)) | None => 0,
    }
}

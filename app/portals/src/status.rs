//! The job-status checks of the portals (SEC-031, JOB-013, JOB-014, D-4349; T664).
//!
//! Every `match` on `jobs`' `JobStatus` in this crate lives here and names every variant
//! (`docs/architecture.md` section 4 rule 5).
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use askhava_jobs::api::JobStatus;

/// JOB-013, JOB-014, PICK-020, D-4349: where a Confirm press finds its job: `Some(false)` in
/// Awaiting confirm (the press that leaves it, inside the Confirm window), `Some(true)` in
/// Payment pending (a retry press inside FEE-020's card-fix hour), `None` in any other status
/// (the job moved on, so the press's move is lost).
#[must_use]
pub(crate) const fn press_retries(status: JobStatus) -> Option<bool> {
    match status {
        JobStatus::AwaitingConfirm => Some(false),
        JobStatus::PaymentPending => Some(true),
        JobStatus::Intake
        | JobStatus::Open
        | JobStatus::NoQuotesYet
        | JobStatus::QuotesReady
        | JobStatus::Connected
        | JobStatus::Done
        | JobStatus::Closed
        | JobStatus::ClosedEarly
        | JobStatus::NotCovered
        | JobStatus::Cancelled => None,
    }
}

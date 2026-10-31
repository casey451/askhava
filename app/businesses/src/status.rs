//! A business account's six statuses and the only moves between them (BIZ-009, BIZ-003, BIZ-016,
//! SEC-031).
//!
//! Every `match` on a status lives here and names every variant, so a new status does not build
//! until each place handles it (`docs/architecture.md` section 4 rule 5). Insurance never appears
//! in an event, so it can never pause or activate a business (BIZ-008, D-061).
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants
)]

use crate::rules::Event;

/// A business account's one status (BIZ-009), stored in `businesses.business.status` as
/// [`AccountStatus::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountStatus {
    /// Signed up; waiting for the identity code and the license or city-list match.
    Pending,
    /// Receives cards.
    Active,
    /// Receives nothing until resumed; the row holds a [`PauseReason`].
    Paused,
    /// Signed up only for coming-later categories (BIZ-021).
    Waitlist,
    /// Flagged at sign-up; waits for Casey (BIZ-016).
    NeedsReview,
    /// Gone for good (final).
    Removed,
}

impl AccountStatus {
    /// All six statuses (BIZ-009).
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Active,
        Self::Paused,
        Self::Waitlist,
        Self::NeedsReview,
        Self::Removed,
    ];

    /// The stored code name, as in `businesses.business.status` and the audit row.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Waitlist => "waitlist",
            Self::NeedsReview => "needs_review",
            Self::Removed => "removed",
        }
    }

    /// The status a stored code names, if any.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.code() == code)
    }
}

/// Why a business is paused (BIZ-009); stored in `businesses.business.pause_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PauseReason {
    /// The business paused itself in the portal (BIZ-010).
    SelfPaused,
    /// Casey paused it (BIZ-016).
    Owner,
    /// Three complaints in 90 days (BIZ-012).
    Complaints,
    /// The license lookup stayed unknown, ended or lapsed (LIC-008, LIC-009).
    License,
    /// A license change left no covered job type (BIZ-050).
    NoCoveredJobType,
}

impl PauseReason {
    /// Every reason.
    pub const ALL: [Self; 5] = [
        Self::SelfPaused,
        Self::Owner,
        Self::Complaints,
        Self::License,
        Self::NoCoveredJobType,
    ];

    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SelfPaused => "self",
            Self::Owner => "owner",
            Self::Complaints => "complaints",
            Self::License => "license",
            Self::NoCoveredJobType => "no_covered_job_type",
        }
    }

    /// The reason a stored code names, if any.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// The status refused the event and stays as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("business status move refused")]
pub struct Refused;

/// The status `event` moves a business in `from` to (BIZ-009, BIZ-003, BIZ-016).
///
/// Sign-up activates only with the code and the match and no flag; a flag (even before the code,
/// as when no record contact can get one, BIZ-002) sends it to needs review, where only Casey's approval, with the code and the match, activates. Only an active
/// business pauses and only a paused one resumes; removed is final.
///
/// # Errors
///
/// [`Refused`] when no rule allows the move.
pub fn next(from: AccountStatus, event: Event) -> Result<AccountStatus, Refused> {
    use AccountStatus as S;
    match (from, event) {
        (S::Pending, Event::Checked(c)) if c.flagged => Ok(S::NeedsReview),
        (S::Pending, Event::Checked(c)) if c.code_done && c.match_passed => Ok(S::Active),
        (S::NeedsReview, Event::OwnerApproved(c)) if c.code_done && c.match_passed => Ok(S::Active),
        (S::Waitlist, Event::CategoryOpened) => Ok(S::Pending),
        (S::Active, Event::Pause(_)) => Ok(S::Paused),
        (S::Paused, Event::Resume) => Ok(S::Active),
        (S::Pending | S::Active | S::Paused | S::Waitlist | S::NeedsReview, Event::Remove) => {
            Ok(S::Removed)
        }
        (
            S::Pending | S::Active | S::Paused | S::Waitlist | S::NeedsReview | S::Removed,
            Event::Checked(_)
            | Event::OwnerApproved(_)
            | Event::CategoryOpened
            | Event::Pause(_)
            | Event::Resume
            | Event::Remove,
        ) => Err(Refused),
    }
}

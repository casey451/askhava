//! Pure rules of the business records: the job-type checklist within the license and the town
//! list (BIZ-004, BIZ-010, LIC-043), and the events that may move a status (BIZ-009). No I/O.

use std::collections::BTreeSet;

use crate::models::BusinessesError;
use crate::status::PauseReason;

/// What sign-up has proved so far (BIZ-003, BIZ-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checks {
    /// The identity code was entered (BIZ-002).
    pub code_done: bool,
    /// The license or city-list match passed (LIC-011, LIC-027).
    pub match_passed: bool,
    /// A record or name mismatch, a fraud signal or no usable record contact (BIZ-002, BIZ-016).
    pub flagged: bool,
}

/// Something that may move a business's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Sign-up's checks were (re)run.
    Checked(Checks),
    /// Casey approved a needs-review sign-up; it still needs the code and the match (BIZ-003).
    OwnerApproved(Checks),
    /// A category the waitlisted business chose opened (BIZ-021).
    CategoryOpened,
    /// The business is paused, for this reason.
    Pause(PauseReason),
    /// The business is resumed.
    Resume,
    /// The business is removed.
    Remove,
}

/// One ticked job type: its catalog category code and job type name (BIZ-004).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobTypeChoice {
    /// The catalog category code, such as `plumbing_repair`.
    pub category: String,
    /// The catalog job type name.
    pub job_type: String,
}

impl JobTypeChoice {
    /// A choice of `job_type` in `category`.
    #[must_use]
    pub fn new(category: &str, job_type: &str) -> Self {
        Self {
            category: category.to_owned(),
            job_type: job_type.to_owned(),
        }
    }
}

/// A code as the catalog writes towns and categories: lower case, digits and `_`, starting with a
/// letter, at most 64 characters (the migration's check).
fn is_code(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The checklist to store: every ticked job type must be one `allowed` holds, which the caller
/// builds from what the license covers as the license map says plus the no-license job types
/// (BIZ-004, BIZ-049, LIC-043). Repeats collapse; an empty list is allowed (the owner can untick
/// any).
///
/// # Errors
///
/// [`BusinessesError::BadCode`] for a malformed category or job type;
/// [`BusinessesError::NotCovered`] for a job type `allowed` lacks.
pub fn checklist(
    ticked: &[JobTypeChoice],
    allowed: &BTreeSet<JobTypeChoice>,
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    let mut out = BTreeSet::new();
    for choice in ticked {
        let name = choice.job_type.trim();
        if !is_code(&choice.category) || name.is_empty() || name.len() > 100 {
            return Err(BusinessesError::BadCode);
        }
        if !allowed.contains(choice) {
            return Err(BusinessesError::NotCovered);
        }
        out.insert(choice.clone());
    }
    Ok(out.into_iter().collect())
}

/// The towns to store, sorted with repeats collapsed (BIZ-010).
///
/// # Errors
///
/// [`BusinessesError::BadCode`] for a town that is not a catalog code.
pub fn towns(towns: &[&str]) -> Result<Vec<String>, BusinessesError> {
    if towns.iter().all(|t| is_code(t)) {
        let set: BTreeSet<String> = towns.iter().map(|t| (*t).to_owned()).collect();
        Ok(set.into_iter().collect())
    } else {
        Err(BusinessesError::BadCode)
    }
}

//! Pure rules of the business records: the job-type checklist within the license and the town
//! list (BIZ-004, BIZ-010, LIC-043), the events that may move a status (BIZ-009), and the
//! self-pause and own quiet times rules (BIZ-010, D-085). No I/O.

use std::collections::BTreeSet;

use jiff::civil::Time;

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
/// [`BusinessesError::BadCode`] for a malformed category or a job type that is empty, over 100
/// bytes or has leading or trailing spaces;
/// [`BusinessesError::NotCovered`] for a job type `allowed` lacks.
pub fn checklist(
    ticked: &[JobTypeChoice],
    allowed: &BTreeSet<JobTypeChoice>,
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    let mut out = BTreeSet::new();
    for choice in ticked {
        let name = &choice.job_type;
        if !is_code(&choice.category) || name.trim() != name || name.is_empty() || name.len() > 100
        {
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

/// First minute after midnight inside the app-wide window, Arizona time (QH-001, D-003). It
/// repeats `messaging`'s private window hours, which T182 may not change (D-968).
pub const APP_OPEN_MINUTE: i16 = 8 * 60;
/// First minute outside the app-wide window, so 19:59 is in and 20:00 is out (QH-001, D-101).
pub const APP_CLOSE_MINUTE: i16 = 20 * 60;

/// The asked-for own quiet times would widen the app-wide window, be empty or not fall on whole
/// minutes (BIZ-010, D-085).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("own quiet times must narrow the app-wide window")]
pub struct Widens;

/// A business's own notice window in Arizona time (QH-002): notices may go from `open` up to but
/// not including `close`. The checked constructor keeps it inside the app-wide 8 a.m. to 8 p.m.
/// window, so it can only narrow it (BIZ-010, D-085, D-968).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnHours {
    open: i16,
    close: i16,
}

fn whole_minute(t: Time) -> Option<i16> {
    (t.second() == 0 && t.subsec_nanosecond() == 0)
        .then(|| i16::from(t.hour()) * 60 + i16::from(t.minute()))
}

impl OwnHours {
    /// The window from `open` to `close` (BIZ-010, D-085).
    ///
    /// # Errors
    ///
    /// [`Widens`] when `open` is before 8:00, `close` after 20:00, `open` is not before `close`,
    /// or either has seconds.
    pub fn new(open: Time, close: Time) -> Result<Self, Widens> {
        match (whole_minute(open), whole_minute(close)) {
            (Some(o), Some(c)) => Self::from_minutes(o, c),
            _ => Err(Widens),
        }
    }

    /// The window from stored minutes after midnight (migration `0057_businesses_own_hours.sql`).
    ///
    /// # Errors
    ///
    /// [`Widens`] when the minutes leave the app-wide window or the window is empty.
    pub fn from_minutes(open: i16, close: i16) -> Result<Self, Widens> {
        if APP_OPEN_MINUTE <= open && open < close && close <= APP_CLOSE_MINUTE {
            Ok(Self { open, close })
        } else {
            Err(Widens)
        }
    }

    /// The first minute after midnight inside the window.
    #[must_use]
    pub const fn open_minute(self) -> i16 {
        self.open
    }

    /// The first minute after midnight outside the window.
    #[must_use]
    pub const fn close_minute(self) -> i16 {
        self.close
    }
}

/// Whether a business notice may go at `at`, Arizona wall-clock time (QH-002): inside the app-wide
/// window and, when the business set its own quiet times, inside those too, so its own times only
/// ever narrow the window (BIZ-010, D-085, QH-001, QH-005). It checks Arizona time only: the
/// recipient's area-code time zone check (the stricter of Arizona and area-code time, QH-001,
/// QH-003) and the QH-004 exceptions remain the sender's to apply.
#[must_use]
pub fn may_notify(own: Option<OwnHours>, at: Time) -> bool {
    let minute = i16::from(at.hour()) * 60 + i16::from(at.minute());
    let own = own.map_or(APP_OPEN_MINUTE..APP_CLOSE_MINUTE, |h| h.open..h.close);
    (APP_OPEN_MINUTE..APP_CLOSE_MINUTE).contains(&minute) && own.contains(&minute)
}

/// Whether a business may resume itself: only from a pause it chose (BIZ-010); a pause for
/// complaints, the license, no covered job type or by Casey ends only by its own rule (BIZ-009,
/// D-969). It compares the stored code, as SEC-031 keeps status matches in `status`.
#[must_use]
pub fn may_resume_itself(reason: Option<PauseReason>) -> bool {
    reason.is_some_and(|r| r.code() == "self")
}

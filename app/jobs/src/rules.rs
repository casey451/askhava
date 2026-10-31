//! Pure job-lifecycle rules: which moves are allowed and when each waiting status times out
//! (JOB-002, JOB-003; `docs/architecture.md` section 6). No I/O; property-tested in
//! `tests/prop_job_machine.rs`.
//!
//! The timeouts here are each waiting status's backstop timer, set when the job enters it, on the
//! business clock (8 a.m. to 8 p.m. Arizona time every day, JOB-007, D-004) or, for an emergency
//! job's confirm and payment waits, the 60-minute emergency clock (PICK-044). Later tasks add the
//! finer timers that are not status moves (the Intake reminder at 24 hours, HOLD and away-time
//! extensions, the Connected call check and check-ins). [`connected_move`] is where a Connected
//! job's check-in reply leads (JOB-015) and [`after_windows`] what happens once every card's quote
//! window has ended (JOB-025, TXT-042, D-140).

use askhava_core::api::{add_business_minutes, arizona, arizona_time};
use jiff::{SignedDuration, Timestamp};

use crate::status::JobStatus;

/// Intake closes early after 72 hours with no reply (INT-019, D-095).
pub(crate) const INTAKE_HOURS: i64 = 72;
/// Open becomes No quotes yet at this Arizona hour the next day (JOB-010).
pub(crate) const NO_QUOTES_HOUR: i8 = 18;
/// No quotes yet closes after 5 days (JOB-011).
pub(crate) const NO_QUOTES_DAYS: i64 = 5;
/// Quotes ready closes after 3 days with no pick, restarted on every entry (JOB-012, JOB-025).
pub(crate) const NO_PICK_DAYS: i64 = 3;
/// Awaiting confirm returns after 4 business hours (JOB-013, PICK-019).
pub(crate) const CONFIRM_BUSINESS_MINUTES: u32 = 4 * 60;
/// Payment pending returns after 1 business hour (JOB-014, FEE-020).
pub(crate) const PAYMENT_BUSINESS_MINUTES: u32 = 60;
/// An emergency job's confirm and payment waits are 60 clock minutes (PICK-044).
const EMERGENCY_MINUTES: i64 = 60;
/// Connected closes 7 days after the day-14 check-in with no reply (JOB-026).
pub(crate) const CONNECTED_DAYS: i64 = 14 + 7;

/// How long a waiting status waits; each status's wait is [`JobStatus::wait`] (JOB-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Clock hours from entry.
    Hours(i64),
    /// Clock days from entry.
    Days(i64),
    /// The given Arizona hour on the day after entry (JOB-010).
    NextDayAt(i8),
    /// Business minutes from entry (JOB-007), or 60 clock minutes on an emergency job (PICK-044).
    BusinessMinutes(u32),
}

/// A waiting status's timeout: when it fires and where the job then moves (JOB-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    /// When the timer fires.
    pub at: Timestamp,
    /// The status the job moves to if it is still waiting then.
    pub then: JobStatus,
}

/// The timeout of `status` entered at `entered`; none for a final status (JOB-003, JOB-007,
/// JOB-010 to JOB-014, JOB-026). `emergency` shortens the confirm and payment waits (PICK-044).
#[must_use]
pub fn timeout(status: JobStatus, entered: Timestamp, emergency: bool) -> Option<Timeout> {
    let (wait, then) = status.wait()?;
    let clock = |minutes: i64| {
        entered
            .saturating_add(SignedDuration::from_mins(minutes))
            .ok()
    };
    let at = match wait {
        Wait::Hours(h) => clock(h * 60),
        Wait::Days(d) => clock(d * 24 * 60),
        Wait::NextDayAt(hour) => next_day_at(entered, hour),
        Wait::BusinessMinutes(_) if emergency => clock(EMERGENCY_MINUTES),
        Wait::BusinessMinutes(m) => Some(add_business_minutes(entered, m)),
    };
    Some(Timeout {
        at: at.unwrap_or(Timestamp::MAX),
        then,
    })
}

/// `hour`:00 Arizona time on the day after `entered` (JOB-010, JOB-007).
fn next_day_at(entered: Timestamp, hour: i8) -> Option<Timestamp> {
    let day = arizona_time(entered).date().tomorrow().ok()?;
    let at = day.at(hour, 0, 0, 0).to_zoned(arizona()).ok()?;
    Some(at.timestamp())
}

/// A homeowner's check-in answer on a Connected job, as `intake` parsed it (DONE or LISTO, NO,
/// PROBLEM; JOB-015, JOB-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// The work is done.
    Done,
    /// The business never reached her (or, for call contact, the work is not going ahead).
    No,
    /// Something went wrong.
    Problem,
}

/// Which check-in the reply answers (JOB-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckIn {
    /// Day 3, counted from the later of the release and the earliest visit date.
    Day3,
    /// Day 14.
    Day14,
}

/// How the homeowner chose to be contacted (JOB-015, D-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    /// The business calls her.
    Call,
    /// The business emails her.
    Email,
}

/// Where a Connected job moves on `reply` (JOB-015, D-143): DONE to Done, PROBLEM to Closed, NO to
/// Closed, except a day-3 NO under email contact, which records a no-show and returns the job to
/// Quotes ready.
#[must_use]
pub fn connected_move(reply: Reply, check_in: CheckIn, contact: Contact) -> JobStatus {
    match (reply, check_in, contact) {
        (Reply::Done, _, _) => JobStatus::Done,
        (Reply::No, CheckIn::Day3, Contact::Email) => JobStatus::QuotesReady,
        (Reply::No | Reply::Problem, _, _) => JobStatus::Closed,
    }
}

/// A Quotes ready job's counts once every card's 24-hour quote window has ended (TXT-042,
/// JOB-025), as `quotes` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    /// Businesses that quoted (`<k>` in TXT-042).
    pub quoted: u32,
    /// Businesses whose card was sent (`<N>` in TXT-042).
    pub sent: u32,
    /// Quotes she can still choose: not withdrawn and not a failed pick's (D-045).
    pub choosable: u32,
}

/// What ending every quote window does to a Quotes ready job (JOB-025, TXT-042, D-140).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsEnd {
    /// No choosable quote is left: the job closes and she is told.
    Close,
    /// She gets the one all-quotes-in text and the job waits for her pick.
    AllQuotesIn,
}

/// The JOB-025 rule: close only when no choosable quote is left, else the all-quotes-in text.
#[must_use]
pub fn after_windows(windows: Windows) -> WindowsEnd {
    if windows.choosable == 0 {
        WindowsEnd::Close
    } else {
        WindowsEnd::AllQuotesIn
    }
}

/// Checks the move from `from` to `to` against the arrows (JOB-002).
///
/// # Errors
///
/// [`Refused`] when no arrow joins them; the caller logs it as a bug and changes nothing.
pub fn check_move(from: JobStatus, to: JobStatus) -> Result<(), Refused> {
    if from.can_move_to(to) {
        Ok(())
    } else {
        Err(Refused { from, to })
    }
}

/// A move the arrows do not allow (JOB-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    /// The job's status.
    pub from: JobStatus,
    /// The status the move asked for.
    pub to: JobStatus,
}

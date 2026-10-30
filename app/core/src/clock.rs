//! The injectable clock, Arizona time and the business clock (JOB-007, D-003, D-004).
//!
//! Instants are stored as UTC `timestamptz`; all zone logic lives here
//! (postgres-operations P7). Arizona keeps no daylight saving, so Arizona time
//! is a fixed UTC-7 offset, which the clock tests compare with the
//! `America/Phoenix` zone. Code that reads "now" takes a `&dyn Clock`, so
//! tests and the test-control router move time with [`ManualClock`].

use std::sync::{Mutex, PoisonError};

use jiff::tz::{Offset, TimeZone};
use jiff::{SignedDuration, Timestamp, Zoned};
use thiserror::Error;

/// Hour (Arizona time) the business clock opens, every day of the week (JOB-007, D-004).
pub const BUSINESS_OPEN_HOUR: i8 = 8;
/// Hour (Arizona time) the business clock closes, every day of the week (JOB-007, D-004).
pub const BUSINESS_CLOSE_HOUR: i8 = 20;

const ARIZONA_OFFSET_HOURS: i8 = -7;
const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;
const OPEN: i64 = BUSINESS_OPEN_HOUR as i64 * HOUR;
const CLOSE: i64 = BUSINESS_CLOSE_HOUR as i64 * HOUR;

/// Why a zone could not be used.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClockError {
    /// The name is not an IANA zone in the bundled database.
    #[error("unknown time zone")]
    UnknownZone,
}

/// The source of "now"; inject it so tests control time.
pub trait Clock: Send + Sync {
    /// The current instant.
    fn now(&self) -> Timestamp;
}

/// The real clock the binaries use.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A clock that moves only when told, for tests and the test-control router (SEC-035).
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<Timestamp>,
}

impl ManualClock {
    /// A clock stopped at `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Moves the clock to `at`.
    pub fn set(&self, at: Timestamp) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = at;
    }

    /// Moves the clock by `by`, saturating at the ends of the supported range.
    pub fn advance(&self, by: SignedDuration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now.saturating_add(by).unwrap_or(*now);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Arizona's zone: UTC-7 all year, no daylight saving (JOB-007).
#[must_use]
pub fn arizona() -> TimeZone {
    TimeZone::fixed(Offset::constant(ARIZONA_OFFSET_HOURS))
}

/// The instant as Arizona wall-clock time (JOB-007).
#[must_use]
pub fn arizona_time(at: Timestamp) -> Zoned {
    at.to_zoned(arizona())
}

/// An IANA zone by name, for a recipient's local time (D-003, D-025).
///
/// # Errors
///
/// [`ClockError::UnknownZone`] when the bundled database lacks the name.
pub fn zone(name: &str) -> Result<TimeZone, ClockError> {
    TimeZone::get(name).map_err(|_| ClockError::UnknownZone)
}

/// The instant as wall-clock time in `zone`, which `messaging` uses for quiet hours (D-003).
#[must_use]
pub fn local_time(at: Timestamp, zone: &TimeZone) -> Zoned {
    at.to_zoned(zone.clone())
}

/// Whether `at` falls in 8:00 a.m. to 8:00 p.m. Arizona time (JOB-007, D-004);
/// 8:00 p.m. itself is closed.
#[must_use]
pub fn is_business_hours(at: Timestamp) -> bool {
    let second_of_day = arizona_seconds(at).rem_euclid(DAY);
    (OPEN..CLOSE).contains(&second_of_day)
}

/// The instant `minutes` business minutes after `start` (JOB-007, D-004): only
/// time between 8:00 a.m. and 8:00 p.m. Arizona time counts, and a start
/// outside those hours counts from the next opening. A timer can end at
/// exactly 8:00 p.m.
#[must_use]
pub fn add_business_minutes(start: Timestamp, minutes: u32) -> Timestamp {
    let origin = arizona_seconds(start);
    let mut at = origin;
    let mut left = i64::from(minutes) * 60;
    loop {
        at = next_open_second(at);
        let room = at - at.rem_euclid(DAY) + CLOSE - at;
        if left <= room {
            at += left;
            break;
        }
        left -= room;
        at += room;
    }
    start
        .saturating_add(SignedDuration::from_secs(at - origin))
        .unwrap_or(Timestamp::MAX)
}

/// `at` if the business clock is open then, else the next 8:00 a.m. (JOB-007).
fn next_open_second(at: i64) -> i64 {
    let second_of_day = at.rem_euclid(DAY);
    let day_start = at - second_of_day;
    if second_of_day < OPEN {
        day_start + OPEN
    } else if second_of_day >= CLOSE {
        day_start + DAY + OPEN
    } else {
        at
    }
}

fn arizona_seconds(at: Timestamp) -> i64 {
    at.as_second() + i64::from(ARIZONA_OFFSET_HOURS) * HOUR
}

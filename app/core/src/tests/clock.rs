//! The clock, Arizona time and the business clock (JOB-007, D-003, D-004).
//!
//! Example cases pin the boundaries (8:00 a.m. open, 8:00 p.m. closed, the
//! overnight skip, weekends counted like weekdays); property tests check that
//! a business timer always ends inside the window and that adding in two
//! steps equals adding at once (rust-production R17).

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;

use crate::clock::{
    Clock, ClockError, ManualClock, add_business_minutes, arizona, arizona_time, is_business_hours,
    local_time, zone,
};

/// A fixed instant from Arizona wall-clock text (UTC-7).
fn az(text: &str) -> Timestamp {
    format!("{text}-07:00").parse().unwrap()
}

/// JOB-007: Arizona time is UTC-7 in winter and summer alike, matching `America/Phoenix`.
#[test]
fn arizona_has_no_daylight_saving() {
    let phoenix = zone("America/Phoenix").unwrap();
    for text in [
        "2026-01-15T12:00:00Z",
        "2026-07-15T12:00:00Z",
        "2026-03-08T10:30:00Z",
        "2026-11-01T09:30:00Z",
    ] {
        let at: Timestamp = text.parse().unwrap();
        assert_eq!(arizona_time(at).offset(), local_time(at, &phoenix).offset());
        assert_eq!(arizona_time(at).hour(), local_time(at, &arizona()).hour());
    }
}

/// D-003: a recipient zone with daylight saving differs from Arizona in summer; an unknown zone is refused.
#[test]
fn recipient_local_time_uses_their_zone() {
    let denver = zone("America/Denver").unwrap();
    let summer = az("2026-07-15T19:30:00");
    assert_eq!(local_time(summer, &denver).hour(), 20);
    assert_eq!(arizona_time(summer).hour(), 19);
    assert_eq!(
        zone("Mars/Olympus_Mons").unwrap_err(),
        ClockError::UnknownZone
    );
}

/// JOB-007: 8:00 a.m. is open, 7:59:59 p.m. is open, 8:00 p.m. is closed, every day of the week.
#[test]
fn business_hours_boundaries() {
    assert!(!is_business_hours(az("2026-10-31T07:59:59")));
    assert!(is_business_hours(az("2026-10-31T08:00:00")));
    assert!(is_business_hours(az("2026-11-01T19:59:59")));
    assert!(!is_business_hours(az("2026-11-01T20:00:00")));
    assert!(!is_business_hours(az("2026-11-02T00:00:00")));
}

/// JOB-007, D-004: 4 business hours from 4:00 p.m. ends at exactly 8:00 p.m.
#[test]
fn timer_may_end_at_closing() {
    assert_eq!(
        add_business_minutes(az("2026-10-30T16:00:00"), 240),
        az("2026-10-30T20:00:00")
    );
}

/// JOB-007, D-004: time after closing carries to the next morning, Saturday included.
#[test]
fn timer_skips_the_night() {
    assert_eq!(
        add_business_minutes(az("2026-10-30T18:00:00"), 240),
        az("2026-10-31T10:00:00")
    );
    assert_eq!(
        add_business_minutes(az("2026-10-30T22:15:00"), 60),
        az("2026-10-31T09:00:00")
    );
    assert_eq!(
        add_business_minutes(az("2026-10-31T05:00:00"), 30),
        az("2026-10-31T08:30:00")
    );
    assert_eq!(
        add_business_minutes(az("2026-10-30T09:00:00"), 3 * 720),
        az("2026-11-02T09:00:00")
    );
}

/// JOB-007: the manual clock moves only when told.
#[test]
fn manual_clock_moves_when_told() {
    let clock = ManualClock::new(az("2026-10-30T08:00:00"));
    clock.advance(SignedDuration::from_mins(90));
    assert_eq!(clock.now(), az("2026-10-30T09:30:00"));
    clock.set(az("2026-12-25T12:00:00"));
    assert_eq!(clock.now(), az("2026-12-25T12:00:00"));
}

fn any_instant() -> impl Strategy<Value = Timestamp> {
    (1_700_000_000_i64..2_000_000_000).prop_map(|second| Timestamp::from_second(second).unwrap())
}

proptest! {
    /// JOB-007: a timer never ends before its start and always ends inside 8:00 a.m. to 8:00 p.m.
    #[test]
    fn timer_ends_inside_the_window(start in any_instant(), minutes in 1_u32..20_000) {
        let end = add_business_minutes(start, minutes);
        prop_assert!(end > start);
        let hour_minute = (arizona_time(end).hour(), arizona_time(end).minute());
        prop_assert!(is_business_hours(end) || hour_minute == (20, 0));
    }

    /// JOB-007: adding a then b business minutes equals adding a + b at once.
    #[test]
    fn timers_compose(start in any_instant(), a in 1_u32..5_000, b in 1_u32..5_000) {
        prop_assert_eq!(add_business_minutes(add_business_minutes(start, a), b), add_business_minutes(start, a + b));
    }
}

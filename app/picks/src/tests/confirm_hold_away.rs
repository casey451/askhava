//! The pure HOLD, Away and emergency-window rules (TD-PICK-20, TD-PICK-21, TD-PICK-23;
//! PICK-043, PICK-044, BIZ-034). Fake job numbers only; times are Arizona wall-clock in November
//! 2026.

use askhava_businesses::api::Roles;
use jiff::{SignedDuration, Timestamp};

use crate::confirm::{
    BusinessWord, ConfirmSettings, HoldAsk, NightLine, TimeRefusal, TimeWord, WindowRules,
    away_for, away_until, confirm_by, emergency_fix_by, emergency_plan, hold, hold_allowed,
    read_business_word, read_time_word, window_start,
};

/// Arizona wall-clock time on November `day`, 2026 (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

fn cards_only() -> Roles {
    Roles::new(true, false).expect("roles")
}

/// A first HOLD by the owner on a regular job.
fn ask() -> HoldAsk {
    HoldAsk {
        by: Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    }
}

/// TD-PICK-20 (PICK-043, D-252, D-477): HOLD at 1:59 into a window opened at 10:00 moves its end
/// from 14:00 to 16:00, 6 business hours in all; a second HOLD, one from a "Cards and quotes"
/// phone and one on an emergency job are refused with the reason; after the end, too late.
#[test]
fn td_pick_20_hold_adds_2_business_hours_once() {
    let rules = WindowRules::default();
    let due = confirm_by(az(9, 10, 0), ConfirmSettings::default());
    assert_eq!(due, az(9, 14, 0));
    assert_eq!(hold(ask(), (az(9, 11, 59), due), rules), Ok(az(9, 16, 0)));
    let billing = Roles::new(false, true).expect("roles");
    let member = HoldAsk {
        by: billing,
        ..ask()
    };
    assert_eq!(hold(member, (az(9, 11, 59), due), rules), Ok(az(9, 16, 0)));
    let refused = [
        (
            HoldAsk {
                held: true,
                ..ask()
            },
            TimeRefusal::AlreadyHeld,
        ),
        (
            HoldAsk {
                by: cards_only(),
                ..ask()
            },
            TimeRefusal::Role,
        ),
        (
            HoldAsk {
                emergency: true,
                ..ask()
            },
            TimeRefusal::Emergency,
        ),
    ];
    for (asked, reason) in refused {
        assert_eq!(hold(asked, (az(9, 11, 59), due), rules), Err(reason));
    }
    assert_eq!(hold(ask(), (due, due), rules), Err(TimeRefusal::Ended));
    let late = confirm_by(az(9, 18, 30), ConfirmSettings::default());
    assert_eq!(hold(ask(), (az(9, 19, 0), late), rules), Ok(az(10, 12, 30)));
    let short = WindowRules {
        hold_minutes: 60,
        ..rules
    };
    assert_eq!(hold(ask(), (az(9, 11, 59), due), short), Ok(az(9, 15, 0)));
}

/// TD-PICK-20 (PICK-033, PICK-043, D-259, D-285): HOLD and ESPERA read the job number, AWAY and
/// AUSENTE the hours; SÍ and CONFIRMAR work like YES and CONFIRM; anything else is none.
#[test]
fn td_pick_20_hold_and_away_words_in_english_and_spanish() {
    assert_eq!(read_time_word("HOLD 1042"), Some(TimeWord::Hold(1042)));
    assert_eq!(read_time_word("espera 1042."), Some(TimeWord::Hold(1042)));
    assert_eq!(read_time_word("Away 3"), Some(TimeWord::Away(3)));
    assert_eq!(read_time_word("AUSENTE 2!"), Some(TimeWord::Away(2)));
    for none in [
        "HOLD",
        "HOLD on 1042",
        "HOLD 1042 now",
        "STOP 1042",
        "AWAY x",
        "",
    ] {
        assert_eq!(read_time_word(none), None, "{none}");
    }
    assert_eq!(
        read_business_word("SÍ 1042"),
        Some((BusinessWord::Yes, Some(1042)))
    );
    assert_eq!(
        read_business_word("CONFIRMAR 1042"),
        Some((BusinessWord::Confirm, Some(1042)))
    );
}

/// TD-PICK-21 (PICK-044, D-270, D-346): an emergency pick sent at 23:00 by a business that ticked
/// after-hours is still open at 23:59:59 and ends at 00:00:00, its reminder at 23:30; a card
/// declined at 23:10 has 60 clock minutes to fix.
#[test]
fn td_pick_21_emergency_windows_are_60_clock_minutes() {
    let rules = WindowRules::default();
    let plan = emergency_plan(az(9, 23, 0), true, rules);
    assert_eq!(plan.send_at, az(9, 23, 0));
    assert_eq!(plan.confirm_by, az(10, 0, 0));
    let last_second = az(10, 0, 0) - SignedDuration::from_secs(1);
    assert!(plan.open_at(az(9, 23, 0)));
    assert!(plan.open_at(last_second));
    assert!(!plan.open_at(az(10, 0, 0)));
    assert!(!plan.open_at(az(9, 22, 59)));
    assert_eq!(plan.reminder_at, Some(az(9, 23, 30)));
    assert_eq!(plan.night, NightLine::None);
    assert_eq!(emergency_fix_by(az(9, 23, 10), rules), az(10, 0, 10));
    let day = emergency_plan(az(9, 10, 0), false, rules);
    assert_eq!((day.confirm_by, day.night), (az(9, 11, 0), NightLine::None));
}

/// TD-PICK-21 (PICK-044, D-346): a business without the after-hours tick picked at 22:00 gets its
/// notice at 08:00 and the window counts from then; picked at 19:30 its window ends after 8 p.m.,
/// so the homeowner is warned and the 20:00 reminder, held to 08:00 past the 20:30 end, is
/// dropped; a window ending exactly at 8 p.m. warns nothing. A non-emergency pick at 19:30 still counts business hours.
#[test]
fn td_pick_21_night_picks_of_a_business_without_after_hours() {
    let rules = WindowRules::default();
    let night = emergency_plan(az(9, 22, 0), false, rules);
    assert_eq!(night.send_at, az(10, 8, 0));
    assert_eq!(night.confirm_by, az(10, 9, 0));
    assert_eq!(night.reminder_at, Some(az(10, 8, 30)));
    assert_eq!(night.night, NightLine::SeesAtEight);
    let dusk = emergency_plan(az(9, 19, 30), false, rules);
    assert_eq!(dusk.send_at, az(9, 19, 30));
    assert_eq!(dusk.confirm_by, az(9, 20, 30));
    assert_eq!(dusk.reminder_at, None);
    assert_eq!(dusk.night, NightLine::MayMissAfterEight);
    let edge = emergency_plan(az(9, 19, 0), false, rules);
    assert_eq!(
        (edge.reminder_at, edge.night),
        (Some(az(9, 19, 30)), NightLine::None)
    );
    let regular = confirm_by(az(9, 19, 30), ConfirmSettings::default());
    assert_eq!(regular, az(10, 11, 30));
}

/// TD-PICK-23 (BIZ-034, PICK-043, D-284, D-285): AWAY 3 at 10:00 ends at 13:00 and a pick sent at
/// 11:00 starts its clock then, delayed, so its HOLD is refused; AWAY 5, AWAY 0 by day or at
/// 22:00 and a "Cards and quotes" phone are refused; an emergency pick during away is not delayed; HOLD on a "Today" regular job
/// is accepted.
#[test]
fn td_pick_23_away_delays_the_clock_and_blocks_hold() {
    let rules = WindowRules::default();
    let away = away_for(Roles::BOTH, az(9, 10, 0), 3, rules);
    assert_eq!(away, Ok(az(9, 13, 0)));
    assert_eq!(
        window_start(az(9, 11, 0), away.ok(), false),
        (az(9, 13, 0), true)
    );
    assert_eq!(
        window_start(az(9, 11, 0), away.ok(), true),
        (az(9, 11, 0), false)
    );
    assert_eq!(
        window_start(az(9, 14, 0), away.ok(), false),
        (az(9, 14, 0), false)
    );
    assert_eq!(
        window_start(az(9, 11, 0), None, false),
        (az(9, 11, 0), false)
    );
    let delayed = HoldAsk {
        away_delayed: true,
        ..ask()
    };
    assert_eq!(hold_allowed(delayed), Err(TimeRefusal::AwayDelayed));
    assert_eq!(hold_allowed(ask()), Ok(()));
    let too_far = away_for(Roles::BOTH, az(9, 10, 0), 5, rules);
    assert_eq!(too_far, Err(TimeRefusal::TooFar));
    assert_eq!(
        away_for(Roles::BOTH, az(9, 10, 0), 0, rules),
        Err(TimeRefusal::TooFar)
    );
    assert_eq!(
        away_for(Roles::BOTH, az(9, 22, 0), 0, rules),
        Err(TimeRefusal::TooFar)
    );
    assert_eq!(
        away_for(cards_only(), az(9, 22, 0), 0, rules),
        Err(TimeRefusal::Role)
    );
    assert_eq!(
        away_for(cards_only(), az(9, 10, 0), 2, rules),
        Err(TimeRefusal::Role)
    );
}

/// TD-PICK-23 (BIZ-034, D-285): "Away until" from the portal may be up to 4 business hours ahead,
/// counted in business hours across the night, and never in the past.
#[test]
fn td_pick_23_away_until_a_time() {
    let rules = WindowRules::default();
    let at = (az(9, 18, 0), az(10, 10, 0));
    assert_eq!(away_until(Roles::BOTH, at, rules), Ok(az(10, 10, 0)));
    let past = away_until(Roles::BOTH, (az(9, 18, 0), az(10, 10, 1)), rules);
    assert_eq!(past, Err(TimeRefusal::TooFar));
    let before = away_until(Roles::BOTH, (az(9, 18, 0), az(9, 17, 0)), rules);
    assert_eq!(before, Err(TimeRefusal::TooFar));
    let billing = Roles::new(false, true).expect("roles");
    assert_eq!(away_until(billing, at, rules), Ok(az(10, 10, 0)));
}

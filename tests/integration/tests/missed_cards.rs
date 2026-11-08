//! The monthly missed-cards account email (TD-BIZ-20, TD-PICK-33 BIZ-044 case; BIZ-044, BIZ-025).
//! Fake counts and links only.
//!
//! 6 cards with 3 ended without a quote sends one email on the 1st with the counts and the spam
//! and backup links; 6 and 2 sends none; 2 and 2 sends none (under 3); switched off sends none.
//! The English line equals the spec text, a Spanish line exists, and neither holds a WORD-024
//! pressure word. Your numbers' "cards that ended without a quote" is T085's page (BIZ-025); the
//! owner health page's share is TD-OPS-02's (D-491).

use askhava_messaging::api::{
    Language, MissedSettings, MonthCards, first_of_month, missed_email_due,
};
use askhava_picks::api::missed_cards_line;
use jiff::Timestamp;
use jiff::civil::date;

fn az(month: i8, day: i8, hour: i8) -> Timestamp {
    date(2026, month, day)
        .at(hour, 0, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

fn month(got: u32, ended_unquoted: u32) -> MonthCards {
    MonthCards {
        got,
        quoted: got - ended_unquoted,
        ended_unquoted,
    }
}

const LINKS: (&str, &str) = (
    "https://askhava.com/b/inbox-check",
    "https://askhava.com/b/notices",
);

/// TD-BIZ-20 (BIZ-044, D-331): who gets it and when.
#[test]
fn td_biz_20_who_gets_the_missed_cards_email() {
    let s = MissedSettings::default();
    assert!(missed_email_due(month(6, 3), s, true));
    assert!(!missed_email_due(month(6, 2), s, true));
    assert!(!missed_email_due(month(2, 2), s, true), "under 3 cards");
    assert!(!missed_email_due(month(6, 3), s, false), "switched off");
    assert!(!missed_email_due(
        month(0, 0),
        MissedSettings {
            min_cards: 0,
            share_percent: 50
        },
        true
    ));
    assert!(first_of_month(az(12, 1, 8)));
    assert!(!first_of_month(az(12, 2, 8)));
    assert!(
        !first_of_month(az(12, 1, 6) - jiff::SignedDuration::from_hours(7)),
        "Nov 30 in Arizona"
    );
}

/// TD-BIZ-20, TD-PICK-33 (BIZ-044, D-406): the words equal the spec, with a Spanish line.
#[test]
fn td_pick_33_the_missed_cards_line_equals_the_spec() {
    let en = missed_cards_line(month(6, 3), LINKS, Language::En);
    assert_eq!(
        en,
        "Ask Hava: Last month you got 6 job requests and quoted 3. 3 closed before you quoted. If our emails land in Spam or Promotions, this 1-minute fix moves them to your inbox: https://askhava.com/b/inbox-check. Or add a second address: https://askhava.com/b/notices."
    );
    let es = missed_cards_line(month(6, 3), LINKS, Language::Es);
    assert!(es.starts_with("Ask Hava: El mes pasado recibió 6 solicitudes"));
    assert!(es.contains(LINKS.0) && es.contains(LINKS.1));
    let pressure = [
        "hurry",
        "don't miss out",
        "act now",
        "limited time",
        "last chance",
        "apúrese",
        "no se lo pierda",
    ];
    for line in [en, es] {
        let folded = line.to_lowercase();
        assert!(pressure.iter().all(|w| !folded.contains(w)), "{line}");
    }
}

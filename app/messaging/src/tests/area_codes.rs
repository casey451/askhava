//! One area-code table (TD-QH-06; CON-018, QH-002, D-2630): the Canadian set for the HELP roaming
//! line is read from the zones of `area_codes.json`, so it cannot drift from the quiet-hours
//! table. Fake numbers only: 555 exchanges under real area codes.

use jiff::Timestamp;

use crate::api::{
    AreaCodes, Channel, Decision, HelpSettings, Kind, Language, Outgoing, decide, help_text,
};
use crate::keywords::canadian_codes;

const ROAMING: &str = "Canadian numbers may pay roaming charges";
const NEW_ONTARIO: [&str; 4] = ["382", "683", "753", "942"];

fn roams(code: &str) -> bool {
    let settings = HelpSettings::new((2, 24), None, (None, "+15205550100")).expect("settings");
    let phone = format!("+1{code}5550123");
    help_text(&settings, Language::En, &phone)
        .expect("help text")
        .contains(ROAMING)
}

fn sends(phone: &str, when: &str) -> bool {
    let table = AreaCodes::embedded().expect("table");
    let msg = Outgoing {
        phone,
        kind: Kind::Other,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    };
    let now: Timestamp = when.parse().expect("timestamp");
    matches!(decide(&table, &msg, now), Ok(Decision::Send))
}

/// TD-QH-06: every Canadian code has a zone, and the four new Ontario codes are in the set.
#[test]
fn td_qh_06_every_canadian_code_has_a_zone() {
    let table = AreaCodes::embedded().expect("table");
    let codes = canadian_codes();
    assert!(codes.len() >= 54);
    for code in &codes {
        let zones = table.zones_for(&format!("+1{code}5550123")).expect("zone");
        assert!(!zones.is_empty(), "{code}");
    }
    for code in NEW_ONTARIO {
        assert!(codes.contains(&code), "{code}");
    }
}

/// TD-QH-06: 257 (British Columbia) and every Canadian code get the roaming line.
#[test]
fn td_qh_06_canadian_codes_get_the_roaming_line() {
    assert!(roams("257"));
    for code in canadian_codes() {
        assert!(roams(code), "{code}");
    }
}

/// TD-QH-06: no US code gets the roaming line.
#[test]
fn td_qh_06_no_us_code_gets_the_roaming_line() {
    let table = AreaCodes::embedded().expect("table");
    let canadian = canadian_codes();
    for (code, _) in table.entries() {
        if !canadian.contains(&code) {
            assert!(!roams(code), "{code}");
        }
    }
}

/// TD-QH-06: 382, 683, 753 and 942 are textable inside 8 a.m. to 8 p.m. in Arizona and Toronto.
/// 2026-07-15 11:00 Arizona is 14:00 in Toronto; 2026-07-15 18:00 Toronto is 15:00 in Arizona.
#[test]
fn td_qh_06_new_ontario_codes_are_textable_in_both_zones() {
    for code in NEW_ONTARIO {
        let phone = format!("+1{code}5550123");
        assert!(sends(&phone, "2026-07-15T11:00:00-07:00"), "{code}");
        assert!(sends(&phone, "2026-07-15T15:00:00-07:00"), "{code}");
        assert!(
            !sends(&phone, "2026-07-15T17:30:00-07:00"),
            "{code} Toronto 20:30"
        );
    }
}

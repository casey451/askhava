//! One area-code table (TD-QH-06; CON-018, QH-002, D-2630): the Canadian set for the HELP roaming
//! line is read from the zones of `area_codes.json`, so it cannot drift from the quiet-hours
//! table. Fake numbers only: 555 exchanges under real area codes.

use jiff::Timestamp;

use crate::api::{
    AreaCodes, Channel, Decision, HelpSettings, Kind, Language, Outgoing, decide, help_text,
};
use crate::keywords::{CANADIAN_ZONES, canadian_codes};

const ROAMING: &str = "Canadian numbers may pay roaming charges";
const NEW_ONTARIO: [&str; 4] = ["382", "683", "753", "942"];
/// Known US codes (Arizona, New York, Pacific, Alaska, Hawaii, Puerto Rico) that never roam.
const US_CODES: [&str; 8] = ["928", "520", "602", "212", "310", "907", "808", "787"];
/// Every non-Canadian zone in `area_codes.json`; a new zone must be classified here or in
/// `CANADIAN_ZONES`.
const US_ZONES: [&str; 13] = [
    "America/Adak",
    "America/Anchorage",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/New_York",
    "America/Phoenix",
    "America/Puerto_Rico",
    "America/St_Thomas",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Pago_Pago",
    "Pacific/Saipan",
];
/// The exact Canadian set: the old 54-code list plus 257 (British Columbia).
const CANADIAN_EXPECTED: [&str; 55] = [
    "204", "226", "236", "249", "250", "257", "263", "289", "306", "343", "354", "365", "367",
    "368", "382", "403", "416", "418", "428", "431", "437", "438", "450", "468", "474", "506",
    "514", "519", "548", "579", "581", "584", "587", "604", "613", "639", "647", "672", "683",
    "705", "709", "742", "753", "778", "780", "782", "807", "819", "825", "867", "873", "879",
    "902", "905", "942",
];

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
    let mut codes = canadian_codes();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes, CANADIAN_EXPECTED);
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
    for code in US_CODES {
        assert!(!roams(code), "{code}");
    }
    let table = AreaCodes::embedded().expect("table");
    for (code, _) in table.entries() {
        if !CANADIAN_EXPECTED.contains(&code) {
            assert!(!roams(code), "{code}");
        }
    }
}

/// TD-QH-06: every zone in the table is classified as Canadian or US, so a new or moved zone
/// fails here until someone decides which it is.
#[test]
fn td_qh_06_every_zone_is_classified() {
    let table = AreaCodes::embedded().expect("table");
    for (code, zone) in table.entries() {
        assert!(
            CANADIAN_ZONES.contains(&zone) != US_ZONES.contains(&zone),
            "{code}: zone {zone} is not classified exactly once"
        );
    }
}

/// TD-QH-06: the four new Ontario codes sit in America/Toronto alone.
#[test]
fn td_qh_06_new_ontario_codes_are_toronto() {
    let table = AreaCodes::embedded().expect("table");
    for code in NEW_ONTARIO {
        let zones = table.zones_for(&format!("+1{code}5550123")).expect("zone");
        assert_eq!(zones, ["America/Toronto"], "{code}");
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

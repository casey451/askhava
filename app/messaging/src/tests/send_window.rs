//! The homeowner send window `intake` asks for (TD-QH-01: QH-001, QH-003, QH-006; D-1301). Fake
//! numbers only: 555 exchanges under real area codes. The vault read and the suppression check of
//! `send_window` run in `tests/integration/tests/homeowner_job_texts.rs`.

use jiff::Timestamp;
use proptest::prelude::*;

use crate::api::{AreaCodes, Channel, Decision, Kind, Outgoing, SendWindow, decide, window};

const AZ: &str = "+14805550101";
const PACIFIC: &str = "+12135550102";
const EASTERN: &str = "+12125550103";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("test timestamp")
}

fn at(phone: &str, when: &str) -> SendWindow {
    let table = AreaCodes::embedded().expect("table");
    window(&table, phone, ts(when)).expect("window")
}

fn hold(until: &str) -> SendWindow {
    SendWindow::Hold { until: ts(until) }
}

/// TD-QH-01, QH-001: 07:59:59 no, 08:00:00 yes, 19:59:59 yes, 20:00:00 no in Arizona (D-101);
/// a held text waits for the next 8 a.m. (QH-006).
#[test]
fn td_qh_01_send_window_edges() {
    assert_eq!(at(AZ, "2026-07-15T14:59:59Z"), hold("2026-07-15T15:00:00Z"));
    assert_eq!(at(AZ, "2026-07-15T15:00:00Z"), SendWindow::Now);
    assert_eq!(at(AZ, "2026-07-16T02:59:59Z"), SendWindow::Now);
    assert_eq!(at(AZ, "2026-07-16T03:00:00Z"), hold("2026-07-16T15:00:00Z"));
}

/// TD-QH-01, QH-001, QH-006: a Pacific number at 08:30 Arizona in January (07:30 Pacific) waits
/// for 08:00 Pacific; in July it goes; an Eastern number at 18:00 Arizona in July (21:00 Eastern)
/// waits for the next instant both zones are open, 08:00 Arizona.
#[test]
fn td_qh_01_send_window_uses_the_area_code_zone_too() {
    assert_eq!(
        at(PACIFIC, "2026-01-15T15:30:00Z"),
        hold("2026-01-15T16:00:00Z")
    );
    assert_eq!(at(PACIFIC, "2026-07-15T15:30:00Z"), SendWindow::Now);
    assert_eq!(
        at(EASTERN, "2026-07-16T01:00:00Z"),
        hold("2026-07-16T15:00:00Z")
    );
}

/// TD-QH-01, QH-003: an area code not in the table, or a number that is not `+1`, is never
/// texted, at any hour (D-100, D-139).
#[test]
fn td_qh_01_send_window_refuses_an_unknown_area_code() {
    for phone in ["+19995550104", "4805550105"] {
        assert_eq!(at(phone, "2026-07-15T18:00:00Z"), SendWindow::NotTextable);
    }
}

proptest! {
    /// TD-QH-01 property, QH-001: the window is open exactly when `rules::decide` sends an update
    /// text (allowed in Arizona and in the zone), and a hold ends where `decide` says.
    #[test]
    fn td_qh_01_send_window_equals_decide(secs in 1_767_225_600_i64..1_798_761_600, pick in 0_usize..3) {
        let phone = [AZ, PACIFIC, EASTERN][pick];
        let table = AreaCodes::embedded().expect("table");
        let now = Timestamp::from_second(secs).expect("instant");
        let msg = Outgoing {
            phone,
            kind: Kind::Update,
            channel: Channel::Text,
            emergency_job: false,
            after_hours_business: false,
            emergency_emails: false,
        };
        let expected = match decide(&table, &msg, now).expect("decision") {
            Decision::Send => SendWindow::Now,
            Decision::Hold { until } => SendWindow::Hold { until },
        };
        prop_assert_eq!(window(&table, phone, now).expect("window"), expected);
    }
}

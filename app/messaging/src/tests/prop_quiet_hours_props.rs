//! Property tests for quiet hours (TD-QH-01 property, QH-001, QH-002, QH-006, QH-008, QH-009).

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;

use crate::api::{AreaCodes, Channel, Decision, Kind, Outgoing, decide, retry};

fn msg(phone: &str, kind: Kind) -> Outgoing<'_> {
    Outgoing {
        phone,
        kind,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    }
}

fn local_ok(at: Timestamp, zone: &str) -> bool {
    (8..20).contains(&at.in_tz(zone).expect("zone").hour())
}

proptest! {
    /// TD-QH-01: allowed equals allowed in Arizona and allowed in the recipient's zone, across a
    /// span covering both daylight-saving changes; a hold ends at the first allowed second.
    #[test]
    fn allowed_is_both_zones(secs in 1_767_225_600_i64..1_798_761_600, pick in 0usize..400) {
        let table = AreaCodes::embedded().expect("table");
        let entries: Vec<(&str, &str)> = table.entries().collect();
        let (code, zone) = entries[pick % entries.len()];
        let phone = format!("+1{code}5550100");
        let now = Timestamp::from_second(secs).expect("instant");
        let expected = local_ok(now, "America/Phoenix") && local_ok(now, zone);
        let got = decide(&table, &msg(&phone, Kind::Other), now).expect("decision");
        match got {
            Decision::Send => prop_assert!(expected),
            Decision::Hold { until } => {
                prop_assert!(!expected);
                prop_assert!(until > now);
                prop_assert!(until.duration_since(now) <= SignedDuration::from_hours(48));
                prop_assert!(local_ok(until, "America/Phoenix") && local_ok(until, zone));
                let before = until - SignedDuration::from_secs(1);
                prop_assert!(!(local_ok(before, "America/Phoenix") && local_ok(before, zone)));
            }
        }
        prop_assert_eq!(retry(&table, &msg(&phone, Kind::Other), now), Ok(got));
    }

    /// QH-004: the always-allowed kinds go at any instant.
    #[test]
    fn safety_always_sent(secs in 1_767_225_600_i64..1_798_761_600) {
        let table = AreaCodes::embedded().expect("table");
        let now = Timestamp::from_second(secs).expect("instant");
        for kind in [Kind::Safety911, Kind::KeywordReply, Kind::PickCode] {
            prop_assert_eq!(decide(&table, &msg("+12125550103", kind), now), Ok(Decision::Send));
        }
    }
}

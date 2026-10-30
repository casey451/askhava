//! Quiet-hours unit tests (TD-QH-01, TD-QH-02; QH-001 to QH-009, TXT-025). Fake numbers only: 555
//! exchanges under real area codes, never a person's number.

use jiff::Timestamp;

use crate::api::{
    AreaCodes, Channel, Decision, Held, Kind, Outgoing, QuietHoursError, Release, decide,
    plan_release, retry,
};

const AZ: &str = "+14805550101";
const PACIFIC: &str = "+12135550102";
const EASTERN: &str = "+12125550103";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("test timestamp")
}

fn text(phone: &str, kind: Kind) -> Outgoing<'_> {
    Outgoing {
        phone,
        kind,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    }
}

fn table() -> AreaCodes {
    AreaCodes::embedded().expect("table")
}

fn at(phone: &str, kind: Kind, when: &str) -> Decision {
    decide(&table(), &text(phone, kind), ts(when)).expect("decision")
}

/// TD-QH-01, QH-001: the edges 07:59:59 no, 08:00:00 yes, 19:59:59 yes, 20:00:00 no (D-101).
#[test]
fn td_qh_01_window_edges_in_arizona() {
    let held = |w| matches!(at(AZ, Kind::Other, w), Decision::Hold { .. });
    assert!(held("2026-07-15T07:59:59-07:00"));
    assert!(!held("2026-07-15T08:00:00-07:00"));
    assert!(!held("2026-07-15T19:59:59-07:00"));
    assert!(held("2026-07-15T20:00:00-07:00"));
}

/// TD-QH-01, QH-001, QH-002: Pacific at 08:30 Arizona is held in January and sent in July.
#[test]
fn td_qh_01_pacific_january_held_july_sent() {
    let jan = at(PACIFIC, Kind::Other, "2026-01-15T08:30:00-07:00");
    assert_eq!(
        jan,
        Decision::Hold {
            until: ts("2026-01-15T09:00:00-07:00")
        }
    );
    assert_eq!(
        at(PACIFIC, Kind::Other, "2026-07-15T08:30:00-07:00"),
        Decision::Send
    );
}

/// TD-QH-01: Eastern at 18:00 Arizona in July is 21:00 Eastern, held to 08:00 Arizona next day (Eastern opens earlier).
#[test]
fn td_qh_01_eastern_evening_held() {
    let d = at(EASTERN, Kind::Other, "2026-07-15T18:00:00-07:00");
    assert_eq!(
        d,
        Decision::Hold {
            until: ts("2026-07-16T08:00:00-07:00")
        }
    );
}

/// TD-QH-01, QH-009: both daylight-saving change days of a Pacific recipient; Arizona never shifts.
#[test]
fn td_qh_01_daylight_saving_change_days() {
    // Pacific springs forward 2026-03-08 at 10:00Z: the day before, 08:30 Arizona is 07:30 PST (held to
    // 09:00 Arizona); the day of, it is already PDT, so 08:30 Arizona is 08:30 Pacific (sent).
    let d = at(PACIFIC, Kind::Other, "2026-03-07T08:30:00-07:00");
    assert_eq!(
        d,
        Decision::Hold {
            until: ts("2026-03-07T09:00:00-07:00")
        }
    );
    assert_eq!(
        at(PACIFIC, Kind::Other, "2026-03-08T08:30:00-07:00"),
        Decision::Send
    );
    // 2026-11-01: Pacific falls back; from 08:30 Arizona the Pacific clock reads 07:30 again.
    assert_eq!(
        at(PACIFIC, Kind::Other, "2026-10-31T08:30:00-07:00"),
        Decision::Send
    );
    let d = at(PACIFIC, Kind::Other, "2026-11-01T08:30:00-07:00");
    assert_eq!(
        d,
        Decision::Hold {
            until: ts("2026-11-01T08:00:00-08:00")
        }
    );
    // Eastern, same days: the late evening edge moves with the Eastern shift.
    assert!(matches!(
        at(EASTERN, Kind::Other, "2026-03-08T17:30:00-07:00"),
        Decision::Hold { .. }
    ));
    assert_eq!(
        at(EASTERN, Kind::Other, "2026-03-08T16:30:00-07:00"),
        Decision::Send
    );
}

/// TD-QH-01, QH-003: a code not in the table, a non-+1 number and a malformed one are refused, even
/// for an exception kind.
#[test]
fn td_qh_01_unknown_area_code_never_texted() {
    let t = table();
    let now = ts("2026-07-15T12:00:00-07:00");
    for phone in [
        "+18765550100",
        "+18095550100",
        "+19995550100",
        "+442071234567",
        "+1480555",
    ] {
        for kind in [Kind::Other, Kind::Safety911] {
            assert!(decide(&t, &text(phone, kind), now).is_err());
        }
    }
    assert_eq!(
        t.zone_for("+18765550100"),
        Err(QuietHoursError::UnknownAreaCode)
    );
    assert_eq!(
        t.zone_for("+442071234567"),
        Err(QuietHoursError::NotNorthAmerican)
    );
}

/// TD-QH-01, QH-003, D-100: every code in the table has a real zone; Caribbean codes are absent.
#[test]
fn td_qh_01_every_table_code_has_a_zone() {
    let t = table();
    let mut n = 0;
    for (code, zone) in t.entries() {
        n += 1;
        assert_eq!(code.len(), 3);
        let phone = format!("+1{code}5550100");
        assert_eq!(t.zone_for(&phone).expect("zone"), zone);
        jiff::tz::TimeZone::get(zone).expect("known zone");
    }
    assert!(n > 350);
    for code in ["876", "809", "829", "849", "284", "242", "268"] {
        assert!(t.zone_for(&format!("+1{code}5550100")).is_err());
    }
    for code in ["787", "939", "340", "671", "670", "684", "709"] {
        assert!(t.zone_for(&format!("+1{code}5550100")).is_ok());
    }
    assert_eq!(t.zone_for("+17095550100"), Ok("America/St_Johns"));
    assert!(AreaCodes::parse(r#"{"America/Phoenix":["480","480"]}"#).is_err());
    assert!(AreaCodes::parse(r#"{"Nowhere/Land":["480"]}"#).is_err());
}

const NIGHT: &str = "2026-07-15T23:00:00-07:00";

/// TD-QH-02, QH-004: each always-allowed night exception goes at 23:00.
#[test]
fn td_qh_02_night_exceptions_sent() {
    for kind in [
        Kind::Safety911,
        Kind::KeywordReply,
        Kind::PickCode,
        Kind::AfterHoursNotice,
    ] {
        assert_eq!(at(AZ, kind, NIGHT), Decision::Send);
    }
    let emergency = |kind| Outgoing {
        emergency_job: true,
        ..text(AZ, kind)
    };
    for kind in [Kind::Quote, Kind::JobQuestion, Kind::PickStep] {
        assert_eq!(
            decide(&table(), &emergency(kind), ts(NIGHT)),
            Ok(Decision::Send)
        );
        assert!(
            matches!(at(AZ, kind, NIGHT), Decision::Hold { .. }),
            "non-emergency {kind:?}"
        );
    }
}

/// TD-QH-02, QH-004, D-101: a reply 14:59 after the person's text goes; 15:00 and 15:01 are held.
#[test]
fn td_qh_02_direct_reply_fifteen_minutes() {
    let inbound = ts("2026-07-15T22:45:01-07:00");
    let reply = Kind::DirectReply {
        inbound_at: inbound,
    };
    assert_eq!(at(AZ, reply, "2026-07-15T23:00:00-07:00"), Decision::Send);
    let held = Decision::Hold {
        until: ts("2026-07-16T08:00:00-07:00"),
    };
    assert_eq!(at(AZ, reply, "2026-07-15T23:00:01-07:00"), held);
    assert_eq!(at(AZ, reply, "2026-07-15T23:00:02-07:00"), held);
    let before = Kind::DirectReply {
        inbound_at: ts("2026-07-15T23:30:00-07:00"),
    };
    assert_eq!(at(AZ, before, NIGHT), held);
}

/// TD-QH-02, QH-004, QH-005, D-054, D-346: night cards and confirm requests are held except an
/// emergency card by email to an after-hours business and a person with the switch on.
#[test]
fn td_qh_02_cards_and_confirms_at_night() {
    let t = table();
    let card = |kind, emergency, after_hours, switch, channel| Outgoing {
        phone: AZ,
        kind,
        channel,
        emergency_job: emergency,
        after_hours_business: after_hours,
        emergency_emails: switch,
    };
    let eight = Decision::Hold {
        until: ts("2026-07-16T08:00:00-07:00"),
    };
    for kind in [Kind::JobCard, Kind::ConfirmRequest] {
        let go = card(kind, true, true, true, Channel::Email);
        assert_eq!(decide(&t, &go, ts(NIGHT)), Ok(Decision::Send));
        for (e, a, s, c) in [
            (false, true, true, Channel::Email),
            (true, false, true, Channel::Email),
            (true, true, false, Channel::Email),
            (true, true, true, Channel::Text),
        ] {
            assert_eq!(decide(&t, &card(kind, e, a, s, c), ts(NIGHT)), Ok(eight));
        }
    }
    let day = ts("2026-07-16T08:00:00-07:00");
    assert_eq!(
        decide(
            &t,
            &card(Kind::JobCard, false, false, false, Channel::Email),
            day
        ),
        Ok(Decision::Send)
    );
}

/// TD-QH-02, QH-006: a held text is sent at 08:00:00 and not a second before.
#[test]
fn td_qh_02_held_text_sent_at_window_start() {
    let Decision::Hold { until } = at(AZ, Kind::Update, NIGHT) else {
        panic!("held")
    };
    assert_eq!(until, ts("2026-07-16T08:00:00-07:00"));
    assert_eq!(
        at(AZ, Kind::Update, "2026-07-16T08:00:00-07:00"),
        Decision::Send
    );
    assert!(matches!(
        at(AZ, Kind::Update, "2026-07-16T07:59:59-07:00"),
        Decision::Hold { .. }
    ));
}

/// TD-QH-02, QH-008: a retry obeys the same rule as the first send.
#[test]
fn td_qh_02_retries_obey_quiet_hours() {
    let t = table();
    let msg = text(PACIFIC, Kind::Other);
    for when in [
        NIGHT,
        "2026-07-15T12:00:00-07:00",
        "2026-01-15T08:30:00-07:00",
    ] {
        assert_eq!(retry(&t, &msg, ts(when)), decide(&t, &msg, ts(when)));
    }
    assert!(matches!(
        retry(&t, &msg, ts(NIGHT)),
        Ok(Decision::Hold { .. })
    ));
    let stop = text(PACIFIC, Kind::KeywordReply);
    assert_eq!(retry(&t, &stop, ts(NIGHT)), Ok(Decision::Send));
}

fn held(id: u64, job: u64, who: u64, kind: Kind, close: bool) -> Held {
    Held {
        id,
        job_id: job,
        recipient_id: who,
        kind,
        announces_close: close,
    }
}

/// TD-QH-02, TXT-025, D-158: two updates held overnight for one job and recipient merge into one.
#[test]
fn td_qh_02_held_updates_merge() {
    let plan = plan_release(
        &[
            held(1, 10, 7, Kind::Update, false),
            held(2, 10, 8, Kind::Update, false),
            held(3, 10, 7, Kind::Update, false),
            held(4, 10, 7, Kind::Update, false),
            held(5, 11, 7, Kind::Update, false),
        ],
        |_| false,
    );
    assert_eq!(
        plan,
        vec![
            Release::Merged(vec![1, 3, 4]),
            Release::Send(2),
            Release::Send(5)
        ]
    );
}

/// TD-QH-02, QH-007, D-461: a held text for a closed job is dropped, except the closing text.
#[test]
fn td_qh_02_closed_job_drops_except_closing_text() {
    let plan = plan_release(
        &[
            held(1, 10, 7, Kind::Update, false),
            held(2, 10, 7, Kind::Other, true),
            held(3, 10, 7, Kind::Update, false),
            held(4, 11, 7, Kind::Other, false),
        ],
        |job| job == 10,
    );
    assert_eq!(
        plan,
        vec![
            Release::Drop(1),
            Release::Send(2),
            Release::Drop(3),
            Release::Send(4)
        ]
    );
}

//! The connection rules case by case (TD-CALL-04: CALL-007, CALL-008; the CALL-007 cases of
//! TD-FEE-13; D-141, D-1690): a person answering is talking whatever the length, a machine never
//! is, the homeowner's calls are not business attempts, and the 45-second mark is for reports.
//! Fake call SIDs and line IDs only.

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use sqlx::types::Uuid;

use crate::api::{
    AnsweredBy, Caller, FinishedCall, REPORT_MARK, answered_by, business_attempt, finished_call,
    reached_report_mark, talked,
};

const SID: &str = "CA00000000000000000000000000000078";

fn ended() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn callback(status: &str, seconds: &str, detected: Option<&str>) -> Vec<(String, String)> {
    let mut params = vec![
        ("CallSid".to_owned(), SID.to_owned()),
        ("CallStatus".to_owned(), status.to_owned()),
        ("CallDuration".to_owned(), seconds.to_owned()),
    ];
    if let Some(d) = detected {
        params.push(("AnsweredBy".to_owned(), d.to_owned()));
    }
    params
}

fn finished(caller: Caller, params: &[(String, String)]) -> Option<FinishedCall> {
    finished_call(Uuid::nil(), caller, params, ended())
}

/// TD-CALL-04 (CALL-007, CALL-008, D-141): a person answering a business call for 5 seconds is
/// talking, logged as a business attempt of 5 seconds, short of the 45-second mark.
#[test]
fn td_call_04_a_person_answering_for_5_seconds_is_talked() {
    let call = finished(Caller::Business, &callback("completed", "5", Some("human"))).unwrap();
    assert_eq!(call.answered, AnsweredBy::Person);
    assert_eq!(call.caller, Caller::Business);
    assert_eq!(call.length, SignedDuration::from_secs(5));
    assert_eq!(call.call_sid, SID);
    assert!(talked(call.answered));
    assert!(business_attempt(call.caller));
    assert!(!reached_report_mark(call.length));
}

/// TD-CALL-04, TD-FEE-13 (CALL-007, D-141): a machine answering for 90 seconds is never talking,
/// past the 45-second mark or not; every machine result counts as a machine.
#[test]
fn td_call_04_a_machine_for_90_seconds_is_not_talked() {
    for detected in [
        "machine_start",
        "machine_end_beep",
        "machine_end_other",
        "fax",
    ] {
        let call = finished(
            Caller::Business,
            &callback("completed", "90", Some(detected)),
        );
        let call = call.unwrap();
        assert_eq!(call.answered, AnsweredBy::Machine, "{detected}");
        assert!(!talked(call.answered));
        assert!(reached_report_mark(call.length));
    }
}

/// TD-CALL-04, TD-FEE-13 (CALL-007, D-141): a call the homeowner makes that the business answers
/// is talking, and never a business call attempt.
#[test]
fn td_call_04_a_homeowner_call_the_business_answers_is_talked_not_an_attempt() {
    let call = finished(Caller::Homeowner, &callback("completed", "20", None)).unwrap();
    assert_eq!(call.answered, AnsweredBy::Person);
    assert!(talked(call.answered));
    assert!(!business_attempt(call.caller));
}

/// TD-CALL-04 (CALL-007, D-141): the 45-second mark is exactly 45 seconds and only marks a call.
#[test]
fn td_call_04_the_45_second_mark_is_for_reports_only() {
    assert_eq!(REPORT_MARK, SignedDuration::from_secs(45));
    assert!(!reached_report_mark(SignedDuration::from_secs(44)));
    assert!(reached_report_mark(SignedDuration::from_secs(45)));
    for seconds in ["5", "44", "45", "600"] {
        let call = finished(
            Caller::Business,
            &callback("completed", seconds, Some("human")),
        );
        assert!(talked(call.unwrap().answered), "{seconds}");
    }
}

/// TD-CALL-04 (CALL-008, D-1690): an unanswered call is logged as nobody, 0 seconds when the
/// provider sends no length; `unknown` on a completed call is a person.
#[test]
fn td_call_04_unanswered_calls_log_nobody() {
    for status in ["busy", "no-answer", "failed", "canceled"] {
        let call = finished(Caller::Business, &callback(status, "0", None)).unwrap();
        assert_eq!(call.answered, AnsweredBy::Nobody, "{status}");
        assert!(!talked(call.answered));
    }
    let mut params = callback("no-answer", "0", None);
    params.retain(|(k, _)| k != "CallDuration");
    let call = finished(Caller::Business, &params).unwrap();
    assert_eq!(call.length, SignedDuration::ZERO);
    assert_eq!(
        answered_by("completed", Some("unknown")),
        AnsweredBy::Person
    );
}

/// TD-CALL-04 (CALL-008): a callback that is not a finished call, or names no valid SID or
/// length, logs nothing.
#[test]
fn td_call_04_a_callback_that_is_not_a_finished_call_is_refused() {
    assert!(finished(Caller::Business, &callback("in-progress", "5", None)).is_none());
    assert!(finished(Caller::Business, &callback("ringing", "0", None)).is_none());
    assert!(finished(Caller::Business, &callback("completed", "-5", None)).is_none());
    assert!(finished(Caller::Business, &callback("completed", "5s", None)).is_none());
    for sid in [
        "",
        "CA123",
        "SM00000000000000000000000000000078",
        "CA0000000000000000000000000000007/",
    ] {
        let mut params = callback("completed", "5", None);
        params[0].1 = sid.to_owned();
        assert!(finished(Caller::Business, &params).is_none(), "{sid}");
    }
    assert!(finished(Caller::Business, &callback("completed", "5", None)[1..]).is_none());
    let mut no_status = callback("completed", "5", None);
    no_status.remove(1);
    assert!(finished(Caller::Business, &no_status).is_none());
}

proptest! {
    /// TD-CALL-04 (CALL-007, D-141): whatever the length and direction, a person answering is
    /// talking and a machine or nobody never is; only the business's calls are attempts.
    #[test]
    fn td_call_04_talking_depends_only_on_who_answered(
        seconds in 0u32..100_000,
        homeowner in any::<bool>(),
        detected in prop::sample::select(vec!["human", "unknown", "machine_start", "fax", "none"]),
    ) {
        let caller = if homeowner { Caller::Homeowner } else { Caller::Business };
        let detected = (detected != "none").then_some(detected);
        let params = callback("completed", &seconds.to_string(), detected);
        let call = finished(caller, &params).unwrap();
        let machine = detected.is_some_and(|d| d.starts_with("machine") || d == "fax");
        prop_assert_eq!(talked(call.answered), !machine);
        prop_assert_eq!(business_attempt(call.caller), !homeowner);
        prop_assert_eq!(reached_report_mark(call.length), seconds >= 45);
    }
}

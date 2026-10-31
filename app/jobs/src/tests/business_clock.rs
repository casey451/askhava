//! The business clock and the emergency clock of every waiting status, and the pure rules of
//! JOB-015 and JOB-025 (TD-JOB-03: JOB-007, JOB-010 to JOB-015, JOB-025, JOB-026, TXT-042).
//!
//! Times are written in UTC; Arizona is UTC-7 all year (JOB-007, D-004). The database cases,
//! the texts and the timers firing are in `tests/integration/tests/job_timers.rs`.

use jiff::Timestamp;

use crate::rules::{
    CheckIn, Contact, Reply, Windows, WindowsEnd, after_windows, connected_move, timeout,
};
use crate::status::JobStatus;

/// When `status` entered at `entered` times out, and the code of the status it then moves to.
fn due(status: JobStatus, entered: &str, emergency: bool) -> (String, &'static str) {
    let at: Timestamp = entered.parse().expect("instant");
    let t = timeout(status, at, emergency).expect("a waiting status");
    (t.at.to_string(), t.then.code())
}

/// TD-JOB-03: a pick at 7:30 p.m. Arizona time has its 4 business hours end at 11:30 a.m. the
/// next day, counting only 8 a.m. to 8 p.m. (JOB-007, JOB-013, PICK-019, D-482).
#[test]
fn td_job_03_a_pick_at_7_30_pm_confirms_by_11_30_am() {
    let pick = "2026-11-03T02:30:00Z"; // 7:30 p.m. Nov 2, Arizona
    let expected = ("2026-11-03T18:30:00Z".to_owned(), "quotes_ready");
    let confirm = due(JobStatus::AwaitingConfirm, pick, false);
    assert_eq!(confirm, expected);
}

/// TD-JOB-03: on an emergency job the confirm and payment waits are 60 clock minutes at any hour
/// (JOB-013, JOB-014, PICK-044).
#[test]
fn td_job_03_emergency_waits_run_on_the_clock_at_night() {
    let night = "2026-11-03T06:00:00Z"; // 11 p.m. Nov 2, Arizona
    let expected = ("2026-11-03T07:00:00Z".to_owned(), "quotes_ready");
    let confirm = due(JobStatus::AwaitingConfirm, night, true);
    let payment = due(JobStatus::PaymentPending, night, true);
    assert_eq!((confirm, payment), (expected.clone(), expected));
}

/// TD-JOB-03: Payment pending returns after 1 business hour; entered at 7:30 p.m. it ends at
/// 8:30 a.m. (JOB-014, FEE-020).
#[test]
fn td_job_03_payment_pending_waits_one_business_hour() {
    let late = "2026-11-03T02:30:00Z";
    let expected = ("2026-11-03T15:30:00Z".to_owned(), "quotes_ready");
    let payment = due(JobStatus::PaymentPending, late, false);
    assert_eq!(payment, expected);
}

/// TD-JOB-03: an Open job becomes No quotes yet at 6 p.m. Arizona time the next day, whether the
/// cards went out in the morning or just before midnight (JOB-010, D-030).
#[test]
fn td_job_03_open_becomes_no_quotes_yet_at_6_pm_the_next_day() {
    let expected = ("2026-11-04T01:00:00Z".to_owned(), "no_quotes_yet");
    for entered in ["2026-11-02T15:00:00Z", "2026-11-03T06:59:00Z"] {
        let open = due(JobStatus::Open, entered, false);
        assert_eq!(open, expected, "{entered}");
    }
}

/// TD-JOB-03: No quotes yet closes after 5 days, Quotes ready after 3 days, and Connected 7 days
/// after the day-14 check-in (JOB-011, JOB-012, JOB-026, D-046).
#[test]
fn td_job_03_day_counted_closes() {
    let entered = "2026-11-02T17:00:00Z";
    let cases = [
        (JobStatus::NoQuotesYet, "2026-11-07T17:00:00Z"),
        (JobStatus::QuotesReady, "2026-11-05T17:00:00Z"),
        (JobStatus::Connected, "2026-11-23T17:00:00Z"),
    ];
    for (status, at) in cases {
        assert_eq!(due(status, entered, false), (at.to_owned(), "closed"));
    }
}

/// TD-JOB-03: only Open, No quotes yet and Quotes ready send a text when they time out, and the
/// Connected close sends none, so no review request (JOB-010 to JOB-012, JOB-026, D-046, D-461).
#[test]
fn td_job_03_which_timeouts_send_a_text() {
    let texts: Vec<_> = JobStatus::ALL
        .into_iter()
        .filter_map(|s| s.timeout_text().map(|t| (s.code(), t)))
        .collect();
    let expected = [
        ("open", "no_quotes_yet"),
        ("no_quotes_yet", "closed_no_quotes"),
        ("quotes_ready", "closed_no_pick"),
    ];
    assert_eq!(texts, expected);
}

/// TD-JOB-03: DONE moves a Connected job to Done, PROBLEM and NO close it, and a day-3 NO under
/// email contact returns it to Quotes ready (JOB-015, D-143).
#[test]
fn td_job_03_connected_replies() {
    let cases = [
        (Reply::Done, CheckIn::Day3, Contact::Call, "done"),
        (Reply::Done, CheckIn::Day14, Contact::Email, "done"),
        (Reply::Problem, CheckIn::Day3, Contact::Email, "closed"),
        (Reply::Problem, CheckIn::Day14, Contact::Call, "closed"),
        (Reply::No, CheckIn::Day3, Contact::Call, "closed"),
        (Reply::No, CheckIn::Day14, Contact::Email, "closed"),
        (Reply::No, CheckIn::Day3, Contact::Email, "quotes_ready"),
    ];
    for (reply, day, contact, to) in cases {
        let moved = connected_move(reply, day, contact).code();
        assert_eq!(moved, to, "{reply:?} {day:?} {contact:?}");
    }
}

/// TD-JOB-03: once every window has ended the job closes only with no choosable quote left;
/// otherwise she gets the all-quotes-in text (JOB-025, TXT-042, D-140).
#[test]
fn td_job_03_every_window_ended() {
    let none_left = Windows {
        quoted: 2,
        sent: 5,
        choosable: 0,
    };
    let one_left = Windows {
        choosable: 1,
        ..none_left
    };
    assert_eq!(after_windows(none_left), WindowsEnd::Close);
    assert_eq!(after_windows(one_left), WindowsEnd::AllQuotesIn);
}

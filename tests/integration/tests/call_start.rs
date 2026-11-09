//! T282: TD-CALL-15 (CALL-007, CALL-008, FEE-022, TXT-026; D-1836, D-2129): each call's start,
//! not its end, reaches the unreachable refund rule. The pure cases build `LoggedCall`s with and
//! without a start and judge them through `picks`' `call_record` and `payments`' FEE-022 rule; the
//! database cases join the dialed leg's `initiated` callback to the start the voice webhook stored,
//! create it when that write was lost, never move it, and read it back through `call_log`. Fake
//! pool number, fake SIDs and random IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{
    AnsweredBy, Caller, Connected, FinishedCall, LoggedCall, assign_number, call_log, call_start,
    call_start_confirmed, log_finished_call,
};
use askhava_core::api::Settings;
use askhava_payments::api::{
    Contact, EmailCheck, RefundDecision, RefundReason, Unmet, UnreachableFacts, decide_unreachable,
};
use askhava_picks::api::call_record;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Arizona wall time on November `day`, 2026, to the second.
fn az(day: i8, hour: i8, minute: i8, second: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, second, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

/// An unanswered business call that started at `start` (`None`: never recorded) and ended at `end`.
fn logged(start: Option<Timestamp>, end: Timestamp) -> LoggedCall {
    LoggedCall {
        caller: Caller::Business,
        answered: AnsweredBy::Nobody,
        length: SignedDuration::ZERO,
        reached_45s: false,
        started_at: start,
        ended_at: end,
    }
}

/// FEE-022 at 96 hours on a call-contact, non-emergency pick released Nov 2 at 9 a.m.
fn decide(calls: &[LoggedCall]) -> RefundDecision {
    let records: Vec<_> = calls.iter().map(call_record).collect();
    let facts = UnreachableFacts {
        released_at: az(2, 9, 0, 0),
        emergency: false,
        calls: &records,
        notice_attempted: true,
        done_or_problem: false,
        contact: Contact::Call,
        email_check: EmailCheck::NoReply,
        charged: true,
        refunded_before: false,
    };
    decide_unreachable(&facts, az(9, 9, 0, 0))
}

const PAID: RefundDecision = RefundDecision::Refund(RefundReason::Unreachable);

/// A first attempt on Nov 3 at 2 p.m., well inside every limit.
fn first() -> LoggedCall {
    logged(Some(az(3, 14, 0, 0)), az(3, 14, 1, 0))
}

/// TD-CALL-15 (FEE-022, TXT-026, D-1836): the 8 a.m. to 8 p.m. hours are judged on the start.
#[test]
fn td_call_15_hours_are_judged_on_the_start() {
    let late = logged(Some(az(3, 19, 59, 0)), az(3, 20, 1, 0));
    assert_eq!(call_record(&late).at, az(3, 19, 59, 0));
    assert_eq!(decide(&[first(), late]), PAID);
    let early = logged(Some(az(3, 7, 59, 0)), az(3, 8, 1, 0));
    assert_eq!(
        decide(&[first(), early]),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
    );
}

/// TD-CALL-15 (FEE-022): the 3-hour spacing is between starts, never ends.
#[test]
fn td_call_15_spacing_is_judged_on_the_starts() {
    let a = logged(Some(az(3, 10, 0, 0)), az(3, 10, 30, 0));
    let b = logged(Some(az(3, 13, 0, 0)), az(3, 13, 10, 0));
    assert_eq!(decide(&[a, b]), PAID, "starts 3:00 apart, ends 2:40");
    let a = logged(Some(az(3, 10, 0, 0)), az(3, 10, 1, 0));
    let b = logged(Some(az(3, 12, 59, 59)), az(3, 13, 5, 0));
    assert_eq!(
        decide(&[a, b]),
        RefundDecision::NoRefund(Unmet::AttemptsTooClose),
        "ends 3:04 apart, starts 2:59:59"
    );
}

/// TD-CALL-15 (FEE-022): a start at exactly release + 72 hours counts; one second later does not,
/// whatever its end.
#[test]
fn td_call_15_the_72_hours_are_judged_on_the_start() {
    let edge = logged(Some(az(5, 9, 0, 0)), az(5, 9, 2, 0));
    assert_eq!(decide(&[first(), edge]), PAID);
    let past = logged(Some(az(5, 9, 0, 1)), az(5, 9, 2, 0));
    assert_eq!(
        decide(&[first(), past]),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
    );
}

/// TD-CALL-15 (FEE-022, D-1836): a call whose start was never recorded is judged by its end for
/// every check and still counts as a qualifying attempt.
#[test]
fn td_call_15_a_call_with_no_start_is_judged_by_its_end() {
    let a = logged(None, az(3, 10, 0, 0));
    assert_eq!(call_record(&a).at, az(3, 10, 0, 0));
    assert_eq!(decide(&[a, logged(None, az(3, 13, 0, 0))]), PAID);
    let too_close = logged(None, az(3, 12, 59, 59));
    assert_eq!(
        decide(&[a, too_close]),
        RefundDecision::NoRefund(Unmet::AttemptsTooClose)
    );
    for outside in [az(3, 20, 1, 0), az(5, 9, 0, 1)] {
        assert_eq!(
            decide(&[first(), logged(None, outside)]),
            RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
        );
    }
}

/// The fake SID `CA` + 32 characters ending in `n`.
fn sid(n: u32) -> String {
    format!("CA{n:032}")
}

/// A one-number fake pool, built from `ci` settings.
fn pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("config");
    config.pool().clone()
}

/// A fresh line on the fake pool.
async fn line(db: &mut PgConnection) -> Uuid {
    let pool = pool();
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: Uuid::new_v4(),
        business: Uuid::new_v4(),
    };
    assign_number(db, &pool, job, az(2, 9, 0, 0))
        .await
        .expect("line")
        .id()
}

/// Logs the finished dialed leg `dialed` on `line`, ended at `end`.
async fn finish(db: &mut PgConnection, line: Uuid, dialed: &str, end: Timestamp) {
    let call = FinishedCall {
        line,
        call_sid: dialed.to_owned(),
        caller: Caller::Business,
        answered: AnsweredBy::Nobody,
        length_seconds: 0,
        ended_at: end,
    };
    assert!(log_finished_call(db, &call).await.expect("log"));
}

/// TD-CALL-15 (CALL-008, D-1836): the dialed leg's `initiated` callback joins the start the voice
/// webhook stored by its parent SID; a repeat or late callback never moves it; `call_log` returns
/// it as `started_at`. Run as the app role, so its column grant is what allows the join.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_15_the_initiated_callback_joins_the_webhook_start(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let line = line(&mut db).await;
    let stored = az(3, 19, 59, 0);
    sqlx::query(
        "INSERT INTO calls.call_start (parent_sid, line_id, started_at) VALUES ($1, $2, $3)",
    )
    .bind(sid(1))
    .bind(line)
    .bind(stored.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("webhook start");
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let later = az(3, 19, 59, 4);
    let got = call_start_confirmed(&mut db, line, &sid(1), &sid(2), later).await;
    assert_eq!(got.expect("confirm"), stored);
    let again = call_start_confirmed(&mut db, line, &sid(1), &sid(3), az(3, 21, 0, 0)).await;
    assert_eq!(
        again.expect("repeat"),
        stored,
        "a late callback never moves it"
    );
    assert_eq!(
        call_start(&mut db, &sid(1)).await.expect("read"),
        Some((line, stored))
    );
    finish(&mut db, line, &sid(2), az(3, 20, 1, 0)).await;
    let log = call_log(&mut db, line).await.expect("log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].started_at, Some(stored));
    assert_eq!(log[0].ended_at, az(3, 20, 1, 0));
    assert_eq!(call_record(&log[0]).at, stored);
}

/// TD-CALL-15 (CALL-008, D-1836): when the webhook's write was lost the callback creates the start
/// at its own time, and a later one never moves it; a logged call with no start reads `None` and
/// is judged by its end.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_15_a_lost_webhook_start_is_created_by_the_callback(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let line = line(&mut db).await;
    let at = az(3, 10, 0, 0);
    let made = call_start_confirmed(&mut db, line, &sid(11), &sid(12), at).await;
    assert_eq!(made.expect("create"), at);
    let late = call_start_confirmed(&mut db, line, &sid(11), &sid(12), az(3, 11, 0, 0)).await;
    assert_eq!(late.expect("repeat"), at);
    let bad = call_start_confirmed(&mut db, line, &sid(13), "not-a-sid", at).await;
    assert!(bad.is_err(), "a malformed SID is refused");
    finish(&mut db, line, &sid(12), az(3, 10, 5, 0)).await;
    finish(&mut db, line, &sid(14), az(3, 13, 30, 0)).await;
    let log = call_log(&mut db, line).await.expect("log");
    let starts: Vec<_> = log.iter().map(|c| c.started_at).collect();
    assert_eq!(starts, [Some(at), None]);
    assert_eq!(call_record(&log[1]).at, az(3, 13, 30, 0));
    assert_eq!(decide(&log), PAID);
}

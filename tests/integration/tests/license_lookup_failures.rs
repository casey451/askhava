//! The live lookup at the pick and what happens when a lookup or a list fails (LIC-007, LIC-008,
//! LIC-009, LIC-039).
//!
//! Proves, as the app role and with a fake license source, that a pick makes exactly one lookup;
//! that a site down, a CAPTCHA, a changed page or a row for another number is "unknown", records
//! nothing, blocks Confirm and leaves the verified status as it was; the 24- and 72-hour clocks to
//! the second and as settings; that an end or bond-cancel date pauses from 00:00 Arizona time; and
//! that a failing list raises no alert at 11:59, one listed alert at 12:00 and the push at 24:00,
//! while each active business gets one lookup that updates its verified status. All fake.

use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, LicenseSource};
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    FailureLimits, Freshness, LookupOutcome, UnknownStanding, alert_list_failure, end_date_pauses,
    lookup_at_pick, lookup_while_list_fails, roc_check_allows_card, unknown_standing,
};
use askhava_ops::api::Severity;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0x7e57_0180_0001);
const ACTIVE: &str = "license_number,class,status,issued_on\nROC900180,R-37,Active,2015-06-30\n";
const SUSPENDED: &str = "license_number,class,status,issued_on\nROC900180,R-37,Suspended,\n";

/// A fake license source answering every lookup with `answer` and counting the calls.
struct Fake {
    answer: Option<&'static str>,
    calls: AtomicUsize,
}

impl Fake {
    fn new(answer: Option<&'static str>) -> Self {
        Self {
            answer,
            calls: AtomicUsize::new(0),
        }
    }
}

impl LicenseSource for Fake {
    async fn fetch_daily_list(&self, _list: &str) -> Result<Vec<u8>, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn lookup_license(&self, _number: &str) -> Result<Vec<u8>, AdapterError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer
            .map(|a| a.as_bytes().to_vec())
            .ok_or(AdapterError::NotSent)
    }
}

fn at(s: &str) -> Timestamp {
    s.parse().expect("timestamp")
}

fn any_age() -> Freshness {
    Freshness {
        checked_since: Timestamp::UNIX_EPOCH,
        listed_since: date(2000, 1, 1),
    }
}

async fn allows_card(conn: &mut sqlx::PgConnection) -> bool {
    roc_check_allows_card(conn, BUSINESS, "roc 900180", any_age())
        .await
        .expect("card check")
}

/// TD-LIC-04: exactly one throttled live lookup at the pick, recorded as a check.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pick_makes_exactly_one_lookup(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let fake = Fake::new(Some(ACTIVE));
    let now = at("2026-10-31T17:00:00Z");
    let got = lookup_at_pick(&mut conn, &fake, (BUSINESS, "roc 900180"), now).await;
    assert_eq!(got, Ok(LookupOutcome::Active));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1, "one lookup, no retry");
    assert!(got.expect("outcome").allows_confirm());
    assert!(allows_card(&mut conn).await, "recorded as an active check");
}

/// TD-LIC-05: a failed or misread lookup is unknown, can't Confirm and never changes a verified status.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_lookup_is_unknown_and_keeps_the_verified_status(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let now = at("2026-10-31T17:00:00Z");
    let verified = Fake::new(Some(ACTIVE));
    lookup_at_pick(&mut conn, &verified, (BUSINESS, "ROC900180"), now)
        .await
        .expect("verified");
    let other = "license_number,class,status,issued_on\nROC900999,R-37,Suspended,\n";
    let captcha = "<html><body>Please prove you are not a robot</body></html>";
    let changed = "number;class;state\nROC900180;R-37;Suspended\n";
    for answer in [None, Some(captcha), Some(changed), Some(other), Some("")] {
        let fake = Fake::new(answer);
        let got = lookup_at_pick(&mut conn, &fake, (BUSINESS, "ROC900180"), now).await;
        assert_eq!(got, Ok(LookupOutcome::Unknown));
        assert!(
            !LookupOutcome::Unknown.allows_confirm(),
            "no Confirm while unknown"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        assert!(
            allows_card(&mut conn).await,
            "a misread never changes a verified status"
        );
    }
    let checks: i64 = sqlx::query_scalar("SELECT count(*) FROM licensing.license_check")
        .fetch_one(&mut *conn)
        .await
        .expect("count");
    assert_eq!(checks, 1, "an unknown lookup records nothing");
}

/// TD-LIC-05: warned at 24:00:00, kept at 71:59:59, paused at 72:00:00; both numbers are settings.
#[test]
fn unknown_is_kept_then_warned_then_paused() {
    let since = at("2026-10-01T00:00:00Z");
    let after =
        |h: i64, s: i64| since + SignedDuration::from_hours(h) + SignedDuration::from_secs(s);
    let limits = FailureLimits::default();
    assert_eq!(
        unknown_standing(since, after(23, 3599), limits),
        UnknownStanding::Kept
    );
    assert_eq!(
        unknown_standing(since, after(24, 0), limits),
        UnknownStanding::Warned
    );
    assert_eq!(
        unknown_standing(since, after(71, 3599), limits),
        UnknownStanding::Warned
    );
    assert_eq!(
        unknown_standing(since, after(72, 0), limits),
        UnknownStanding::Paused
    );
    let shorter = FailureLimits {
        warn_after: SignedDuration::from_hours(1),
        pause_after: SignedDuration::from_hours(2),
        ..limits
    };
    assert_eq!(
        unknown_standing(since, after(1, 0), shorter),
        UnknownStanding::Warned
    );
    assert_eq!(
        unknown_standing(since, after(2, 0), shorter),
        UnknownStanding::Paused
    );
}

/// TD-LIC-05: an end or bond-cancel date pauses at 00:00 Arizona time that day, not 23:59 before.
#[test]
fn end_and_bond_cancel_dates_pause_from_arizona_midnight() {
    let day = Some(date(2026, 11, 5));
    let eve = at("2026-11-05T06:59:59Z");
    let midnight = at("2026-11-05T07:00:00Z");
    assert!(!end_date_pauses(day, None, eve), "23:59:59 the day before");
    assert!(end_date_pauses(day, None, midnight), "license end at 00:00");
    assert!(!end_date_pauses(None, day, eve));
    assert!(
        end_date_pauses(None, day, midnight),
        "bond cancellation at 00:00"
    );
    assert!(!end_date_pauses(None, None, midnight));
}

/// TD-LIC-15: no alert at 11:59, one listed alert at 12:00, the push at 24:00; while failing each
/// active business gets one lookup and a verified status updates.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failing_list_alerts_and_lookups_keep_statuses_current(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let start = at("2026-10-31T07:00:00Z");
    let (limits, minute) = (FailureLimits::default(), SignedDuration::from_mins(1));
    let h = |n: i64| start + SignedDuration::from_hours(n);
    let mut raised = Vec::new();
    for (last, now) in [
        (h(11), h(12) - minute),
        (h(12) - minute, h(12)),
        (h(12), h(13)),
    ] {
        raised.push(
            alert_list_failure(&mut conn, (start, last, now), limits)
                .await
                .expect("alert"),
        );
    }
    assert_eq!(
        raised,
        [None, Some(Severity::Normal), None],
        "one listed, non-push alert"
    );
    let push = alert_list_failure(&mut conn, (start, h(23), h(24)), limits).await;
    assert_eq!(
        push,
        Ok(Some(Severity::High)),
        "still failing at 24 hours: the push"
    );
    lookup_at_pick(
        &mut conn,
        &Fake::new(Some(ACTIVE)),
        (BUSINESS, "ROC900180"),
        start,
    )
    .await
    .expect("verified");
    let fake = Fake::new(Some(SUSPENDED));
    let other = Uuid::from_u128(0x7e57_0180_0002);
    let businesses = [(BUSINESS, "ROC900180"), (other, "ROC900180")];
    let got = lookup_while_list_fails(&mut conn, &fake, &businesses, h(12)).await;
    assert_eq!(got, Ok(vec![LookupOutcome::Inactive; 2]));
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        2,
        "one lookup per active business"
    );
    assert!(!allows_card(&mut conn).await, "the verified status updated");
    let alerts: Vec<String> = sqlx::query_scalar(
        "SELECT severity FROM ops.alert WHERE kind = 'licensing.list_failing' ORDER BY severity",
    )
    .fetch_all(&pool)
    .await
    .expect("alerts");
    assert_eq!(alerts, ["high", "normal"], "one of each, no repeat");
}

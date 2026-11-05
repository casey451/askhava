//! The Confirm window, the pick notice and the license checks in the database (TD-PICK-09,
//! TD-PICK-15, TD-JOB-03; PICK-019, PICK-020, PICK-035, PICK-037). Fake IDs, numbers and license
//! numbers only.
//!
//! A job waits in Awaiting confirm with one pick; the pick notice goes to the owner and the
//! "Confirm and billing" member only; the window starts from the notice's real send and ends 4
//! business hours later (a setting), when the pick expires and the job returns to Quotes ready; a
//! stale timer does nothing; the license is looked up once live at the pick and re-checked from
//! the stored results at Confirm.

use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, LicenseSource};
use askhava_businesses::api::{AccountStatus, Acting, Roles, add_member, add_owner, create};
use askhava_core::api::{Claimed, Guarded};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, CONFIRM_ENDS_KIND, ConfirmSettings, LicenseAtPick, PICK_MADE_KIND,
    PicksError, Refusal, StoredFacts, check_at_confirm, check_at_pick, notice_sent,
    run_confirm_ends, run_pick_notice,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

const NUMBER: &str = "ROC 900163";
const ACTIVE: &str = "license_number,class,status,issued_on\n{number},R-37,Active,2015-06-30\n";

/// Arizona wall-clock time on November `day`, 2026 (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    }
}

/// A job picked at `at` and waiting in Awaiting confirm with one page pick of `business`.
async fn picked(pool: &PgPool, business: Uuid, at: Timestamp) -> (Uuid, Uuid) {
    picked_job(pool, (business, false), at).await
}

/// [`picked`], on a job chosen as an emergency when `emergency` (PICK-044).
async fn picked_job(
    pool: &PgPool,
    (business, emergency): (Uuid, bool),
    at: Timestamp,
) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, emergency, Actor::System)
        .await
        .expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
    ] {
        let moved = move_to(&mut db, job, step, at, Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    (job, pick)
}

async fn tasks(pool: &PgPool, kind: &str) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, jiff_sqlx::Timestamp)> =
        sqlx::query_as("SELECT payload, run_at FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(kind)
            .fetch_all(pool)
            .await
            .expect("tasks");
    rows.into_iter().map(|(p, t)| (p, t.to_jiff())).collect()
}

async fn pick_status(pool: &PgPool, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("status")
}

async fn job_status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("conn");
    current(&mut db, job).await.expect("job").status.code()
}

/// TD-PICK-09, TD-JOB-03 (PICK-019, D-345, D-381, D-482): the window counts from the notice's real
/// send, held at 7:41 p.m. by the email limit, and ends at 11:41 the next day; the window's end
/// expires the pick and returns the job to Quotes ready; run again, it does nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_no_confirm_in_4_business_hours_returns_to_quotes_ready(pool: PgPool) {
    let (job, pick) = picked(&pool, Uuid::from_u128(0xb063), az(9, 17, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let due = notice_sent(&mut db, pick, az(9, 19, 41), ConfirmSettings::default()).await;
    assert_eq!(due, Ok(Some(az(10, 11, 41))));
    let ends = tasks(&pool, CONFIRM_ENDS_KIND).await;
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].1, az(10, 11, 41));
    let timer_from = current(&mut db, job).await.expect("job").timer_from;
    assert!(
        timer_from > az(9, 19, 41),
        "the jobs backstop ends after the window"
    );
    run_confirm_ends(pool.clone(), claimed(ends[0].0.clone()))
        .await
        .expect("ends");
    assert_eq!(job_status(&pool, job).await, "quotes_ready");
    assert_eq!(pick_status(&pool, pick).await, "expired");
    run_confirm_ends(pool.clone(), claimed(ends[0].0.clone()))
        .await
        .expect("again");
    assert_eq!(job_status(&pool, job).await, "quotes_ready");
    let late = notice_sent(&mut db, pick, az(10, 12, 0), ConfirmSettings::default()).await;
    assert!(late.is_err(), "an expired pick starts no window");
}

/// TD-PICK-09 (PICK-019, D-192, D-491): 3 business hours by the setting end at 10:41; a stale
/// timer from an earlier start does nothing once the window restarted later.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_the_window_is_a_setting_and_stale_timers_do_nothing(pool: PgPool) {
    let (job, pick) = picked(&pool, Uuid::from_u128(0xb064), az(9, 17, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let three = ConfirmSettings {
        window_minutes: 180,
    };
    let first = notice_sent(&mut db, pick, az(9, 17, 0), three).await;
    assert_eq!(first, Ok(Some(az(9, 20, 0))));
    let later = notice_sent(&mut db, pick, az(9, 19, 41), three).await;
    assert_eq!(later, Ok(Some(az(10, 10, 41))));
    let ends = tasks(&pool, CONFIRM_ENDS_KIND).await;
    assert_eq!(ends.len(), 2);
    run_confirm_ends(pool.clone(), claimed(ends[0].0.clone()))
        .await
        .expect("stale");
    assert_eq!(job_status(&pool, job).await, "awaiting_confirm");
    assert_eq!(pick_status(&pool, pick).await, "awaiting_confirm");
}

/// PICK-019, D-807, D-1503: the sender telling the same send twice restarts `jobs`' backstop once
/// and queues one window end, with the same deadline both times.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_a_repeated_send_restarts_nothing(pool: PgPool) {
    let (job, pick) = picked(&pool, Uuid::from_u128(0xb067), az(9, 9, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let first = notice_sent(&mut db, pick, az(9, 10, 0), ConfirmSettings::default()).await;
    let before = current(&mut db, job).await.expect("job");
    let again = notice_sent(&mut db, pick, az(9, 10, 0), ConfirmSettings::default()).await;
    let after = current(&mut db, job).await.expect("job");
    assert_eq!(first, Ok(Some(az(9, 14, 0))));
    assert_eq!(again, first);
    assert_eq!(
        after.timer_version, before.timer_version,
        "no second restart"
    );
    assert_eq!(after.timer_from, before.timer_from);
    assert_eq!(tasks(&pool, CONFIRM_ENDS_KIND).await.len(), 1);
}

/// PICK-019, PICK-035, PICK-044, D-1503: a notice held until after `jobs` returned the job to
/// Quotes ready (its backstop, or an emergency's 60 minutes) starts no window, so no "Accept by"
/// goes, and the dead pick is stored as expired; nothing is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_a_notice_held_past_the_backstop_expires_the_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    for (business, emergency) in [(0xb068, false), (0xb069, true)] {
        let (job, pick) =
            picked_job(&pool, (Uuid::from_u128(business), emergency), az(9, 9, 0)).await;
        let back = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
        let moved = move_to(&mut db, job, back, az(9, 13, 0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "the timer fired");
        let late = notice_sent(&mut db, pick, az(9, 14, 0), ConfirmSettings::default()).await;
        assert_eq!(late, Err(PicksError::MovedOn), "emergency {emergency}");
        assert_eq!(pick_status(&pool, pick).await, "expired");
        assert_eq!(job_status(&pool, job).await, "quotes_ready");
    }
    assert!(tasks(&pool, CONFIRM_ENDS_KIND).await.is_empty());
}

/// PICK-044: an emergency job still waiting keeps `jobs`' 60 clock minutes, so the send starts no
/// business-hour window and the pick keeps waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_an_emergency_keeps_its_60_minutes(pool: PgPool) {
    let (job, pick) = picked_job(&pool, (Uuid::from_u128(0xb06a), true), az(9, 9, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = notice_sent(&mut db, pick, az(9, 9, 5), ConfirmSettings::default()).await;
    assert_eq!(sent, Ok(None));
    assert_eq!(pick_status(&pool, pick).await, "awaiting_confirm");
    assert_eq!(job_status(&pool, job).await, "awaiting_confirm");
}

/// TD-PICK-15 (PICK-037, D-191, D-492): the pick notice goes once to the owner and the "Confirm
/// and billing" member, never to a "Cards and quotes" member, with the job number and the
/// window's length; the emergency subject only on an emergency job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_15_the_notice_goes_to_the_owner_and_billing_members(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = create(&mut db, false, az(1, 9, 0)).await.expect("business");
    let (owner_box, billing_box, cards_box) = (
        Uuid::from_u128(0xd1),
        Uuid::from_u128(0xd2),
        Uuid::from_u128(0xd3),
    );
    let owner = add_owner(&mut db, business, owner_box, az(1, 9, 0))
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let billing = Roles::new(false, true).expect("roles");
    let cards = Roles::new(true, false).expect("roles");
    for (mailbox, roles) in [(billing_box, billing), (cards_box, cards)] {
        add_member(&mut db, by, mailbox, roles, az(1, 9, 0))
            .await
            .expect("member");
    }
    let (job, pick) = picked(&pool, business, az(9, 10, 0)).await;
    let event = json!({"pick": pick, "job": job, "business": business});
    for _ in 0..2 {
        run_pick_notice(
            pool.clone(),
            ConfirmSettings::default(),
            claimed(event.clone()),
        )
        .await
        .expect("notice");
    }
    let notices = tasks(&pool, BUSINESS_NOTICE_KIND).await;
    assert_eq!(notices.len(), 1, "one notice per pick");
    let notice = &notices[0].0;
    assert_eq!(notice["template"], "pick_notice");
    assert_eq!(notice["to"], json!([owner_box, billing_box]));
    assert_eq!(notice["values"]["window"], 240);
    assert!(notice["values"]["job"].as_i64().is_some_and(|n| n >= 1001));
    assert_eq!(PICK_MADE_KIND, "picks.pick_made");
}

/// A fake license source answering every lookup with `answer` and counting the calls.
struct Fake {
    answer: Option<&'static str>,
    calls: AtomicUsize,
}

impl LicenseSource for Fake {
    async fn fetch_daily_list(&self, _list: &str) -> Result<Vec<u8>, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn lookup_license(&self, number: &str) -> Result<Vec<u8>, AdapterError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer
            .map(|a| a.replace("{number}", number).into_bytes())
            .ok_or(AdapterError::NotSent)
    }
}

fn facts(account: AccountStatus) -> StoredFacts {
    StoredFacts {
        account,
        license_ends_on: None,
        bond_cancelled_on: None,
    }
}

/// TD-PICK-09 (PICK-020, D-056): one live lookup at the pick; with the lookup down and no daily
/// result the pick waits; Confirm uses the stored results with no second lookup, and refuses a
/// paused account, a passed license-end date, a stale check and a pick past its window.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_09_license_at_the_pick_and_at_confirm(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = Uuid::from_u128(0xb065);
    let down = Fake {
        answer: None,
        calls: AtomicUsize::new(0),
    };
    let waits = check_at_pick(&mut db, &down, (business, NUMBER), az(9, 10, 0)).await;
    assert_eq!(waits, Ok(LicenseAtPick::Waits));
    let up = Fake {
        answer: Some(ACTIVE),
        calls: AtomicUsize::new(0),
    };
    let at_pick = check_at_pick(&mut db, &up, (business, NUMBER), az(9, 10, 0)).await;
    assert_eq!(at_pick, Ok(LicenseAtPick::Eligible));
    assert_eq!(up.calls.load(Ordering::SeqCst), 1, "one live lookup");
    let later = check_at_pick(&mut db, &down, (business, NUMBER), Timestamp::now()).await;
    let window = (Timestamp::now(), az(30, 12, 0));
    let ok = check_at_confirm(
        &mut db,
        (business, NUMBER),
        facts(AccountStatus::Active),
        window,
    );
    assert_eq!(ok.await, Ok(Ok(())), "the pick lookup is the stored result");
    assert_eq!(
        later,
        Ok(LicenseAtPick::Eligible),
        "a daily result under 24 hours is used"
    );
    assert_eq!(
        up.calls.load(Ordering::SeqCst),
        1,
        "Confirm makes no lookup"
    );
    let paused = facts(AccountStatus::Paused);
    let refused = check_at_confirm(&mut db, (business, NUMBER), paused, window).await;
    assert_eq!(refused, Ok(Err(Refusal::Account)));
    let mut ended = facts(AccountStatus::Active);
    ended.license_ends_on = Some(date(2026, 11, 1));
    let refused = check_at_confirm(&mut db, (business, NUMBER), ended, window).await;
    assert_eq!(refused, Ok(Err(Refusal::License)));
    let other = (Uuid::from_u128(0xb066), NUMBER);
    let unchecked = check_at_confirm(&mut db, other, facts(AccountStatus::Active), window).await;
    assert_eq!(unchecked, Ok(Err(Refusal::License)));
    let past = (az(30, 12, 0), az(30, 12, 0));
    let expired = check_at_confirm(
        &mut db,
        (business, NUMBER),
        facts(AccountStatus::Active),
        past,
    );
    assert_eq!(expired.await, Ok(Err(Refusal::Expired)));
}

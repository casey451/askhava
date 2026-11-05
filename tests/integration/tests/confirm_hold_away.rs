//! HOLD, Away and the emergency windows against the database (TD-PICK-20, TD-PICK-21, TD-PICK-23;
//! PICK-043, PICK-044, BIZ-034). Fake IDs only.
//!
//! The roles stored by `businesses` decide who may HOLD and set Away; an emergency job's
//! Awaiting confirm and Payment pending timers in `jobs` land where `picks`' emergency plan and
//! card-fix rule say, 60 clock minutes at any hour, and its pick notice starts no business-hour
//! window.

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create, members};
use askhava_core::api::Guarded;
use askhava_jobs::api::rules::timeout;
use askhava_jobs::api::{JobStatus, TIMER_KIND, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    ConfirmSettings, HoldAsk, TimeRefusal, WindowRules, away_for, confirm_by, emergency_fix_by,
    emergency_plan, hold, notice_sent,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Arizona wall-clock time on November `day`, 2026 (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

/// TD-PICK-20, TD-PICK-23 (PICK-043, BIZ-034, D-252, D-285): of a business's stored members, the
/// owner and the "Confirm and billing" member may HOLD and set Away; the "Cards and quotes"
/// member is refused both.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_20_only_the_owner_and_billing_may_hold_or_set_away(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = create(&mut db, false, az(1, 9, 0)).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::from_u128(0xe1), az(1, 9, 0))
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    for (mailbox, cards, billing) in [(0xe2, false, true), (0xe3, true, false)] {
        let roles = Roles::new(cards, billing).expect("roles");
        add_member(&mut db, by, Uuid::from_u128(mailbox), roles, az(1, 9, 0))
            .await
            .expect("member");
    }
    let team = members(&mut db, business).await.expect("members");
    let rules = WindowRules::default();
    let due = confirm_by(az(9, 10, 0), ConfirmSettings::default());
    let mut answers: Vec<_> = team
        .iter()
        .map(|m| {
            let ask = HoldAsk {
                by: m.roles,
                emergency: false,
                held: false,
                away_delayed: false,
            };
            (
                m.mailbox,
                hold(ask, (az(9, 11, 59), due), rules),
                away_for(m.roles, az(9, 10, 0), 3, rules),
            )
        })
        .collect();
    answers.sort_by_key(|a| a.0);
    assert_eq!(
        answers,
        vec![
            (Uuid::from_u128(0xe1), Ok(az(9, 16, 0)), Ok(az(9, 13, 0))),
            (Uuid::from_u128(0xe2), Ok(az(9, 16, 0)), Ok(az(9, 13, 0))),
            (
                Uuid::from_u128(0xe3),
                Err(TimeRefusal::Role),
                Err(TimeRefusal::Role)
            ),
        ]
    );
}

async fn timer_due(pool: &PgPool, job: Uuid) -> Timestamp {
    let at: jiff_sqlx::Timestamp = sqlx::query_scalar(
        "SELECT run_at FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(TIMER_KIND)
    .bind(job.to_string())
    .fetch_one(pool)
    .await
    .expect("timer");
    at.to_jiff()
}

/// TD-PICK-21 (PICK-044, JOB-013, JOB-014, D-270): an emergency pick at 23:00 waits in Awaiting
/// confirm until 00:00:00 by `jobs`' timer, the plan's Confirm end, with no business-hour window
/// from the notice's send; a card declined at 23:10 has until 00:10, as `jobs`' Payment pending
/// wait says.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_21_the_emergency_window_matches_the_jobs_timer(pool: PgPool) {
    let at = az(9, 23, 0);
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, true, Actor::System)
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
    let plan = emergency_plan(at, true, WindowRules::default());
    assert_eq!(plan.confirm_by, az(10, 0, 0));
    assert_eq!(timer_due(&pool, job).await, plan.confirm_by);
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(Uuid::from_u128(0xe9))
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    let sent = notice_sent(&mut db, pick, at, ConfirmSettings::default()).await;
    assert_eq!(sent, Ok(None));
    assert_eq!(timer_due(&pool, job).await, plan.confirm_by);
    let declined = az(9, 23, 10);
    let pay = timeout(JobStatus::PaymentPending, declined, true).expect("wait");
    assert_eq!(emergency_fix_by(declined, WindowRules::default()), pay.at);
    assert_eq!(pay.at, az(10, 0, 10));
}

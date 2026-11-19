//! "Hold for 2 more hours" stored and honoured in the database (PICK-043, PICK-019, BIZ-034;
//! D-3675, D-3890; T456). Fake IDs only.
//!
//! The pick notice's send stores the Confirm end in `picks.pick.confirm_due` (none for an
//! emergency job); a HOLD by the owner or a "Confirm and billing" member moves it 2 business
//! hours once, with `jobs`' backstop and a new window-end task, so the earlier task does nothing;
//! a second HOLD, a HOLD on an Away-delayed pick, on an emergency job, by a "Cards and quotes"
//! member or before the window runs changes nothing. The Away half (`set_away`, the delayed
//! start) is T543's; here the Away mark is seeded as T543 will store it.

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create, members};
use askhava_core::api::{Claimed, Guarded, add_business_minutes};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    CONFIRM_ENDS_KIND, ConfirmSettings, TimeRefusal, WindowRules, hold_allowed, hold_ask,
    hold_pick, notice_sent, run_confirm_ends,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use serde_json::Value;
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

/// A job picked at `at` (an emergency when `emergency`), waiting with one page pick of `business`.
async fn picked(pool: &PgPool, (business, emergency): (Uuid, bool), at: Timestamp) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, emergency, Actor::System).await;
    let job = job.expect("job");
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

/// A non-emergency pick whose notice went at 7:41 p.m. on the 9th: its window ends 11:41 a.m.
async fn sent_pick(pool: &PgPool, business: Uuid) -> (Uuid, Uuid) {
    let (job, pick) = picked(pool, (business, false), az(9, 17, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let due = notice_sent(&mut db, pick, az(9, 19, 41), ConfirmSettings::default()).await;
    assert_eq!(due, Ok(Some(az(10, 11, 41))));
    (job, pick)
}

async fn due_of(pool: &PgPool, pick: Uuid) -> Option<Timestamp> {
    let due: Option<jiff_sqlx::Timestamp> =
        sqlx::query_scalar("SELECT confirm_due FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(pool)
            .await
            .expect("due");
    due.map(|t| t.to_jiff())
}

async fn window_ends(pool: &PgPool) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, jiff_sqlx::Timestamp)> =
        sqlx::query_as("SELECT payload, run_at FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(CONFIRM_ENDS_KIND)
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

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    }
}

/// PICK-019, PICK-043, PICK-044: the notice's send stores the window's end; an emergency job's
/// notice stores none (its 60 clock minutes are `jobs`' own) and is never held.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_043_the_send_stores_the_confirm_end(pool: PgPool) {
    let (_, pick) = sent_pick(&pool, Uuid::from_u128(0x4561)).await;
    assert_eq!(due_of(&pool, pick).await, Some(az(10, 11, 41)));
    let business = Uuid::from_u128(0x4562);
    let (_, urgent) = picked(&pool, (business, true), az(9, 17, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = notice_sent(&mut db, urgent, az(9, 17, 1), ConfirmSettings::default()).await;
    assert_eq!(sent, Ok(None));
    assert_eq!(due_of(&pool, urgent).await, None);
    let ask = hold_ask(&mut db, urgent, Roles::BOTH).await.expect("ask");
    assert!(ask.expect("waits").1.emergency);
    let held = hold_pick(
        &mut db,
        urgent,
        Roles::BOTH,
        az(9, 17, 5),
        WindowRules::default(),
    )
    .await;
    assert_eq!(held, Ok(Err(TimeRefusal::Emergency)));
}

/// PICK-043, D-3750: a HOLD at 9 a.m. moves the 11:41 end to 1:41 p.m. once, with `jobs`'
/// backstop 2 business hours later and a new window end; the old end does nothing, a second
/// HOLD changes nothing, and the new end lapses the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_043_a_hold_moves_the_window_end_once(pool: PgPool) {
    let business = Uuid::from_u128(0x4563);
    let (job, pick) = sent_pick(&pool, business).await;
    let mut db = pool.acquire().await.expect("conn");
    let from = current(&mut db, job).await.expect("job").timer_from;
    let rules = WindowRules::default();
    let held = hold_pick(&mut db, pick, Roles::BOTH, az(10, 9, 0), rules).await;
    assert_eq!(held, Ok(Ok(az(10, 13, 41))));
    assert_eq!(due_of(&pool, pick).await, Some(az(10, 13, 41)));
    let moved = current(&mut db, job).await.expect("job").timer_from;
    assert_eq!(
        moved,
        add_business_minutes(from, 120),
        "the backstop moved too"
    );
    let ask = hold_ask(&mut db, pick, Roles::BOTH).await.expect("ask");
    assert_eq!(ask.map(|(b, a)| (b, a.held)), Some((business, true)));
    let again = hold_pick(&mut db, pick, Roles::BOTH, az(10, 10, 0), rules).await;
    assert_eq!(again, Ok(Err(TimeRefusal::AlreadyHeld)));
    assert_eq!(due_of(&pool, pick).await, Some(az(10, 13, 41)));
    let ends = window_ends(&pool).await;
    assert_eq!(ends.len(), 2);
    assert_eq!((ends[0].1, ends[1].1), (az(10, 11, 41), az(10, 13, 41)));
    run_confirm_ends(pool.clone(), claimed(ends[0].0.clone()))
        .await
        .expect("old end");
    assert_eq!(pick_status(&pool, pick).await, "awaiting_confirm");
    run_confirm_ends(pool.clone(), claimed(ends[1].0.clone()))
        .await
        .expect("new end");
    assert_eq!(pick_status(&pool, pick).await, "expired");
}

/// PICK-043, BIZ-034, D-285: a pick whose clock started late because the business was away
/// (the mark T543 stores) is never held; nothing is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_043_an_away_delayed_pick_is_never_held(pool: PgPool) {
    let (_, pick) = sent_pick(&pool, Uuid::from_u128(0x4564)).await;
    sqlx::query("UPDATE picks.pick SET away_delayed = true WHERE id = $1")
        .bind(pick)
        .execute(&pool)
        .await
        .expect("away");
    let mut db = pool.acquire().await.expect("conn");
    let ask = hold_ask(&mut db, pick, Roles::BOTH).await.expect("ask");
    assert!(ask.expect("waits").1.away_delayed);
    let rules = WindowRules::default();
    let held = hold_pick(&mut db, pick, Roles::BOTH, az(10, 9, 0), rules).await;
    assert_eq!(held, Ok(Err(TimeRefusal::AwayDelayed)));
    assert_eq!(due_of(&pool, pick).await, Some(az(10, 11, 41)));
    assert_eq!(window_ends(&pool).await.len(), 1, "no new window end");
}

/// PICK-043, BIZ-023, D-284: the stored roles decide: the owner and a "Confirm and billing"
/// member may HOLD, a "Cards and quotes" member may not; no HOLD before the window runs or after
/// it ended.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_043_roles_and_a_window_not_running(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let at = az(9, 8, 0);
    let business = create(&mut db, false, at).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::new_v4(), at).await;
    let by = Acting {
        business,
        member: owner.expect("owner"),
    };
    for (cards, billing) in [(true, false), (false, true)] {
        let roles = Roles::new(cards, billing).expect("roles");
        add_member(&mut db, by, Uuid::new_v4(), roles, at)
            .await
            .expect("member");
    }
    let team = members(&mut db, business).await.expect("team");
    let (_, pick) = sent_pick(&pool, business).await;
    let rules = WindowRules::default();
    for member in team {
        let by = if member.owner {
            Roles::BOTH
        } else {
            member.roles
        };
        let may = member.owner || member.roles.confirm_and_billing();
        let ask = hold_ask(&mut db, pick, by).await.expect("ask");
        assert_eq!(hold_allowed(ask.expect("waits").1).is_ok(), may);
        if !may {
            let held = hold_pick(&mut db, pick, by, az(10, 9, 0), rules).await;
            assert_eq!(held, Ok(Err(TimeRefusal::Role)));
        }
    }
    let late = hold_pick(&mut db, pick, Roles::BOTH, az(10, 11, 41), rules).await;
    assert_eq!(late, Ok(Err(TimeRefusal::Ended)));
    let (_, unsent) = picked(&pool, (Uuid::from_u128(0x4565), false), az(9, 17, 0)).await;
    let early = hold_pick(&mut db, unsent, Roles::BOTH, az(9, 18, 0), rules).await;
    assert_eq!(early, Ok(Err(TimeRefusal::Ended)));
    assert_eq!(window_ends(&pool).await.len(), 1);
}

//! "Away until <time>" stored and honoured in the database (BIZ-034, BIZ-038, PICK-043, PICK-045;
//! D-285, D-309, D-3890; T543). Fake IDs only.
//!
//! `set_away` stores the end for the owner or a "Confirm and billing" member, with one audit row,
//! and refuses a "Cards and quotes" member, a removed member, another business's member and an
//! end not ahead; `away_until` reads none once past. A non-emergency pick notice sent while the
//! business is away starts its Confirm clock when away ends and is marked `away_delayed`, so
//! T456's HOLD refuses it; an emergency pick never waits; a pick sent after away ended starts at
//! its send. A HOLD queues one PICK-043 notice per held pick with the new end.

use askhava_businesses::api::{
    Acting, BusinessesError, Roles, add_member, add_owner, away_until, create, remove_member,
    set_away,
};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    ConfirmSettings, HOLD_END_TEMPLATE, TimeRefusal, WindowRules, hold_pick, notice_sent,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Arizona wall-clock time on November `day`, 2026 (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

/// A business with its owner, a "Confirm and billing" and a "Cards and quotes" member, made at
/// `at`: the business and the three members' `Acting`s.
async fn team(db: &mut PgConnection, at: Timestamp) -> (Uuid, [Acting; 3]) {
    let business = create(db, false, at).await.expect("business");
    let owner = add_owner(db, business, Uuid::new_v4(), at).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let mut people = [owner; 3];
    for (slot, (cards, billing)) in [(1, (false, true)), (2, (true, false))] {
        let roles = Roles::new(cards, billing).expect("roles");
        let member = add_member(db, owner, Uuid::new_v4(), roles, at).await;
        people[slot].member = member.expect("member");
    }
    (business, people)
}

/// A job picked at `at` (an emergency when `emergency`), waiting with one page pick of `business`.
async fn picked(db: &mut PgConnection, (business, emergency): (Uuid, bool), at: Timestamp) -> Uuid {
    let job = askhava_jobs::api::create(db, at, emergency, Actor::System).await;
    let job = job.expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
    ] {
        let moved = move_to(db, job, step, at, Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(at.to_sqlx())
    .fetch_one(db)
    .await
    .expect("pick")
}

async fn away_delayed(pool: &PgPool, pick: Uuid) -> bool {
    sqlx::query_scalar("SELECT away_delayed FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("mark")
}

async fn audit_rows(pool: &PgPool, business: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND field = 'away_until'",
    )
    .bind(business)
    .fetch_one(pool)
    .await
    .expect("audit")
}

/// BIZ-034, BIZ-023, D-285: the owner and a "Confirm and billing" member store the end, each with
/// one audit row; a "Cards and quotes" member, a removed member, another business's member and
/// an end not ahead are refused and store nothing; the end reads none once past.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_034_set_away_stores_the_end_for_the_roles_that_may(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let at = az(10, 8, 0);
    let (business, [owner, billing, cards]) = team(&mut db, at).await;
    let (_, [stranger, ..]) = team(&mut db, at).await;
    let other = Acting {
        business,
        member: stranger.member,
    };
    for (by, until) in [(cards, az(10, 9, 0)), (other, az(10, 9, 0)), (owner, at)] {
        let set = set_away(&mut db, by, until, at).await;
        assert_eq!(set, Err(BusinessesError::Refused));
    }
    assert_eq!(away_until(&mut db, business, at).await, Ok(None));
    assert_eq!(
        audit_rows(&pool, business).await,
        0,
        "a refusal writes nothing"
    );
    set_away(&mut db, owner, az(10, 10, 0), at)
        .await
        .expect("owner");
    assert_eq!(
        away_until(&mut db, business, at).await,
        Ok(Some(az(10, 10, 0)))
    );
    set_away(&mut db, billing, az(10, 11, 0), at)
        .await
        .expect("billing");
    assert_eq!(
        away_until(&mut db, business, at).await,
        Ok(Some(az(10, 11, 0)))
    );
    assert_eq!(audit_rows(&pool, business).await, 2, "one audit row each");
    let past = away_until(&mut db, business, az(10, 11, 0)).await;
    assert_eq!(past, Ok(None), "none once past");
    remove_member(&mut db, owner, billing.member, at)
        .await
        .expect("removed");
    let removed = set_away(&mut db, billing, az(10, 12, 0), at).await;
    assert_eq!(removed, Err(BusinessesError::Refused));
    assert_eq!(
        away_until(&mut db, business, at).await,
        Ok(Some(az(10, 11, 0)))
    );
}

/// BIZ-034, PICK-043, PICK-045, D-285: away until 10 a.m., a pick sent at 9 starts its 4 business
/// hours at 10 (ends 2 p.m.), marked `away_delayed`, so its HOLD is refused and nothing moves; an
/// emergency pick sent while away never waits; a pick sent after away ended starts at its send.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_034_a_pick_sent_while_away_starts_when_away_ends(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (business, [owner, ..]) = team(&mut db, az(10, 7, 0)).await;
    set_away(&mut db, owner, az(10, 10, 0), az(10, 8, 0))
        .await
        .expect("away");
    let settings = ConfirmSettings::default();
    let pick = picked(&mut db, (business, false), az(10, 8, 30)).await;
    let due = notice_sent(&mut db, pick, az(10, 9, 0), settings).await;
    assert_eq!(
        due,
        Ok(Some(az(10, 14, 0))),
        "the clock starts when away ends"
    );
    assert!(away_delayed(&pool, pick).await);
    let rules = WindowRules::default();
    let held = hold_pick(&mut db, pick, Roles::BOTH, az(10, 11, 0), rules).await;
    assert_eq!(held, Ok(Err(TimeRefusal::AwayDelayed)), "T456's refusal");
    let urgent = picked(&mut db, (business, true), az(10, 8, 30)).await;
    let sent = notice_sent(&mut db, urgent, az(10, 9, 0), settings).await;
    assert_eq!(sent, Ok(None), "an emergency pick never waits (PICK-044)");
    assert!(!away_delayed(&pool, urgent).await);
    let later = picked(&mut db, (business, false), az(10, 10, 30)).await;
    let due = notice_sent(&mut db, later, az(10, 11, 0), settings).await;
    assert_eq!(
        due,
        Ok(Some(az(10, 15, 0))),
        "away over: the clock starts at the send"
    );
    assert!(!away_delayed(&pool, later).await);
}

/// PICK-043, D-3890: a HOLD queues one business notice with the new end in the same
/// transaction; a refused second HOLD queues no other.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_043_one_hold_notice_per_held_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (business, _) = team(&mut db, az(10, 7, 0)).await;
    let settings = ConfirmSettings::default();
    let pick = picked(&mut db, (business, false), az(10, 8, 30)).await;
    let due = notice_sent(&mut db, pick, az(10, 9, 0), settings).await;
    assert_eq!(due, Ok(Some(az(10, 13, 0))));
    let rules = WindowRules::default();
    let held = hold_pick(&mut db, pick, Roles::BOTH, az(10, 10, 0), rules).await;
    assert_eq!(held, Ok(Ok(az(10, 15, 0))));
    let again = hold_pick(&mut db, pick, Roles::BOTH, az(10, 11, 0), rules).await;
    assert_eq!(again, Ok(Err(TimeRefusal::AlreadyHeld)));
    let notices: Vec<(Value, Option<String>)> = sqlx::query_as(
        "SELECT payload, dedupe_key FROM core.task \
         WHERE kind = 'messaging.business_notice' AND payload->>'template' = $1",
    )
    .bind(HOLD_END_TEMPLATE)
    .fetch_all(&pool)
    .await
    .expect("notices");
    assert_eq!(notices.len(), 1, "one notice per held pick");
    let (payload, key) = &notices[0];
    assert_eq!(
        key.as_deref(),
        Some(&*format!("messaging.business_notice.hold_end.{pick}"))
    );
    assert_eq!(payload["business"], business.to_string());
    assert_eq!(payload["values"]["time"], az(10, 15, 0).as_second());
    let to = payload["to"].as_array().map(Vec::len);
    assert_eq!(
        to,
        Some(2),
        "the owner and the \"Confirm and billing\" member, never cards-only"
    );
}

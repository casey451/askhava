//! "Busy for <n> days" stored and ended in the database (BIZ-028, BIZ-038, BIZ-010; D-309,
//! D-3675, D-3890; T463). Fake IDs only.
//!
//! `set_busy` pauses job cards through the self pause, stores the end in `busy_until` with its
//! audit row and queues one end task at the end, for the owner or a "Confirm and billing" member;
//! a "Cards and quotes" member, a removed member, another business's member, days outside 1 to
//! 60 and a business that is not active are refused with nothing stored or queued. The end task
//! resumes the business only while it is still self-paused from that same pause: after a Resume,
//! a Resume and a new plain pause, or a rule's pause, it changes nothing.

use askhava_businesses::api::{
    AccountStatus, Acting, BUSY_END_KIND, BusinessesError, Checks, Event, PauseReason, Roles,
    add_member, add_owner, apply, business, create, pause_for_rule, pause_self, remove_member,
    resume_self, run_busy_end, set_busy,
};
use askhava_core::api::Claimed;
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// An active business with its owner, a "Confirm and billing" and a "Cards and quotes" member,
/// made at `at`: the business and the three members' `Acting`s.
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
    let checks = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let event = Event::Checked(checks);
    let moved = apply(db, business, event, Actor::System, at).await;
    assert_eq!(moved.expect("active"), AccountStatus::Active);
    (business, people)
}

/// The business's status code, pause reason code and stored busy end.
async fn state(db: &mut PgConnection, id: Uuid) -> (&'static str, Option<&'static str>, bool) {
    let read = business(&mut *db, id).await.expect("business");
    let end: Option<bool> =
        sqlx::query_scalar("SELECT busy_until IS NOT NULL FROM businesses.business WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *db)
            .await
            .expect("busy read");
    let reason = read.pause_reason.map(PauseReason::code);
    (read.status.code(), reason, end == Some(true))
}

/// The queued end tasks of `business` as the worker would claim them, with their due times.
async fn end_tasks(pool: &PgPool, business: Uuid) -> Vec<(Claimed, Timestamp)> {
    let rows: Vec<(i64, serde_json::Value, jiff_sqlx::Timestamp)> = sqlx::query_as(
        "SELECT id, payload, run_at FROM core.task WHERE kind = $1 AND payload->>'business' = $2 \
         ORDER BY id",
    )
    .bind(BUSY_END_KIND)
    .bind(business.to_string())
    .fetch_all(pool)
    .await
    .expect("tasks");
    let claimed = |(id, payload, run_at): (i64, serde_json::Value, jiff_sqlx::Timestamp)| {
        let kind = BUSY_END_KIND.to_owned();
        let task = Claimed {
            id,
            kind,
            payload,
            attempts: 1,
        };
        (task, run_at.to_jiff())
    };
    rows.into_iter().map(claimed).collect()
}

/// The audit rows naming `busy_until` on `business`.
async fn audit_rows(pool: &PgPool, business: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND field = 'busy_until'",
    )
    .bind(business)
    .fetch_one(pool)
    .await
    .expect("audit")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn busy_pauses_stores_the_end_and_its_task_resumes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let (id, [owner, _, _]) = team(&mut db, now).await;
    let until = set_busy(&mut db, owner, 3, now).await.expect("busy");
    let span = until.duration_since(now);
    let off = (span - SignedDuration::from_hours(72)).abs();
    assert!(off < SignedDuration::from_millis(1), "3 days on");
    assert_eq!(state(&mut db, id).await, ("paused", Some("self"), true));
    assert_eq!(
        audit_rows(&pool, id).await,
        1,
        "one audit row naming the field"
    );
    let tasks = end_tasks(&pool, id).await;
    assert_eq!(tasks.len(), 1, "one end task");
    let (task, due) = tasks.into_iter().next().expect("task");
    assert_eq!(due, until, "due at the end");
    let later = until + SignedDuration::from_secs(1);
    run_busy_end(pool.clone(), task.clone(), later)
        .await
        .expect("ran");
    assert_eq!(state(&mut db, id).await.0, "active", "resumed at the end");
    run_busy_end(pool.clone(), task, later)
        .await
        .expect("again");
    assert_eq!(
        state(&mut db, id).await.0,
        "active",
        "a repeat changes nothing"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_end_task_changes_nothing_after_a_resume_or_another_pause(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let (id, [_, billing, _]) = team(&mut db, now).await;
    set_busy(&mut db, billing, 2, now).await.expect("busy");
    let (task, _) = end_tasks(&pool, id).await.remove(0);
    let at = |s| now + SignedDuration::from_secs(s);
    resume_self(&mut db, id, at(1)).await.expect("resumed");
    run_busy_end(pool.clone(), task.clone(), at(2))
        .await
        .expect("ran");
    assert_eq!(state(&mut db, id).await.0, "active", "after a Resume");
    pause_self(&mut db, id, at(3)).await.expect("plain pause");
    run_busy_end(pool.clone(), task.clone(), at(4))
        .await
        .expect("ran");
    let paused = ("paused", Some("self"), true);
    assert_eq!(
        state(&mut db, id).await,
        paused,
        "a later plain pause stays"
    );
    resume_self(&mut db, id, at(5)).await.expect("resumed");
    set_busy(&mut db, billing, 1, at(6))
        .await
        .expect("busy again");
    let rule = PauseReason::Owner;
    pause_for_rule(&mut db, id, rule, Actor::System, at(7))
        .await
        .expect("rule pause");
    let (task, _) = end_tasks(&pool, id).await.remove(1);
    run_busy_end(pool.clone(), task, at(8)).await.expect("ran");
    assert_eq!(
        state(&mut db, id).await.1,
        Some("owner"),
        "a rule's pause stays"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn only_the_owner_or_confirm_and_billing_sets_busy_and_only_when_active(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let (id, [owner, billing, cards]) = team(&mut db, now).await;
    let (other, [stranger, _, _]) = team(&mut db, now).await;
    let roles = Roles::new(false, true).expect("roles");
    let gone = add_member(&mut db, owner, Uuid::new_v4(), roles, now).await;
    let gone = gone.expect("member");
    remove_member(&mut db, owner, gone, now)
        .await
        .expect("removed");
    let removed = Acting {
        business: id,
        member: gone,
    };
    let foreign = Acting {
        business: id,
        member: stranger.member,
    };
    let refused = [
        (cards, 3),
        (removed, 3),
        (foreign, 3),
        (owner, 0),
        (owner, 61),
    ];
    for (by, days) in refused {
        let out = set_busy(&mut db, by, days, now).await;
        assert!(matches!(out, Err(BusinessesError::Refused)), "{days}");
    }
    pause_self(&mut db, id, now).await.expect("paused");
    let out = set_busy(&mut db, billing, 3, now).await;
    assert!(matches!(out, Err(BusinessesError::Refused)), "not active");
    assert_eq!(state(&mut db, id).await, ("paused", Some("self"), false));
    assert!(end_tasks(&pool, id).await.is_empty(), "nothing queued");
    assert_eq!(audit_rows(&pool, id).await, 0, "nothing audited");
    let theirs = set_busy(&mut db, stranger, 60, now).await;
    assert!(theirs.is_ok(), "60 days is the longest");
    assert_eq!(state(&mut db, other).await, ("paused", Some("self"), true));
}

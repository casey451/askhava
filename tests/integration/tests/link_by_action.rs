//! `calls::api::link_by_action`: the email's "Link this phone" action on a blocked call's pending
//! link (CALL-011, CALL-012, BIZ-033, BIZ-038; D-078, D-311, D-3567, D-3675; T458).
//!
//! Real PostgreSQL. A pending link (`calls.link_request`) holds only keyed digests, so every
//! fixture here is a fake digest, never a number: the owner's action links the phone approved
//! and findable in `messaging`'s sender directory; a "Confirm and billing" member's links it by
//! that member, not approved, held out of the directory, with the owner's
//! `businesses.phone_linked` email queued, until the owner's `approve_phone` releases it; an
//! expired, used or another business's pending link, a person who may not link and a digest
//! another phone holds are refused with no change at all. Fake IDs only.

use askhava_businesses::api::{
    Acting, PHONE_LINKED_KIND, Roles, add_member, add_owner, approve_phone, create, remove_member,
};
use askhava_calls::api::{LINK_REQUEST_LIFE, LinkAction, link_by_action};
use askhava_messaging::api::sender_put_pending;
use askhava_ops::api::export_audit;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The blocked call's time; every other time is counted from it.
fn at(seconds: i64) -> Timestamp {
    let base: Timestamp = "2026-03-02T17:00:00Z".parse().expect("time");
    base + SignedDuration::from_secs(seconds)
}

/// One business: its owner, a "Confirm and billing" member and a "Cards and quotes" member.
struct Team {
    business: Uuid,
    owner: Acting,
    biller: Acting,
    quoter: Acting,
}

async fn team(db: &mut PgConnection) -> Team {
    let business = create(db, false, at(-60)).await.expect("business");
    let member = add_owner(db, business, Uuid::new_v4(), at(-60))
        .await
        .expect("owner");
    let owner = Acting { business, member };
    let mut add = async |roles: Roles| {
        let member = add_member(db, owner, Uuid::new_v4(), roles, at(-60))
            .await
            .expect("member");
        Acting { business, member }
    };
    let biller = add(Roles::new(false, true).expect("roles")).await;
    let quoter = add(Roles::new(true, false).expect("roles")).await;
    Team {
        business,
        owner,
        biller,
        quoter,
    }
}

/// A pending link of `business` stored at `at(0)` on a fake open line, holding the fake digest
/// `[byte; 32]` under key version 1 (CALL-011): its ID.
async fn pending(db: &mut PgConnection, business: Uuid, byte: u8) -> Uuid {
    sqlx::query_scalar(
        "WITH line AS (INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, \
         pool_number, shared, assigned_at) VALUES (gen_random_uuid(), gen_random_uuid(), $1, \
         '+19285550142', false, $2) RETURNING id), \
         request AS (INSERT INTO calls.link_request (line_id, business_id, requested_at, \
         expires_at) SELECT id, $1, $2, $2 + interval '24 hours' FROM line RETURNING id), \
         digest AS (INSERT INTO calls.link_request_digest (request_id, index_version, digest) \
         SELECT id, 1, $3 FROM request) \
         SELECT id FROM request",
    )
    .bind(business)
    .bind(at(0).to_sqlx())
    .bind(vec![byte; 32])
    .fetch_one(db)
    .await
    .expect("pending link")
}

async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

/// Everything a link changes, counted: staff phones, sender-directory digests, held digests,
/// used pending links, pending digests, queued owner emails and audit rows.
async fn state(db: &mut PgConnection) -> [i64; 7] {
    let audit = export_audit(db, 0, 1000).await.expect("audit").len();
    [
        count(db, "SELECT count(*) FROM businesses.staff_phone").await,
        count(db, "SELECT count(*) FROM messaging.sender_directory").await,
        count(db, "SELECT count(*) FROM messaging.sender_held").await,
        count(
            db,
            "SELECT count(*) FROM calls.link_request WHERE used_at IS NOT NULL",
        )
        .await,
        count(db, "SELECT count(*) FROM calls.link_request_digest").await,
        count(
            db,
            "SELECT count(*) FROM core.task WHERE kind = 'businesses.phone_linked'",
        )
        .await,
        i64::try_from(audit).expect("audit rows"),
    ]
}

/// The one staff phone of `business`: its line, who linked it, who set its number, whether it
/// is approved, and whether any digit of a number is stored.
async fn phone(db: &mut PgConnection, business: Uuid) -> (Uuid, Uuid, Uuid, bool, bool) {
    sqlx::query_as(
        "SELECT id, linked_by, number_set_by, approved_at IS NOT NULL, number_end IS NOT NULL \
         FROM businesses.staff_phone WHERE business_id = $1 AND NOT is_main",
    )
    .bind(business)
    .fetch_one(db)
    .await
    .expect("one staff phone")
}

/// CALL-011, CALL-012: the owner's action links the blocked phone approved and findable at
/// once, uses the pending link up and deletes its digests; no owner email is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_012_the_owners_action_links_the_phone_approved(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let request = pending(&mut db, t.business, 0xab).await;
    let before = state(&mut db).await;
    assert_eq!(PHONE_LINKED_KIND, "businesses.phone_linked");
    let linked = link_by_action(&mut db, t.owner, request, at(60)).await;
    assert_eq!(linked, Ok(LinkAction::Linked));
    let (_, by, set_by, approved, digits) = phone(&mut db, t.business).await;
    assert_eq!((by, set_by), (t.owner.member, t.owner.member));
    assert!(approved, "the owner's link works at once (BIZ-033)");
    assert!(!digits, "no digit of the number is stored (PRIV-002)");
    let after = state(&mut db).await;
    let [phones, filed, held, used, digests, emails, audit] = before;
    assert_eq!(
        after,
        [
            phones + 1,
            filed + 1,
            held,
            used + 1,
            digests - 1,
            emails,
            audit + 1
        ],
        "one phone, its digest filed, the pending link used and emptied, one audit row"
    );
    let again = link_by_action(&mut db, t.owner, request, at(120)).await;
    assert_eq!(again, Ok(LinkAction::Refused), "a used pending link");
    assert_eq!(state(&mut db).await, after, "and nothing changed");
}

/// CALL-012, BIZ-033: a "Confirm and billing" member's action links the phone by that member,
/// not approved and out of the sender directory, and queues the owner's email; the owner's
/// approval releases it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_012_a_members_action_waits_for_the_owner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let request = pending(&mut db, t.business, 0xab).await;
    let before = state(&mut db).await;
    let linked = link_by_action(&mut db, t.biller, request, at(60)).await;
    assert_eq!(linked, Ok(LinkAction::Waiting));
    let (line, by, set_by, approved, digits) = phone(&mut db, t.business).await;
    assert_eq!((by, set_by), (t.biller.member, t.biller.member));
    assert!(!approved, "it waits for the owner (BIZ-033)");
    assert!(!digits);
    let after = state(&mut db).await;
    let [phones, filed, held, used, digests, emails, audit] = before;
    assert_eq!(
        after,
        [
            phones + 1,
            filed,
            held + 1,
            used + 1,
            digests - 1,
            emails + 1,
            audit + 2
        ],
        "held, not filed; the owner's email queued with its audit row"
    );
    let approval = approve_phone(&mut db, t.owner, (line, at(60)), at(90)).await;
    assert_eq!(
        approval,
        Ok(()),
        "the owner approves the number set at the link"
    );
    let released = state(&mut db).await;
    assert_eq!((released[1], released[2]), (filed + 1, held));
}

/// CALL-011: a pending link lives until its `expires_at`, not at it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_011_an_expired_pending_link_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let request = pending(&mut db, t.business, 0xab).await;
    let before = state(&mut db).await;
    let life = LINK_REQUEST_LIFE.as_secs();
    let late = link_by_action(&mut db, t.owner, request, at(life)).await;
    assert_eq!(late, Ok(LinkAction::Refused), "at the expiry");
    assert_eq!(state(&mut db).await, before, "nothing changed");
    let in_time = link_by_action(&mut db, t.owner, request, at(life - 1)).await;
    assert_eq!(in_time, Ok(LinkAction::Linked), "one second before it");
}

/// CALL-012, BIZ-038: another business's pending link, an unknown one, a "Cards and quotes"
/// member, a removed member and a member of another business are refused with no change.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_012_a_person_who_may_not_link_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let other = team(&mut db).await;
    let request = pending(&mut db, t.business, 0xab).await;
    let theirs = pending(&mut db, other.business, 0xcd).await;
    remove_member(&mut db, t.owner, t.biller.member, at(30))
        .await
        .expect("removed");
    let before = state(&mut db).await;
    let planted = Acting {
        business: t.business,
        member: other.owner.member,
    };
    for (by, pending, why) in [
        (t.owner, theirs, "another business's pending link"),
        (t.owner, Uuid::new_v4(), "an unknown pending link"),
        (t.quoter, request, "a \"Cards and quotes\" member"),
        (t.biller, request, "a removed member"),
        (other.owner, request, "another business's owner"),
        (planted, request, "a member of another business"),
    ] {
        let refused = link_by_action(&mut db, by, pending, at(60)).await;
        assert_eq!(refused, Ok(LinkAction::Refused), "{why}");
        assert_eq!(state(&mut db).await, before, "{why}: nothing changed");
    }
}

/// CALL-011, D-3567: a digest another phone already holds refuses the link and rolls the new
/// phone back, for the owner and for a member.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_011_a_number_another_phone_holds_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let request = pending(&mut db, t.business, 0xab).await;
    let holder = (Uuid::new_v4(), Uuid::new_v4());
    sender_put_pending(&mut db, holder, &[(1, [0xab; 32])])
        .await
        .expect("another phone holds the digest");
    let before = state(&mut db).await;
    for by in [t.owner, t.biller] {
        let refused = link_by_action(&mut db, by, request, at(60)).await;
        assert_eq!(refused, Ok(LinkAction::Refused));
        assert_eq!(state(&mut db).await, before, "nothing changed");
    }
}

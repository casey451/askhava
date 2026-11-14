//! TD-BIZ-52 (BIZ-033, CALL-012; D-311, D-3081, T396): a phone a member other than the owner links
//! or changes is held out of `messaging`'s sender directory until the owner approves it.
//!
//! Real PostgreSQL; every lookup goes through `messaging::api::sender_business`, and the directory
//! link is the composition's (`messaging::api::sender_add` with the vault). The call and text
//! routes are T398's. Fake numbers only (555-01xx) and fake IDs.

use askhava_businesses::api::{
    Acting, Roles, SenderLink, TeamError, add_member, add_owner, approve_phone, change_phone_at,
    create, link_by_reply, link_phone, phone_standing, reject_phone, remove_member, waiting_phones,
};
use askhava_messaging::api::{OutboxError, sender_add, sender_business};
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &self.0, business_line, number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::BadPhone => TeamError::BadPhone,
                OutboxError::SenderTaken => TeamError::PhoneTaken,
                _ => TeamError::Database,
            })
    }
}

fn directory() -> Directory {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Directory(Vault::new(&key(0x54), &key(0x64), &url).expect("vault"))
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

fn n(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// One business with its owner, a "Confirm and billing" member and a member holding both roles.
struct Team {
    business: Uuid,
    owner: Acting,
    biller: Acting,
    both: Acting,
}

async fn team(db: &mut PgConnection, seed: u128) -> Team {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(seed), at(1))
        .await
        .expect("owner");
    let owner = Acting { business, member };
    let mut add = async |id: u128, roles: Roles| {
        let member = add_member(db, owner, Uuid::from_u128(id), roles, at(1))
            .await
            .expect("member");
        Acting { business, member }
    };
    let biller = add(seed + 1, Roles::new(false, true).expect("roles")).await;
    let both = add(seed + 2, Roles::BOTH).await;
    Team {
        business,
        owner,
        biller,
        both,
    }
}

async fn found(db: &mut PgConnection, dir: &Directory, number: &str) -> Option<Uuid> {
    sender_business(db, &dir.0, number).await.expect("lookup")
}

async fn link(db: &mut PgConnection, dir: &Directory, by: Acting, number: &str, m: i64) -> Uuid {
    link_phone(db, dir, (by, false), &n(number), at(m))
        .await
        .expect("link")
}

async fn count(db: &mut PgConnection, sql: &'static str, line: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(line)
        .fetch_one(db)
        .await
        .expect("count")
}

const APPROVED_AUDIT: &str = "SELECT count(*) FROM ops.audit_log \
     WHERE action = 'businesses.phone_approved' AND subject_id = $1";
const HELD: &str = "SELECT count(*) FROM messaging.sender_held WHERE line_id = $1";
const SET: &str = "SELECT number_set_at, number_set_by, number_end, approved_at IS NULL \
     FROM businesses.staff_phone WHERE id = $1";

async fn set_facts(db: &mut PgConnection, line: Uuid) -> (Timestamp, Uuid, String, bool) {
    let (set_at, by, end, waiting): (jiff_sqlx::Timestamp, Uuid, String, bool) =
        sqlx::query_as(SET)
            .bind(line)
            .fetch_one(db)
            .await
            .expect("phone");
    (set_at.to_jiff(), by, end, waiting)
}

/// TD-BIZ-52: a "Confirm and billing" member's link and a both-roles member's link each wait,
/// found for no business, until the owner approves; then the business is found. Every link records
/// when and by whom its number was set, and its last 4 digits only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_a_members_link_waits_for_the_owners_approval(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xe0).await;
    for (by, number, minute) in [(t.biller, "928-555-0120", 2), (t.both, "928-555-0121", 3)] {
        let line = link(&mut db, &dir, by, number, minute).await;
        assert_eq!(found(&mut db, &dir, number).await, None);
        assert_eq!(phone_standing(&mut db, line).await, Ok(Some(false)));
        let facts = set_facts(&mut db, line).await;
        assert_eq!(facts, (at(minute), by.member, number[8..].to_owned(), true));
        let waiting = waiting_phones(&mut db, t.business).await.expect("waiting");
        assert_eq!(waiting.len(), 1);
        assert_eq!((waiting[0].line, waiting[0].set_by), (line, by.member));
        approve_phone(&mut db, t.owner, (line, at(minute)), at(9))
            .await
            .expect("approve");
        assert_eq!(found(&mut db, &dir, number).await, Some(t.business));
        assert_eq!(phone_standing(&mut db, line).await, Ok(Some(true)));
        assert_eq!(count(&mut db, APPROVED_AUDIT, line).await, 1);
        assert_eq!(count(&mut db, HELD, line).await, 0);
    }
}

/// TD-BIZ-52: approval by a "Confirm and billing" member, a both-roles member, another business's
/// owner, of an unknown line, and after another business linked the number, each leave the phone
/// waiting, its number not found for its business, and no approval audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_planted_approvals_are_refused(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xe4).await;
    let other = team(&mut db, 0xe8).await;
    let line = link(&mut db, &dir, t.biller, "928-555-0122", 2).await;
    let unknown = Uuid::from_u128(0xe9);
    for (by, which, want) in [
        (t.biller, line, TeamError::NotAllowed),
        (t.both, line, TeamError::NotAllowed),
        (other.owner, line, TeamError::UnknownPhone),
        (t.owner, unknown, TeamError::UnknownPhone),
    ] {
        let refused = approve_phone(&mut db, by, (which, at(2)), at(3)).await;
        assert_eq!(refused, Err(want));
    }
    link(&mut db, &dir, other.owner, "928-555-0122", 4).await;
    let taken = approve_phone(&mut db, t.owner, (line, at(2)), at(5)).await;
    assert_eq!(taken, Err(TeamError::PhoneTaken));
    let held = found(&mut db, &dir, "928-555-0122").await;
    assert_eq!(held, Some(other.business));
    assert_eq!(phone_standing(&mut db, line).await, Ok(Some(false)));
    assert_eq!(count(&mut db, APPROVED_AUDIT, line).await, 0);
    assert_eq!(count(&mut db, HELD, line).await, 1);
}

/// TD-BIZ-52: a declined phone is gone: never found, approving it is not found, and the same
/// number linked again is a new waiting phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_a_declined_phone_never_works(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xf0).await;
    let line = link(&mut db, &dir, t.biller, "928-555-0123", 2).await;
    let refused = reject_phone(&mut db, t.biller, (line, at(2)), at(3)).await;
    assert_eq!(refused, Err(TeamError::NotAllowed));
    reject_phone(&mut db, t.owner, (line, at(2)), at(3))
        .await
        .expect("decline");
    assert_eq!(found(&mut db, &dir, "928-555-0123").await, None);
    assert_eq!(phone_standing(&mut db, line).await, Ok(None));
    assert_eq!(count(&mut db, HELD, line).await, 0);
    let again = approve_phone(&mut db, t.owner, (line, at(2)), at(4)).await;
    assert_eq!(again, Err(TeamError::UnknownPhone));
    let relinked = link(&mut db, &dir, t.biller, "928-555-0123", 5).await;
    assert_ne!(relinked, line);
    assert_eq!(phone_standing(&mut db, relinked).await, Ok(Some(false)));
    assert_eq!(found(&mut db, &dir, "928-555-0123").await, None);
}

/// TD-BIZ-52: the owner's own links (main and staff) and the LINK-reply phone (T391) work at
/// once; approving the owner's link is refused as not waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_the_owners_links_work_at_once(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xf4).await;
    let main = link_phone(&mut db, &dir, (t.owner, true), &n("928-555-0124"), at(2))
        .await
        .expect("main");
    let staff = link(&mut db, &dir, t.owner, "928-555-0125", 3).await;
    assert_eq!(found(&mut db, &dir, "928-555-0124").await, Some(t.business));
    assert_eq!(found(&mut db, &dir, "928-555-0125").await, Some(t.business));
    for (line, minute, end) in [(main, 2, "0124"), (staff, 3, "0125")] {
        let refused = approve_phone(&mut db, t.owner, (line, at(minute)), at(4)).await;
        assert_eq!(refused, Err(TeamError::NotWaiting));
        assert_eq!(
            set_facts(&mut db, line).await,
            (at(minute), t.owner.member, end.to_owned(), false)
        );
    }
    let reply = link_by_reply(&mut db, t.business, at(5))
        .await
        .expect("reply");
    sender_add(&mut db, &dir.0, (t.business, reply), "928-555-0126")
        .await
        .expect("file");
    assert_eq!(found(&mut db, &dir, "928-555-0126").await, Some(t.business));
    assert_eq!(phone_standing(&mut db, reply).await, Ok(Some(true)));
    let (set_at, by, end): (jiff_sqlx::Timestamp, Uuid, Option<String>) = sqlx::query_as(
        "SELECT number_set_at, number_set_by, number_end FROM businesses.staff_phone \
         WHERE id = $1",
    )
    .bind(reply)
    .fetch_one(&mut *db)
    .await
    .expect("reply phone");
    assert_eq!((set_at.to_jiff(), by, end), (at(5), t.owner.member, None));
    assert!(
        waiting_phones(&mut db, t.business)
            .await
            .expect("w")
            .is_empty()
    );
}

/// TD-BIZ-52: a member's change of an approved phone and of an owner-linked phone each leave the
/// new and the old number unfound until the owner approves again; the owner's change works at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_a_members_change_waits_again(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xf8).await;
    let approved = link(&mut db, &dir, t.biller, "928-555-0127", 2).await;
    approve_phone(&mut db, t.owner, (approved, at(2)), at(3))
        .await
        .expect("approve");
    let owners = link(&mut db, &dir, t.owner, "928-555-0128", 3).await;
    let cases = [
        (approved, "928-555-0127", "928-555-0130"),
        (owners, "928-555-0128", "928-555-0131"),
    ];
    for (line, old, new) in cases {
        change_phone_at(&mut db, &dir, (t.both, line), &n(new), at(4))
            .await
            .expect("change");
        assert_eq!(found(&mut db, &dir, new).await, None);
        assert_eq!(found(&mut db, &dir, old).await, None);
        assert_eq!(
            set_facts(&mut db, line).await,
            (at(4), t.both.member, new[8..].to_owned(), true)
        );
        approve_phone(&mut db, t.owner, (line, at(4)), at(5))
            .await
            .expect("approve again");
        assert_eq!(found(&mut db, &dir, new).await, Some(t.business));
    }
    change_phone_at(&mut db, &dir, (t.owner, owners), &n("928-555-0132"), at(6))
        .await
        .expect("owner change");
    assert_eq!(found(&mut db, &dir, "928-555-0132").await, Some(t.business));
    assert_eq!(
        set_facts(&mut db, owners).await,
        (at(6), t.owner.member, "0132".to_owned(), false)
    );
}

/// TD-BIZ-52: the owner's change of a waiting phone puts the new number in place approved, with
/// the owner as the one who set it, and leaves no held digest of the old number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_the_owners_change_of_a_waiting_phone_works_at_once(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0x1f0).await;
    let line = link(&mut db, &dir, t.biller, "928-555-0140", 2).await;
    change_phone_at(&mut db, &dir, (t.owner, line), &n("928-555-0141"), at(3))
        .await
        .expect("owner change");
    assert_eq!(
        set_facts(&mut db, line).await,
        (at(3), t.owner.member, "0141".to_owned(), false)
    );
    assert_eq!(found(&mut db, &dir, "928-555-0141").await, Some(t.business));
    assert_eq!(found(&mut db, &dir, "928-555-0140").await, None);
    assert_eq!(count(&mut db, HELD, line).await, 0);
}

/// TD-BIZ-52: a waiting phone changed from A to B, then approved, finds B and never A, with no
/// held digest of A left; an approval naming the time before the change is refused as number
/// changed and B stays unfound.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_only_the_number_the_owner_saw_is_approved(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0xfc).await;
    let line = link(&mut db, &dir, t.biller, "928-555-0133", 2).await;
    change_phone_at(&mut db, &dir, (t.biller, line), &n("928-555-0134"), at(5))
        .await
        .expect("change");
    let stale = approve_phone(&mut db, t.owner, (line, at(2)), at(6)).await;
    assert_eq!(stale, Err(TeamError::NumberChanged));
    assert_eq!(found(&mut db, &dir, "928-555-0134").await, None);
    assert_eq!(count(&mut db, APPROVED_AUDIT, line).await, 0);
    approve_phone(&mut db, t.owner, (line, at(5)), at(7))
        .await
        .expect("approve");
    assert_eq!(found(&mut db, &dir, "928-555-0134").await, Some(t.business));
    assert_eq!(found(&mut db, &dir, "928-555-0133").await, None);
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.sender_held")
        .fetch_one(&mut *db)
        .await
        .expect("held");
    assert_eq!(held, 0);
}

/// TD-BIZ-52: removing a member drops the held digests of their waiting phones, and no table or
/// audit row holds a whole number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_removal_drops_held_digests_and_no_number_is_stored(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0x1e0).await;
    let line = link(&mut db, &dir, t.both, "928-555-0135", 2).await;
    assert_eq!(count(&mut db, HELD, line).await, 1);
    remove_member(&mut db, t.owner, t.both.member, at(3))
        .await
        .expect("remove");
    assert_eq!(count(&mut db, HELD, line).await, 0);
    assert_eq!(found(&mut db, &dir, "928-555-0135").await, None);
    let relinked = link(&mut db, &dir, t.biller, "928-555-0135", 4).await;
    assert_eq!(count(&mut db, HELD, relinked).await, 1);
    let dump: String = sqlx::query_scalar(
        "SELECT concat((SELECT json_agg(p)::text FROM businesses.staff_phone p), \
         (SELECT json_agg(h)::text FROM messaging.sender_held h), \
         (SELECT json_agg(a)::text FROM ops.audit_log a))",
    )
    .fetch_one(&mut *db)
    .await
    .expect("dump");
    assert!(dump.contains("\"0135\""));
    assert!(!dump.contains("5550135") && !dump.contains("555-0135"));
}

/// TD-BIZ-52, D-3640: an owner-linked phone a "Confirm and billing" member changed to a number
/// that waits is dropped when that member is removed: its held digest is gone and the owner can
/// no longer approve it, so the removed member's number never works.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_52_removal_drops_a_waiting_number_the_member_set(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let t = team(&mut db, 0x200).await;
    let line = link(&mut db, &dir, t.owner, "928-555-0142", 2).await;
    change_phone_at(&mut db, &dir, (t.biller, line), &n("928-555-0143"), at(3))
        .await
        .expect("member change");
    assert_eq!(count(&mut db, HELD, line).await, 1);
    remove_member(&mut db, t.owner, t.biller.member, at(4))
        .await
        .expect("remove");
    assert_eq!(count(&mut db, HELD, line).await, 0);
    let refused = approve_phone(&mut db, t.owner, (line, at(3)), at(5)).await;
    assert_eq!(refused, Err(TeamError::UnknownPhone));
    assert_eq!(found(&mut db, &dir, "928-555-0143").await, None);
    assert_eq!(count(&mut db, APPROVED_AUDIT, line).await, 0);
    assert!(
        waiting_phones(&mut db, t.business)
            .await
            .expect("w")
            .is_empty()
    );
}

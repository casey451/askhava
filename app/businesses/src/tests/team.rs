//! Team members, roles, staff phones and the notice-address lookup (TD-BIZ-34: BIZ-023,
//! CALL-012; D-492, D-964), on a fresh database with fake IDs only.
//!
//! The sender directory here is a stand-in [`SenderLink`] that accepts every number; the real one
//! (`messaging::api::sender_add` with the vault) is exercised by
//! `tests/integration/tests/sender_directory_sync.rs`.

use askhava_ops::api::{EXPORT_PAGE_MAX, export_audit};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::api::{
    Acting, NoticePurpose, Roles, SenderLink, TeamError, add_member, add_owner, change_phone,
    create, link_phone, members, notice_addresses, remove_member, set_roles, unlink_phone,
};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

struct AnyNumber;

impl SenderLink for AnyNumber {
    async fn link(
        &self,
        _db: &mut PgConnection,
        _business_line: (Uuid, Uuid),
        _number: &SecretString,
    ) -> Result<(), TeamError> {
        Ok(())
    }
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

fn number() -> SecretString {
    SecretString::from("928-555-0170".to_owned())
}

fn cards() -> Roles {
    Roles::new(true, false).expect("cards")
}

fn confirm() -> Roles {
    Roles::new(false, true).expect("confirm")
}

async fn business_with_owner(db: &mut Db) -> (Uuid, Acting) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(0xa0), at(1))
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    (business, by)
}

fn acting(business: Uuid, member: Uuid) -> Acting {
    Acting { business, member }
}

async fn audit_rows(db: &mut Db, id: Uuid) -> usize {
    let rows = export_audit(db, 0, EXPORT_PAGE_MAX).await.expect("audit");
    rows.iter()
        .filter(|r| r.subject_kind == "businesses.team_member" && r.subject_id == id)
        .count()
}

async fn linked_by(db: &mut Db, member: Uuid) -> i64 {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.staff_phone WHERE linked_by = $1",
        member
    )
    .fetch_one(&mut **db)
    .await
    .expect("count")
}

/// TD-BIZ-34 (BIZ-023): a member's roles are one or both, never neither; the owner holds both.
#[test]
fn td_biz_34_roles_are_one_or_both() {
    assert_eq!(Roles::new(false, false), None);
    assert_eq!(Roles::new(true, true), Some(Roles::BOTH));
    assert!(cards().cards_and_quotes() && !cards().confirm_and_billing());
    assert!(confirm().confirm_and_billing() && !confirm().cards_and_quotes());
}

/// D-492, D-964: cards go to "Cards and quotes", picks, release and money to "Confirm and
/// billing", account notices to the owner alone.
#[test]
fn notice_purposes_fit_their_roles() {
    use NoticePurpose::{Account, Cards, Money, PicksAndRelease};
    for (roles, fits) in [
        (cards(), [true, false, false, false]),
        (confirm(), [false, true, false, true]),
        (Roles::BOTH, [true, true, false, true]),
    ] {
        let got = [Cards, PicksAndRelease, Account, Money].map(|p| roles.fits(p));
        assert_eq!(got, fits, "{roles:?}");
    }
}

/// TD-BIZ-34 (BIZ-023): adding, changing and removing a member write one audit row each; an
/// unchanged role set writes none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_each_member_change_writes_one_audit_row(mut db: Db) {
    let (business, by) = business_with_owner(&mut db).await;
    let member = add_member(&mut db, by, Uuid::from_u128(0xa1), cards(), at(2))
        .await
        .expect("add");
    assert_eq!(audit_rows(&mut db, member).await, 1);
    set_roles(&mut db, by, member, Roles::BOTH)
        .await
        .expect("change");
    assert_eq!(audit_rows(&mut db, member).await, 2);
    set_roles(&mut db, by, member, Roles::BOTH)
        .await
        .expect("same");
    assert_eq!(audit_rows(&mut db, member).await, 2);
    remove_member(&mut db, by, member, at(3))
        .await
        .expect("remove");
    assert_eq!(audit_rows(&mut db, member).await, 3);
    assert_eq!(members(&mut db, business).await.expect("members").len(), 1);
}

/// TD-BIZ-34 (BIZ-023): the owner holds both roles, can't drop either and is never removed; a
/// business has one owner, and the table refuses an owner with one role even to direct SQL.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_the_owner_always_holds_both_roles(mut db: Db) {
    let (business, by) = business_with_owner(&mut db).await;
    let owner = by.member;
    for roles in [cards(), confirm()] {
        assert_eq!(
            set_roles(&mut db, by, owner, roles).await,
            Err(TeamError::OwnerKeepsBoth)
        );
    }
    assert_eq!(
        remove_member(&mut db, by, owner, at(2)).await,
        Err(TeamError::OwnerKeepsBoth)
    );
    let second = add_owner(&mut db, business, Uuid::from_u128(0xa9), at(2)).await;
    assert_eq!(second, Err(TeamError::OwnerExists));
    let direct = sqlx::query!(
        "UPDATE businesses.team_member SET cards_and_quotes = false WHERE id = $1",
        owner
    )
    .execute(&mut *db)
    .await;
    assert!(direct.is_err(), "an owner with one role was stored");
    let m = members(&mut db, business).await.expect("members");
    assert!(m[0].owner && m[0].roles == Roles::BOTH);
}

/// TD-BIZ-34 (BIZ-023, CALL-012): removing a member who linked two staff phones unlinks both, and
/// a retried removal is refused; the one end-sessions event is counted in
/// `tests/integration/tests/team_members.rs`, since this crate may not read `core.task`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_removal_unlinks_both_phones(mut db: Db) {
    let (business, by) = business_with_owner(&mut db).await;
    let member = add_member(&mut db, by, Uuid::from_u128(0xa2), confirm(), at(2))
        .await
        .expect("add");
    let biller = acting(business, member);
    for minute in [3, 4] {
        link_phone(&mut db, &AnyNumber, (biller, false), &number(), at(minute))
            .await
            .expect("link");
    }
    assert_eq!(linked_by(&mut db, member).await, 2);
    remove_member(&mut db, by, member, at(5))
        .await
        .expect("remove");
    assert_eq!(
        remove_member(&mut db, by, member, at(6)).await,
        Err(TeamError::UnknownMember)
    );
    assert_eq!(linked_by(&mut db, member).await, 0);
}

/// CALL-012: only the owner or a "Confirm and billing" member links a phone, only the owner the
/// main phone, and a business has one main phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_012_who_may_link_a_phone(mut db: Db) {
    let (business, owner) = business_with_owner(&mut db).await;
    let carder = add_member(&mut db, owner, Uuid::from_u128(0xa3), cards(), at(2))
        .await
        .expect("cards");
    let biller = add_member(&mut db, owner, Uuid::from_u128(0xa4), confirm(), at(2))
        .await
        .expect("confirm");
    let (carder, biller) = (acting(business, carder), acting(business, biller));
    let n = number();
    let refused = link_phone(&mut db, &AnyNumber, (carder, false), &n, at(3)).await;
    assert_eq!(refused, Err(TeamError::NotAllowed));
    let refused = link_phone(&mut db, &AnyNumber, (biller, true), &n, at(3)).await;
    assert_eq!(refused, Err(TeamError::NotAllowed));
    link_phone(&mut db, &AnyNumber, (owner, true), &n, at(3))
        .await
        .expect("main");
    let again = link_phone(&mut db, &AnyNumber, (owner, true), &n, at(4)).await;
    assert_eq!(again, Err(TeamError::MainExists));
}

/// D-492: a notice goes to the owner plus the current members whose roles fit its purpose.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn notice_addresses_are_the_owner_and_fitting_members(mut db: Db) {
    let (business, by) = business_with_owner(&mut db).await;
    let mailbox = Uuid::from_u128;
    for (n, roles) in [(0xb1, cards()), (0xb2, confirm()), (0xb3, Roles::BOTH)] {
        add_member(&mut db, by, mailbox(n), roles, at(2))
            .await
            .expect("add");
    }
    let gone = add_member(&mut db, by, mailbox(0xb4), Roles::BOTH, at(2))
        .await
        .expect("add");
    remove_member(&mut db, by, gone, at(3))
        .await
        .expect("remove");
    let owner = mailbox(0xa0);
    for (purpose, want) in [
        (
            NoticePurpose::Cards,
            vec![owner, mailbox(0xb1), mailbox(0xb3)],
        ),
        (
            NoticePurpose::PicksAndRelease,
            vec![owner, mailbox(0xb2), mailbox(0xb3)],
        ),
        (
            NoticePurpose::Money,
            vec![owner, mailbox(0xb2), mailbox(0xb3)],
        ),
        (NoticePurpose::Account, vec![owner]),
    ] {
        let mut got = notice_addresses(&mut db, business, purpose)
            .await
            .expect("lookup");
        got[1..].sort();
        assert_eq!(got, want, "{purpose:?}");
    }
}

/// BIZ-023: only the owner adds, changes or removes a member; a member, even with both roles, is
/// refused and nothing changes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_023_only_the_owner_manages_members(mut db: Db) {
    let (business, by) = business_with_owner(&mut db).await;
    let both = add_member(&mut db, by, Uuid::from_u128(0xa5), Roles::BOTH, at(2))
        .await
        .expect("add");
    let other = add_member(&mut db, by, Uuid::from_u128(0xa6), cards(), at(2))
        .await
        .expect("add");
    let member = acting(business, both);
    let added = add_member(&mut db, member, Uuid::from_u128(0xa7), cards(), at(3)).await;
    assert_eq!(added, Err(TeamError::NotAllowed));
    let changed = set_roles(&mut db, member, other, Roles::BOTH).await;
    assert_eq!(changed, Err(TeamError::NotAllowed));
    let removed = remove_member(&mut db, member, other, at(3)).await;
    assert_eq!(removed, Err(TeamError::NotAllowed));
    let self_change = set_roles(&mut db, member, both, cards()).await;
    assert_eq!(self_change, Err(TeamError::NotAllowed));
    assert_eq!(audit_rows(&mut db, other).await, 1);
    let m = members(&mut db, business).await.expect("members");
    assert_eq!(m.len(), 3);
    assert!(m.iter().all(|m| m.id != other || m.roles == cards()));
}

/// BIZ-023, CALL-012: one business's owner never reaches another business's members or phones,
/// whether it names its own business or the other one, and the table refuses a phone linked by
/// another business's member even to direct SQL.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn one_business_cannot_touch_anothers_team(mut db: Db) {
    let (a, owner_a) = business_with_owner(&mut db).await;
    let member = add_member(&mut db, owner_a, Uuid::from_u128(0xa8), confirm(), at(2))
        .await
        .expect("add");
    let line = link_phone(&mut db, &AnyNumber, (owner_a, false), &number(), at(2))
        .await
        .expect("link");
    let (_, owner_b) = business_with_owner(&mut db).await;
    let posing = acting(a, owner_b.member);
    for by in [owner_b, posing] {
        let want = if by == posing {
            TeamError::NotAllowed
        } else {
            TeamError::UnknownMember
        };
        assert_eq!(set_roles(&mut db, by, member, cards()).await, Err(want));
        assert_eq!(remove_member(&mut db, by, member, at(3)).await, Err(want));
        let want = if by == posing {
            TeamError::NotAllowed
        } else {
            TeamError::UnknownPhone
        };
        let changed = change_phone(&mut db, &AnyNumber, (by, line), &number()).await;
        assert_eq!(changed, Err(want));
        assert_eq!(unlink_phone(&mut db, by, line).await, Err(want));
    }
    let added = add_member(&mut db, posing, Uuid::from_u128(0xa9), cards(), at(3)).await;
    assert_eq!(added, Err(TeamError::NotAllowed));
    let linked = link_phone(&mut db, &AnyNumber, (posing, false), &number(), at(3)).await;
    assert_eq!(linked, Err(TeamError::NotAllowed));
    assert_eq!(members(&mut db, a).await.expect("members").len(), 2);
    assert_eq!(linked_by(&mut db, owner_a.member).await, 1);
    let foreign = sqlx::query!(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, linked_at) \
         VALUES ($1, $2, now())",
        a,
        owner_b.member
    )
    .execute(&mut *db)
    .await;
    assert!(
        foreign.is_err(),
        "a phone linked by another business's member was stored"
    );
}

/// CALL-012: a "Cards and quotes" member may not change or unlink a phone; a "Confirm and
/// billing" member may handle a staff phone the owner linked but not the main phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn call_012_who_may_change_or_unlink_a_phone(mut db: Db) {
    let (business, owner) = business_with_owner(&mut db).await;
    let carder = add_member(&mut db, owner, Uuid::from_u128(0xaa), cards(), at(2))
        .await
        .expect("cards");
    let biller = add_member(&mut db, owner, Uuid::from_u128(0xab), confirm(), at(2))
        .await
        .expect("confirm");
    let (carder, biller) = (acting(business, carder), acting(business, biller));
    let n = number();
    let main = link_phone(&mut db, &AnyNumber, (owner, true), &n, at(3))
        .await
        .expect("main");
    let staff = link_phone(&mut db, &AnyNumber, (owner, false), &n, at(3))
        .await
        .expect("staff");
    for line in [main, staff] {
        let changed = change_phone(&mut db, &AnyNumber, (carder, line), &n).await;
        assert_eq!(changed, Err(TeamError::NotAllowed));
        assert_eq!(
            unlink_phone(&mut db, carder, line).await,
            Err(TeamError::NotAllowed)
        );
    }
    let changed = change_phone(&mut db, &AnyNumber, (biller, main), &n).await;
    assert_eq!(changed, Err(TeamError::NotAllowed));
    assert_eq!(
        unlink_phone(&mut db, biller, main).await,
        Err(TeamError::NotAllowed)
    );
    change_phone(&mut db, &AnyNumber, (biller, staff), &n)
        .await
        .expect("change staff");
    unlink_phone(&mut db, biller, staff)
        .await
        .expect("unlink staff");
    change_phone(&mut db, &AnyNumber, (owner, main), &n)
        .await
        .expect("change main");
    unlink_phone(&mut db, owner, main)
        .await
        .expect("unlink main");
}

/// BIZ-023: a member row is never deleted by the app; removal is `removed_at`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_app_role_cannot_delete_a_team_member(mut db: Db) {
    business_with_owner(&mut db).await;
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let deleted = sqlx::query!("DELETE FROM businesses.team_member")
        .execute(&mut *db)
        .await;
    assert!(deleted.is_err());
}

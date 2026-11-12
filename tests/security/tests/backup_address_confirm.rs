//! A backup notice address is never a person's main address, also when a mailbox changes role
//! between steps: an address that became a member's main address after the backup request is
//! refused at confirmation; a mailbox that is a current member's verified backup cannot be added
//! as a member (D-3174); and `notice_addresses` lists each mailbox once (TD-BIZ-44: BIZ-043;
//! T322, D-2209). All IDs are fake.

use askhava_businesses::api::{
    Acting, EmailCheckError, NoticePurpose, Roles, TeamError, add_member, add_owner,
    confirm_backup, create, notice_addresses, record_step_up, remove_backup, remove_member,
    request_backup,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: Uuid = Uuid::from_u128(0xe1);
const BILLING_BOX: Uuid = Uuid::from_u128(0xe2);
const SPARE_BOX: Uuid = Uuid::from_u128(0xe3);
const SHARED_BACKUP: Uuid = Uuid::from_u128(0xf1);
const OTHER_BACKUP: Uuid = Uuid::from_u128(0xf2);

fn secret() -> SecretString {
    SecretString::from("fake-backup-confirm-secret-for-tests-only-0123".to_owned())
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

fn billing_roles() -> Roles {
    Roles::new(false, true).expect("roles")
}

/// The owner and a "Confirm and billing" member, each with a fresh Verify code at 0.
async fn team(db: &mut PgConnection) -> [Acting; 2] {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_BOX, at(0))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let billing = add_member(db, owner, BILLING_BOX, billing_roles(), at(0)).await;
    let billing = Acting {
        business,
        member: billing.expect("member"),
    };
    for p in [owner, billing] {
        record_step_up(db, p, at(0)).await.expect("code");
    }
    [owner, billing]
}

async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

async fn add(db: &mut PgConnection, by: Acting, mailbox: Uuid, t: i64) {
    let s = secret();
    let code = request_backup(db, &s, by, (by.member, mailbox), at(t)).await;
    let code = code.expect("code");
    let taken = confirm_backup(db, &s, (by.business, by.member), &code, at(t)).await;
    assert!(taken.expect("confirm"));
}

/// TD-BIZ-44 (BIZ-043): a mailbox that became a member's main address between the backup request
/// and its confirmation is refused at confirmation with the right code; nothing is verified,
/// logged or told, and no notice lists it twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_44_a_new_main_address_is_refused_at_confirmation(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let [owner, _] = team(&mut db).await;
    let code = request_backup(&mut db, &s, owner, (owner.member, SPARE_BOX), at(1)).await;
    let code = code.expect("not yet anyone's main address");
    add_member(&mut db, owner, SPARE_BOX, billing_roles(), at(2))
        .await
        .expect("a pending request does not block a member");
    let late = confirm_backup(&mut db, &s, (owner.business, owner.member), &code, at(3)).await;
    assert_eq!(late, Err(EmailCheckError::NotAllowed));
    let verified = "SELECT count(*) FROM businesses.backup_address WHERE verified_at IS NOT NULL";
    assert_eq!(count(&mut db, verified).await, 0);
    let logged = "SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.backup_changed'";
    assert_eq!(count(&mut db, logged).await, 0);
    let told = "SELECT count(*) FROM core.task WHERE kind = 'businesses.backup_changed'";
    assert_eq!(count(&mut db, told).await, 0);
    let picks = notice_addresses(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(
        picks.expect("picks"),
        vec![OWNER_BOX, BILLING_BOX, SPARE_BOX]
    );
}

/// TD-BIZ-44 (BIZ-043, D-3174): adding a member whose mailbox is another person's verified backup
/// is refused and changes nothing; once that backup is removed (or its person is no longer a
/// member) the same mailbox may be added.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_44_a_verified_backup_cannot_become_a_member(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing] = team(&mut db).await;
    add(&mut db, billing, SPARE_BOX, 1).await;
    let refused = add_member(&mut db, owner, SPARE_BOX, billing_roles(), at(2)).await;
    assert_eq!(refused, Err(TeamError::MailboxIsBackup));
    let members = "SELECT count(*) FROM businesses.team_member";
    assert_eq!(count(&mut db, members).await, 2, "no member added");
    let picks = notice_addresses(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(
        picks.expect("picks"),
        vec![OWNER_BOX, BILLING_BOX, SPARE_BOX]
    );
    remove_backup(&mut db, billing, billing.member, at(3))
        .await
        .expect("removed");
    add_member(&mut db, owner, SPARE_BOX, billing_roles(), at(4))
        .await
        .expect("no backup left");
    add(&mut db, billing, SHARED_BACKUP, 5).await;
    remove_member(&mut db, owner, billing.member, at(6))
        .await
        .expect("remove");
    add_member(&mut db, owner, SHARED_BACKUP, billing_roles(), at(7))
        .await
        .expect("a removed member's backup does not block");
    add(&mut db, owner, OTHER_BACKUP, 8).await;
    let other = create(&mut db, false, at(9)).await.expect("business");
    let boss = add_owner(&mut db, other, Uuid::from_u128(0xe9), at(9)).await;
    let boss = Acting {
        business: other,
        member: boss.expect("owner"),
    };
    add_member(&mut db, boss, OTHER_BACKUP, billing_roles(), at(10))
        .await
        .expect("another business's backup does not block");
}

/// TD-BIZ-44 (BIZ-043): two people with the same verified backup put it on a notice once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_44_notice_addresses_lists_each_mailbox_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing] = team(&mut db).await;
    add(&mut db, owner, SHARED_BACKUP, 1).await;
    add(&mut db, billing, SHARED_BACKUP, 2).await;
    let picks = notice_addresses(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(
        picks.expect("picks"),
        vec![OWNER_BOX, SHARED_BACKUP, BILLING_BOX]
    );
    let money = notice_addresses(&mut db, owner.business, NoticePurpose::Money).await;
    let money = money.expect("money");
    assert_eq!(money.iter().filter(|&&m| m == SHARED_BACKUP).count(), 1);
}

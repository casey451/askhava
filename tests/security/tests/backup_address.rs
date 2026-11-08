//! Each person's backup notice address: verified before it gets anything, then every notice that
//! person gets but account notices, the release only for the owner or "Confirm and billing", only
//! that person or the owner may change it and only with a fresh Verify code, the main address is
//! told and every change is audit-logged (TD-BIZ-19, TD-SEC-17: BIZ-043, SEC-024, SEC-038). All IDs
//! are fake.

use askhava_businesses::api::{
    Acting, BACKUP_CHANGED_KIND, EmailCheckError, NoticePurpose, Roles, add_member, add_owner,
    backup_changed_words, confirm_backup, create, notice_addresses, record_step_up, remove_backup,
    remove_member, request_backup,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: Uuid = Uuid::from_u128(0xc1);
const BILLING_BOX: Uuid = Uuid::from_u128(0xc2);
const CARDS_BOX: Uuid = Uuid::from_u128(0xc3);
const OWNER_BACKUP: Uuid = Uuid::from_u128(0xd1);
const BILLING_BACKUP: Uuid = Uuid::from_u128(0xd2);
const CARDS_BACKUP: Uuid = Uuid::from_u128(0xd3);

fn secret() -> SecretString {
    SecretString::from("fake-backup-secret-for-tests-only-0123456789".to_owned())
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// The owner, a "Confirm and billing" member and a "Cards and quotes" member, each with a fresh
/// Verify code at 0.
async fn team(db: &mut PgConnection) -> [Acting; 3] {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_BOX, at(0))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let mut people = vec![owner];
    for (mailbox, roles) in [(BILLING_BOX, (false, true)), (CARDS_BOX, (true, false))] {
        let roles = Roles::new(roles.0, roles.1).expect("roles");
        let member = add_member(db, owner, mailbox, roles, at(0)).await;
        people.push(Acting {
            business,
            member: member.expect("member"),
        });
    }
    for p in &people {
        record_step_up(db, *p, at(0)).await.expect("code");
    }
    [people[0], people[1], people[2]]
}

async fn audits(db: &mut PgConnection, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(db)
        .await
        .expect("audit rows")
}

async fn told(db: &mut PgConnection, main: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 \
         AND payload->'to' = jsonb_build_array($2::text)",
    )
    .bind(BACKUP_CHANGED_KIND)
    .bind(main.to_string())
    .fetch_one(db)
    .await
    .expect("tasks")
}

async fn add(db: &mut PgConnection, by: Acting, mailbox: Uuid, t: i64) {
    let s = secret();
    let code = request_backup(db, &s, by, (by.member, mailbox), at(t)).await;
    let code = code.expect("code");
    let taken = confirm_backup(db, &s, (by.business, by.member), &code, at(t)).await;
    assert!(taken.expect("confirm"));
}

async fn to(db: &mut PgConnection, business: Uuid, purpose: NoticePurpose) -> Vec<Uuid> {
    notice_addresses(db, business, purpose)
        .await
        .expect("addresses")
}

/// TD-SEC-17 (SEC-038, D-492): a backup change with no Verify code in the last 5 minutes is
/// refused and logged; a 4-minute-old code works; at 5:01 it is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_17_a_backup_change_needs_a_code_from_the_last_five_minutes(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let [owner, ..] = team(&mut db).await;
    let change = (owner.member, OWNER_BACKUP);
    let late = request_backup(&mut db, &s, owner, change, at(301)).await;
    assert_eq!(late.err(), Some(EmailCheckError::NoFreshCode), "5:01");
    assert_eq!(audits(&mut db, "businesses.backup_change_refused").await, 1);
    let future = request_backup(&mut db, &s, owner, change, at(-1)).await;
    assert_eq!(future.err(), Some(EmailCheckError::NoFreshCode));
    let ok = request_backup(&mut db, &s, owner, change, at(240)).await;
    assert_eq!(ok.expect("4 minutes is fresh").len(), 6);
    let gone = remove_backup(&mut db, owner, owner.member, at(400)).await;
    assert_eq!(gone.err(), Some(EmailCheckError::NoFreshCode));
}

/// TD-BIZ-19 (BIZ-043): nothing until verified; then the owner's and "Confirm and billing"
/// backups get the pick at the same time as the main addresses, a "Cards and quotes" backup gets
/// cards but never a release, account notices go to the main address alone, and the main
/// address is told of the change, which is audit-logged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_19_a_backup_gets_that_persons_notices_once_verified(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let [owner, billing, cards] = team(&mut db).await;
    let business = owner.business;
    let code = request_backup(&mut db, &s, owner, (owner.member, OWNER_BACKUP), at(10)).await;
    let code = code.expect("code");
    let picks = to(&mut db, business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(
        picks,
        vec![OWNER_BOX, BILLING_BOX],
        "nothing before the code"
    );
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let bad = confirm_backup(&mut db, &s, (business, owner.member), wrong, at(11)).await;
    assert!(!bad.expect("wrong code"));
    let ok = confirm_backup(&mut db, &s, (business, owner.member), &code, at(12)).await;
    assert!(ok.expect("right code"));
    add(&mut db, billing, BILLING_BACKUP, 20).await;
    add(&mut db, cards, CARDS_BACKUP, 30).await;
    let picks = to(&mut db, business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(
        picks,
        vec![OWNER_BOX, OWNER_BACKUP, BILLING_BOX, BILLING_BACKUP]
    );
    assert!(
        !picks.contains(&CARDS_BACKUP),
        "no release to a Cards and quotes backup"
    );
    let card_list = to(&mut db, business, NoticePurpose::Cards).await;
    assert_eq!(
        card_list,
        vec![OWNER_BOX, OWNER_BACKUP, CARDS_BOX, CARDS_BACKUP]
    );
    let money = to(&mut db, business, NoticePurpose::Money).await;
    assert!(money.contains(&BILLING_BACKUP));
    let account = to(&mut db, business, NoticePurpose::Account).await;
    assert_eq!(
        account,
        vec![OWNER_BOX],
        "account notices go to the main address only"
    );
    assert_eq!(
        told(&mut db, OWNER_BOX).await,
        1,
        "the main address is told"
    );
    assert_eq!(audits(&mut db, "businesses.backup_changed").await, 3);
    let [subject, body] = backup_changed_words(false);
    assert!(subject.ends_with("Your backup address changed") && body.contains("reply"));
    assert!(backup_changed_words(true)[0].contains("respaldo"));
}

/// TD-BIZ-19 (BIZ-043, SEC-024): another member's session cannot add or change a person's backup
/// (refused and logged); the owner's can; a change keeps the old backup until the new is verified.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_19_only_that_person_or_the_owner_changes_it(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let [owner, billing, cards] = team(&mut db).await;
    let theirs = request_backup(&mut db, &s, cards, (billing.member, CARDS_BACKUP), at(1)).await;
    assert_eq!(theirs.err(), Some(EmailCheckError::NotAllowed));
    let remove = remove_backup(&mut db, cards, billing.member, at(1)).await;
    assert_eq!(remove.err(), Some(EmailCheckError::NotAllowed));
    assert_eq!(audits(&mut db, "businesses.backup_change_refused").await, 2);
    let code = request_backup(&mut db, &s, owner, (billing.member, BILLING_BACKUP), at(2)).await;
    let code = code.expect("code");
    let taken = confirm_backup(&mut db, &s, (owner.business, billing.member), &code, at(3)).await;
    assert!(taken.expect("owner's change"));
    let next = request_backup(&mut db, &s, billing, (billing.member, OWNER_BACKUP), at(4)).await;
    assert!(next.is_ok(), "a member changes their own backup");
    let picks = to(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert!(
        picks.contains(&BILLING_BACKUP),
        "the old backup works until the new one is verified"
    );
    assert!(!picks.contains(&OWNER_BACKUP));
    assert_eq!(told(&mut db, BILLING_BOX).await, 1);
}

/// TD-BIZ-19 (BIZ-043, SEC-024): removing the backup or the member stops every notice to it; a
/// removal tells the main address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_19_removing_it_or_the_member_stops_it(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let [owner, billing, cards] = team(&mut db).await;
    add(&mut db, owner, OWNER_BACKUP, 1).await;
    add(&mut db, billing, BILLING_BACKUP, 2).await;
    let removed = remove_backup(&mut db, owner, owner.member, at(3)).await;
    assert!(removed.expect("removed"));
    assert_eq!(told(&mut db, OWNER_BOX).await, 2, "added, then removed");
    assert_eq!(audits(&mut db, "businesses.backup_removed").await, 1);
    let again = remove_backup(&mut db, cards, cards.member, at(3)).await;
    assert!(!again.expect("nothing to remove"));
    remove_member(&mut db, owner, billing.member, at(4))
        .await
        .expect("remove");
    let picks = to(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(picks, vec![OWNER_BOX], "no backup of a removed member");
    let s = secret();
    let late = confirm_backup(
        &mut db,
        &s,
        (owner.business, billing.member),
        "123456",
        at(5),
    )
    .await;
    assert!(!late.expect("no open request"));
}

/// TD-BIZ-19 (BIZ-043): a backup may not be the person's own main address or any current
/// member's main address, so no notice lists one mailbox twice; nothing is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_19_a_backup_is_never_a_main_address(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let [owner, billing, cards] = team(&mut db).await;
    for (by, mailbox) in [
        (owner, OWNER_BOX),
        (billing, OWNER_BOX),
        (cards, BILLING_BOX),
    ] {
        let r = request_backup(&mut db, &s, by, (by.member, mailbox), at(1)).await;
        assert_eq!(r.err(), Some(EmailCheckError::NotAllowed), "{mailbox}");
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM businesses.backup_address")
        .fetch_one(&mut *db)
        .await
        .expect("rows");
    assert_eq!(rows, 0);
    let picks = to(&mut db, owner.business, NoticePurpose::PicksAndRelease).await;
    assert_eq!(picks, vec![OWNER_BOX, BILLING_BOX]);
}

//! `messaging`'s sender directory follows every main and staff phone `businesses` adds, changes or
//! removes (TD-BIZ-34: BIZ-023, CALL-012; D-491, D-963).
//!
//! The directory link here is the one the composition uses: `messaging::api::sender_add` with the
//! vault. A refused number leaves no phone row behind, since the link runs in the same
//! transaction. All numbers and IDs are fake.

use askhava_businesses::api::{
    Roles, SenderLink, TeamError, add_member, add_owner, change_phone, create, link_phone,
    remove_member, unlink_phone,
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
    Directory(Vault::new(&key(0x52), &key(0x62), &url).expect("vault"))
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

fn n(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

async fn found(db: &mut PgConnection, dir: &Directory, number: &str) -> Option<Uuid> {
    sender_business(db, &dir.0, number).await.expect("lookup")
}

async fn owned_business(db: &mut PgConnection) -> (Uuid, Uuid) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(0xc0), at(1))
        .await
        .expect("owner");
    (business, owner)
}

/// TD-BIZ-34 (D-491): a main phone added, changed or removed is in or out of the directory at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_the_main_phone_follows_into_the_directory(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let (business, owner) = owned_business(&mut db).await;
    let main = link_phone(&mut db, &dir, (owner, true), &n("928-555-0180"), at(2))
        .await
        .expect("link");
    assert_eq!(found(&mut db, &dir, "(928) 555-0180").await, Some(business));
    change_phone(&mut db, &dir, main, &n("928-555-0181"))
        .await
        .expect("change");
    assert_eq!(found(&mut db, &dir, "928-555-0180").await, None);
    assert_eq!(found(&mut db, &dir, "928-555-0181").await, Some(business));
    unlink_phone(&mut db, main).await.expect("unlink");
    assert_eq!(found(&mut db, &dir, "928-555-0181").await, None);
    assert_eq!(
        unlink_phone(&mut db, main).await,
        Err(TeamError::UnknownPhone)
    );
}

/// TD-BIZ-34 (CALL-012, D-491): a staff phone follows too, and leaves with the member who linked it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_a_staff_phone_follows_into_the_directory(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let (business, _) = owned_business(&mut db).await;
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(&mut db, business, Uuid::from_u128(0xc1), roles, at(2))
        .await
        .expect("member");
    let line = link_phone(&mut db, &dir, (member, false), &n("928-555-0182"), at(3))
        .await
        .expect("link");
    assert_eq!(found(&mut db, &dir, "928-555-0182").await, Some(business));
    change_phone(&mut db, &dir, line, &n("928-555-0183"))
        .await
        .expect("change");
    assert_eq!(found(&mut db, &dir, "928-555-0182").await, None);
    assert_eq!(found(&mut db, &dir, "928-555-0183").await, Some(business));
    remove_member(&mut db, member, at(4)).await.expect("remove");
    assert_eq!(found(&mut db, &dir, "928-555-0183").await, None);
}

/// D-491, D-963: a bad or already linked number is refused and leaves no phone row behind.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refused_number_leaves_no_phone_row(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let (_, owner) = owned_business(&mut db).await;
    let (_, other) = owned_business(&mut db).await;
    link_phone(&mut db, &dir, (owner, false), &n("928-555-0184"), at(2))
        .await
        .expect("link");
    let taken = link_phone(&mut db, &dir, (other, false), &n("9285550184"), at(3)).await;
    assert_eq!(taken, Err(TeamError::PhoneTaken));
    let bad = link_phone(&mut db, &dir, (other, false), &n("555"), at(3)).await;
    assert_eq!(bad, Err(TeamError::BadPhone));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM businesses.staff_phone")
        .fetch_one(&mut *db)
        .await
        .expect("rows");
    assert_eq!(rows, 1);
}

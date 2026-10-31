//! Team members across `businesses`, `ops`' audit log, `core`'s task queue and `messaging`'s
//! sender directory (TD-BIZ-34: BIZ-023, CALL-012; D-191, D-491).
//!
//! Proves on a fresh database that each member change writes one audit row, that the owner keeps
//! both roles, and that removing a member who linked two staff phones takes both out of the
//! directory and queues exactly one end-sessions event (`portals` ends the sessions, TD-BIZ-06).
//! The directory link is the composition's: `messaging::api::sender_add` with the vault. All
//! numbers and IDs are fake.

use askhava_businesses::api::{
    MEMBER_REMOVED_KIND, Roles, SenderLink, TeamError, add_member, add_owner, create, link_phone,
    members, remove_member, set_roles,
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
    Directory(Vault::new(&key(0x53), &key(0x63), &url).expect("vault"))
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

async fn count(db: &mut PgConnection, sql: &'static str, id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(db)
        .await
        .expect("count")
}

async fn found(db: &mut PgConnection, dir: &Directory, number: &str) -> Option<Uuid> {
    sender_business(db, &dir.0, number).await.expect("lookup")
}

const MEMBER_AUDIT: &str = "SELECT count(*) FROM ops.audit_log \
     WHERE subject_kind = 'businesses.team_member' AND subject_id::text = $1";
const REMOVED_EVENTS: &str = "SELECT count(*) FROM core.task \
     WHERE kind = 'businesses.member_removed' AND payload->>'member' = $1";

/// TD-BIZ-34 (BIZ-023, CALL-012, D-491): add, change and remove each write one audit row; the
/// removal unlinks both staff phones the member linked, takes both out of the sender directory
/// and queues one end-sessions event; the owner keeps both roles throughout.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_34_removing_a_member_unlinks_both_phones_once(pool: PgPool) {
    let dir = directory();
    let mut db = pool.acquire().await.expect("connection");
    let business = create(&mut db, false, at(0)).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::from_u128(0xd0), at(1))
        .await
        .expect("owner");
    let cards = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, business, Uuid::from_u128(0xd1), cards, at(2))
        .await
        .expect("add");
    let id = member.to_string();
    set_roles(&mut db, member, Roles::BOTH)
        .await
        .expect("change");
    assert_eq!(count(&mut db, MEMBER_AUDIT, &id).await, 2);
    for (minute, number) in [(3, "928-555-0190"), (4, "928-555-0191")] {
        let number = SecretString::from(number.to_owned());
        link_phone(&mut db, &dir, (member, false), &number, at(minute))
            .await
            .expect("link");
    }
    assert_eq!(found(&mut db, &dir, "928-555-0190").await, Some(business));
    assert_eq!(found(&mut db, &dir, "928-555-0191").await, Some(business));
    remove_member(&mut db, member, at(5)).await.expect("remove");
    let again = remove_member(&mut db, member, at(6)).await;
    assert_eq!(again, Err(TeamError::UnknownMember));
    assert_eq!(found(&mut db, &dir, "928-555-0190").await, None);
    assert_eq!(found(&mut db, &dir, "928-555-0191").await, None);
    assert_eq!(count(&mut db, MEMBER_AUDIT, &id).await, 3);
    assert_eq!(MEMBER_REMOVED_KIND, "businesses.member_removed");
    assert_eq!(count(&mut db, REMOVED_EVENTS, &id).await, 1);
    let left = members(&mut db, business).await.expect("members");
    assert_eq!(left.len(), 1);
    assert!(left[0].id == owner && left[0].roles == Roles::BOTH);
    assert_eq!(
        set_roles(&mut db, owner, cards).await,
        Err(TeamError::OwnerKeepsBoth)
    );
}

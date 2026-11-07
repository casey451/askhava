//! Notice-address and channel changes: owner only, a fresh Verify code, verify-before-use, the old
//! address told, every change audit-logged, and the one askhava.com sender (TD-SEC-07, TD-SEC-17,
//! TD-BIZ-13: SEC-024, SEC-025, SEC-038, BIZ-036, BIZ-040). All IDs are fake.

use askhava_businesses::api::{
    Acting, CHANGED_KIND, EmailCheckError, NoticeChannel, Returned, Roles, add_member, add_owner,
    confirm_mailbox, create, mark_returned, may_get_notices, record_step_up, request_notice_change,
    verified_by_link,
};
use askhava_messaging::api::FROM;
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, ChannelChangeError, ChannelFacts, CookieJar,
    NoticeAddress, Realm, business_principal, change_channel, notice_settings_page, start_session,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: Uuid = Uuid::from_u128(0xb1);
const MEMBER_BOX: Uuid = Uuid::from_u128(0xb2);
const NEW_BOX: Uuid = Uuid::from_u128(0xb3);

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// A business with its owner and one "Cards and quotes" member: (owner, member) as `Acting`.
async fn team(db: &mut PgConnection) -> (Acting, Acting) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_BOX, at(0))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let roles = Roles::new(true, false).expect("roles");
    let member = add_member(db, owner, MEMBER_BOX, roles, at(0))
        .await
        .expect("member");
    (owner, Acting { business, member })
}

async fn audits(db: &mut PgConnection, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(db)
        .await
        .expect("audit rows")
}

/// TD-SEC-17 (SEC-038, D-492): with a live session but no Verify code in the last 5 minutes the
/// change is refused and logged; a 4-minute-old code works; at 5:01 it is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_17_a_change_needs_a_code_from_the_last_five_minutes(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let (owner, _) = team(&mut db).await;
    let change = (owner.member, NEW_BOX);
    let none = request_notice_change(&mut db, &s, owner, change, at(0)).await;
    assert_eq!(none.err(), Some(EmailCheckError::NoFreshCode));
    assert_eq!(audits(&mut db, "businesses.notice_change_refused").await, 1);
    record_step_up(&mut db, owner, at(0)).await.expect("code");
    let late = request_notice_change(&mut db, &s, owner, change, at(301)).await;
    assert_eq!(
        late.err(),
        Some(EmailCheckError::NoFreshCode),
        "5:01 is refused"
    );
    let ok = request_notice_change(&mut db, &s, owner, change, at(240)).await;
    assert!(ok.expect("4 minutes is fresh").is_some());
}

/// TD-SEC-07 (SEC-024, BIZ-040): a member cannot change the owner's address; a changed address
/// gets nothing until verified, then the old one is told and the change is audit-logged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_07_only_the_owner_changes_and_the_new_address_is_verified_first(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let (owner, member) = team(&mut db).await;
    verified_by_link(&mut db, owner, OWNER_BOX, at(0))
        .await
        .expect("link");
    record_step_up(&mut db, member, at(0)).await.expect("code");
    let theirs = request_notice_change(&mut db, &s, member, (owner.member, NEW_BOX), at(1)).await;
    assert_eq!(theirs.err(), Some(EmailCheckError::NotAllowed));
    let own = request_notice_change(
        &mut db,
        &s,
        member,
        (member.member, Uuid::from_u128(0xb4)),
        at(1),
    )
    .await;
    assert!(
        own.expect("own address").is_some(),
        "a member changes their own address"
    );
    record_step_up(&mut db, owner, at(0)).await.expect("code");
    let change = (owner.member, NEW_BOX);
    let code = request_notice_change(&mut db, &s, owner, change, at(2)).await;
    let code = code.expect("owner").expect("code");
    assert!(
        !may_get_notices(&mut db, owner.business, NEW_BOX)
            .await
            .expect("read")
    );
    assert!(
        may_get_notices(&mut db, owner.business, OWNER_BOX)
            .await
            .expect("read")
    );
    let ok = confirm_mailbox(&mut db, &s, (owner.business, NEW_BOX), &code, at(3)).await;
    assert!(ok.expect("confirmed"));
    assert!(
        may_get_notices(&mut db, owner.business, NEW_BOX)
            .await
            .expect("read")
    );
    let told: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND payload->>'old' = $2",
    )
    .bind(CHANGED_KIND)
    .bind(OWNER_BOX.to_string())
    .fetch_one(&mut *db)
    .await
    .expect("tasks");
    assert_eq!(told, 1, "the old address is told");
    assert_eq!(
        audits(&mut db, "businesses.notice_address_changed").await,
        1
    );
    assert_eq!(
        audits(&mut db, "businesses.notice_change_requested").await,
        2
    );
    assert_eq!(audits(&mut db, "businesses.notice_change_refused").await, 1);
}

/// TD-SEC-07 (SEC-025): every business email comes from the one askhava.com sending subdomain.
#[test]
fn td_sec_07_business_email_comes_from_the_sending_subdomain() {
    assert_eq!(FROM, "\"Ask Hava\" <jobs@notify.askhava.com>");
}

async fn principal(db: &mut PgConnection, who: Acting) -> BusinessPrincipal {
    let login = (Realm::Business, who.member);
    let jar = start_session(db, CookieJar::new(), login, BUSINESS_SESSION_LIFE, at(0)).await;
    let jar = jar.expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(who.member)
    .bind(who.business)
    .execute(&mut *db)
    .await
    .expect("business session");
    business_principal(db, &jar, at(1))
        .await
        .expect("read")
        .expect("signed in")
}

/// TD-BIZ-13 (BIZ-036, BIZ-040, SEC-024): the page lists verified and waiting addresses, shows the
/// returned banner, offers only email while texting is off, and only the owner changes it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_13_the_notice_page_and_owner_only_channel(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (owner, member) = team(&mut db).await;
    let (boss, staff) = (
        principal(&mut db, owner).await,
        principal(&mut db, member).await,
    );
    let shown = |a: &str, verified, returned| NoticeAddress {
        shown: a.to_owned(),
        verified,
        returned,
    };
    let list = [
        shown("owner@example.test", true, false),
        shown("new@example.test", false, false),
        shown("old@example.test", false, true),
    ];
    let off = ChannelFacts {
        channel: NoticeChannel::Text,
        texting_on: false,
    };
    let html = notice_settings_page(&boss, &list, off, "tok").expect("page");
    assert!(html.contains("owner@example.test") && html.contains("Not confirmed yet"));
    assert!(
        html.contains("Ask Hava emails to old@example.test are being returned or marked as spam")
    );
    assert!(html.contains("value=\"email\" checked") && !html.contains("value=\"text\""));
    let on = ChannelFacts {
        texting_on: true,
        ..off
    };
    let html = notice_settings_page(&boss, &list, on, "tok").expect("page");
    assert!(html.contains("value=\"both\"") && html.contains("value=\"text\" checked"));
    let html = notice_settings_page(&staff, &list, on, "tok").expect("page");
    assert!(
        !html.contains("<form")
            && html.contains("Only the account owner")
            && html.contains("Now: Text")
    );
    assert!(
        !html.contains("serviceWorker") && !html.contains("Notification"),
        "no push"
    );
    let r = change_channel(&mut db, &staff, on, at(2)).await;
    assert_eq!(r, Err(ChannelChangeError::NotOwner));
    assert_eq!(
        change_channel(&mut db, &boss, off, at(2)).await,
        Err(ChannelChangeError::TextingOff)
    );
    let email = ChannelFacts {
        channel: NoticeChannel::Email,
        ..off
    };
    assert_eq!(change_channel(&mut db, &boss, email, at(2)).await, Ok(()));
}

/// TD-SEC-07, TD-BIZ-13 (BIZ-040): a spam complaint stops every notice to the address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_07_a_complaint_stops_notices(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (owner, _) = team(&mut db).await;
    verified_by_link(&mut db, owner, OWNER_BOX, at(0))
        .await
        .expect("link");
    let to = (owner.business, OWNER_BOX);
    assert!(
        mark_returned(&mut db, to, Returned::Complaint, at(3))
            .await
            .expect("marked")
    );
    assert!(
        !may_get_notices(&mut db, owner.business, OWNER_BOX)
            .await
            .expect("read")
    );
}

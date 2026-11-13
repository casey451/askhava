//! TD-PRIV-29 (BIZ-037, BIZ-040, BIZ-051, SEC-024, PRIV-002, PRIV-016; D-3262, T415): the
//! production phone link and the daily business contact sweep against PostgreSQL. Every record is
//! made through module APIs; mailboxes and numbers are touched at the database's clock, so each
//! sweep runs at a `now` days ahead of it. Fake addresses (`example.com`) and 555-01xx numbers only.

use askhava_businesses::api::{
    Acting, ContactIds, Roles, add_member, add_owner, change_phone, contact_ids_in_use,
    create as create_business, leave, link_phone, record_step_up, remove_member, request_backup,
    request_notice_change, unlink_phone,
};
use askhava_core::api::MIGRATOR;
use askhava_messaging::api::{
    Audience, EmailClass, Language, MailboxKind, NewEmail, Notice, PostalAddress, Recipient,
    queue_email, sender_business,
};
use askhava_privacy::api::{
    BusinessContactError, BusinessPurpose, Vault, mailbox_for, read_business_mailbox,
    read_staff_phone,
};
use askhava_server::business_contacts::{ContactDirectory, sweep_business_contacts};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn days(n: i64) -> Timestamp {
    Timestamp::now() + SignedDuration::from_hours(n * 24)
}

fn number(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// A business with its owner signing in at `owner`; returns the owner's acting handle.
async fn business(db: &mut PgConnection, owner: Uuid, now: Timestamp) -> Acting {
    let business = create_business(db, false, now).await.expect("business");
    let member = add_owner(db, business, owner, now).await.expect("owner");
    Acting { business, member }
}

async fn member(db: &mut PgConnection, by: Acting, mailbox: Uuid, now: Timestamp) -> Uuid {
    add_member(db, by, mailbox, Roles::BOTH, now)
        .await
        .expect("member")
}

async fn mailbox(db: &mut PgConnection, vault: &Vault, address: &str) -> Uuid {
    mailbox_for(db, vault, address).await.expect("mailbox")
}

async fn has_mailbox(db: &mut PgConnection, vault: &Vault, id: Uuid) -> bool {
    match read_business_mailbox(db, vault, BusinessPurpose::SignInLink, id).await {
        Ok(_) => true,
        Err(BusinessContactError::NotFound) => false,
        Err(e) => panic!("read failed: {e}"),
    }
}

async fn phone_of(db: &mut PgConnection, vault: &Vault, line: Uuid) -> Option<String> {
    match read_staff_phone(db, vault, BusinessPurpose::SignInCode, line).await {
        Ok(n) => Some(n.expose_secret().to_owned()),
        Err(BusinessContactError::NotFound) => None,
        Err(e) => panic!("read failed: {e}"),
    }
}

async fn link(db: &mut PgConnection, dir: &ContactDirectory, by: Acting, n: &str) -> Uuid {
    link_phone(db, dir, (by, false), &number(n), Timestamp::now())
        .await
        .expect("linked")
}

/// One unsent business email to `mailbox`, queued at `now`.
async fn queue_to(db: &mut PgConnection, mailbox: Uuid, now: Timestamp) {
    let postal = PostalAddress::new(
        "PO Box 1234, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    )
    .expect("box");
    let to = Recipient {
        id: mailbox,
        audience: Audience::Business,
        language: Language::En,
        manage_url: "https://example.com/b",
    };
    let email = NewEmail {
        template_id: "pick_notice",
        subject: &[Uuid::from_u128(3)],
        class: EmailClass::Notice,
        notice: Notice::StillOpen(1),
    };
    queue_email(db, (&secret(), &postal), &to, &email, now)
        .await
        .expect("queued");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_priv_29_link_writes_the_digest_and_the_sealed_number_together(pool: PgPool) {
    let dir = ContactDirectory::new(vault());
    let vault = vault();
    let mut db = pool.acquire().await.expect("conn");
    let by = business(&mut db, Uuid::new_v4(), Timestamp::now()).await;
    let line = link(&mut db, &dir, by, "928-555-0142").await;
    let found = sender_business(&mut db, &vault, "+19285550142").await;
    assert_eq!(found.expect("lookup"), Some(by.business));
    assert_eq!(
        phone_of(&mut db, &vault, line).await.as_deref(),
        Some("+19285550142")
    );
    change_phone(&mut db, &dir, (by, line), &number("928-555-0143"))
        .await
        .expect("changed");
    let old = sender_business(&mut db, &vault, "+19285550142").await;
    assert_eq!(old.expect("lookup"), None);
    let new = sender_business(&mut db, &vault, "+19285550143").await;
    assert_eq!(new.expect("lookup"), Some(by.business));
    assert_eq!(
        phone_of(&mut db, &vault, line).await.as_deref(),
        Some("+19285550143")
    );
}

/// A business whose owner, member `kept` (with a backup and an open address change) and removed
/// member `gone` (the same, left behind) are all set at `now`; returns the IDs by role.
async fn team_with_leftovers(db: &mut PgConnection, now: Timestamp) -> ([Uuid; 4], [Uuid; 3]) {
    let ids: [Uuid; 7] = std::array::from_fn(|_| Uuid::new_v4());
    let by = business(db, ids[0], now).await;
    record_step_up(db, by, now).await.expect("step-up");
    let kept = member(db, by, ids[1], now).await;
    let gone = member(db, by, ids[4], now).await;
    for (who, backup, new) in [(kept, ids[2], ids[3]), (gone, ids[5], ids[6])] {
        request_backup(db, &secret(), by, (who, backup), now)
            .await
            .expect("backup");
        request_notice_change(db, &secret(), by, (who, new), now)
            .await
            .expect("change");
    }
    remove_member(db, by, gone, now).await.expect("removed");
    ([ids[0], ids[1], ids[2], ids[3]], [ids[4], ids[5], ids[6]])
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_priv_29_in_use_lists_current_contacts_not_a_removed_members_leftovers(pool: PgPool) {
    let dir = ContactDirectory::new(self::vault());
    let mut db = pool.acquire().await.expect("conn");
    let then = Timestamp::now();
    let (current, leftover) = team_with_leftovers(&mut db, then).await;
    let left = business(&mut db, Uuid::new_v4(), then).await;
    let left_line = link(&mut db, &dir, left, "928-555-0150").await;
    leave(&mut db, left.business, then).await.expect("left");
    let by = business(&mut db, Uuid::new_v4(), then).await;
    let line = link(&mut db, &dir, by, "928-555-0151").await;
    let since = then + SignedDuration::from_secs(60);
    let ContactIds { mailboxes, lines } = contact_ids_in_use(&mut db, since).await.expect("ids");
    for id in current {
        assert!(mailboxes.contains(&id), "a current contact is in use");
    }
    for id in leftover {
        assert!(
            !mailboxes.contains(&id),
            "a removed member's leftover is not"
        );
    }
    assert_eq!(lines, vec![line]);
    assert!(!lines.contains(&left_line));
    let before = contact_ids_in_use(&mut db, then).await.expect("ids");
    for id in leftover {
        assert!(before.mailboxes.contains(&id), "stopped at or after since");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_priv_29_the_sweep_keeps_fresh_and_used_contacts_then_forgets_the_rest(pool: PgPool) {
    let dir = ContactDirectory::new(vault());
    let vault = vault();
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let fresh = mailbox(&mut db, &vault, "fresh.signup@example.com").await;
    let owner = mailbox(&mut db, &vault, "owner.one@example.com").await;
    let gone = mailbox(&mut db, &vault, "removed.member@example.com").await;
    let shared = mailbox(&mut db, &vault, "two.teams@example.com").await;
    let mailed = mailbox(&mut db, &vault, "still.mailed@example.com").await;
    let leaver = mailbox(&mut db, &vault, "left.owner@example.com").await;
    let by = business(&mut db, owner, now).await;
    let line = link(&mut db, &dir, by, "928-555-0160").await;
    let unlinked = link(&mut db, &dir, by, "928-555-0161").await;
    unlink_phone(&mut db, by, unlinked).await.expect("unlinked");
    for id in [gone, shared, mailed] {
        let m = member(&mut db, by, id, now).await;
        remove_member(&mut db, by, m, now).await.expect("removed");
    }
    let other = business(&mut db, Uuid::new_v4(), now).await;
    member(&mut db, other, shared, now).await;
    let left = business(&mut db, leaver, now).await;
    let left_line = link(&mut db, &dir, left, "928-555-0162").await;
    leave(&mut db, left.business, now).await.expect("left");

    assert_eq!(sweep_business_contacts(&pool, now).await, Ok(0));
    sweep_business_contacts(&pool, days(3))
        .await
        .expect("swept");
    assert!(
        !has_mailbox(&mut db, &vault, fresh).await,
        "minted, unused, older than 2 days"
    );
    for id in [owner, gone, shared, mailed, leaver] {
        assert!(
            has_mailbox(&mut db, &vault, id).await,
            "in use or stopped under 30 days"
        );
    }
    assert_eq!(phone_of(&mut db, &vault, unlinked).await, None);
    assert!(phone_of(&mut db, &vault, line).await.is_some());

    queue_to(&mut db, mailed, days(29)).await;
    sweep_business_contacts(&pool, days(31))
        .await
        .expect("swept");
    for id in [owner, shared, mailed] {
        assert!(
            has_mailbox(&mut db, &vault, id).await,
            "in use or named by an unsent email"
        );
    }
    for id in [gone, leaver] {
        assert!(
            !has_mailbox(&mut db, &vault, id).await,
            "out of use for 30 days"
        );
    }
    assert!(phone_of(&mut db, &vault, line).await.is_some());
    assert_eq!(phone_of(&mut db, &vault, left_line).await, None);
    assert_eq!(sweep_business_contacts(&pool, days(31)).await, Ok(0));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_priv_29_a_mailbox_touched_in_the_last_two_days_is_kept(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("conn");
    let fresh = mailbox(&mut db, &vault, "new.signup@example.com").await;
    sweep_business_contacts(&pool, days(1))
        .await
        .expect("swept");
    assert!(has_mailbox(&mut db, &vault, fresh).await);
    let again = mailbox(&mut db, &vault, "new.signup@example.com").await;
    assert_eq!(again, fresh);
    let swept = sweep_business_contacts(&pool, days(2) - SignedDuration::from_secs(600)).await;
    assert_eq!(swept, Ok(0));
    assert!(has_mailbox(&mut db, &vault, fresh).await);
}

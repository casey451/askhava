//! Business members' sealed contacts on a fresh database (TD-PRIV-28; SEC-004, BIZ-037, PRIV-002, PRIV-003, PRIV-004, PRIV-016, PRIV-024, PRIV-026, SEC-047; D-3262).
//!
//! Proves, as the app role, that `mailbox_for` gives one mailbox per address however it is cased or
//! wrapped, the one `email_index(.., true)` finds, moving `touched_at` on every call and making one
//! row from two simultaneous calls; that no column, audit row or alert holds a fake value in clear;
//! that `store_staff_phone` replaces a line's number inside the caller's transaction; that each read
//! is audited and counted under `vault_read.server.<purpose>`, refused inside a transaction, and a
//! phone read under any purpose but `SignInCode` refused with one High alert; that `forget_unused`
//! keeps what is in use or recent and is safe to repeat; and that a key rotation leaves every value
//! readable and findable, a duplicate address skipped and reported. The module logs nothing, so no log line can hold a value. All data and
//! keys are fake.

use askhava_integration::set_app_role;
use askhava_privacy::api::{
    BusinessContactError, BusinessPurpose, Vault, email_index, forget_unused, mailbox_for,
    mailboxes_of, read_business_mailbox, read_staff_phone, rotate_business_batch,
    store_staff_phone,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool, Postgres};

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const ADDRESS: &str = "pat.quill@example.com";
const PHONE: &str = "(928) 555-0144";

fn key(version: u16, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn vault(seal: &str, index: &str) -> Vault {
    Vault::new(&secret(seal), &secret(index), &secret(DB_URL)).expect("vault keys")
}

fn v1() -> Vault {
    vault(&key(1, 0x31), &key(1, 0x41))
}

fn v2() -> Vault {
    let seal = format!("{},{}", key(2, 0x32), key(1, 0x31));
    vault(&seal, &format!("{},{}", key(2, 0x42), key(1, 0x41)))
}

async fn app_conn(pool: &PgPool) -> PoolConnection<Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn scalar_i64(conn: &mut PgConnection, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(conn)
        .await
        .expect("count")
}

async fn touched(conn: &mut PgConnection, id: Uuid) -> Timestamp {
    let at: jiff_sqlx::Timestamp =
        sqlx::query_scalar("SELECT touched_at FROM privacy.business_mailbox WHERE id = $1")
            .bind(id)
            .fetch_one(conn)
            .await
            .expect("touched_at");
    at.to_jiff()
}

/// Every text and byte value of the three tables, the audit log and the alerts, lower-cased.
async fn everything_stored(conn: &mut PgConnection) -> String {
    sqlx::query_scalar(
        "SELECT lower(concat_ws('|', \
           (SELECT string_agg(encode(address, 'escape'), '|') FROM privacy.business_mailbox), \
           (SELECT string_agg(encode(digest, 'escape'), '|') FROM privacy.business_mailbox_index), \
           (SELECT string_agg(encode(number, 'escape'), '|') FROM privacy.staff_phone), \
           (SELECT string_agg(a::text, '|') FROM ops.audit_log a), \
           (SELECT string_agg(l::text, '|') FROM ops.alert l)))",
    )
    .fetch_one(conn)
    .await
    .expect("stored values")
}

/// TD-PRIV-28: one address is one mailbox however written, found by the business email index,
/// touched on every call; a value with no `@` stores nothing; nothing holds the address in clear.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_one_address_is_one_mailbox_found_by_the_email_index(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let first = mailbox_for(&mut conn, &vault, ADDRESS).await.expect("mint");
    let at = touched(&mut conn, first).await;
    let wrapped = "Pat Quill <Pat.Quill@EXAMPLE.com>";
    let second = mailbox_for(&mut conn, &vault, wrapped)
        .await
        .expect("again");
    assert_eq!(first, second, "a second business gets the same mailbox");
    assert!(touched(&mut conn, first).await > at, "touched_at moved");
    let (version, digest) = email_index(&vault, ADDRESS, true)[0];
    let indexed: Uuid = sqlx::query_scalar(
        "SELECT mailbox_id FROM privacy.business_mailbox_index WHERE key_version = $1 AND digest = $2",
    )
    .bind(i32::from(version))
    .bind(&digest[..])
    .fetch_one(&mut *conn)
    .await
    .expect("indexed under the email digest");
    assert_eq!(indexed, first);
    assert_eq!(
        mailboxes_of(&mut conn, &vault, ADDRESS).await,
        Ok(vec![first])
    );
    let unknown = mailboxes_of(&mut conn, &vault, "nobody@example.com").await;
    assert_eq!(unknown, Ok(Vec::new()));
    assert_eq!(
        mailboxes_of(&mut conn, &vault, "no-at-sign").await,
        Ok(Vec::new())
    );
    let bad = mailbox_for(&mut conn, &vault, "pat.quill.example.com").await;
    assert_eq!(bad, Err(BusinessContactError::BadAddress));
    let rows = scalar_i64(
        &mut conn,
        "SELECT count(*) FROM privacy.business_mailbox WHERE $1 <> ''",
        "x",
    )
    .await;
    assert_eq!(rows, 1, "the bad value stored nothing");
    store_staff_phone(&mut conn, &vault, Uuid::new_v4(), PHONE)
        .await
        .expect("phone");
    let stored = everything_stored(&mut conn).await;
    assert!(
        !stored.contains("quill") && !stored.contains("5550144"),
        "a value in clear"
    );
}

/// TD-PRIV-28: two simultaneous calls for a new address make one mailbox row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_two_simultaneous_calls_make_one_mailbox(pool: PgPool) {
    let vault = v1();
    for round in 0..5 {
        let address = format!("crew{round}@example.com");
        let (mut a, mut b) = (app_conn(&pool).await, app_conn(&pool).await);
        let (x, y) = tokio::join!(
            mailbox_for(&mut a, &vault, &address),
            mailbox_for(&mut b, &vault, &address)
        );
        assert_eq!(x.expect("a"), y.expect("b"));
    }
    let (mut a, mut b) = (app_conn(&pool).await, app_conn(&pool).await);
    let mut first = a.begin().await.expect("begin");
    let held = mailbox_for(&mut first, &vault, ADDRESS)
        .await
        .expect("held");
    let commit_later = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        first.commit().await.expect("commit")
    };
    let (_, loser) = tokio::join!(commit_later, mailbox_for(&mut b, &vault, ADDRESS));
    assert_eq!(
        loser,
        Ok(held),
        "the call that lost the race returns the winner's mailbox"
    );
    let mut conn = app_conn(&pool).await;
    let rows = scalar_i64(
        &mut conn,
        "SELECT count(*) FROM privacy.business_mailbox WHERE $1 <> ''",
        "x",
    )
    .await;
    assert_eq!(rows, 6);
}

/// TD-PRIV-28: a mailbox the sweep deletes while `mailbox_for` is finding it is never returned;
/// the call mints a new one (D-3516).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_a_mailbox_deleted_mid_lookup_is_never_returned(pool: PgPool) {
    let vault = v1();
    let (mut a, mut b) = (app_conn(&pool).await, app_conn(&pool).await);
    let old = mailbox_for(&mut a, &vault, ADDRESS).await.expect("mint");
    let mut sweep = b.begin().await.expect("begin");
    sqlx::query("DELETE FROM privacy.business_mailbox WHERE id = $1")
        .bind(old)
        .execute(&mut *sweep)
        .await
        .expect("delete");
    let commit_later = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        sweep.commit().await.expect("commit")
    };
    let (_, found) = tokio::join!(commit_later, mailbox_for(&mut a, &vault, ADDRESS));
    let found = found.expect("minted");
    assert_ne!(found, old, "the deleted mailbox is not returned");
    assert_eq!(mailboxes_of(&mut a, &vault, ADDRESS).await, Ok(vec![found]));
}

/// Indexes mailbox `id` under the v1 index key, as a server still on that key would.
async fn index_under_v1(conn: &mut PgConnection, id: Uuid) {
    let (version, digest) = *email_index(&v1(), ADDRESS, true).first().expect("digest");
    sqlx::query(
        "INSERT INTO privacy.business_mailbox_index (purpose, key_version, digest, mailbox_id) \
         VALUES ('email', $1, $2, $3)",
    )
    .bind(i32::from(version))
    .bind(&digest[..])
    .bind(id)
    .execute(&mut *conn)
    .await
    .expect("index");
}

/// TD-PRIV-28: when a mailbox indexed under the old key shares its address with one indexed under
/// the active key (two servers mid-deploy), the rotation skips it, still findable, moves every
/// other row, then reports `DuplicateAddress` until one of the pair is removed (D-3516, D-3517).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_a_rotation_refuses_a_duplicate_address(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let old = mailbox_for(&mut conn, &v1(), ADDRESS).await.expect("mint");
    let other = mailbox_for(&mut conn, &v1(), "lee.marsh@example.com")
        .await
        .expect("mint");
    store_staff_phone(&mut conn, &v1(), Uuid::new_v4(), PHONE)
        .await
        .expect("phone");
    sqlx::query("DELETE FROM privacy.business_mailbox_index WHERE mailbox_id = $1")
        .bind(old)
        .execute(&mut *conn)
        .await
        .expect("unindex");
    let vault = v2();
    let new = mailbox_for(&mut conn, &vault, ADDRESS).await.expect("mint");
    index_under_v1(&mut conn, old).await;
    let mut both = vec![old, new];
    both.sort();
    assert_eq!(
        mailboxes_of(&mut conn, &vault, ADDRESS).await,
        Ok(both.clone())
    );
    assert_eq!(rotate_business_batch(&mut conn, &vault, 10).await, Ok(2));
    assert_eq!(
        rotate_business_batch(&mut conn, &vault, 10).await,
        Err(BusinessContactError::DuplicateAddress)
    );
    assert_eq!(mailboxes_of(&mut conn, &vault, ADDRESS).await, Ok(both));
    let read = read_business_mailbox(&mut conn, &vault, BusinessPurpose::PhoneApproval, other);
    assert_eq!(
        read.await.expect("moved").expose_secret(),
        "lee.marsh@example.com"
    );
    sqlx::query("DELETE FROM privacy.business_mailbox WHERE id = $1")
        .bind(old)
        .execute(&mut *conn)
        .await
        .expect("recover");
    assert_eq!(rotate_business_batch(&mut conn, &vault, 10).await, Ok(0));
    assert_eq!(
        mailboxes_of(&mut conn, &vault, ADDRESS).await,
        Ok(vec![new])
    );
}

/// TD-PRIV-28: `store_staff_phone` runs in the caller's transaction and replaces the number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_a_staff_phone_is_replaced_inside_the_callers_transaction(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let line = Uuid::new_v4();
    let mut tx = conn.begin().await.expect("begin");
    store_staff_phone(&mut tx, &vault, line, PHONE)
        .await
        .expect("store");
    tx.rollback().await.expect("rollback");
    let code = BusinessPurpose::SignInCode;
    let gone = read_staff_phone(&mut conn, &vault, code, line).await;
    assert_eq!(gone.err(), Some(BusinessContactError::NotFound));
    let mut tx = conn.begin().await.expect("begin");
    store_staff_phone(&mut tx, &vault, line, PHONE)
        .await
        .expect("store");
    store_staff_phone(&mut tx, &vault, line, "928-555-0145")
        .await
        .expect("replace");
    tx.commit().await.expect("commit");
    let phone = read_staff_phone(&mut conn, &vault, code, line)
        .await
        .expect("read");
    assert_eq!(phone.expose_secret(), "+19285550145");
    let bad = store_staff_phone(&mut conn, &vault, line, "555-01").await;
    assert_eq!(bad, Err(BusinessContactError::BadPhone));
}

/// TD-PRIV-28: each purpose opens the address with one audit row and one read count; only
/// `SignInCode` opens the phone; a read inside a transaction is refused with nothing audited.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_reads_are_audited_counted_and_refused_by_purpose(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let mailbox = mailbox_for(&mut conn, &vault, ADDRESS).await.expect("mint");
    let line = Uuid::new_v4();
    store_staff_phone(&mut conn, &vault, line, PHONE)
        .await
        .expect("phone");
    let mut tx = conn.begin().await.expect("begin");
    let inside = read_business_mailbox(&mut tx, &vault, BusinessPurpose::SignInLink, mailbox).await;
    assert_eq!(inside.err(), Some(BusinessContactError::InTransaction));
    let inside = read_staff_phone(&mut tx, &vault, BusinessPurpose::SignInCode, line).await;
    assert_eq!(inside.err(), Some(BusinessContactError::InTransaction));
    tx.rollback().await.expect("rollback");
    let audits = "SELECT count(*) FROM ops.audit_log WHERE action LIKE $1";
    assert_eq!(
        scalar_i64(&mut conn, audits, "vault_read.server.%").await,
        0
    );
    for purpose in BusinessPurpose::ALL {
        let value = read_business_mailbox(&mut conn, &vault, purpose, mailbox).await;
        assert_eq!(value.expect("address").expose_secret(), ADDRESS);
        assert_eq!(scalar_i64(&mut conn, audits, purpose.action()).await, 1);
        let counted =
            "SELECT coalesce(sum(reads), 0)::int8 FROM privacy.read_count WHERE action = $1";
        assert_eq!(scalar_i64(&mut conn, counted, purpose.action()).await, 1);
    }
    let phone = read_staff_phone(&mut conn, &vault, BusinessPurpose::SignInCode, line).await;
    assert_eq!(phone.expect("phone").expose_secret(), "+19285550144");
    let missing = read_business_mailbox(&mut conn, &vault, BusinessPurpose::SignInLink, line).await;
    assert_eq!(missing.err(), Some(BusinessContactError::NotFound));
    let alerts = "SELECT count(*) FROM ops.alert WHERE severity = 'high' AND subject_kind = $1";
    for purpose in BusinessPurpose::ALL
        .into_iter()
        .filter(|p| !p.opens_phone())
    {
        let refused = read_staff_phone(&mut conn, &vault, purpose, line).await;
        assert_eq!(refused.err(), Some(BusinessContactError::PhoneRefused));
        assert_eq!(scalar_i64(&mut conn, alerts, purpose.action()).await, 1);
        assert_eq!(
            scalar_i64(&mut conn, audits, purpose.action()).await,
            1,
            "nothing read"
        );
    }
}

/// TD-PRIV-28: the sweep forgets only what is unused and old, and a second run changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_forget_unused_keeps_what_is_used_or_recent(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let old = mailbox_for(&mut conn, &vault, "old@example.com")
        .await
        .expect("old");
    let kept = mailbox_for(&mut conn, &vault, "kept@example.com")
        .await
        .expect("kept");
    let (old_line, kept_line) = (Uuid::new_v4(), Uuid::new_v4());
    store_staff_phone(&mut conn, &vault, old_line, PHONE)
        .await
        .expect("old line");
    store_staff_phone(&mut conn, &vault, kept_line, "928-555-0146")
        .await
        .expect("kept line");
    let before = Timestamp::now();
    let recent = mailbox_for(&mut conn, &vault, "recent@example.com")
        .await
        .expect("recent");
    let recent_line = Uuid::new_v4();
    store_staff_phone(&mut conn, &vault, recent_line, "928-555-0147")
        .await
        .expect("recent line");
    let in_use = (&[kept][..], &[kept_line][..]);
    assert_eq!(forget_unused(&mut conn, in_use, before).await, Ok(2));
    assert_eq!(
        forget_unused(&mut conn, in_use, before).await,
        Ok(0),
        "safe to repeat"
    );
    assert_eq!(
        mailboxes_of(&mut conn, &vault, "old@example.com").await,
        Ok(Vec::new())
    );
    for id in [kept, recent] {
        let read =
            read_business_mailbox(&mut conn, &vault, BusinessPurpose::AccountEmail, id).await;
        assert!(read.is_ok(), "an in-use or recent mailbox stays");
    }
    for line in [kept_line, recent_line] {
        let read = read_staff_phone(&mut conn, &vault, BusinessPurpose::SignInCode, line).await;
        assert!(read.is_ok(), "an in-use or recent line stays");
    }
    let code = BusinessPurpose::SignInCode;
    let gone = read_staff_phone(&mut conn, &vault, code, old_line).await;
    assert_eq!(gone.err(), Some(BusinessContactError::NotFound));
    let gone = read_business_mailbox(&mut conn, &vault, BusinessPurpose::SignInLink, old).await;
    assert_eq!(gone.err(), Some(BusinessContactError::NotFound));
}

/// TD-PRIV-28, PRIV-024: v1 rows stay readable and findable while v2 is active, the batch moves
/// every row to v2 with audited reads, and each value stays readable and findable afterwards.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_28_a_key_rotation_keeps_every_value_readable_and_findable(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let mailbox = mailbox_for(&mut conn, &v1(), ADDRESS).await.expect("mint");
    let line = Uuid::new_v4();
    store_staff_phone(&mut conn, &v1(), line, PHONE)
        .await
        .expect("phone");
    let vault = v2();
    assert_eq!(
        mailboxes_of(&mut conn, &vault, ADDRESS).await,
        Ok(vec![mailbox])
    );
    assert_eq!(
        mailbox_for(&mut conn, &vault, ADDRESS).await,
        Ok(mailbox),
        "found, not minted"
    );
    assert_eq!(rotate_business_batch(&mut conn, &vault, 10).await, Ok(2));
    assert_eq!(rotate_business_batch(&mut conn, &vault, 10).await, Ok(0));
    let stale = "SELECT (SELECT count(*) FROM privacy.business_mailbox WHERE key_version < 2) \
                 + (SELECT count(*) FROM privacy.business_mailbox_index WHERE key_version < 2) \
                 + (SELECT count(*) FROM privacy.staff_phone WHERE key_version < 2) + length($1)";
    assert_eq!(scalar_i64(&mut conn, stale, "").await, 0, "every row on v2");
    let audits = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(
        scalar_i64(&mut conn, audits, "vault_read.privacy.rotation").await,
        2
    );
    assert_eq!(
        mailboxes_of(&mut conn, &vault, ADDRESS).await,
        Ok(vec![mailbox])
    );
    let read =
        read_business_mailbox(&mut conn, &vault, BusinessPurpose::PhoneApproval, mailbox).await;
    assert_eq!(read.expect("address").expose_secret(), ADDRESS);
    let read = read_staff_phone(&mut conn, &vault, BusinessPurpose::SignInCode, line).await;
    assert_eq!(read.expect("phone").expose_secret(), "+19285550144");
}

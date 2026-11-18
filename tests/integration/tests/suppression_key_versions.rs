//! No `VAULT_INDEX_KEY` version is retired while an opt-out is held under it (TD-PRIV-31; PRIV-024,
//! CON-035, CON-022; D-4418, D-4473).
//!
//! Proves, against PostgreSQL as the app role, that a contact's homeowner-phone digest is kept
//! under every configured index key version (stored, rotated, re-indexed, and a digest under a
//! dropped version moved by the next `rotate_batch`); that `messaging`'s start check passes on an
//! empty list, passes while every held version is configured and fails naming the missing version
//! numbers only; that `phone_digests` reads a contact's digests as `blind_index` computes them, in
//! an open transaction too; and that `settle_index_versions` prunes rows under a dropped version
//! only beside a configured one and refuses, deleting nothing, when a contact has none. Fake
//! numbers and keys only.

use askhava_integration::set_app_role;
use askhava_messaging::api::{
    RevokeChannel, SuppressionVersionsError, check_suppression_versions, suppress,
};
use askhava_privacy::api::{
    IndexPurpose, IndexVersionsError, NewContact, Vault, find_by_phone, phone_digests,
    rotate_batch, settle_index_versions, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection as _, PgConnection, PgPool};

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONE: &str = "928-555-0141";
const OTHER: &str = "928-555-0142";
const NORMAL: &str = "+19285550141";

/// A `<version>:<base64>` key entry of 32 copies of `byte`: `tests/integration/tests/vault.rs`'s
/// `key(version, byte)` helper (each test file is its own crate).
fn key(version: u16, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn vault(seal: &str, index: &str) -> Vault {
    Vault::new(&secret(seal), &secret(index), &secret(DB_URL)).expect("vault keys")
}

/// Seal and index keys at v1 only.
fn v1() -> Vault {
    vault(&key(1, 0x11), &key(1, 0x21))
}

/// Seal v2 active beside v1, index keys v1 and v2.
fn both() -> Vault {
    vault(
        &format!("{},{}", key(2, 0x12), key(1, 0x11)),
        &format!("{},{}", key(1, 0x21), key(2, 0x22)),
    )
}

/// Seal keys v1 and v2, the index key at v2 only: v1 dropped from `VAULT_INDEX_KEY`.
fn v2() -> Vault {
    vault(&format!("{},{}", key(2, 0x12), key(1, 0x11)), &key(2, 0x22))
}

fn with_phone(phone: &str) -> NewContact {
    NewContact {
        phone: Some(secret(phone)),
        ..NewContact::default()
    }
}

async fn app_conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn store(conn: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    store_contact(conn, vault, &with_phone(phone))
        .await
        .expect("store")
}

/// Contact `id`'s homeowner-phone key versions, ascending.
async fn versions(conn: &mut PgConnection, id: Uuid) -> Vec<u16> {
    let mut held: Vec<u16> = phone_digests(conn, id)
        .await
        .expect("digests")
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    held.sort_unstable();
    held
}

async fn plant(
    conn: &mut PgConnection,
    purpose: &str,
    (version, digest): (u16, [u8; 32]),
    id: Uuid,
) {
    sqlx::query(
        "INSERT INTO privacy.contact_index (purpose, key_version, digest, contact_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(purpose)
    .bind(i32::from(version))
    .bind(&digest[..])
    .bind(id)
    .execute(conn)
    .await
    .expect("plant a digest");
}

async fn index_rows(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM privacy.contact_index")
        .fetch_one(conn)
        .await
        .expect("count")
}

async fn list(conn: &mut PgConnection, vault: &Vault, phone: &str) {
    let now: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    suppress(conn, vault, phone, (RevokeChannel::Portal, "portal"), now)
        .await
        .expect("suppress");
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// TD-PRIV-31: `index_versions()` reads the configured versions, ascending.
#[test]
fn td_priv_31_index_versions_lists_every_configured_version() {
    assert_eq!(both().index_versions(), [1, 2]);
    assert_eq!(v2().index_versions(), [2]);
}

/// TD-PRIV-31, CON-035: an empty list passes with any key set.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_an_empty_list_passes_with_any_key_set(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    for keys in [v1(), both(), v2()] {
        assert_eq!(check_suppression_versions(&mut conn, &keys).await, Ok(()));
    }
}

/// TD-PRIV-31, PRIV-024: a contact stored before v2 holds a v1 and a v2 digest after
/// `rotate_batch`; one stored after holds both at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_every_phone_contact_holds_a_digest_per_configured_version(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let before = store(&mut conn, &v1(), PHONE).await;
    assert_eq!(versions(&mut conn, before).await, [1]);
    let after = store(&mut conn, &both(), OTHER).await;
    assert_eq!(
        versions(&mut conn, after).await,
        [1, 2],
        "stored with both: both at once"
    );
    assert_eq!(
        rotate_batch(&mut conn, &both(), 10).await.expect("rotate"),
        1
    );
    assert_eq!(
        rotate_batch(&mut conn, &both(), 10).await.expect("rotate"),
        0
    );
    assert_eq!(versions(&mut conn, before).await, [1, 2]);
    assert_eq!(versions(&mut conn, after).await, [1, 2]);
}

/// TD-PRIV-31, PRIV-024: re-indexing a contact whose index names an earlier number (the phone
/// changed) replaces both digests with the sealed number's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_a_phone_change_replaces_both_digests(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let id = store(&mut conn, &v1(), PHONE).await;
    sqlx::query("DELETE FROM privacy.contact_index WHERE contact_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("clear");
    for digest in both().blind_index(IndexPurpose::HomeownerPhone, "+19285550142") {
        plant(&mut conn, "homeowner-phone", digest, id).await;
    }
    assert_eq!(
        rotate_batch(&mut conn, &both(), 10).await.expect("rotate"),
        1
    );
    let held = phone_digests(&mut conn, id).await.expect("digests");
    assert_eq!(
        held,
        both().blind_index(IndexPurpose::HomeownerPhone, NORMAL)
    );
    let earlier = find_by_phone(&mut conn, &both(), OTHER)
        .await
        .expect("find");
    assert_eq!(earlier, None, "the earlier number's digests are gone");
}

/// TD-PRIV-31, PRIV-024: a contact whose only digest is under a dropped version is picked by the
/// next `rotate_batch` and left with exactly the configured ones.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_a_digest_under_a_dropped_version_is_rebuilt(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let id = store(&mut conn, &v1(), PHONE).await;
    let only_v2 = vault(&key(1, 0x11), &key(2, 0x22));
    assert_eq!(
        rotate_batch(&mut conn, &only_v2, 10).await.expect("rotate"),
        1
    );
    assert_eq!(
        rotate_batch(&mut conn, &only_v2, 10).await.expect("rotate"),
        0
    );
    assert_eq!(versions(&mut conn, id).await, [2]);
    let found = find_by_phone(&mut conn, &only_v2, PHONE)
        .await
        .expect("find");
    assert_eq!(found, Some(id));
}

/// TD-PRIV-31, CON-013, CON-035: a number listed with only v1 passes with v1 and with v1 plus
/// v2, and fails naming version 1 with only v2; the refusal names versions only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_a_v1_listing_refuses_a_key_set_without_v1(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    list(&mut conn, &v1(), PHONE).await;
    assert_eq!(check_suppression_versions(&mut conn, &v1()).await, Ok(()));
    assert_eq!(check_suppression_versions(&mut conn, &both()).await, Ok(()));
    let refused = check_suppression_versions(&mut conn, &v2()).await;
    assert_eq!(refused, Err(SuppressionVersionsError::Missing(vec![1])));
    let text = refused.expect_err("refused").to_string();
    assert!(text.contains("[1]") && text.contains("D-4418"), "{text}");
    for (_, digest) in both().blind_index(IndexPurpose::HomeownerPhone, NORMAL) {
        assert!(!text.contains(&hex(&digest)), "the refusal holds no digest");
    }
    assert!(!text.contains("555"), "the refusal holds no number");
}

/// TD-PRIV-31, CON-035: a listing made with v1 and v2 configured holds a v1 row, so it passes
/// with both and fails with only v2.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_a_listing_under_both_still_holds_v1(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    list(&mut conn, &both(), PHONE).await;
    assert_eq!(check_suppression_versions(&mut conn, &both()).await, Ok(()));
    let refused = check_suppression_versions(&mut conn, &v2()).await;
    assert_eq!(refused, Err(SuppressionVersionsError::Missing(vec![1])));
}

/// TD-PRIV-31, D-4473: `phone_digests` equals `blind_index` of the number, highest version
/// first; a contact with no phone gives none; it reads inside an open transaction.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_phone_digests_read_the_stored_index(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let id = store(&mut conn, &both(), PHONE).await;
    let bare = store_contact(&mut conn, &both(), &NewContact::default())
        .await
        .expect("no phone");
    let mut tx = conn.begin().await.expect("begin");
    let held = phone_digests(&mut tx, id)
        .await
        .expect("inside a transaction");
    let want = both().blind_index(IndexPurpose::HomeownerPhone, NORMAL);
    assert_eq!(held, want);
    assert_eq!(
        held.first().map(|(v, _)| *v),
        Some(2),
        "the highest version first"
    );
    assert_eq!(phone_digests(&mut tx, bare).await.expect("bare"), []);
    tx.commit().await.expect("commit");
}

/// TD-PRIV-31, D-4473: with only v2 configured the start step deletes a contact's v1
/// homeowner-phone row beside its v2 one, keeps every `sender-dir` and `email` row, and passes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_settle_prunes_a_dropped_version_beside_a_configured_one(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let id = store(&mut conn, &both(), PHONE).await;
    let v1_digest = v1().blind_index(IndexPurpose::SenderDir, "fake-line")[0];
    plant(&mut conn, "sender-dir", v1_digest, id).await;
    let mail = v1().blind_index(IndexPurpose::Email, "sample@example.com")[0];
    plant(&mut conn, "email", mail, id).await;
    assert_eq!(settle_index_versions(&mut conn, &v2()).await, Ok(()));
    assert_eq!(versions(&mut conn, id).await, [2]);
    assert_eq!(
        index_rows(&mut conn).await,
        3,
        "the sender-dir and email rows stay"
    );
    let found = find_by_phone(&mut conn, &v2(), PHONE).await.expect("find");
    assert_eq!(found, Some(id));
}

/// TD-PRIV-31, D-4473: with only v2 configured a contact holding only a v1 homeowner-phone row
/// refuses the start, naming the configured versions and the count only, and nothing is deleted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_31_settle_refuses_a_contact_left_unindexed(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let id = store(&mut conn, &v1(), PHONE).await;
    store(&mut conn, &both(), OTHER).await;
    let before = index_rows(&mut conn).await;
    let refused = settle_index_versions(&mut conn, &v2()).await;
    let want = IndexVersionsError::Unindexed {
        configured: vec![2],
        count: 1,
    };
    assert_eq!(refused, Err(want));
    assert_eq!(index_rows(&mut conn).await, before, "nothing is deleted");
    let text = refused.expect_err("refused").to_string();
    assert!(
        !text.contains(&id.to_string()),
        "the refusal names no contact"
    );
    let digest = v1().blind_index(IndexPurpose::HomeownerPhone, NORMAL)[0].1;
    assert!(!text.contains(&hex(&digest)), "the refusal holds no digest");
}

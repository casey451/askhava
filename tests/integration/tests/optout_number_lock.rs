//! The per-number opt-out lock and the consent sweep's re-check (TD-CON-37; CON-022, CON-035;
//! D-079, D-4418).
//!
//! Proves, against PostgreSQL, that while a transaction holds `still_unlisted`'s lock for a number
//! an opt-out of it (portal `suppress`, a texted STOP) waits and lists the number only after that
//! commit; that `still_unlisted` waits on an open opt-out's lock and then sees it; that it answers
//! false for a listed number and true for one never listed; that `listing_key` is `Some` only with
//! a digest under every configured index key version and reads no vault plaintext; that a v1-only
//! listing still counts once the contact also holds a v2 digest; that a held row under a version
//! outside the key answers false for any number; and that the lock on one number does not hold up
//! another. Fake numbers and keys only.

use std::time::Duration;

use askhava_messaging::api::{
    InboundText, ListingKey, ProviderKeyword, Relation, RevokeChannel, listing_key,
    on_inbound_text, still_unlisted, suppress,
};
use askhava_ops::api::export_audit;
use askhava_privacy::api::{
    IndexPurpose, NewContact, Vault, phone_digests, rotate_batch, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;
use sqlx::{Connection as _, PgConnection, PgPool, Postgres};
use tokio::task::JoinHandle;

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONE: &str = "+19285550171";
const OTHER: &str = "+19285550172";

/// A `<version>:<base64>` key entry of 32 copies of `byte`: `tests/integration/tests/vault.rs`'s
/// `key(version, byte)` helper (TD-PRIV-09; each test file is its own crate).
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

/// The index key at v2 only: v1 dropped from `VAULT_INDEX_KEY`.
fn v2() -> Vault {
    vault(&format!("{},{}", key(2, 0x12), key(1, 0x11)), &key(2, 0x22))
}

fn now() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

/// The key of `phone` as `record` computes it from the number.
fn key_of(vault: &Vault, phone: &str) -> ListingKey {
    listing_key(
        vault,
        &vault.blind_index(IndexPurpose::HomeownerPhone, phone),
    )
    .expect("key")
}

async fn store(conn: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        phone: Some(secret(phone)),
        ..NewContact::default()
    };
    store_contact(conn, vault, &new).await.expect("store")
}

async fn list(conn: &mut PgConnection, vault: &Vault, phone: &str) -> bool {
    suppress(conn, vault, phone, (RevokeChannel::Portal, "portal"), now())
        .await
        .expect("suppress")
}

/// `still_unlisted` for `phone` in its own committed transaction.
async fn unlisted(conn: &mut PgConnection, vault: &Vault, phone: &str) -> bool {
    let key = key_of(vault, phone);
    let mut tx = conn.begin().await.expect("begin");
    let answer = still_unlisted(&mut tx, &key).await.expect("re-check");
    tx.commit().await.expect("commit");
    answer
}

async fn pid(conn: &mut PgConnection) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(conn)
        .await
        .expect("pid")
}

/// A bounded poll of `pg_locks` until backend `pid` waits on an advisory lock; fails, never hangs.
async fn seen_waiting(observer: &mut PgConnection, pid: i32) {
    for _ in 0..200 {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks \
             WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
        )
        .bind(pid)
        .fetch_one(&mut *observer)
        .await
        .expect("pg_locks");
        if waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("backend {pid} never waited on an advisory lock");
}

async fn finish<T>(task: JoinHandle<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("finished in time")
        .expect("task")
}

/// Runs `suppress` (portal) of `phone` on `conn` in its own task.
fn spawn_suppress(mut conn: PoolConnection<Postgres>, phone: &'static str) -> JoinHandle<bool> {
    tokio::spawn(async move { list(&mut conn, &v1(), phone).await })
}

/// Runs a texted STOP from `phone` (contact `her`) through `on_inbound_text` in its own task.
fn spawn_stop(
    mut conn: PoolConnection<Postgres>,
    her: Uuid,
    phone: &'static str,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let stop = InboundText {
            inbox_id: Uuid::from_u128(0x701),
            contact: her,
            relation: Relation::OpenRequest,
            phone,
            text: "STOP",
            provider: ProviderKeyword::OptOut,
            emergency: false,
        };
        let id_secret = secret("fake-id-secret-for-tests");
        on_inbound_text(&mut conn, (&id_secret, &v1()), &stop, now())
            .await
            .expect("inbound");
    })
}

/// TD-CON-37: while the sweep's re-check holds a number's lock, a portal opt-out and a texted STOP
/// of it each wait, and finish only after that commit, the number then listed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_an_opt_out_waits_for_the_sweeps_lock(pool: PgPool) {
    let mut observer = pool.acquire().await.expect("connection");
    let her = store(&mut observer, &v1(), PHONE).await;
    for texted in [false, true] {
        let mut sweep = pool.acquire().await.expect("connection");
        let mut tx = sweep.begin().await.expect("begin");
        assert!(
            still_unlisted(&mut tx, &key_of(&v1(), PHONE))
                .await
                .expect("re-check")
        );
        let mut conn = pool.acquire().await.expect("connection");
        let backend = pid(&mut conn).await;
        let task = if texted {
            tokio::spawn(async move {
                finish(spawn_stop(conn, her, PHONE)).await;
                true
            })
        } else {
            spawn_suppress(conn, PHONE)
        };
        seen_waiting(&mut observer, backend).await;
        assert!(
            !task.is_finished(),
            "the opt-out waits for the sweep's commit"
        );
        tx.commit().await.expect("commit");
        assert!(finish(task).await);
        assert!(!unlisted(&mut observer, &v1(), PHONE).await, "listed");
        sqlx::query("DELETE FROM messaging.suppression")
            .execute(&mut *observer)
            .await
            .expect("clear the list");
    }
}

/// TD-CON-37: `still_unlisted` waits on an open opt-out's lock (its `begin` a savepoint inside an
/// outer transaction) and, after the outer commit, answers false.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_the_re_check_waits_for_an_open_opt_out(pool: PgPool) {
    let mut observer = pool.acquire().await.expect("connection");
    let mut opting = pool.acquire().await.expect("connection");
    let mut outer = opting.begin().await.expect("outer");
    assert!(list(&mut outer, &v1(), PHONE).await, "newly listed");
    let mut sweep = pool.acquire().await.expect("connection");
    let backend = pid(&mut sweep).await;
    let task = tokio::spawn(async move { unlisted(&mut sweep, &v1(), PHONE).await });
    seen_waiting(&mut observer, backend).await;
    assert!(
        !task.is_finished(),
        "the re-check waits for the opt-out's commit"
    );
    outer.commit().await.expect("commit");
    assert!(!finish(task).await, "the committed opt-out is seen");
}

/// TD-CON-37: a number listed before the re-check answers false; one never listed answers true.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_a_listed_number_answers_false_and_an_unlisted_one_true(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    assert!(list(&mut conn, &v1(), PHONE).await);
    assert!(!unlisted(&mut conn, &v1(), PHONE).await);
    assert!(unlisted(&mut conn, &v1(), OTHER).await);
}

/// TD-CON-37, PRIV-024: with one version configured the contact's indexed digest is its key, read
/// with no vault-read audit row; with v1 and v2 it is `Some` with both after `rotate_batch`, and
/// `None` while the contact holds only its v1 digest.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_the_listing_key_needs_every_configured_version(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    let her = store(&mut conn, &v1(), PHONE).await;
    let indexed = phone_digests(&mut conn, her).await.expect("digests");
    let one = listing_key(&v1(), &indexed).expect("one version");
    assert_eq!(format!("{one:?}"), "ListingKey { versions: [1], .. }");
    let audit = export_audit(&mut conn, 0, 1000).await.expect("audit");
    assert!(!audit.iter().any(|r| r.action.starts_with("vault_read.")));
    assert!(
        listing_key(&both(), &indexed).is_none(),
        "v2 digest missing"
    );
    assert_eq!(
        rotate_batch(&mut conn, &both(), 10).await.expect("rotate"),
        1
    );
    let rotated = phone_digests(&mut conn, her).await.expect("digests");
    let two = listing_key(&both(), &rotated).expect("both versions");
    assert_eq!(format!("{two:?}"), "ListingKey { versions: [1, 2], .. }");
    let mut tx = conn.begin().await.expect("begin");
    assert!(still_unlisted(&mut tx, &two).await.expect("re-check"));
    tx.commit().await.expect("commit");
    let v1_only: Vec<_> = rotated.into_iter().filter(|(v, _)| *v == 1).collect();
    assert!(
        listing_key(&both(), &v1_only).is_none(),
        "only its v1 digest"
    );
}

/// TD-CON-37, PRIV-024: a number listed while only v1 existed still answers false once its contact
/// holds v1 and v2 digests, both configured; an unlisted number's key answers true.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_a_v1_listing_counts_after_rotation(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    assert!(list(&mut conn, &v1(), PHONE).await);
    let listed = store(&mut conn, &v1(), PHONE).await;
    let open = store(&mut conn, &v1(), OTHER).await;
    assert_eq!(
        rotate_batch(&mut conn, &both(), 10).await.expect("rotate"),
        2
    );
    for (id, expected) in [(listed, false), (open, true)] {
        let indexed = phone_digests(&mut conn, id).await.expect("digests");
        let key = listing_key(&both(), &indexed).expect("both versions");
        let mut tx = conn.begin().await.expect("begin");
        assert_eq!(
            still_unlisted(&mut tx, &key).await.expect("re-check"),
            expected
        );
        tx.commit().await.expect("commit");
    }
}

/// TD-CON-37: with only v2 configured while a v1 row is held (refused at start by T702, built here
/// directly), the re-check answers false for any number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_a_row_outside_the_keys_versions_answers_false(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    assert!(list(&mut conn, &v1(), PHONE).await);
    for phone in [PHONE, OTHER] {
        assert!(!unlisted(&mut conn, &v2(), phone).await, "held v1 row");
    }
}

/// TD-CON-37: the lock on one number does not hold up `suppress` of another.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_37_one_numbers_lock_does_not_hold_up_another(pool: PgPool) {
    let mut sweep = pool.acquire().await.expect("connection");
    let mut tx = sweep.begin().await.expect("begin");
    assert!(
        still_unlisted(&mut tx, &key_of(&v1(), PHONE))
            .await
            .expect("re-check")
    );
    let conn = pool.acquire().await.expect("connection");
    assert!(finish(spawn_suppress(conn, OTHER)).await, "listed at once");
    tx.commit().await.expect("commit");
}

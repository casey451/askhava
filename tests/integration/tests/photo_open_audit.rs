//! A photo's plaintext leaves `open_link` only after its open audit has committed on its own
//! (TD-PRIV-17; PRIV-003, PRIV-014, PRIV-023, PRIV-028; D-753, D-1838).
//!
//! As the app role on a fresh database: a connection inside an sqlx transaction or a raw `BEGIN`
//! is refused with `InTransaction` and gets no bytes; on a bare connection the
//! `privacy.photo_opened` row is visible from another connection as soon as the bytes come back;
//! a failed audit write returns no bytes; an unknown or expired link is `LinkDead`; a storage
//! failure returns no bytes and writes no open row. The photo is a made-up 2 by 2 PNG and the
//! keys are fake.

use askhava_adapters::api::LocalStorage;
use askhava_core::api::{IdSecret, LinkToken};
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    CleanPhoto, LINK_TTL_SECS, PhotoError, PhotoSeal, clean, issue_link, open_link, store_photo,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0273);
const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a730000001049\
                       444154789c6338a1a101440c100a00212e04611ee06dc20000000049454e44ae426082";

/// A fresh storage directory, removed when the test ends.
struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

struct Kit {
    dir: Dir,
    storage: LocalStorage,
    seal: PhotoSeal,
    secret: SecretString,
    ids: IdSecret,
}

fn kit(test: &str) -> Kit {
    let root = std::env::temp_dir().join(format!("askhava-open-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let storage = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    let key = SecretString::from(format!("1:{}", STANDARD.encode([7u8; 32])));
    let secret = SecretString::from("fake-id-secret-for-photo-open-audit-0123");
    Kit {
        dir: Dir(root),
        storage,
        seal: PhotoSeal::new(&key).expect("seal"),
        ids: IdSecret::new(1, secret.clone()).expect("id secret"),
        secret,
    }
}

fn fixture() -> Vec<u8> {
    let hex: String = PNG_HEX.split_whitespace().collect();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}

fn at() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("time")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// A stored photo, a live link to it and its cleaned bytes.
async fn linked(conn: &mut PgConnection, kit: &Kit) -> (Uuid, LinkToken, Vec<u8>) {
    let photo = clean(fixture()).await.expect("clean");
    let deps = (&kit.seal, &kit.storage, &kit.secret);
    let id = store_photo(conn, deps, JOB, &photo, at())
        .await
        .expect("store");
    let token = issue_link(conn, &kit.ids, id, at()).await.expect("link");
    (id, token, photo.as_bytes().to_vec())
}

/// The committed `privacy.photo_opened` rows of `photo`, read on a connection of its own.
async fn opened_rows(pool: &PgPool, photo: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action = 'privacy.photo_opened' \
         AND subject_kind = 'privacy.photo' AND subject_id = $1",
    )
    .bind(photo)
    .fetch_one(pool)
    .await
    .expect("count")
}

async fn open(
    conn: &mut PgConnection,
    kit: &Kit,
    token: &LinkToken,
) -> Result<CleanPhoto, PhotoError> {
    let soon = at() + SignedDuration::from_secs(LINK_TTL_SECS - 1);
    open_link(conn, (&kit.seal, &kit.storage), token, soon).await
}

/// TD-PRIV-17, PRIV-003, D-1838: inside an sqlx transaction or a raw `BEGIN`, `open_link` is
/// `InTransaction`, returns no bytes and leaves no open row, even after the caller commits.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_17_a_connection_inside_a_transaction_gets_no_bytes(pool: PgPool) {
    let kit = kit("tx");
    let mut conn = app(&pool).await;
    let (id, token, _) = linked(&mut conn, &kit).await;
    let mut tx = conn.begin().await.expect("begin");
    let in_sqlx = open(&mut tx, &kit, &token).await;
    assert!(matches!(in_sqlx, Err(PhotoError::InTransaction)));
    tx.commit().await.expect("commit");
    sqlx::raw_sql("BEGIN")
        .execute(&mut conn)
        .await
        .expect("raw begin");
    let in_raw = open(&mut conn, &kit, &token).await;
    assert!(matches!(in_raw, Err(PhotoError::InTransaction)));
    sqlx::raw_sql("COMMIT")
        .execute(&mut conn)
        .await
        .expect("raw commit");
    assert_eq!(opened_rows(&pool, id).await, 0);
}

/// TD-PRIV-17, PRIV-003, PRIV-014, PRIV-023: on a bare connection the open row has committed,
/// seen from another connection, by the time the bytes come back; each open adds one row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_17_a_bare_open_returns_the_photo_after_its_audit_committed(pool: PgPool) {
    let kit = kit("bare");
    let mut conn = app(&pool).await;
    let (id, token, clean_bytes) = linked(&mut conn, &kit).await;
    let photo = open(&mut conn, &kit, &token).await.expect("open");
    assert_eq!(opened_rows(&pool, id).await, 1);
    assert_eq!(photo.as_bytes(), clean_bytes);
    open(&mut conn, &kit, &token).await.expect("open again");
    assert_eq!(opened_rows(&pool, id).await, 2);
}

/// TD-PRIV-17, PRIV-003: when the open row cannot be written, no bytes are returned.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_17_a_failed_audit_write_returns_no_bytes(pool: PgPool) {
    let kit = kit("audit");
    let mut conn = app(&pool).await;
    let (id, token, _) = linked(&mut conn, &kit).await;
    sqlx::query("REVOKE INSERT ON ops.audit_log FROM askhava_app")
        .execute(&pool)
        .await
        .expect("revoke");
    let failed = open(&mut conn, &kit, &token).await;
    assert!(matches!(failed, Err(PhotoError::Database)));
    assert_eq!(opened_rows(&pool, id).await, 0);
}

/// TD-PRIV-17, PRIV-014, PRIV-028: an unknown or expired link is `LinkDead` and opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_17_an_unknown_or_expired_link_is_dead(pool: PgPool) {
    let kit = kit("dead");
    let mut conn = app(&pool).await;
    let (id, token, _) = linked(&mut conn, &kit).await;
    let deps = (&kit.seal, &kit.storage);
    let late = at() + SignedDuration::from_secs(LINK_TTL_SECS);
    let expired = open_link(&mut conn, deps, &token, late).await;
    assert!(matches!(expired, Err(PhotoError::LinkDead)));
    let other = LinkToken::parse(&"cd".repeat(32)).expect("token");
    let unknown = open_link(&mut conn, deps, &other, at()).await;
    assert!(matches!(unknown, Err(PhotoError::LinkDead)));
    assert_eq!(opened_rows(&pool, id).await, 0);
}

/// TD-PRIV-17, PRIV-003, PRIV-023: a storage read failure returns no bytes and no open row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_17_a_storage_failure_returns_no_bytes_and_no_audit_row(pool: PgPool) {
    let kit = kit("storage");
    let mut conn = app(&pool).await;
    let (id, token, _) = linked(&mut conn, &kit).await;
    std::fs::remove_file(kit.dir.0.join(format!("photos/{id}"))).expect("remove object");
    let failed = open(&mut conn, &kit, &token).await;
    assert!(matches!(failed, Err(PhotoError::Storage)));
    assert_eq!(opened_rows(&pool, id).await, 0);
}

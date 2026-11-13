//! Photo key rotation on a fresh database (TD-PRIV-24; PRIV-023, PRIV-024; D-2626, D-3542).
//!
//! As the app role: a photo sealed under `VAULT_KEY` v1 still opens through its link after the
//! drill (`rotate_batch` and `rotate_photos` with `2:K2,1:K1` until both return 0) once only `2:K2`
//! is left; a run with no old photo moves nothing; a re-seal stopped after its new object is
//! written leaves the photo readable from its old row; the new object opens only under its row's
//! version and its own job and photo ID; a failed old-object delete leaves the row moved; a row moved
//! meanwhile is left alone; a connection inside a transaction is refused. The fixture is a made-up 2 by 2 PNG, the keys are fake.

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, LocalStorage, StorageAdapter, StorageDelete,
};
use askhava_core::api::IdSecret;
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    NewContact, PhotoError, PhotoSeal, Vault, clean, issue_link, open_link, rotate_batch,
    rotate_photos, store_contact, store_photo,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0354);
const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a730000001049444154\
                       789c6338a1a101440c100a00212e04611ee06dc20000000049454e44ae426082";
const ID_SECRET: &str = "fake-id-secret-for-photo-rotation-0123456789";

/// A `<version>:<base64>` key entry of 32 copies of `byte`.
fn key(version: u16, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn seal(keys: &str) -> PhotoSeal {
    PhotoSeal::new(&secret(keys)).expect("seal")
}

fn both() -> String {
    format!("{},{}", key(2, 0x12), key(1, 0x11))
}

/// A fresh storage directory, removed when the test ends.
struct Dir(std::path::PathBuf, LocalStorage);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(test: &str) -> Dir {
    let root = std::env::temp_dir().join(format!("askhava-rotate-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let storage = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    Dir(root, storage)
}

/// Storage that writes each object and then reports the write as failed: a re-seal stopped after
/// its new object is written.
struct StopsAfterPut<'a>(&'a LocalStorage);

impl StorageAdapter for StopsAfterPut<'_> {
    async fn put_object(
        &self,
        key: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        self.0.put_object(key, name, bytes).await?;
        Err(AdapterError::OutcomeUnknown)
    }

    async fn read_object(&self, name: &str) -> Result<Vec<u8>, AdapterError> {
        self.0.read_object(name).await
    }
}

impl StorageDelete for StopsAfterPut<'_> {
    async fn delete_object(&self, key: &IdempotencyKey, name: &str) -> Result<(), AdapterError> {
        self.0.delete_object(key, name).await
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>, AdapterError> {
        self.0.list_objects(prefix).await
    }
}

/// Storage whose delete fails (`FailsDelete`) or which, before each write, points the photo's row
/// at another object as the owner (`MovesRow`): a row moved while it is being re-sealed.
enum Fault<'a> {
    FailsDelete(&'a LocalStorage),
    MovesRow(&'a LocalStorage, &'a PgPool, Uuid),
}

impl Fault<'_> {
    fn store(&self) -> &LocalStorage {
        match self {
            Self::FailsDelete(store) | Self::MovesRow(store, ..) => store,
        }
    }
}

impl StorageAdapter for Fault<'_> {
    async fn put_object(
        &self,
        key: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        if let Self::MovesRow(_, pool, photo) = self {
            sqlx::query("UPDATE privacy.photo SET storage_key = 'photos/elsewhere' WHERE id = $1")
                .bind(photo)
                .execute(*pool)
                .await
                .expect("move the row");
        }
        self.store().put_object(key, name, bytes).await
    }

    async fn read_object(&self, name: &str) -> Result<Vec<u8>, AdapterError> {
        self.store().read_object(name).await
    }
}

impl StorageDelete for Fault<'_> {
    async fn delete_object(&self, key: &IdempotencyKey, name: &str) -> Result<(), AdapterError> {
        match self {
            Self::FailsDelete(_) => Err(AdapterError::OutcomeUnknown),
            Self::MovesRow(store, ..) => store.delete_object(key, name).await,
        }
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>, AdapterError> {
        self.store().list_objects(prefix).await
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

/// Stores the fixture under `keys` and returns its ID and cleaned bytes.
async fn stored(conn: &mut PgConnection, keys: &str, storage: &LocalStorage) -> (Uuid, Vec<u8>) {
    let photo = clean(fixture()).await.expect("clean");
    let deps = (&seal(keys), storage, &secret(ID_SECRET));
    let id = store_photo(conn, deps, JOB, &photo, at())
        .await
        .expect("store");
    (id, photo.as_bytes().to_vec())
}

/// Opens `photo` through a fresh link with only `keys` loaded.
async fn opened(
    conn: &mut PgConnection,
    keys: &str,
    storage: &LocalStorage,
    photo: Uuid,
) -> Result<Vec<u8>, PhotoError> {
    let ids = IdSecret::new(1, secret(ID_SECRET)).expect("id secret");
    let token = issue_link(conn, &ids, photo, at()).await.expect("link");
    open_link(conn, (&seal(keys), storage), &token, at())
        .await
        .map(|photo| photo.as_bytes().to_vec())
}

/// The row's key version and object name, read by the owner.
async fn row(pool: &PgPool, photo: Uuid) -> (i32, String) {
    sqlx::query_as("SELECT key_version, storage_key FROM privacy.photo WHERE id = $1")
        .bind(photo)
        .fetch_one(pool)
        .await
        .expect("photo row")
}

/// How many `ops.audit_log` rows name `photo` with `action`, read by the owner.
async fn audits(pool: &PgPool, photo: Uuid, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND action = $2")
        .bind(photo)
        .bind(action)
        .fetch_one(pool)
        .await
        .expect("audit count")
}

/// TD-PRIV-24, PRIV-023, PRIV-024: a v1 photo, the drill with `2:K2,1:K1` until `rotate_batch` and
/// `rotate_photos` both return 0, then `2:K2` alone: the link opens the same bytes and the v1
/// object is gone. Fails on a `main` without `rotate_photos`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_v1_photo_opens_after_the_drill_drops_the_old_key(pool: PgPool) {
    let store = dir("drill");
    let mut conn = app(&pool).await;
    let (id, bytes) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    let v1 = Vault::new(
        &secret(&key(1, 0x11)),
        &secret(&key(1, 0x21)),
        &secret(DB_URL),
    );
    store_contact(&mut conn, &v1.expect("v1"), &NewContact::default())
        .await
        .expect("contact");
    let index = format!("{},{}", key(2, 0x22), key(1, 0x21));
    let vault = Vault::new(&secret(&both()), &secret(&index), &secret(DB_URL)).expect("v2");
    let deps = (&seal(&both()), &store.1, &secret(ID_SECRET));
    let mut runs = 0;
    loop {
        let contacts = rotate_batch(&mut conn, &vault, 1).await.expect("contacts");
        let photos = rotate_photos(&mut conn, deps, 1).await.expect("photos");
        if contacts + photos == 0 {
            break;
        }
        runs += 1;
    }
    assert_eq!(runs, 1, "one contact and one photo moved in one run");
    let (version, name) = row(&pool, id).await;
    assert_eq!(version, 2);
    assert_ne!(name, format!("photos/{id}"));
    assert!(store.1.read_object(&format!("photos/{id}")).await.is_err());
    assert_eq!(audits(&pool, id, "privacy.photo_resealed").await, 1);
    assert_eq!(
        audits(&pool, id, "privacy.photo_opened").await,
        0,
        "a re-seal is no open"
    );
    let after = opened(&mut conn, &key(2, 0x12), &store.1, id).await;
    assert_eq!(after.expect("opens under v2 alone"), bytes);
}

/// TD-PRIV-24, PRIV-024: with no photo under an older version, a run moves nothing and returns 0.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_run_with_no_old_photos_returns_0(pool: PgPool) {
    let store = dir("none");
    let mut conn = app(&pool).await;
    let deps = (&seal(&both()), &store.1, &secret(ID_SECRET));
    assert_eq!(rotate_photos(&mut conn, deps, 10).await.expect("empty"), 0);
    let (id, _) = stored(&mut conn, &both(), &store.1).await;
    assert_eq!(rotate_photos(&mut conn, deps, 10).await.expect("v2"), 0);
    assert_eq!(row(&pool, id).await, (2, format!("photos/{id}")));
}

/// TD-PRIV-24, PRIV-023, D-1793: a re-seal stopped after its new object is written leaves the row
/// and its v1 object as they were (the photo opens) and one orphan; a rerun finishes the move.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_reseal_stopped_after_the_new_object_leaves_the_photo_readable(pool: PgPool) {
    let store = dir("stopped");
    let mut conn = app(&pool).await;
    let (id, bytes) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    let stops = StopsAfterPut(&store.1);
    let deps = (&seal(&both()), &stops, &secret(ID_SECRET));
    let stopped = rotate_photos(&mut conn, deps, 10).await;
    assert!(matches!(stopped, Err(PhotoError::Storage)));
    assert_eq!(row(&pool, id).await, (1, format!("photos/{id}")));
    assert_eq!(
        store.1.list_objects("photos/").await.expect("list").len(),
        2
    );
    let read = opened(&mut conn, &both(), &store.1, id).await;
    assert_eq!(read.expect("opens from the old row"), bytes);
    let deps = (&seal(&both()), &store.1, &secret(ID_SECRET));
    assert_eq!(rotate_photos(&mut conn, deps, 10).await.expect("rerun"), 1);
    let after = opened(&mut conn, &key(2, 0x12), &store.1, id).await;
    assert_eq!(after.expect("opens under v2 alone"), bytes);
}

/// TD-PRIV-24, PRIV-023: the re-sealed object names v2 and opens only under its row's version and
/// its own photo ID: under another photo's row, or with its row set back to v1, it fails.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_new_object_opens_only_under_its_rows_version_and_aad(pool: PgPool) {
    let store = dir("bound");
    let mut conn = app(&pool).await;
    let (id, _) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    let (other, _) = stored(&mut conn, &both(), &store.1).await;
    let deps = (&seal(&both()), &store.1, &secret(ID_SECRET));
    assert_eq!(rotate_photos(&mut conn, deps, 10).await.expect("rotate"), 1);
    let (_, name) = row(&pool, id).await;
    let sealed = store.1.read_object(&name).await.expect("new object");
    assert_eq!(sealed[..2], [0, 2], "the object names v2");
    let copy = "photos/copied-object";
    let put = IdempotencyKey::derive(&secret(ID_SECRET), "test.copy", &[copy]);
    store.1.put_object(&put, copy, sealed).await.expect("copy");
    sqlx::query("UPDATE privacy.photo SET storage_key = $1 WHERE id = $2")
        .bind(copy)
        .bind(other)
        .execute(&pool)
        .await
        .expect("point the other row at the copy");
    let moved = opened(&mut conn, &both(), &store.1, other).await;
    assert!(matches!(moved, Err(PhotoError::Sealed)));
    sqlx::query("UPDATE privacy.photo SET key_version = 1 WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("set the row back to v1");
    let stale = opened(&mut conn, &both(), &store.1, id).await;
    assert!(matches!(stale, Err(PhotoError::Sealed)));
}

/// TD-PRIV-24, PRIV-023, D-1793: a failed delete of the old object returns `Storage` with the row
/// already on v2 and its new object, which opens under `2:K2` alone; the old object is an orphan.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_old_object_delete_leaves_the_row_moved(pool: PgPool) {
    let store = dir("nodelete");
    let mut conn = app(&pool).await;
    let (id, bytes) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    let fails = Fault::FailsDelete(&store.1);
    let deps = (&seal(&both()), &fails, &secret(ID_SECRET));
    let failed = rotate_photos(&mut conn, deps, 10).await;
    assert!(matches!(failed, Err(PhotoError::Storage)));
    let (version, name) = row(&pool, id).await;
    assert_eq!(version, 2);
    assert_ne!(name, format!("photos/{id}"));
    assert!(store.1.read_object(&format!("photos/{id}")).await.is_ok());
    let after = opened(&mut conn, &key(2, 0x12), &store.1, id).await;
    assert_eq!(after.expect("opens under v2 alone"), bytes);
}

/// TD-PRIV-24, PRIV-023, D-3542: a row moved to another object while it is re-sealed is left alone:
/// the run returns 0, the row keeps v1 and the other object, the old object stays, and the new
/// object is one extra orphan.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_row_moved_meanwhile_is_left_alone(pool: PgPool) {
    let store = dir("moved");
    let mut conn = app(&pool).await;
    let (id, _) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    let moves = Fault::MovesRow(&store.1, &pool, id);
    let deps = (&seal(&both()), &moves, &secret(ID_SECRET));
    assert_eq!(rotate_photos(&mut conn, deps, 10).await.expect("run"), 0);
    assert_eq!(row(&pool, id).await, (1, "photos/elsewhere".to_owned()));
    assert!(store.1.read_object(&format!("photos/{id}")).await.is_ok());
    assert_eq!(
        store.1.list_objects("photos/").await.expect("list").len(),
        2
    );
    assert_eq!(audits(&pool, id, "privacy.photo_resealed").await, 0);
}

/// TD-PRIV-24, PRIV-023, D-3545: a connection inside a transaction is refused before any row or
/// object changes, so an outer rollback can never undo a move whose old object is gone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_connection_inside_a_transaction_is_refused(pool: PgPool) {
    let store = dir("intx");
    let mut conn = app(&pool).await;
    let (id, _) = stored(&mut conn, &key(1, 0x11), &store.1).await;
    sqlx::query("BEGIN")
        .execute(&mut conn)
        .await
        .expect("begin");
    let deps = (&seal(&both()), &store.1, &secret(ID_SECRET));
    let refused = rotate_photos(&mut conn, deps, 10).await;
    assert!(matches!(refused, Err(PhotoError::InTransaction)));
    sqlx::query("ROLLBACK")
        .execute(&mut conn)
        .await
        .expect("rollback");
    assert_eq!(row(&pool, id).await, (1, format!("photos/{id}")));
    assert_eq!(
        store.1.list_objects("photos/").await.expect("list").len(),
        1
    );
}

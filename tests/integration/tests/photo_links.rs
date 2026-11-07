//! Photos in storage and their links on a fresh database (TD-PRIV-04, TD-PRIV-08, TD-PRIV-13
//! photo cases; PRIV-013, PRIV-014, PRIV-023, PRIV-028).
//!
//! As the app role: a cleaned photo is sealed before the storage stand-in sees it, so the stored
//! bytes are ciphertext with no image header and no metadata; it opens only through a live link
//! whose token is in the path, and the link stops working after its lifetime; sealed bytes moved
//! to another photo's place fail to open. The fixture is a made-up 2 by 2 PNG with a fake GPS
//! text chunk, and the keys are fake.

use askhava_adapters::api::{IdempotencyKey, LocalStorage, StorageAdapter};
use askhava_core::api::{IdSecret, LinkToken};
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    LINK_TTL_SECS, PhotoError, PhotoSeal, clean, issue_link, open_link, photo_path, store_photo,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0090);
const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a73000000137445\
                       58744750530046414b452d4750532d33342e34384ea7dc82a60000001049444154789c6338a1\
                       a101440c100a00212e04611ee06dc20000000049454e44ae426082";
const GPS: &[u8] = b"FAKE-GPS-34.48N";

/// A fresh storage directory, removed when the test ends.
struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

struct Kit {
    _dir: Dir,
    storage: LocalStorage,
    seal: PhotoSeal,
    secret: SecretString,
    ids: IdSecret,
}

fn kit(test: &str) -> Kit {
    let root = std::env::temp_dir().join(format!("askhava-photos-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let storage = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    let key = SecretString::from(format!("1:{}", STANDARD.encode([3u8; 32])));
    let secret = SecretString::from("fake-id-secret-for-photo-links-0123456789");
    Kit {
        _dir: Dir(root),
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn stored(conn: &mut PgConnection, kit: &Kit, at: Timestamp) -> (Uuid, Vec<u8>) {
    let photo = clean(fixture()).await.expect("clean");
    let deps = (&kit.seal, &kit.storage, &kit.secret);
    let id = store_photo(conn, deps, JOB, &photo, at)
        .await
        .expect("store");
    (id, photo.as_bytes().to_vec())
}

/// TD-PRIV-04, TD-PRIV-08, TD-PRIV-13, PRIV-014, PRIV-023, PRIV-028: the bucket holds ciphertext
/// only; the photo opens through a live link with its token in the path, and not after it expires.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_photo_is_ciphertext_in_storage_and_opens_only_through_a_live_link(pool: PgPool) {
    let kit = kit("live");
    let mut conn = app(&pool).await;
    let at = Timestamp::from_second(1_800_000_000).expect("time");
    let (id, clean_bytes) = stored(&mut conn, &kit, at).await;
    let raw = kit
        .storage
        .read_object(&format!("photos/{id}"))
        .await
        .expect("object");
    assert!(!contains(&raw, GPS) && !contains(&raw, &clean_bytes[..16]));
    assert!(!raw.starts_with(&[0xFF, 0xD8]) && !contains(&raw, b"PNG"));
    let token = issue_link(&mut conn, &kit.ids, id, at).await.expect("link");
    let path = photo_path(&token);
    assert_eq!(path, format!("/photos/{}", token.as_str()));
    assert!(!path.contains('?'));
    let deps = (&kit.seal, &kit.storage);
    let soon = at + SignedDuration::from_secs(LINK_TTL_SECS - 1);
    let opened = open_link(&mut conn, deps, &token, soon)
        .await
        .expect("open");
    assert_eq!(opened.as_bytes(), clean_bytes);
    assert!(!contains(opened.as_bytes(), GPS));
    let late = at + SignedDuration::from_secs(LINK_TTL_SECS);
    assert_eq!(
        open_link(&mut conn, deps, &token, late).await,
        Err(PhotoError::LinkDead)
    );
    let other = LinkToken::parse(&"ab".repeat(32)).expect("token");
    assert_eq!(
        open_link(&mut conn, deps, &other, at).await,
        Err(PhotoError::LinkDead)
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_kind = 'privacy.photo' AND subject_id = $1",
    )
    .bind(id)
    .fetch_one(&mut conn)
    .await
    .expect("audit count");
    assert_eq!(audited, 3);
    let missing = issue_link(&mut conn, &kit.ids, Uuid::from_u128(9), at).await;
    assert_eq!(missing.err(), Some(PhotoError::NotFound));
}

/// TD-PRIV-04, PRIV-023: one photo's sealed bytes put in another photo's place fail to open.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_sealed_photo_moved_to_another_photo_fails_to_open(pool: PgPool) {
    let kit = kit("moved");
    let mut conn = app(&pool).await;
    let at = Timestamp::from_second(1_800_000_000).expect("time");
    let (first, _) = stored(&mut conn, &kit, at).await;
    let (second, _) = stored(&mut conn, &kit, at).await;
    let sealed = kit
        .storage
        .read_object(&format!("photos/{first}"))
        .await
        .expect("object");
    let key = IdempotencyKey::derive(&kit.secret, "test.copy", &["moved"]);
    kit.storage
        .put_object(&key, "photos/moved", sealed)
        .await
        .expect("copy");
    sqlx::query("UPDATE privacy.photo SET storage_key = 'photos/moved' WHERE id = $1")
        .bind(second)
        .execute(&pool)
        .await
        .expect("move as the migration role");
    let token = issue_link(&mut conn, &kit.ids, second, at)
        .await
        .expect("link");
    let opened = open_link(&mut conn, (&kit.seal, &kit.storage), &token, at).await;
    assert_eq!(opened, Err(PhotoError::Sealed));
}

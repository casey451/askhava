//! `server rotate-keys` and `server retire-index-version` against a fresh database as the app
//! role (PRIV-024, D-4418, D-4748, D-5412, D-5420).
//!
//! Proves: contacts, a staff phone, a photo and opt-outs sealed and listed under v1 end, after
//! `rotate-keys` under `2:,1:` (both vault keys rotating), with every row under v2 and the v1
//! digests kept; `retire-index-version 1` removes the v1 opt-outs; with v1 dropped a second run
//! removes the stale v1 phone digests and `check_index_keys` passes; the active version, `abc`,
//! no vault keys and a v1 photo with no photo settings exit non-zero changing nothing; a batch that
//! never reaches 0 stops at its cap while the real batches at size 2 finish within theirs; no
//! output holds a planted value. Fake 555-01xx numbers, fake keys and a made-up 2 by 2 PNG.

use askhava_adapters::api::LocalStorage;
use askhava_core::api::Settings;
use askhava_messaging::api::{
    RevokeChannel, check_suppression_versions, rekey_suppression_batch, suppress,
    suppression_backlog,
};
use askhava_privacy::api::{
    NewContact, PhotoSeal, Vault, clean, phone_digests, rotate_batch, rotate_business_batch,
    rotate_photos, rotation_backlog, store_contact, store_photo, store_staff_phone,
};
use askhava_server::rotate_keys::{BatchStop, run, run_batches};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONES: [&str; 5] = [
    "+19285550171",
    "+19285550172",
    "+19285550173",
    "+19285550174",
    "+19285550175",
];
const STAFF: &str = "+19285550179";
const ID_SECRET: &str = "fake-id-secret-for-rotate-keys-0123456789ab";
const PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a730000001049444154\
                       789c6338a1a101440c100a00212e04611ee06dc20000000049454e44ae426082";

fn key(version: u16, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// `(VAULT_KEY, VAULT_INDEX_KEY)` holding `versions`, newest first.
fn keys(versions: &[u16]) -> (String, String) {
    let list = |base: u8| {
        let entries: Vec<String> = versions
            .iter()
            .map(|v| key(*v, base + u8::try_from(*v).expect("version")))
            .collect();
        entries.join(",")
    };
    (list(0x10), list(0x20))
}

fn vault(versions: &[u16]) -> Vault {
    let (seal, index) = keys(versions);
    Vault::new(&secret(&seal), &secret(&index), &secret(DB_URL)).expect("keys")
}

/// The command's settings pairs: the vault keys for `versions` (none when empty) and, with
/// `storage`, the photo settings.
fn pairs(versions: &[u16], storage: Option<&str>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if !versions.is_empty() {
        let (seal, index) = keys(versions);
        pairs.push(("VAULT_KEY".to_owned(), seal));
        pairs.push(("VAULT_INDEX_KEY".to_owned(), index));
    }
    if let Some(base) = storage {
        pairs.push(("ASKHAVA_STORAGE_BASE_URL".to_owned(), base.to_owned()));
        pairs.push(("ASKHAVA_ID_SECRET".to_owned(), ID_SECRET.to_owned()));
    }
    pairs
}

fn settings() -> Settings {
    let pairs = [("ASKHAVA_APP_ENV", "dev"), ("ASKHAVA_DATABASE_URL", DB_URL)];
    Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned()))).expect("settings")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut conn)
        .await
        .expect("SET ROLE askhava_app");
    conn
}

/// A fresh storage directory as a `file://` URL, removed when the test ends.
struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!("askhava-rk-{test}-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary directory");
        Self(root)
    }

    fn url(&self) -> String {
        format!("file://{}", self.0.display())
    }
}

/// Runs the command `args` with `pairs`; answers its result (the error's text) and its output.
async fn command(
    pool: &PgPool,
    pairs: Vec<(String, String)>,
    args: &[&str],
) -> (Result<(), String>, String) {
    let mut conn = app(pool).await;
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let mut out = Vec::new();
    let result = run(&settings(), pairs, &mut conn, &args, &mut out).await;
    (
        result.map_err(|e| e.to_string()),
        String::from_utf8(out).expect("utf-8"),
    )
}

/// Seeds `phones` as contacts each opted out, one staff phone and one photo, all under v1.
async fn seed(pool: &PgPool, storage: &str, phones: &[&str]) -> Vec<Uuid> {
    let mut conn = app(pool).await;
    let v1 = vault(&[1]);
    let mut ids = Vec::new();
    for phone in phones {
        let new = NewContact {
            phone: Some(secret(phone)),
            ..NewContact::default()
        };
        ids.push(store_contact(&mut conn, &v1, &new).await.expect("contact"));
        let at: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
        suppress(&mut conn, &v1, phone, (RevokeChannel::Portal, "portal"), at)
            .await
            .expect("opt-out");
    }
    store_staff_phone(&mut conn, &v1, Uuid::new_v4(), STAFF)
        .await
        .expect("staff phone");
    let hex: String = PNG_HEX.split_whitespace().collect();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"));
    let photo = clean(bytes.collect()).await.expect("clean");
    let (seal, store) = (
        PhotoSeal::new(&secret(&keys(&[1]).0)).expect("seal"),
        LocalStorage::new(storage).expect("storage"),
    );
    let at = Timestamp::from_second(1_800_000_000).expect("time");
    store_photo(
        &mut conn,
        (&seal, &store, &secret(ID_SECRET)),
        Uuid::new_v4(),
        &photo,
        at,
    )
    .await
    .expect("photo");
    ids
}

/// What each batch still has to move under keys `2:,1:`: vault, business, photos, opt-outs.
async fn backlog(pool: &PgPool) -> (u64, u64, u64, u64) {
    let (mut conn, keys) = (app(pool).await, vault(&[2, 1]));
    let left = rotation_backlog(&mut conn, &keys).await.expect("backlog");
    let optouts = suppression_backlog(&mut conn, &keys)
        .await
        .expect("opt-out backlog");
    (left.vault, left.business, left.photos, optouts)
}

/// Each contact's homeowner-phone digest versions, newest first (no digest is kept).
async fn digest_versions(pool: &PgPool, contacts: &[Uuid]) -> Vec<Vec<u16>> {
    let mut conn = app(pool).await;
    let mut all = Vec::new();
    for contact in contacts {
        let digests = phone_digests(&mut conn, *contact).await.expect("digests");
        all.push(digests.into_iter().map(|(version, _)| version).collect());
    }
    all
}

/// Whether no opt-out is held under a version the key set `2:` lacks (the start check's half).
async fn only_v2_optouts(pool: &PgPool) -> bool {
    check_suppression_versions(&mut app(pool).await, &vault(&[2]))
        .await
        .is_ok()
}

/// No phone (with or without its country code), key, secret or contact ID in `text`.
fn assert_clean(text: &str, contacts: &[Uuid]) {
    let (seal, index) = keys(&[2, 1]);
    let mut planted: Vec<String> = PHONES
        .iter()
        .chain([&STAFF])
        .flat_map(|p| [p.to_string(), p[2..].to_owned(), p[5..].to_owned()])
        .collect();
    planted.extend(
        seal.split(',')
            .chain(index.split(','))
            .map(|k| k[2..].to_owned()),
    );
    planted.push(ID_SECRET.to_owned());
    planted.extend(contacts.iter().map(Uuid::to_string));
    for value in planted {
        assert!(
            !text.contains(&value),
            "output holds a planted value: {text}"
        );
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_two_version_rotation_runs_to_the_end_and_retires_v1(pool: PgPool) {
    let dir = Dir::new("full");
    let ids = seed(&pool, &dir.url(), &PHONES[..2]).await;
    let mut said = String::new();
    let (done, out) = command(&pool, pairs(&[2, 1], Some(&dir.url())), &["rotate-keys"]).await;
    assert_eq!(done, Ok(()), "{out}");
    assert_eq!(out.lines().count(), 4, "{out}");
    assert!(out.lines().all(|l| l.ends_with(", 0 skipped")), "{out}");
    said.push_str(&out);
    assert_eq!(backlog(&pool).await, (0, 0, 0, 0), "every row under v2");
    assert_eq!(digest_versions(&pool, &ids).await, [[2, 1], [2, 1]]);
    assert!(
        !only_v2_optouts(&pool).await,
        "the v1 opt-outs are still held"
    );
    for refused in [
        ["retire-index-version", "2"],
        ["retire-index-version", "abc"],
    ] {
        let (done, out) = command(&pool, pairs(&[2, 1], None), &refused).await;
        said.push_str(&done.expect_err("refused"));
        assert!(out.is_empty());
        assert!(!only_v2_optouts(&pool).await, "nothing was retired");
    }
    let (done, out) = command(&pool, pairs(&[2, 1], None), &["retire-index-version", "1"]).await;
    assert_eq!(done, Ok(()));
    assert!(out.contains(": 2 opt-out rows removed"), "{out}");
    said.push_str(&out);
    assert!(only_v2_optouts(&pool).await, "v1 retired");
    assert_eq!(
        backlog(&pool).await.3,
        0,
        "every opt-out still listed under v2"
    );
    let (done, out) = command(&pool, pairs(&[2], Some(&dir.url())), &["rotate-keys"]).await;
    assert_eq!(done, Ok(()), "{out}");
    said.push_str(&out);
    assert_eq!(digest_versions(&pool, &ids).await, [[2], [2]]);
    askhava_server::check_index_keys(&pool, Some(&vault(&[2])))
        .await
        .expect("start check");
    assert_clean(&said, &ids);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_vault_keys_and_a_stranded_photo_exit_non_zero_changing_nothing(pool: PgPool) {
    let dir = Dir::new("refused");
    let ids = seed(&pool, &dir.url(), &PHONES[..1]).await;
    let (done, out) = command(&pool, pairs(&[], Some(&dir.url())), &["rotate-keys"]).await;
    let mut said = done.expect_err("no vault");
    assert!(out.is_empty());
    let (done, out) = command(&pool, pairs(&[2, 1], None), &["rotate-keys"]).await;
    let stranded = done.expect_err("stranded photo");
    assert!(
        stranded.contains("1 photos") && stranded.contains("ASKHAVA_STORAGE_BASE_URL"),
        "{stranded}"
    );
    assert!(out.is_empty());
    said.push_str(&stranded);
    assert_eq!(backlog(&pool).await, (1, 1, 1, 1), "nothing moved");
    assert_eq!(digest_versions(&pool, &ids).await, [[1]]);
    assert_clean(&said, &ids);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_batch_that_never_reaches_zero_stops_at_its_cap(pool: PgPool) {
    let mut conn = app(&pool).await;
    let mut calls = 0;
    let never = async |_: &mut PgConnection, _: i64| {
        calls += 1;
        Ok(1)
    };
    assert_eq!(
        run_batches(&mut conn, 2, 5, never).await,
        Err(BatchStop::Capped)
    );
    assert_eq!(calls, 4, "5 / 2 + 2 passes");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_real_batches_at_size_two_finish_within_their_caps(pool: PgPool) {
    let dir = Dir::new("cap");
    seed(&pool, &dir.url(), &PHONES).await;
    for line in 0..4 {
        store_staff_phone(
            &mut app(&pool).await,
            &vault(&[1]),
            Uuid::new_v4(),
            &format!("+1928555018{line}"),
        )
        .await
        .expect("staff");
    }
    let (v, mut conn) = (vault(&[2, 1]), app(&pool).await);
    let backlog = rotation_backlog(&mut conn, &v).await.expect("backlog");
    assert_eq!((backlog.vault, backlog.business, backlog.photos), (5, 5, 1));
    let moved = run_batches(
        &mut conn,
        2,
        backlog.vault,
        async |db: &mut PgConnection, n| rotate_batch(db, &v, n).await.map_err(|e| e.to_string()),
    )
    .await;
    assert_eq!(moved, Ok(5));
    let moved = run_batches(
        &mut conn,
        2,
        backlog.business,
        async |db: &mut PgConnection, n| {
            rotate_business_batch(db, &v, n)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await;
    assert_eq!(moved, Ok(5));
    let (seal, store) = (
        PhotoSeal::new(&secret(&keys(&[2, 1]).0)).expect("seal"),
        LocalStorage::new(&dir.url()).expect("storage"),
    );
    let id = secret(ID_SECRET);
    let moved = run_batches(
        &mut conn,
        2,
        backlog.photos,
        async |db: &mut PgConnection, n| {
            rotate_photos(db, (&seal, &store, &id), n)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await;
    assert_eq!(moved, Ok(1));
    let optouts = suppression_backlog(&mut conn, &v)
        .await
        .expect("opt-out backlog");
    assert_eq!(optouts, 5);
    let batch = async |db: &mut PgConnection, n: i64| {
        let moved = rekey_suppression_batch(db, &v, usize::try_from(n).expect("size"))
            .await
            .map_err(|e| e.to_string())?;
        Ok(u64::try_from(moved).expect("count"))
    };
    assert_eq!(run_batches(&mut conn, 2, optouts, batch).await, Ok(5));
    assert_eq!(self::backlog(&pool).await, (0, 0, 0, 0));
}

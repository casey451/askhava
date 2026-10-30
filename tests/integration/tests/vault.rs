//! The contact vault on a fresh database (TD-PRIV-01, TD-PRIV-09, TD-SEC-26; PRIV-001 to PRIV-004, PRIV-024, SEC-047).
//!
//! Proves, as the app role, that no column outside `privacy` holds a contact field and the vault's
//! columns hold only sealed bytes; that a sealed value copied to another row or column fails to
//! open; that the vault keys are refused when they repeat or sit in the database credential; that
//! every plaintext read writes an audit row naming its reader, purpose and field; that the
//! rotation drill reads v1 rows while v2 is active, ends with no v1 row or digest and still finds
//! every number; and that an hour's reads above three times the 14-day baseline, or a read with an
//! unregistered purpose, raises one High alert. The export and the runbook's key inventory are
//! the nightly export's and T117's cases of TD-PRIV-09. All data and keys are fake.

use askhava_integration::set_app_role;
use askhava_ops::api::export_audit;
use askhava_privacy::api::{
    BASELINE_HOURS, Field, IndexPurpose, NewContact, Purpose, Reader, Vault, VaultError,
    check_read_rates, find_by_phone, read_contact, rotate_batch, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONE: &str = "(928) 555-0101";
const SEND: (Reader, Purpose) = (Reader::Messaging, Purpose::Send);

/// A `<version>:<base64>` key entry of 32 copies of `byte`.
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
    vault(&key(1, 0x11), &key(1, 0x21))
}

fn zelda(phone: &str) -> NewContact {
    NewContact {
        name: Some(secret("Zelda Canary")),
        phone: Some(secret(phone)),
        email: Some(secret("zelda@example.com")),
        address: Some(secret("12 Test Street")),
        onsite_contact: None,
    }
}

async fn app_conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn count(conn: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(conn)
        .await
        .expect("count")
}

fn shown(value: Option<&SecretString>) -> Option<&str> {
    value.map(ExposeSecret::expose_secret)
}

/// TD-PRIV-01, PRIV-001: contact-field columns exist only in `privacy`, and there only as bytes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_column_outside_privacy_holds_a_contact_field(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let outside = count(
        &mut conn,
        "SELECT count(*) FROM information_schema.columns WHERE table_schema NOT IN \
         ('privacy', 'pg_catalog', 'information_schema') AND column_name ~ \
         '(phone|email|address|street|first_name|last_name|full_name|onsite_contact)'",
    )
    .await;
    assert_eq!(outside, 0, "a contact field outside the vault");
    let plain = count(
        &mut conn,
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'privacy' \
         AND table_name = 'contact' AND column_name IN ('name', 'phone', 'email', 'address', \
         'onsite_contact') AND data_type <> 'bytea'",
    )
    .await;
    assert_eq!(plain, 0, "every vault field is sealed bytes");
}

/// TD-PRIV-01, PRIV-002: the stored bytes hold no plaintext, and a sealed value copied to another
/// row or column fails to open (associated data).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_sealed_value_opens_only_in_its_own_place(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let a = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("a");
    let b = store_contact(&mut conn, &vault, &zelda("928.555.0102"))
        .await
        .expect("b");
    let raw: Vec<u8> = sqlx::query_scalar("SELECT name FROM privacy.contact WHERE id = $1")
        .bind(a)
        .fetch_one(&mut *conn)
        .await
        .expect("raw");
    assert!(
        !raw.windows(5).any(|w| w == b"Zelda"),
        "plaintext in the column"
    );
    let details = read_contact(&mut conn, &vault, SEND, a, &[Field::Name, Field::Phone])
        .await
        .expect("read");
    assert_eq!(shown(details.name.as_ref()), Some("Zelda Canary"));
    assert_eq!(shown(details.phone.as_ref()), Some("+19285550101"));
    assert!(
        details.email.is_none(),
        "a field not asked for stays closed"
    );
    for sql in [
        "UPDATE privacy.contact SET name = (SELECT name FROM privacy.contact WHERE id = $1) WHERE id = $2",
        "UPDATE privacy.contact SET email = (SELECT name FROM privacy.contact WHERE id = $1) WHERE id = $1 AND $2 <> $1",
    ] {
        sqlx::query(sql)
            .bind(a)
            .bind(b)
            .execute(&mut *conn)
            .await
            .expect("copy");
    }
    let moved = read_contact(&mut conn, &vault, SEND, b, &[Field::Name]).await;
    assert_eq!(
        moved.err(),
        Some(VaultError::Sealed),
        "copied to another row"
    );
    let column = read_contact(&mut conn, &vault, SEND, a, &[Field::Email]).await;
    assert_eq!(
        column.err(),
        Some(VaultError::Sealed),
        "copied to another column"
    );
}

/// TD-PRIV-01, PRIV-002, PRIV-024: the vault key is not the database credential, nor the index key.
#[test]
fn the_vault_keys_are_separate_from_the_database_credential() {
    let seal = key(1, 0x11);
    let in_url = secret(&format!("postgres://app:{}@db.invalid/askhava", &seal[2..]));
    let refused =
        |s: &str, i: &str, url: &SecretString| Vault::new(&secret(s), &secret(i), url).err();
    let msg = Some(VaultError::Keys("a key is inside the database credential"));
    assert_eq!(refused(&seal, &key(1, 0x21), &in_url), msg);
    let shared = Some(VaultError::Keys(
        "VAULT_KEY and VAULT_INDEX_KEY share a key",
    ));
    assert_eq!(refused(&seal, &key(2, 0x11), &secret(DB_URL)), shared);
    for bad in ["", "1", "x:AAAA", "0:AAAA", "1:@@@", "1:AAAA"] {
        assert!(
            refused(bad, &key(1, 0x21), &secret(DB_URL)).is_some(),
            "{bad:?}"
        );
    }
    let repeated = format!("{},{}", key(1, 0x11), key(1, 0x12));
    assert!(refused(&repeated, &key(1, 0x21), &secret(DB_URL)).is_some());
}

/// TD-PRIV-01, PRIV-002, D-752: a key percent-encoded, unpadded or URL-safe in the credential is
/// still found there; a stray `%` is not a key.
#[test]
fn a_key_hidden_by_url_encoding_is_still_refused() {
    let seal = key(1, 0xfb);
    let b64 = &seal[2..];
    assert!(b64.contains('+') && b64.contains('/') && b64.ends_with('='));
    let upper = b64
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D");
    let lower = b64.replace('/', "%2f").replace('=', "%3d");
    let unpadded = b64.trim_end_matches('=').to_owned();
    let url_safe = unpadded.replace('+', "-").replace('/', "_");
    let msg = Some(VaultError::Keys("a key is inside the database credential"));
    for hidden in [upper, lower, unpadded, url_safe] {
        let url = secret(&format!("postgres://app:{hidden}@db.invalid/askhava"));
        let err = Vault::new(&secret(&seal), &secret(&key(1, 0x21)), &url).err();
        assert_eq!(err, msg, "{hidden}");
    }
    let stray = secret("postgres://app:pa%zz%2@db.invalid/askhava");
    assert!(Vault::new(&secret(&seal), &secret(&key(1, 0x21)), &stray).is_ok());
}

/// PRIV-003, D-752: a plaintext read on a connection already inside a transaction is refused, so
/// the caller's rollback can never undo the audit rows of plaintext that left.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_read_inside_the_callers_transaction_is_refused(pool: PgPool) {
    use sqlx::Connection as _;
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let id = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("store");
    let mut tx = conn.begin().await.expect("begin");
    let read = read_contact(&mut tx, &vault, SEND, id, &[Field::Name]).await;
    assert_eq!(read.err(), Some(VaultError::InTransaction));
    tx.rollback().await.expect("rollback");
    let read = read_contact(&mut conn, &vault, SEND, id, &[Field::Name]).await;
    assert!(read.is_ok(), "a bare connection reads");
}

/// PRIV-002, D-752: two contacts racing for one phone: the one whose blind-index insert meets the
/// other's committed digest gets `PhoneTaken`, not a database error.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_phone_taken_in_a_race_is_phone_taken(pool: PgPool) {
    use sqlx::Connection as _;
    let vault = v1();
    let mut first = app_conn(&pool).await;
    let holder = store_contact(&mut first, &vault, &NewContact::default())
        .await
        .expect("holder");
    let (version, digest) = vault.blind_index(IndexPurpose::HomeownerPhone, "+19285550101")[0];
    let mut tx = first.begin().await.expect("begin");
    sqlx::query(
        "INSERT INTO privacy.contact_index (purpose, key_version, digest, contact_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(IndexPurpose::HomeownerPhone.label())
    .bind(i32::from(version))
    .bind(&digest[..])
    .bind(holder)
    .execute(&mut *tx)
    .await
    .expect("plant the racing digest");
    let mut second = app_conn(&pool).await;
    let racer = tokio::spawn(async move { store_contact(&mut second, &v1(), &zelda(PHONE)).await });
    let mut watch = pool.acquire().await.expect("watch");
    while count(
        &mut watch,
        "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock'",
    )
    .await
        == 0
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tx.commit().await.expect("commit");
    let raced = racer.await.expect("join");
    assert_eq!(raced.err(), Some(VaultError::PhoneTaken));
    let contacts = count(&mut first, "SELECT count(*) FROM privacy.contact").await;
    assert_eq!(contacts, 1, "the loser stored nothing");
}

/// TD-PRIV-01, PRIV-003, PRIV-004: every plaintext read writes one audit row per field naming the
/// reader and purpose, in the read's transaction.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_plaintext_read_is_audited_with_reader_and_purpose(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let id = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("store");
    let readback = (Reader::Calls, Purpose::CallReadback);
    read_contact(
        &mut conn,
        &vault,
        readback,
        id,
        &[Field::Name, Field::Address],
    )
    .await
    .expect("read");
    let rows = export_audit(&mut conn, 0, 100).await.expect("export");
    let reads: Vec<_> = rows
        .iter()
        .filter(|r| r.action.starts_with("vault_read."))
        .map(|r| (r.action.as_str(), r.subject_id, r.field.as_deref()))
        .collect();
    let action = "vault_read.calls.call_readback";
    assert_eq!(
        reads,
        [(action, id, Some("name")), (action, id, Some("address"))]
    );
    assert!(
        rows.iter()
            .any(|r| r.action == "contact_created" && r.subject_id == id)
    );
    let missing = read_contact(&mut conn, &vault, SEND, Uuid::nil(), &[Field::Name]).await;
    assert_eq!(missing.err(), Some(VaultError::NotFound));
}

/// The phone is stored normalized, found by its blind index in any spelling, and held by one
/// contact only (PRIV-002, PRIV-024).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_phone_is_found_by_its_blind_index_only(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let id = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("store");
    for spelling in ["928-555-0101", "+1 928 555 0101", "19285550101"] {
        let found = find_by_phone(&mut conn, &vault, spelling)
            .await
            .expect("find");
        assert_eq!(found, Some(id), "{spelling}");
    }
    let none = find_by_phone(&mut conn, &vault, "928-555-0199")
        .await
        .expect("find");
    assert_eq!(none, None);
    let again = store_contact(&mut conn, &vault, &zelda("928 555 0101")).await;
    assert_eq!(again.err(), Some(VaultError::PhoneTaken));
    for bad in ["555-0101", "028-555-0101", "928-555-0101#", "2928-555-0101"] {
        let refused = store_contact(&mut conn, &vault, &zelda(bad)).await;
        assert_eq!(refused.err(), Some(VaultError::BadPhone), "{bad}");
    }
    let bare = store_contact(&mut conn, &vault, &NewContact::default()).await;
    assert!(bare.is_ok(), "a contact may start with no field");
}

/// TD-PRIV-09, PRIV-024: the rotation drill from v1 to v2 with both versions readable.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_rotation_drill_moves_every_row_to_v2(pool: PgPool) {
    let old = v1();
    let mut conn = app_conn(&pool).await;
    let phones = ["928-555-0101", "928-555-0102", "928-555-0103"];
    let mut ids = Vec::new();
    for phone in phones {
        ids.push(
            store_contact(&mut conn, &old, &zelda(phone))
                .await
                .expect("store"),
        );
    }
    let unlisted = NewContact {
        name: Some(secret("Sam Sample")),
        ..NewContact::default()
    };
    ids.push(
        store_contact(&mut conn, &old, &unlisted)
            .await
            .expect("no phone"),
    );
    let both = vault(
        &format!("{},{}", key(2, 0x12), key(1, 0x11)),
        &format!("{},{}", key(1, 0x21), key(2, 0x22)),
    );
    assert_eq!((both.seal_version(), both.index_version()), (2, 2));
    let read = read_contact(&mut conn, &both, SEND, ids[0], &[Field::Name]).await;
    assert!(read.is_ok(), "a v1 row opens while v2 is active");
    let found = find_by_phone(&mut conn, &both, phones[1])
        .await
        .expect("find");
    assert_eq!(
        found,
        Some(ids[1]),
        "a v1 digest is found while v2 is active"
    );
    assert_eq!(rotate_batch(&mut conn, &both, 3).await.expect("rotate"), 3);
    assert_eq!(rotate_batch(&mut conn, &both, 3).await.expect("rotate"), 1);
    assert_eq!(rotate_batch(&mut conn, &both, 2).await.expect("rotate"), 0);
    let v1_rows = count(
        &mut conn,
        "SELECT count(*) FROM privacy.contact WHERE oldest_key_version < 2",
    )
    .await;
    let v1_digests = count(
        &mut conn,
        "SELECT count(*) FROM privacy.contact_index WHERE key_version < 2",
    )
    .await;
    assert_eq!((v1_rows, v1_digests), (0, 0));
    let new = vault(&key(2, 0x12), &key(2, 0x22));
    for (phone, id) in phones.iter().zip(&ids) {
        assert_eq!(
            find_by_phone(&mut conn, &new, phone).await.expect("find"),
            Some(*id)
        );
        let details = read_contact(&mut conn, &new, SEND, *id, &Field::ALL)
            .await
            .expect("read");
        assert_eq!(shown(details.address.as_ref()), Some("12 Test Street"));
    }
    let sam = read_contact(&mut conn, &new, SEND, ids[3], &Field::ALL).await;
    assert_eq!(
        shown(sam.expect("no phone").name.as_ref()),
        Some("Sam Sample")
    );
    let stale = read_contact(&mut conn, &old, SEND, ids[0], &[Field::Name]).await;
    assert_eq!(
        stale.err(),
        Some(VaultError::Sealed),
        "v1 alone no longer opens a moved row"
    );
}

/// Plants `per_hour` reads of `action` in each of the 336 hours before `hour`.
async fn plant_baseline(conn: &mut PgConnection, action: &str, hour: Timestamp, per_hour: i32) {
    sqlx::query(
        "INSERT INTO privacy.read_count (hour, action, reads) SELECT $1 - make_interval(hours => h), $2, $3 \
         FROM generate_series(1, $4) AS h",
    )
    .bind(hour.to_sqlx())
    .bind(action)
    .bind(per_hour)
    .bind(BASELINE_HOURS as i32)
    .execute(conn)
    .await
    .expect("plant");
}

async fn alerts(conn: &mut PgConnection, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'")
        .bind(kind)
        .fetch_one(conn)
        .await
        .expect("alerts")
}

/// TD-SEC-26, SEC-047: a burst at 4x the 14-day baseline raises one High alert; 3x does not; a
/// second check of the hour raises nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_read_burst_above_three_times_baseline_alerts_once(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let id = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("store");
    let hour: jiff_sqlx::Timestamp = sqlx::query_scalar("SELECT date_trunc('hour', now(), 'UTC')")
        .fetch_one(&mut *conn)
        .await
        .expect("hour");
    let hour = hour.to_jiff();
    plant_baseline(&mut conn, "vault_read.calls.bridge_call", hour, 2).await;
    plant_baseline(&mut conn, "vault_read.messaging.send", hour, 2).await;
    let bridge = (Reader::Calls, Purpose::BridgeCall);
    for _ in 0..8 {
        read_contact(&mut conn, &vault, bridge, id, &[Field::Phone])
            .await
            .expect("bridge");
    }
    for _ in 0..6 {
        read_contact(&mut conn, &vault, SEND, id, &[Field::Phone])
            .await
            .expect("send");
    }
    assert_eq!(check_read_rates(&mut conn, hour).await.expect("check"), 1);
    assert_eq!(check_read_rates(&mut conn, hour).await.expect("again"), 0);
    assert_eq!(alerts(&mut conn, "privacy.vault_read_spike").await, 1);
}

/// TD-SEC-26, SEC-047, PRIV-004: a read whose reader, purpose or field is unregistered is refused
/// and raises a High alert at once, before any plaintext or audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_unregistered_purpose_is_refused_and_alerted_at_once(pool: PgPool) {
    let vault = v1();
    let mut conn = app_conn(&pool).await;
    let id = store_contact(&mut conn, &vault, &zelda(PHONE))
        .await
        .expect("store");
    let cases = [
        ((Reader::Calls, Purpose::Send), Field::Name),
        ((Reader::Calls, Purpose::CallReadback), Field::Phone),
        ((Reader::Picks, Purpose::Release), Field::Name),
        ((Reader::PortalsOwner, Purpose::CallbackAlert), Field::Phone),
        ((Reader::Privacy, Purpose::Rotation), Field::Name),
    ];
    for (access, field) in cases {
        let refused = read_contact(&mut conn, &vault, access, id, &[field]).await;
        assert_eq!(
            refused.err(),
            Some(VaultError::UnregisteredPurpose),
            "{access:?}"
        );
    }
    assert_eq!(
        alerts(&mut conn, "privacy.vault_read_unregistered").await,
        5
    );
    let subjects: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT subject_kind, subject_id FROM ops.alert WHERE kind = 'privacy.vault_read_unregistered'",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("subjects");
    let mut kinds: Vec<_> = subjects.iter().map(|(k, _)| k.as_str()).collect();
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        [
            "vault_read.calls.call_readback",
            "vault_read.calls.send",
            "vault_read.picks.release",
            "vault_read.portals_owner.callback_alert",
            "vault_read.privacy.rotation",
        ],
        "each alert names its reader and purpose (D-752)"
    );
    assert!(subjects.iter().all(|(_, s)| *s == id));
    let reads = count(
        &mut conn,
        "SELECT count(*) FROM ops.audit_log WHERE action LIKE 'vault_read.%'",
    )
    .await;
    assert_eq!(reads, 0, "a refused read reads nothing");
}

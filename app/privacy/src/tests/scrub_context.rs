//! The contact-aware inbound read on a real database (TD-AI-19 privacy cases; AI-003, AI-020,
//! PRIV-001 to PRIV-003, PRIV-007, PRIV-024, SEC-047; D-1752). Every name, phone and address is
//! fictional; each test runs as the app role, so the grants of `0130` are what it relies on.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use askhava_ops::api::export_audit;

use crate::api::{
    BASELINE_HOURS, InboundWords, NewContact, VaultError, check_read_rates, delete_my_data,
    purge_contact, read_inbound_for, rotate_batch, store_contact,
};
use crate::vault::Vault;

const WORDS: InboundWords<'static> = InboundWords {
    emergency: &[],
    opt_out: &["stop"],
    help: &["help"],
    start: &["start"],
};
const INBOUND: &str = "vault_read.messaging.inbound";

fn vault_with(seal: &str, index: &str) -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&seal.to_owned().into(), &index.to_owned().into(), &url).expect("keys")
}

fn key(version: u8, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn v1() -> Vault {
    vault_with(&key(1, 0x57), &key(1, 0x67))
}

async fn app(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *conn)
        .await
        .expect("app role");
    conn
}

async fn contact(db: &mut PgConnection, vault: &Vault, phone: &str, name: Option<&str>) -> Uuid {
    let new = NewContact {
        name: name.map(|n| SecretString::from(n.to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    store_contact(db, vault, &new).await.expect("contact")
}

/// The scrubbed text of `raw` for `who`, lower-cased.
async fn read(db: &mut PgConnection, vault: &Vault, who: Uuid, raw: &str) -> String {
    let read = read_inbound_for(db, vault, who, raw, &WORDS)
        .await
        .expect("read");
    read.text.as_str().to_lowercase()
}

/// Her stated-name rows, all or only those under key and index version 2.
async fn stated(db: &mut PgConnection, id: Uuid, moved: bool) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM privacy.stated_name WHERE contact_id = $1
           AND (NOT $2 OR (key_version = 2 AND index_version = 2))"#,
        id,
        moved
    )
    .fetch_one(db)
    .await
    .expect("count")
}

/// The audit rows with `action` on a record of `kind`, as their subject IDs (PRIV-003).
async fn audited(db: &mut PgConnection, action: &str, kind: &str) -> Vec<Uuid> {
    let rows = export_audit(db, 0, 500).await.expect("export");
    rows.into_iter()
        .filter(|r| r.action == action && r.subject_kind == kind)
        .map(|r| r.subject_id)
        .collect()
}

/// TD-AI-19 (AI-020): a name stated in one text is removed from a later one in any case,
/// possessive or NFKC form; a repeat stores no second row; the stored name is sealed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_a_stated_name_is_removed_from_later_texts(pool: PgPool) {
    let (vault, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &vault, "928-555-0161", None).await;
    let first = read(&mut db, &vault, her, "I'm Zebulon Quartermaine").await;
    assert!(!first.contains("zebulon") && !first.contains("quartermaine"));
    for later in [
        "zebulon's sink leaks",
        "ZEBULON here again",
        "Ｚｅｂｕｌｏｎ Ｑｕａｒｔｅｒｍａｉｎｅ asks",
    ] {
        let text = read(&mut db, &vault, her, later).await;
        assert!(!text.contains("zebulon"), "{later}");
        assert!(!text.contains("quartermaine"), "{later}");
        assert!(!text.contains("ｚｅｂｕｌｏｎ"), "{later}");
    }
    read(&mut db, &vault, her, "this is Zebulon").await;
    assert_eq!(stated(&mut db, her, false).await, 2, "one row per name");
    let raw = sqlx::query_scalar!("SELECT name FROM privacy.stated_name")
        .fetch_all(&mut *db)
        .await
        .expect("rows");
    for sealed in raw {
        let shown = String::from_utf8_lossy(&sealed).to_lowercase();
        assert!(!shown.contains("zebulon") && !shown.contains("quartermaine"));
    }
}

/// TD-AI-19 (AI-003, AI-020): the vault name goes without any introduction; a Spanish
/// introduction is remembered; another homeowner's text keeps the word.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_vault_and_spanish_names_and_no_cross_talk(pool: PgPool) {
    let (vault, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &vault, "928-555-0162", Some("Ottoline Vexley")).await;
    let other = contact(&mut db, &vault, "928-555-0163", None).await;
    let text = read(
        &mut db,
        &vault,
        her,
        "Vexley house, ottoline's faucet drips",
    )
    .await;
    assert!(
        !text.contains("vexley") && !text.contains("ottoline"),
        "{text}"
    );
    read(&mut db, &vault, her, "hola, me llamo Quillon").await;
    read(&mut db, &vault, her, "soy Xantipa").await;
    let text = read(&mut db, &vault, her, "quillon y xantipa necesitan ayuda").await;
    assert!(
        !text.contains("quillon") && !text.contains("xantipa"),
        "{text}"
    );
    let theirs = read(&mut db, &vault, other, "the zebulon quillon model leaks").await;
    assert!(
        theirs.contains("quillon"),
        "another homeowner's text is untouched"
    );
    assert_eq!(stated(&mut db, other, false).await, 0);
    let unknown = read_inbound_for(&mut db, &vault, Uuid::from_u128(0x5eed), "hi", &WORDS).await;
    assert_eq!(unknown.err(), Some(VaultError::NotFound));
}

/// TD-AI-19 (PRIV-003, D-1752): inside the caller's transaction the read is allowed; its audit
/// row, its count and a new stated name roll back with it, and the same text read again is
/// audited again, once per text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_the_read_rolls_back_with_the_delivery(pool: PgPool) {
    let (vault, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &vault, "928-555-0164", None).await;
    let mut tx = db.begin().await.expect("delivery");
    read(&mut tx, &vault, her, "I'm Zebulon").await;
    tx.rollback().await.expect("rollback");
    assert_eq!(stated(&mut db, her, false).await, 0);
    assert_eq!(audited(&mut db, INBOUND, "privacy.contact").await.len(), 0);
    let mut tx = db.begin().await.expect("resent delivery");
    read(&mut tx, &vault, her, "I'm Zebulon").await;
    tx.commit().await.expect("commit");
    assert_eq!(stated(&mut db, her, false).await, 1);
    assert_eq!(audited(&mut db, INBOUND, "privacy.contact").await, [her]);
    let reads = sqlx::query_scalar!(
        r#"SELECT coalesce(sum(reads), 0)::int AS "n!" FROM privacy.read_count WHERE action = $1"#,
        INBOUND
    )
    .fetch_one(&mut *db)
    .await
    .expect("reads");
    assert_eq!(reads, 1, "counted once per text");
}

async fn plant_baseline(db: &mut PgConnection, hour: jiff::Timestamp, per_hour: i32) {
    sqlx::query!(
        "INSERT INTO privacy.read_count (hour, action, reads) \
         SELECT $1::timestamptz - make_interval(hours => h), $2, $3 FROM generate_series(1, $4) AS h",
        hour.to_sqlx() as _,
        INBOUND,
        per_hour,
        BASELINE_HOURS as i32,
    )
    .execute(db)
    .await
    .expect("baseline");
}

/// TD-AI-19 (SEC-047, D-750, D-1752): inbound reads count under the grant's own action: a steady
/// hour at the baseline raises nothing; a burst above three times it raises one High alert
/// (`check_read_rates` returns the alerts it raised; `tests/integration/tests/vault.rs` checks the
/// alert rows).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_a_burst_of_texts_raises_one_alert(pool: PgPool) {
    let (vault, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &vault, "928-555-0165", None).await;
    let hour = sqlx::query_scalar!(r#"SELECT date_trunc('hour', now(), 'UTC') AS "h!""#)
        .fetch_one(&mut *db)
        .await
        .expect("hour");
    let hour = hour.to_jiff();
    plant_baseline(&mut db, hour, 2).await;
    for _ in 0..2 {
        read(&mut db, &vault, her, "the sink still drips").await;
    }
    assert_eq!(check_read_rates(&mut db, hour).await.expect("steady"), 0);
    for _ in 0..5 {
        read(&mut db, &vault, her, "the sink still drips").await;
    }
    assert_eq!(check_read_rates(&mut db, hour).await.expect("burst"), 1);
}

/// TD-AI-19 (PRIV-007, PRIV-011, D-1752): the purge and "Delete my data" take her stated names.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_the_purge_and_delete_my_data_take_the_stated_names(pool: PgPool) {
    let (vault, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &vault, "928-555-0166", None).await;
    let him = contact(&mut db, &vault, "928-555-0167", None).await;
    read(&mut db, &vault, her, "I'm Zebulon").await;
    read(&mut db, &vault, him, "me llamo Quillon").await;
    assert!(purge_contact(&mut db, her).await.expect("purge"));
    assert_eq!(stated(&mut db, her, false).await, 0);
    assert!(!purge_contact(&mut db, her).await.expect("again"));
    delete_my_data(&mut db, him, &[]).await.expect("delete");
    assert_eq!(stated(&mut db, him, false).await, 0);
    let text = read(&mut db, &vault, her, "zebulon again").await;
    assert!(text.contains("zebulon"), "nothing of her names is left");
}

/// TD-AI-19 (PRIV-024, PRIV-003, D-1752): a rotation re-seals her stated names under the new
/// keys, audited as `privacy`'s rotation, and they still match; the old vault no longer opens them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ai_19_stated_names_match_after_a_key_rotation(pool: PgPool) {
    let (old, mut db) = (v1(), app(&pool).await);
    let her = contact(&mut db, &old, "928-555-0168", None).await;
    read(&mut db, &old, her, "I'm Zebulon Quartermaine").await;
    let new = vault_with(
        &format!("{},{}", key(2, 0x58), key(1, 0x57)),
        &format!("{},{}", key(2, 0x68), key(1, 0x67)),
    );
    assert_eq!(rotate_batch(&mut db, &new, 10).await.expect("rotate"), 3);
    assert_eq!(rotate_batch(&mut db, &new, 10).await.expect("done"), 0);
    assert_eq!(stated(&mut db, her, true).await, 2);
    assert_eq!(
        audited(
            &mut db,
            "vault_read.privacy.rotation",
            "privacy.stated_name"
        )
        .await
        .len(),
        2
    );
    let text = read(&mut db, &new, her, "quartermaine's roof, zebulon").await;
    assert!(
        !text.contains("zebulon") && !text.contains("quartermaine"),
        "{text}"
    );
    read(&mut db, &new, her, "I'm Zebulon").await;
    assert_eq!(
        stated(&mut db, her, false).await,
        2,
        "a known name adds no row"
    );
    let stale = read_inbound_for(&mut db, &old, her, "hi", &WORDS).await;
    assert_eq!(stale.err(), Some(VaultError::Sealed));
}

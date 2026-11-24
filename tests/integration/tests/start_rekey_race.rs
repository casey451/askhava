//! TD-CON-38 (CON-015, CON-022, CON-035; T738, D-4418, D-4748, D-5228): a texted START racing a
//! suppression re-key, against PostgreSQL as `askhava_app`.
//!
//! Proves: a START through `on_inbound_text` for a listed number, run while another connection's
//! re-key holds that number's opt-out lock (taken by `still_unlisted`, as `suppression_rekey`'s
//! `rekey_one` does) and is writing the number's new active-version row, is seen waiting on the
//! lock (a bounded poll of `pg_locks`) and runs only after the re-key commits; it then lifts every
//! row, the new one included, so the number reads unsuppressed (`still_unlisted`) and the next text
//! goes (CON-015). In the other order the re-key sees the lifted listing and adds nothing.
//! `suppress` and its callers are unchanged. Fake 555-01xx numbers and keys only.

use std::time::Duration;

use askhava_integration::set_app_role;
use askhava_messaging::api::{
    InboundText, ListingKey, Outcome, ProviderKeyword, Relation, RevokeChannel, listing_key,
    on_inbound_text, rekey_suppression_batch, still_unlisted, suppress,
};
use askhava_privacy::api::{IndexPurpose, NewContact, Vault, rotate_batch, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;
use sqlx::{Connection as _, PgConnection, PgPool, Postgres};
use tokio::task::JoinHandle;

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONE: &str = "+19285550173";

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// A vault whose `VAULT_INDEX_KEY` holds `versions` (the first active), version `n` being 32
/// copies of byte `0x30 + n`; the seal key is v1 throughout.
fn vault(versions: &[u8]) -> Vault {
    let key = |v: u8, byte: u8| format!("{v}:{}", STANDARD.encode([byte; 32]));
    let index: Vec<String> = versions.iter().map(|v| key(*v, 0x30 + v)).collect();
    Vault::new(
        &secret(&key(1, 0x13)),
        &secret(&index.join(",")),
        &secret(DB_URL),
    )
    .expect("keys")
}

fn now() -> Timestamp {
    "2026-11-03T17:00:00Z".parse().expect("instant")
}

async fn app_conn(pool: &PgPool) -> PoolConnection<Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// Her contact holding `PHONE`, listed under v1 only, then given a v2 digest by the rotation:
/// the state a re-key moves. Returns her contact ID.
async fn listed_under_v1(conn: &mut PgConnection) -> Uuid {
    let new = NewContact {
        phone: Some(secret(PHONE)),
        ..NewContact::default()
    };
    let her = store_contact(conn, &vault(&[1]), &new)
        .await
        .expect("store");
    let how = (RevokeChannel::Portal, "portal");
    let listed = suppress(conn, &vault(&[1]), PHONE, how, now()).await;
    assert_eq!(listed, Ok(true));
    rotate_batch(conn, &vault(&[2, 1]), 10)
        .await
        .expect("rotate");
    her
}

fn key() -> ListingKey {
    let both = vault(&[2, 1]);
    listing_key(
        &both,
        &both.blind_index(IndexPurpose::HomeownerPhone, PHONE),
    )
    .expect("key")
}

/// The list as `(version, event)` rows: no digest is read out.
async fn rows(conn: &mut PgConnection) -> Vec<(i16, Uuid)> {
    sqlx::query_as(
        "SELECT index_version, event_id FROM messaging.suppression ORDER BY index_version",
    )
    .fetch_all(conn)
    .await
    .expect("rows")
}

/// `still_unlisted` for `PHONE` in its own committed transaction.
async fn unlisted(conn: &mut PgConnection) -> bool {
    let mut tx = conn.begin().await.expect("begin");
    let answer = still_unlisted(&mut tx, &key()).await.expect("re-check");
    tx.commit().await.expect("commit");
    answer
}

/// Her texted START through `on_inbound_text` under the rotated keys.
async fn start(conn: &mut PgConnection, her: Uuid) -> Outcome {
    let msg = InboundText {
        inbox_id: Uuid::from_u128(0x738),
        contact: her,
        relation: Relation::OpenRequest,
        phone: PHONE,
        text: "START",
        provider: ProviderKeyword::Start,
        emergency: false,
    };
    let id_secret = secret("fake-id-secret-for-tests");
    on_inbound_text(conn, (&id_secret, &vault(&[2, 1])), &msg, now())
        .await
        .expect("inbound")
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
    panic!("backend {pid} never waited on the number's lock");
}

/// On a new `askhava_app` connection, her START in its own task, returned once it waits.
async fn spawn_start(pool: &PgPool, observer: &mut PgConnection, her: Uuid) -> JoinHandle<Outcome> {
    let mut conn = app_conn(pool).await;
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *conn)
        .await
        .expect("pid");
    let task = tokio::spawn(async move { start(&mut conn, her).await });
    seen_waiting(observer, pid).await;
    task
}

/// TD-CON-38 (CON-015, CON-035, D-5228): the re-key holds the number's lock, locks its v1 row and
/// writes the v2 row with the v1 row's event and time, as `write_rows` does; a START meanwhile
/// waits on the lock, runs after the commit and lifts both rows, so the number is unsuppressed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_38_a_start_during_a_rekey_waits_and_lifts_every_row(pool: PgPool) {
    let mut observer = app_conn(&pool).await;
    let her = listed_under_v1(&mut observer).await;
    let first = rows(&mut observer).await;
    assert_eq!(first.len(), 1, "listed under v1 only");
    let mut rekey = app_conn(&pool).await;
    let mut tx = rekey.begin().await.expect("begin");
    assert!(!still_unlisted(&mut tx, &key()).await.expect("lock"));
    let v2 = vault(&[2, 1]).blind_index(IndexPurpose::HomeownerPhone, PHONE);
    let (_, digest) = v2.iter().find(|(v, _)| *v == 2).expect("v2 digest");
    sqlx::query(
        "INSERT INTO messaging.suppression (index_version, digest, event_id, since) \
         SELECT 2, $1, event_id, since FROM messaging.suppression WHERE index_version = 1 \
         FOR UPDATE",
    )
    .bind(&digest[..])
    .execute(&mut *tx)
    .await
    .expect("the new active-version row");
    let task = spawn_start(&pool, &mut observer, her).await;
    assert!(!task.is_finished(), "the START waits for the re-key");
    tx.commit().await.expect("commit");
    let outcome = tokio::time::timeout(Duration::from_secs(10), task).await;
    let outcome = outcome.expect("finished in time").expect("task");
    assert_eq!(outcome, Outcome::Resubscribed);
    assert_eq!(
        rows(&mut observer).await,
        [],
        "every row lifted, v2 included"
    );
    assert!(
        unlisted(&mut observer).await,
        "the next text goes (CON-015)"
    );
}

/// TD-CON-38 (CON-015, CON-022): a START first, then the re-key: it sees the lifted listing and
/// adds nothing; the number stays unsuppressed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_38_a_rekey_after_a_start_adds_nothing(pool: PgPool) {
    let mut observer = app_conn(&pool).await;
    let her = listed_under_v1(&mut observer).await;
    assert_eq!(start(&mut observer, her).await, Outcome::Resubscribed);
    let moved = rekey_suppression_batch(&mut observer, &vault(&[2, 1]), 10).await;
    assert_eq!(moved, Ok(0), "nothing to re-key");
    assert_eq!(rows(&mut observer).await, []);
    assert!(unlisted(&mut observer).await);
}

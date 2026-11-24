//! Alerts that do not push, volume alerts that close, and a storage fault that is not tampering
//! (T779; D-5081, F33, F34; OPS-029, OPS-052, OPS-010, PRIV-029), on a fresh database.
//!
//! Proves that the daily volume check resolves, as the system, the volume alerts whose level no
//! longer holds (one ack row and one audit row each), so a later breach raises a fresh one; that
//! the nightly audit-chain check fails and raises nothing when storage answers an unknown outcome,
//! and raises one High alert, not a second on the next night, when it answers not found; that the
//! local storage stand-in maps only a missing file to not found; and that the owner home lists an
//! open alert that never pushed with no button beside a pushed one with its button. All data is
//! fake.
//!
//! The layer table lets a test crate depend on modules but not on `askhava-server`, so the writer
//! file is compiled in place below, as `offdb_stream.rs` does (D-736).

use std::path::PathBuf;
use std::sync::Arc;

use askhava_adapters::api::{AdapterError, IdempotencyKey, LocalStorage, StorageAdapter};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{ManualClock, NewTask, Outcome, Registry, WorkerId, claim, enqueue};
use askhava_ops::api::{
    ALERT_ACKNOWLEDGED_ACTION, Actor, AuditEntry, NewAlert, Severity, VOLUME_HIGH_ALERT,
    VOLUME_LOW_ALERT, VolumeLevel, audit, check_volume, raise_alert, record_push,
};
use askhava_portals::api::owner_home_page;
use jiff::{SignedDuration, Timestamp};
use offdb_stream::{CHAIN_ALERT, OffDbStream, VERIFY_KIND, register};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The writer under test, `app/server/src/offdb_stream.rs`, compiled into this test crate.
#[path = r"../../../app/server/src/offdb_stream.rs"]
#[allow(
    dead_code,
    reason = "only the nightly check is under test here; offdb_stream.rs tests the writer and request log"
)]
mod offdb_stream;

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0779);

/// A volume in bytes on which this database is `percent` full now.
async fn volume_at(db: &mut PgConnection, percent: u64) -> u64 {
    let size: i64 = sqlx::query_scalar("SELECT pg_database_size(current_database())")
        .fetch_one(&mut *db)
        .await
        .expect("size");
    u64::try_from(size).expect("size") * 100 / percent
}

/// Runs the volume check as if the database were `percent` full.
async fn check_at(db: &mut PgConnection, percent: u64) -> VolumeLevel {
    let volume = volume_at(db, percent).await;
    check_volume(db, volume).await.expect("volume check")
}

/// `(open, resolved)` alerts of `kind`.
async fn alerts(db: &mut PgConnection, kind: &str) -> (i64, i64) {
    sqlx::query_as(
        "SELECT count(*) FILTER (WHERE resolved_at IS NULL), \
           count(*) FILTER (WHERE resolved_at IS NOT NULL) FROM ops.alert WHERE kind = $1",
    )
    .bind(kind)
    .fetch_one(&mut *db)
    .await
    .expect("alerts")
}

/// `(system ack rows, system acknowledged audit rows)` for the alerts of `kind`.
async fn closings(db: &mut PgConnection, kind: &str) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM ops.alert_ack k JOIN ops.alert a ON a.id = k.alert_id \
           WHERE a.kind = $1 AND k.acknowledged_by = 'system'), \
         (SELECT count(*) FROM ops.audit_log l JOIN ops.alert a ON a.id = l.subject_id \
           WHERE a.kind = $1 AND l.action = $2 AND l.actor_kind = 'system')",
    )
    .bind(kind)
    .bind(ALERT_ACKNOWLEDGED_ACTION)
    .fetch_one(&mut *db)
    .await
    .expect("closings")
}

/// OPS-052, F34: 72% raises the Low alert, 60% resolves it as the system, 72% raises a new one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_low_volume_alert_closes_when_the_level_drops(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(check_at(&mut db, 72).await, VolumeLevel::Low);
    assert_eq!(alerts(&mut db, VOLUME_LOW_ALERT).await, (1, 0));
    assert_eq!(check_at(&mut db, 60).await, VolumeLevel::Fine);
    assert_eq!(alerts(&mut db, VOLUME_LOW_ALERT).await, (0, 1));
    assert_eq!(closings(&mut db, VOLUME_LOW_ALERT).await, (1, 1));
    assert_eq!(check_at(&mut db, 72).await, VolumeLevel::Low);
    assert_eq!(
        alerts(&mut db, VOLUME_LOW_ALERT).await,
        (1, 1),
        "a second Low"
    );
}

/// OPS-052, F34: above 85% the Low alert stays beside the High one; 75% resolves the High one and
/// keeps or raises the Low one; 90% after a High alert closed raises a fresh one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_high_volume_alert_closes_and_low_stays(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(check_at(&mut db, 75).await, VolumeLevel::Low);
    assert_eq!(check_at(&mut db, 90).await, VolumeLevel::High);
    assert_eq!(alerts(&mut db, VOLUME_LOW_ALERT).await, (1, 0), "Low stays");
    assert_eq!(alerts(&mut db, VOLUME_HIGH_ALERT).await, (1, 0));
    assert_eq!(check_at(&mut db, 75).await, VolumeLevel::Low);
    assert_eq!(alerts(&mut db, VOLUME_HIGH_ALERT).await, (0, 1));
    assert_eq!(closings(&mut db, VOLUME_HIGH_ALERT).await, (1, 1));
    assert_eq!(alerts(&mut db, VOLUME_LOW_ALERT).await, (1, 0), "Low kept");
    assert_eq!(check_at(&mut db, 60).await, VolumeLevel::Fine);
    assert_eq!(check_at(&mut db, 90).await, VolumeLevel::High);
    assert_eq!(
        alerts(&mut db, VOLUME_HIGH_ALERT).await,
        (1, 1),
        "a second High"
    );
    assert_eq!(check_at(&mut db, 75).await, VolumeLevel::Low);
    assert_eq!(
        alerts(&mut db, VOLUME_LOW_ALERT).await,
        (1, 1),
        "Low raised"
    );
}

/// A storage fake whose every read answers one error and whose every write succeeds.
struct Answering(fn() -> AdapterError);

impl StorageAdapter for Answering {
    async fn put_object(
        &self,
        _idempotency: &IdempotencyKey,
        _name: &str,
        _bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn read_object(&self, _name: &str) -> Result<Vec<u8>, AdapterError> {
        Err((self.0)())
    }
}

fn not_found() -> AdapterError {
    AdapterError::Vendor {
        status: 404,
        code: None,
    }
}

/// A stream over `storage` with a clock three hours ahead (past the nightly lag) and its registry.
fn stream_over(storage: Answering) -> (Arc<OffDbStream<Answering>>, Registry) {
    let stream = Arc::new(OffDbStream {
        storage,
        id_secret: SecretString::from("fake-id-secret-for-tests"),
        clock: Arc::new(ManualClock::new(
            Timestamp::now() + SignedDuration::from_hours(3),
        )),
        purge_root: None,
    });
    let mut registry = Registry::new();
    register(&mut registry, Arc::clone(&stream));
    (stream, registry)
}

/// Writes one audit row, queues one nightly run and runs every due task; returns the nightly
/// runs' outcomes.
async fn night(
    pool: &PgPool,
    stream: &OffDbStream<Answering>,
    registry: &Registry,
) -> Vec<Outcome> {
    let mut db = pool.acquire().await.expect("db");
    let entry = AuditEntry::status_change(Actor::System, ("jobs.job", JOB), "open", "picked");
    audit(&mut db, &entry).await.expect("audit");
    let run = NewTask::new(VERIFY_KIND, serde_json::json!({}), Timestamp::now());
    enqueue(&mut db, &run).await.expect("queue");
    let owner = WorkerId::new(pool).await.expect("worker id");
    let now = stream.clock.now();
    let tasks = claim(pool, owner, now, SignedDuration::from_secs(60), 100)
        .await
        .expect("claim");
    let mut nightly = Vec::new();
    for task in tasks {
        let verify = task.kind == VERIFY_KIND;
        let lease = askhava_core::api::Lease::WORKER;
        let outcome = registry
            .run(pool, stream.clock.as_ref(), owner, task, lease)
            .await
            .expect("run");
        if verify {
            nightly.push(outcome);
        }
    }
    nightly
}

/// PRIV-029, F33: a storage outage fails the nightly run, which retries, and raises no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_storage_outage_is_not_a_broken_chain(pool: PgPool) {
    let (stream, registry) = stream_over(Answering(|| AdapterError::OutcomeUnknown));
    let runs = night(&pool, &stream, &registry).await;
    assert!(!runs.is_empty(), "the nightly run ran");
    assert!(
        runs.iter().all(|o| matches!(o, Outcome::Retry(_))),
        "{runs:?}"
    );
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(alerts(&mut db, CHAIN_ALERT).await, (0, 0));
}

/// PRIV-029, F33: a missing object raises one High alert; the next night's break adds none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_missing_object_raises_one_alert_while_open(pool: PgPool) {
    let (stream, registry) = stream_over(Answering(not_found));
    let first = night(&pool, &stream, &registry).await;
    assert!(!first.is_empty() && first.iter().all(|o| *o == Outcome::Done));
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(alerts(&mut db, CHAIN_ALERT).await, (1, 0));
    let second = night(&pool, &stream, &registry).await;
    assert!(!second.is_empty() && second.iter().all(|o| *o == Outcome::Done));
    assert_eq!(
        alerts(&mut db, CHAIN_ALERT).await,
        (1, 0),
        "no second alert"
    );
}

/// PRIV-029, F33: the local stand-in reads a missing file as not found and any other read failure
/// (here a directory where the object would be) as an unknown outcome.
#[tokio::test]
async fn the_local_stand_in_maps_only_a_missing_file_to_not_found() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("askhava-alert-lifecycle-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("audit/0000000000000000001.json")).expect("directory");
    let storage = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    let missing = storage.read_object("audit/0000000000000000002.json").await;
    let unreadable = storage.read_object("audit/0000000000000000001.json").await;
    let _gone = std::fs::remove_dir_all(&root);
    assert!(
        matches!(missing, Err(AdapterError::Vendor { status: 404, .. })),
        "{missing:?}"
    );
    assert!(
        matches!(unreadable, Err(AdapterError::OutcomeUnknown)),
        "{unreadable:?}"
    );
}

/// OPS-029, OPS-035, D-5081: the owner home lists the open unpushed Low volume alert by its plain
/// name with no button, the pushed alert still with its button and only above, and never a
/// resolved alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_owner_home_lists_alerts_that_did_not_push(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    assert_eq!(check_at(&mut db, 72).await, VolumeLevel::Low);
    let pushed = NewAlert {
        kind: "payments.ledger_mismatch",
        severity: Severity::High,
        subject: None,
    };
    let id = raise_alert(&mut db, &pushed).await.expect("alert");
    record_push(&mut db, id, Timestamp::now())
        .await
        .expect("push");
    let closed = NewAlert {
        kind: "storage.backup_missed",
        severity: Severity::High,
        subject: None,
    };
    let closed_id = raise_alert(&mut db, &closed).await.expect("alert");
    sqlx::query("UPDATE ops.alert SET resolved_at = now() WHERE id = $1")
        .bind(closed_id)
        .execute(&mut *db)
        .await
        .expect("resolve");
    let page = owner_home_page(&mut db, "fake-csrf", 4, Timestamp::now())
        .await
        .expect("page");
    let (sent, waiting) = page
        .split_once("Alerts not sent to your phone")
        .expect("the waiting heading");
    assert!(sent.contains("payments.ledger_mismatch") && sent.contains("I've seen this alert"));
    let waiting = waiting.split("<h2>").next().unwrap_or_default();
    assert!(
        waiting.contains("The database is over 70% full (level: normal)"),
        "{waiting}"
    );
    assert!(
        !waiting.contains("<form") && !waiting.contains("<button"),
        "{waiting}"
    );
    assert!(
        !waiting.contains("payments.ledger_mismatch") && !waiting.contains("storage.backup_missed"),
        "{waiting}"
    );
}

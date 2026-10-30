//! The off-database stream on a fresh database and a local storage stand-in (TD-OPS-03,
//! TD-OPS-19; OPS-010, OPS-011, PRIV-029).
//!
//! Proves that an audit row (direct or from a pause switch), a web request line and an AI reading
//! each queue a `core.task` in the writing transaction and reach the storage stand-in when the
//! worker runs it; that a rolled-back change queues nothing; that a request line holds the route
//! template and never the path; that the nightly hash-chain check passes an untouched chain and
//! names the first tampered or missing object, raising a High alert; and that the request-log purge
//! removes days older than 30 only. The 403 on read and delete and object-lock cases are the Phase
//! 2 drill's (T134, T109). All data is fake.
//!
//! The layer table lets a test crate depend on modules but not on `askhava-server`, so the writer
//! file is compiled in place below and the request case builds its router from the same layers
//! `build_router` uses (D-736).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use askhava_adapters::api::LocalStorage;
use askhava_core::api::Settings;
use askhava_core::api::{ManualClock, Outcome, Registry, WorkerId, claim};
use askhava_integration::set_app_role;
use askhava_ops::api::{Actor, AuditEntry, StreamEvent, Switch, audit, set_switch, stream_event};
use axum::Router;
use axum::body::Body;
use axum::http::Request;
use jiff::{SignedDuration, Timestamp};
use offdb_stream::{
    ChainError, OffDbStream, audit_name, log_request, purge_request_log, register, schedule_verify,
    verify_audit_chain,
};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

/// The writer under test, `app/server/src/offdb_stream.rs`, compiled into this test crate.
#[path = r"../../../app/server/src/offdb_stream.rs"]
mod offdb_stream;

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0011);
const READING: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0012);
const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0013);

/// A fresh stand-in directory, a stream over it with a clock three hours ahead (past the settle
/// time and the nightly lag), and the registry holding its handlers.
fn setup(test: &str) -> (PathBuf, Arc<OffDbStream<LocalStorage>>, Registry) {
    let root = std::env::temp_dir().join(format!("askhava-offdb-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("stand-in directory");
    let base = format!("file://{}", root.display());
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_STORAGE_BASE_URL", base.as_str()),
        ("ASKHAVA_ID_SECRET", "fake-id-secret-for-tests"),
    ]
    .map(|(k, v)| (k.to_owned(), v.to_owned()));
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let missing = pairs
        .iter()
        .filter(|(k, _)| k != "ASKHAVA_ID_SECRET")
        .cloned();
    let clock = Arc::new(ManualClock::new(
        Timestamp::now() + SignedDuration::from_hours(3),
    ));
    assert!(OffDbStream::from_settings(&settings, missing, clock.clone()).is_err());
    let stream = Arc::new(OffDbStream::from_settings(&settings, pairs, clock).expect("stream"));
    let mut registry = Registry::new();
    register(&mut registry, Arc::clone(&stream));
    (root, stream, registry)
}

/// Claims and runs every due task once; each must finish.
async fn run_due(pool: &PgPool, stream: &OffDbStream<LocalStorage>, registry: &Registry) -> usize {
    let owner = WorkerId::new(pool).await.expect("worker id");
    let now = stream.clock.now();
    let tasks = claim(pool, owner, now, SignedDuration::from_secs(60), 100)
        .await
        .expect("claim");
    let count = tasks.len();
    for task in tasks {
        let lease = askhava_core::api::Lease::WORKER;
        let outcome = registry
            .run(pool, stream.clock.as_ref(), owner, task, lease)
            .await;
        assert_eq!(outcome, Ok(Outcome::Done));
    }
    count
}

fn object(root: &Path, name: &str) -> serde_json::Value {
    let bytes = std::fs::read(root.join(name)).expect("object written");
    serde_json::from_slice(&bytes).expect("object is JSON")
}

/// Writes `n` audit rows, each in its own transaction, and streams them.
async fn audit_rows(pool: &PgPool, n: u32) -> Vec<i64> {
    let mut ids = Vec::new();
    for _ in 0..n {
        let mut tx = pool.begin().await.expect("begin");
        let entry = AuditEntry::status_change(Actor::System, ("jobs.job", JOB), "open", "picked");
        ids.push(audit(&mut tx, &entry).await.expect("audit"));
        tx.commit().await.expect("commit");
    }
    ids
}

/// TD-OPS-03 (OPS-010, OPS-011): audit rows, a switch change, a request line and an AI reading
/// reach the storage stand-in when their task runs; a rolled-back audit row queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_stream_event_reaches_storage_as_it_happens(pool: PgPool) {
    let (root, stream, registry) = setup("events");
    let ids = audit_rows(&pool, 1).await;
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    set_switch(&mut conn, Switch::Texting, true, Actor::Owner(OWNER))
        .await
        .expect("switch");
    let mut tx = pool.begin().await.expect("begin");
    let entry = AuditEntry::status_change(Actor::System, ("jobs.job", JOB), "open", "closed");
    audit(&mut tx, &entry).await.expect("audit");
    tx.rollback().await.expect("rollback");
    let fields = [("trade", "plumbing"), ("urgency", "this_week")];
    let reading = StreamEvent::AiReading {
        reading_id: READING,
        fields: &fields,
    };
    stream_event(&mut conn, &reading)
        .await
        .expect("queue reading");
    assert_eq!(
        run_due(&pool, &stream, &registry).await,
        3,
        "rolled-back row queued nothing"
    );
    let first = object(&root, &audit_name(ids[0]));
    assert_eq!(first["line"]["to_status"], "picked");
    assert_eq!(first["prev_id"], 0, "the first row starts the chain");
    let switch = object(&root, &audit_name(ids[0] + 1));
    assert_eq!(switch["line"]["action"], "switch_on");
    assert_eq!(switch["prev_id"], ids[0]);
    let ai = object(&root, &format!("ai_reading/{READING}.json"));
    assert_eq!(ai["fields"]["urgency"], "this_week");
    let bad = [("trade", "Call 928-555-0100")];
    let refused = StreamEvent::AiReading {
        reading_id: READING,
        fields: &bad,
    };
    assert!(
        stream_event(&mut conn, &refused).await.is_err(),
        "free text never streams"
    );
}

/// TD-OPS-03 (OPS-011): a web request queues one line holding the request ID, route template and
/// status, never the path.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_request_line_holds_the_route_template_not_the_path(pool: PgPool) {
    let (root, stream, registry) = setup("request");
    let request = Request::get("/q/fake-token-7e57")
        .body(Body::empty())
        .expect("request");
    let router = Router::new()
        .layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            log_request,
        ))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));
    let response = router.oneshot(request).await;
    let response = response.expect("response");
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .expect("id")
        .to_owned();
    assert_eq!(run_due(&pool, &stream, &registry).await, 1);
    let today = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
    let day = std::fs::read_dir(root.join(format!("request/{today}"))).expect("today's folder");
    let files: Vec<_> = day.map(|entry| entry.expect("entry").path()).collect();
    assert_eq!(files.len(), 1, "one line per request");
    let bytes = std::fs::read(&files[0]).expect("request line written");
    let line: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(line["request_id"].as_str(), Some(request_id.as_str()));
    assert_eq!(
        (line["route"].as_str(), line["status"].as_u64()),
        (Some("unmatched"), Some(404))
    );
    assert!(
        !String::from_utf8_lossy(&bytes).contains("fake-token"),
        "the path never reaches the log"
    );
}

/// TD-OPS-19 (PRIV-029, OPS-010): the nightly check passes the untouched chain, then names a
/// tampered and a missing object and raises a High alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_tampered_audit_object_fails_the_nightly_chain_check(pool: PgPool) {
    let (root, stream, registry) = setup("chain");
    let ids = audit_rows(&pool, 3).await;
    run_due(&pool, &stream, &registry).await;
    let mut conn = pool.acquire().await.expect("connection");
    let now = stream.clock.now();
    assert_eq!(
        verify_audit_chain(&mut conn, &stream.storage, now).await,
        Ok(3)
    );
    let path = root.join(audit_name(ids[1]));
    let original = std::fs::read(&path).expect("object");
    let tampered = String::from_utf8(original.clone())
        .expect("utf8")
        .replace("picked", "closed");
    std::fs::write(&path, tampered).expect("tamper");
    let broken = verify_audit_chain(&mut conn, &stream.storage, now).await;
    assert_eq!(broken, Err(ChainError::Broken(ids[1])));
    std::fs::write(&path, &original).expect("restore");
    std::fs::remove_file(root.join(audit_name(ids[2]))).expect("remove");
    let missing = verify_audit_chain(&mut conn, &stream.storage, now).await;
    assert_eq!(missing, Err(ChainError::Broken(ids[2])));
    schedule_verify(&mut conn, Timestamp::now())
        .await
        .expect("schedule");
    assert_eq!(
        run_due(&pool, &stream, &registry).await,
        1,
        "the nightly run"
    );
    let alerts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = 'ops.audit_chain_broken' AND severity = 'high'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("alerts");
    assert_eq!(alerts, 1);
}

/// TD-OPS-19 (PRIV-029): the request-log purge removes days older than 30 and keeps the rest.
#[test]
fn the_request_log_purge_keeps_thirty_days() {
    let root = std::env::temp_dir().join(format!("askhava-offdb-purge-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    let now: Timestamp = "2026-10-30T12:00:00Z".parse().expect("time");
    for day in ["2026-09-29", "2026-09-30", "2026-10-29"] {
        let dir = root.join("request").join(day);
        std::fs::create_dir_all(&dir).expect("day folder");
        std::fs::write(dir.join("fake.json"), b"{}").expect("line");
    }
    assert_eq!(purge_request_log(&root, now).expect("purge"), 1);
    assert!(
        !root.join("request/2026-09-29").exists(),
        "31 days old goes"
    );
    assert!(
        root.join("request/2026-09-30").exists(),
        "30 days old stays"
    );
    assert!(root.join("request/2026-10-29").exists());
}

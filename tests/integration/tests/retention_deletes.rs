//! The retention deletes outside the database on a fresh database (TD-PRIV-20; PRIV-007, PRIV-009,
//! PRIV-027, TXT-020; D-1803, D-1841).
//!
//! Against the local store and the Twilio stand-in (`python3 -m stand_ins.server`, needs `python3`
//! on the path), every handler running as `askhava_app` through the task queue on a manual clock:
//! a purged photo's sealed object goes after its rows, a failed delete retried; an orphan
//! `photos/` object with no row goes once it is over a day old, a younger one and one whose row
//! landed stay; on day 31 every inbound and outbound message body is deleted at the stand-in once,
//! a rerun sending nothing and a provider error retried, and nothing goes before day 31. The local
//! 30-day cut is T093's (`message_log_retention.rs`). Every number, SID, key and byte is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, LocalStorage, SentText, SmsAdapter, SmsBodyDelete,
    StorageAdapter, StorageDelete, TwilioConfig, TwilioSms, twilio_clients,
};
use askhava_core::api::{Claimed, Clock, Lease, ManualClock, Registry, Settings, WorkerId, claim};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    BODY_DELETE_FAILED_ALERT, queue_body_deletes, register_body_deletes, schedule_body_scan,
};
use askhava_privacy::api::{
    PHOTO_DELETE_FAILED_ALERT, PHOTO_DELETE_KIND, purge_photos, register_photo_deletes,
    run_photo_delete, schedule_orphan_sweep, sweep_orphans,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0276);
const OTHER_JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0001_0276);
const DAY: SignedDuration = SignedDuration::from_hours(24);
const FROM: &str = "+15005550006";
const TO: &str = "+15005550010";

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-retention-deletes-0123456789")
}

/// A store under a fresh temporary directory, removed when the test ends.
struct Store(std::path::PathBuf, LocalStorage);

impl Drop for Store {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

fn store(test: &str) -> Store {
    let root = std::env::temp_dir().join(format!("askhava-t276-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let local = LocalStorage::new(&format!("file://{}", root.display())).expect("storage");
    Store(root, local)
}

/// A store whose next call fails once when `fail` is set, so the retry path runs.
struct Flaky {
    inner: LocalStorage,
    fail: AtomicBool,
}

impl StorageAdapter for Flaky {
    async fn put_object(
        &self,
        key: &IdempotencyKey,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AdapterError> {
        self.inner.put_object(key, name, bytes).await
    }

    async fn read_object(&self, name: &str) -> Result<Vec<u8>, AdapterError> {
        self.inner.read_object(name).await
    }
}

impl StorageDelete for Flaky {
    async fn delete_object(&self, key: &IdempotencyKey, name: &str) -> Result<(), AdapterError> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(AdapterError::NotSent);
        }
        self.inner.delete_object(key, name).await
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>, AdapterError> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(AdapterError::NotSent);
        }
        self.inner.list_objects(prefix).await
    }
}

/// The stand-in texting client, counting each redaction call and each body it deleted; fails the
/// next call once when `fail` is set (a provider error).
struct Counting {
    inner: TwilioSms,
    fail: AtomicBool,
    calls: Mutex<Vec<String>>,
    deleted: Mutex<Vec<String>>,
}

impl SmsAdapter for Counting {
    async fn send_text(
        &self,
        key: &IdempotencyKey,
        to: &str,
        from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        self.inner.send_text(key, to, from, body).await
    }
}

impl SmsBodyDelete for Counting {
    async fn delete_message_body(
        &self,
        key: &IdempotencyKey,
        sid: &str,
    ) -> Result<(), AdapterError> {
        self.calls.lock().expect("calls").push(sid.to_owned());
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(AdapterError::Vendor {
                status: 503,
                code: None,
            });
        }
        self.inner.delete_message_body(key, sid).await?;
        self.deleted.lock().expect("deleted").push(sid.to_owned());
        Ok(())
    }
}

/// The stand-in server on a port it picked itself (its first line names it); killed on drop.
struct StandIn(Child, u16);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("listening line");
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let port = line
        .trim_end()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok());
    StandIn(
        child,
        port.unwrap_or_else(|| panic!("stand-in said {line:?}")),
    )
}

fn texting(port: u16) -> Counting {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let numbers: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &numbers.join(",")),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("twilio config");
    let (sms, _, _) = twilio_clients(&config).expect("clients");
    Counting {
        inner: sms,
        fail: AtomicBool::new(false),
        calls: Mutex::default(),
        deleted: Mutex::default(),
    }
}

/// A fake homeowner text received by the stand-in; returns its SID.
async fn inbound(port: u16) -> String {
    let body = json!({ "from": TO, "to": FROM, "body": "Fake drip under the sink" }).to_string();
    let request = format!(
        "POST /_stand_in/twilio/inbound HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, json) = answer.split_once("\r\n\r\n").expect("http answer");
    let parsed: serde_json::Value = serde_json::from_str(json).expect("json");
    parsed["params"]["MessageSid"]
        .as_str()
        .expect("sid")
        .to_owned()
}

/// A pool whose every connection runs as `askhava_app`, for the handlers (R9).
async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// Runs every task due at the clock's now through `registry`, as the worker would.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    for task in tasks {
        registry
            .run(pool, clock, owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

async fn put(store: &impl StorageAdapter, name: &str) {
    let key = IdempotencyKey::derive(&secret(), "photo.put", &[name]);
    store
        .put_object(&key, name, b"fake-sealed-bytes".to_vec())
        .await
        .expect("put");
}

async fn add_row(pool: &PgPool, job: Uuid, name: &str) {
    sqlx::query(
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at) \
         VALUES (gen_random_uuid(), $1, 1, $2, now())",
    )
    .bind(job)
    .bind(name)
    .execute(pool)
    .await
    .expect("photo row");
}

async fn listed(store: &LocalStorage) -> Vec<String> {
    store.list_objects("photos/").await.expect("list")
}

/// TD-PRIV-20 (PRIV-009, PRIV-007, D-1841): the photo purge deletes the rows at once and the sealed
/// objects after, through the task; a failed delete leaves the rows deleted and is retried.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_20_a_purged_photos_object_goes_after_its_rows(pool: PgPool) {
    let dir = store("purged");
    let flaky = Arc::new(Flaky {
        inner: dir.1.clone(),
        fail: AtomicBool::new(true),
    });
    let (mine, other) = ("photos/fake-276-a", "photos/fake-276-b");
    for (job, name) in [(JOB, mine), (OTHER_JOB, other)] {
        put(&dir.1, name).await;
        add_row(&pool, job, name).await;
    }
    assert_eq!(purge_photos(&mut app(&pool).await, JOB).await, Ok(1));
    let clock = Arc::new(ManualClock::new(Timestamp::now()));
    let mut registry = Registry::new();
    register_photo_deletes(&mut registry, Arc::clone(&flaky), secret(), clock.clone());
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM privacy.photo WHERE job_id = $1")
        .bind(JOB)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 0, "the rows go at once");
    assert_eq!(
        listed(&dir.1).await,
        [mine, other],
        "the object waits for the task"
    );
    let app_pool = app_pool(&pool).await;
    run_due(&app_pool, &registry, &clock).await;
    assert_eq!(
        listed(&dir.1).await,
        [mine, other],
        "a failed delete keeps the object"
    );
    assert!(
        !flaky.fail.load(Ordering::SeqCst),
        "the delete was tried and failed"
    );
    clock.advance(SignedDuration::from_secs(60));
    run_due(&app_pool, &registry, &clock).await;
    assert_eq!(
        listed(&dir.1).await,
        [other],
        "the retry deletes it; the other job's stays"
    );
    let bad = |name: &str| Claimed {
        id: 0,
        kind: PHOTO_DELETE_KIND.to_owned(),
        payload: json!({ "name": name }),
        attempts: 1,
    };
    for name in ["stream/fake", "photos/"] {
        assert!(
            run_photo_delete(app_pool.clone(), (&dir.1, &secret()), bad(name))
                .await
                .is_err()
        );
    }
}

/// TD-PRIV-20 (PRIV-009, D-1793, D-1841): the daily sweep deletes an orphan `photos/` object once
/// it is over a day old; a younger orphan and an object whose row landed meanwhile stay.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_20_an_orphan_over_a_day_old_goes_and_a_younger_one_stays(pool: PgPool) {
    let dir = store("orphans");
    let local = Arc::new(dir.1.clone());
    let (old, landed, young, rowed) = (
        "photos/fake-orphan-old",
        "photos/fake-row-landed",
        "photos/fake-orphan-young",
        "photos/fake-rowed",
    );
    for name in [old, landed, rowed] {
        put(&dir.1, name).await;
    }
    add_row(&pool, JOB, rowed).await;
    let start = Timestamp::now();
    let clock = Arc::new(ManualClock::new(start));
    let mut registry = Registry::new();
    register_photo_deletes(&mut registry, Arc::clone(&local), secret(), clock.clone());
    schedule_orphan_sweep(&mut app(&pool).await, start - DAY)
        .await
        .expect("schedule");
    let app_pool = app_pool(&pool).await;
    run_due(&app_pool, &registry, &clock).await;
    assert_eq!(
        listed(&dir.1).await.len(),
        3,
        "nothing goes on the day it is first seen"
    );
    put(&dir.1, young).await;
    add_row(&pool, JOB, landed).await;
    clock.set(start + DAY + SignedDuration::from_secs(1));
    run_due(&app_pool, &registry, &clock).await;
    assert_eq!(listed(&dir.1).await, [young, landed, rowed]);
    let flaky = Flaky {
        inner: dir.1.clone(),
        fail: AtomicBool::new(true),
    };
    let mut conn = app(&pool).await;
    assert!(
        sweep_orphans(&mut conn, &flaky, clock.now()).await.is_err(),
        "a failed list fails the task"
    );
    assert_eq!(
        sweep_orphans(&mut conn, &flaky, clock.now()).await,
        Ok(0),
        "the young one is queued once"
    );
}

async fn add_messages(
    pool: &PgPool,
    (inbound, outbound, young): (&str, &str, &str),
    at: Timestamp,
) {
    sqlx::query("INSERT INTO messaging.inbox (message_sid, received_at) VALUES ($1, $3), ($2, $3 + interval '1 day')")
        .bind(inbound)
        .bind(young)
        .bind(at.to_sqlx())
        .execute(pool)
        .await
        .expect("inbox rows");
    sqlx::query(
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, recipient_kind, \
         subject_ids, status, provider_sid, sent_at) \
         VALUES (repeat('a', 64), 'fake_notice', $1, 'business', ARRAY[$1], 'sent', $2, $3)",
    )
    .bind(JOB)
    .bind(outbound)
    .bind(at.to_sqlx())
    .execute(pool)
    .await
    .expect("outbox row");
}

/// TD-PRIV-20 (PRIV-027, TXT-020, D-1841): on day 31 every inbound and outbound body is deleted at
/// the stand-in once; a rerun sends nothing, a provider error is retried, nothing goes before day 31.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_20_bodies_are_deleted_at_the_provider_once_on_day_31(pool: PgPool) {
    let server = stand_in();
    let sms = Arc::new(texting(server.1));
    let key = IdempotencyKey::derive(&secret(), "sms.send", &["fake-276"]);
    let outbound = sms
        .send_text(&key, TO, FROM, "Ask Hava: fake notice")
        .await
        .expect("send")
        .sid;
    let inbound = inbound(server.1).await;
    let young = format!("SM{}", "f".repeat(32));
    let start = Timestamp::now();
    add_messages(&pool, (&inbound, &outbound, &young), start).await;
    let clock = Arc::new(ManualClock::new(start));
    let mut registry = Registry::new();
    register_body_deletes(&mut registry, Arc::clone(&sms), secret(), clock.clone());
    schedule_body_scan(
        &mut app(&pool).await,
        start + 30 * DAY - SignedDuration::from_hours(1),
    )
    .await
    .expect("schedule");
    let app_pool = app_pool(&pool).await;
    clock.set(start + 31 * DAY - SignedDuration::from_hours(1));
    run_due(&app_pool, &registry, &clock).await;
    assert!(
        sms.calls.lock().expect("calls").is_empty(),
        "nothing goes before day 31"
    );
    sms.fail.store(true, Ordering::SeqCst);
    clock.set(start + 32 * DAY - SignedDuration::from_hours(1));
    for _ in 0..2 {
        run_due(&app_pool, &registry, &clock).await;
        clock.advance(SignedDuration::from_secs(60));
    }
    run_due(&app_pool, &registry, &clock).await;
    let mut deleted = sms.deleted.lock().expect("deleted").clone();
    deleted.sort();
    let mut expected = vec![inbound.clone(), outbound.clone()];
    expected.sort();
    assert_eq!(
        deleted, expected,
        "both bodies deleted, the young one not yet"
    );
    assert_eq!(
        sms.calls.lock().expect("calls").len(),
        3,
        "one provider error, retried once"
    );
    assert_eq!(
        queue_body_deletes(&mut app(&pool).await, clock.now()).await,
        Ok(0)
    );
    clock.advance(DAY);
    run_due(&app_pool, &registry, &clock).await;
    run_due(&app_pool, &registry, &clock).await;
    assert_eq!(
        sms.deleted.lock().expect("deleted").len(),
        2,
        "a rerun sends nothing"
    );
    let calls = sms.calls.lock().expect("calls").clone();
    assert_eq!(
        calls.len(),
        4,
        "only the young SID is sent, once, on its own day 31"
    );
    assert_eq!(
        calls[3], young,
        "the stand-in never had it: 20404 is done, not retried"
    );
}

/// A texting client whose every send and body delete fails (a provider outage).
struct Down;

impl SmsAdapter for Down {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl SmsBodyDelete for Down {
    async fn delete_message_body(
        &self,
        _key: &IdempotencyKey,
        _sid: &str,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// TD-PRIV-20 (PRIV-009, PRIV-027, D-2162): a photo object or body delete that fails on its last
/// attempt is dead-lettered and never queued again, so it raises one High alert of its own kind.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_20_a_delete_failing_its_last_attempt_raises_an_alert(pool: PgPool) {
    let dir = store("last");
    let flaky = Arc::new(Flaky {
        inner: dir.1.clone(),
        fail: AtomicBool::new(true),
    });
    let name = "photos/fake-276-last";
    put(&dir.1, name).await;
    add_row(&pool, JOB, name).await;
    assert_eq!(purge_photos(&mut app(&pool).await, JOB).await, Ok(1));
    let start = Timestamp::now();
    let sids = ["1", "2", "3"].map(|c| format!("SM{}", c.repeat(32)));
    add_messages(&pool, (&sids[0], &sids[1], &sids[2]), start).await;
    let clock = Arc::new(ManualClock::new(
        start + 31 * DAY + SignedDuration::from_hours(1),
    ));
    queue_body_deletes(&mut app(&pool).await, clock.now())
        .await
        .expect("queue");
    let mut registry = Registry::new();
    register_photo_deletes(&mut registry, flaky, secret(), clock.clone());
    register_body_deletes(&mut registry, Arc::new(Down), secret(), clock.clone());
    sqlx::query(
        "UPDATE core.task SET attempts = max_attempts - 1, run_at = now() - interval '1 minute'",
    )
    .execute(&pool)
    .await
    .expect("last attempt next");
    run_due(&app_pool(&pool).await, &registry, &clock).await;
    let alerts: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind, severity FROM ops.alert WHERE subject_id IS NULL ORDER BY kind",
    )
    .fetch_all(&pool)
    .await
    .expect("alerts");
    let high = |kind: &str| (kind.to_owned(), "high".to_owned());
    assert_eq!(
        alerts,
        [
            high(BODY_DELETE_FAILED_ALERT),
            high(BODY_DELETE_FAILED_ALERT),
            high(PHOTO_DELETE_FAILED_ALERT)
        ],
        "each dead-lettered delete raises one alert"
    );
    let dead: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE dead_at IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("dead");
    assert_eq!(dead, 3, "all three deletes are dead-lettered");
}

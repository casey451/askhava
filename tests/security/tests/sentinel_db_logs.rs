//! The stage 2 sentinel scan: nothing written through the vault shows up in clear anywhere else
//! (TD-PRIV-02; PRIV-016, PRIV-019).
//!
//! The fixed sentinel person of `askhava-fixtures` (D-109) is stored through the vault, looked up
//! by phone, read under a grant, refused under a purpose with no grant, and stored again (refused
//! as a taken phone, the error logged at its handling point); a request carrying the sentinels in
//! its path, query and a header passes the same layers as `build_router`; and every queued
//! `core.task` runs, so the off-database stream writes its audit and request objects. Then every
//! table but the sealed `privacy.contact` (bytea read as hex too), every JSON log line, every
//! object in the off-database stand-in and every envelope the Sentry stub received are searched
//! for the sentinels in their common spellings; none may be found. Each place must hold something,
//! so an empty scan cannot pass. T120 runs the same scan after journeys J1 to J4 (TD-PRIV-02, E2E).
//! The sentinel street is the fixture's `SENTINEL_STREET` (D-795). All data and keys are fake.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::LocalStorage;
use askhava_core::api::{
    Environment, Lease, ManualClock, Outcome, Registry, WorkerId, claim, json_layer, request_span,
    sentry_options,
};
use askhava_fixtures::{SENTINEL_EMAIL, SENTINEL_NAME, SENTINEL_STREET};
use askhava_privacy::api::{
    Field, NewContact, Purpose, Reader, Vault, VaultError, find_by_phone, read_contact,
    store_contact,
};
use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use offdb_stream::{OffDbStream, RequestLog, log_request, register};
use secrecy::SecretString;
use sentry::integrations::tracing::ErrorLayer;
use sentry::{Client, Envelope, Hub, SentryFutureExt, Transport};
use sqlx::{AssertSqlSafe, PgPool};
use tower::ServiceExt;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;

/// The off-database stream writer, compiled in place as the layer table keeps `askhava-server`
/// out of test crates (D-736).
#[path = r"../../../app/server/src/offdb_stream.rs"]
#[allow(
    dead_code,
    reason = "only the request log and the task handlers are used here"
)]
mod offdb_stream;

/// The sentinel phone as a homeowner types it; the vault stores it as `+19285550199`.
const TYPED_PHONE: &str = "+1 928 555 0199";
const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";

/// Everything written somewhere: log lines and Sentry envelopes, shared with the stub writers.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Sink {
    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The Sentry stub: a transport that keeps every envelope instead of sending it (PRIV-017).
impl Transport for Sink {
    fn send_envelope(&self, envelope: Envelope) {
        envelope.to_writer(self.clone()).expect("envelope");
    }
}

/// Places that must hold something, so an empty scan cannot pass.
const HELD: [&str; 6] = [
    "json log",
    "sentry stub",
    "ops.audit_log",
    "core.task",
    "/audit/",
    "/request/",
];

/// The sentinel phone's spellings other than the typed one: digits, dashes, spaces and dots.
const PHONE_SPELLINGS: [&str; 5] = ["9285550199", "555-0199", "555 0199", "555.0199", "5550199"];

/// The sentinel spellings searched for, lower case, and each one's hex for bytea columns.
fn needles() -> Vec<String> {
    let names = [SENTINEL_NAME, "zelda", SENTINEL_EMAIL, SENTINEL_STREET];
    let plain: Vec<String> = names
        .into_iter()
        .chain(PHONE_SPELLINGS)
        .map(str::to_lowercase)
        .collect();
    let hex = plain
        .iter()
        .map(|n| n.bytes().map(|b| format!("{b:02x}")).collect());
    plain.iter().cloned().chain(hex).collect()
}

/// PRIV-016, PRIV-019: the sentinels found in `text`, in any letter case.
fn leaks(place: &str, text: &str) -> Vec<String> {
    let text = text.to_lowercase();
    needles()
        .into_iter()
        .filter(|n| text.contains(n.as_str()))
        .map(|n| format!("{place}: {n}"))
        .collect()
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn vault() -> Vault {
    let key = |byte: u8| secret(&format!("1:{}", STANDARD.encode([byte; 32])));
    Vault::new(&key(0x31), &key(0x41), &secret(DB_URL)).expect("vault keys")
}

/// Stores, finds, reads and is refused on the sentinel contact as the app role (PRIV-001 to
/// PRIV-004); the refused second store is logged once, at its handling point (rust brain R5). The
/// connection is detached, so its role never returns to the pool the scan reads through.
async fn use_the_vault(pool: &PgPool) {
    let vault = vault();
    let mut conn = pool.acquire().await.expect("connection").detach();
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut conn)
        .await
        .expect("SET ROLE askhava_app");
    let sentinel = || NewContact {
        name: Some(secret(SENTINEL_NAME)),
        phone: Some(secret(TYPED_PHONE)),
        email: Some(secret(SENTINEL_EMAIL)),
        address: Some(secret(SENTINEL_STREET)),
        onsite_contact: Some(secret(SENTINEL_NAME)),
    };
    let id = store_contact(&mut conn, &vault, &sentinel())
        .await
        .expect("stored");
    let found = find_by_phone(&mut conn, &vault, TYPED_PHONE).await;
    assert_eq!(found.expect("lookup"), Some(id));
    let send = (Reader::Messaging, Purpose::Send);
    read_contact(&mut conn, &vault, send, id, &Field::ALL[..4])
        .await
        .expect("granted read");
    let refused = (Reader::Messaging, Purpose::Release);
    let refusal = read_contact(&mut conn, &vault, refused, id, &[Field::Phone]).await;
    assert!(matches!(refusal, Err(VaultError::UnregisteredPurpose)));
    let again = store_contact(&mut conn, &vault, &sentinel()).await;
    let error = again.expect_err("the phone is taken");
    tracing::error!(error = %error, "contact not stored");
}

/// Sends one request with the sentinels in its path, query and a header through the layers of
/// `build_router` and waits for its request line to be queued (OPS-011, PRIV-016, PRIV-028).
async fn send_a_request(pool: &PgPool) {
    let (log, writer) = RequestLog::spawn(pool.clone());
    let router = Router::new()
        .route("/q/{token}", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(log, log_request))
        .layer(TraceLayer::new_for_http().make_span_with(request_span::<Body>))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));
    let (phone, street) = (
        TYPED_PHONE.replace(' ', ""),
        SENTINEL_STREET.replace(' ', "%20"),
    );
    let uri = format!("/q/{SENTINEL_EMAIL}?phone={phone}&street={street}");
    let request = Request::get(uri)
        .header("x-contact", SENTINEL_NAME)
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert!(response.status().is_success());
    writer.await.expect("the request line was queued");
}

/// Runs every queued `core.task` through the off-database stream handlers into `root` (OPS-010,
/// OPS-011) and returns how many ran.
async fn run_the_stream(pool: &PgPool, root: &Path) -> usize {
    let stream = Arc::new(OffDbStream {
        storage: LocalStorage::new(&format!("file://{}", root.display())).expect("stand-in"),
        id_secret: secret("fake-id-secret-for-tests"),
        clock: Arc::new(ManualClock::new(
            Timestamp::now() + SignedDuration::from_hours(3),
        )),
        purge_root: None,
    });
    let mut registry = Registry::new();
    register(&mut registry, Arc::clone(&stream));
    let owner = WorkerId::new(pool).await.expect("worker id");
    let now = stream.clock.now();
    let tasks = claim(pool, owner, now, SignedDuration::from_secs(60), 100).await;
    let tasks = tasks.expect("claim");
    let count = tasks.len();
    for task in tasks {
        let ran = registry.run(pool, stream.clock.as_ref(), owner, task, Lease::WORKER);
        assert_eq!(ran.await, Ok(Outcome::Done));
    }
    count
}

/// Every table but the sealed vault, one JSON line per row (PRIV-016, PRIV-019).
async fn every_table(pool: &PgPool) -> Vec<(String, String)> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT format('%I.%I', schemaname, tablename) FROM pg_tables WHERE schemaname NOT IN \
         ('pg_catalog', 'information_schema') AND (schemaname, tablename) <> ('privacy', 'contact')",
    )
    .fetch_all(pool)
    .await
    .expect("tables");
    let mut out = Vec::new();
    for table in tables {
        let sql =
            format!("SELECT coalesce(string_agg(row_to_json(t)::text, E'\\n'), '') FROM {table} t");
        let rows: String = sqlx::query_scalar(AssertSqlSafe(sql))
            .fetch_one(pool)
            .await
            .expect("rows");
        out.push((table, rows));
    }
    out
}

/// Every file under `dir`, path and contents.
fn every_object(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("folder") {
        let path: PathBuf = entry.expect("entry").path();
        if path.is_dir() {
            every_object(&path, out);
        } else {
            let text = std::fs::read_to_string(&path).expect("object");
            out.push((path.display().to_string(), text));
        }
    }
}

/// TD-PRIV-02 (PRIV-016, PRIV-019): after the sentinels go through the vault, no non-vault table,
/// log line, off-database object or Sentry stub envelope holds one in clear.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sentinels_written_through_the_vault_leak_nowhere(pool: PgPool) {
    assert!(!leaks("control", &format!("Call {TYPED_PHONE}")).is_empty());
    let (log, sentry) = (Sink::default(), Sink::default());
    let mut options = sentry_options(
        "https://fakepublic@example.com/1".parse().expect("dsn"),
        Environment::Ci,
    );
    options.transport = Some(Arc::new(Arc::new(sentry.clone())));
    let hub = Arc::new(Hub::new(
        Some(Arc::new(Client::with_options(options))),
        Arc::default(),
    ));
    let writer = log.clone();
    let subscriber = tracing_subscriber::registry()
        .with(json_layer(move || writer.clone()))
        .with(ErrorLayer::new().with_filter(LevelFilter::ERROR));
    let _logging = tracing::subscriber::set_default(subscriber);
    let root = std::env::temp_dir().join(format!("askhava-sentinel-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    let ran = async {
        use_the_vault(&pool).await;
        send_a_request(&pool).await;
        run_the_stream(&pool, &root).await
    };
    assert!(
        ran.bind_hub(Arc::clone(&hub)).await >= 3,
        "audit rows and the request line streamed"
    );
    assert!(hub.client().expect("client").flush(None));
    let mut places = every_table(&pool).await;
    places.push(("json log".to_owned(), log.text()));
    places.push(("sentry stub".to_owned(), sentry.text()));
    every_object(&root, &mut places);
    std::fs::remove_dir_all(&root).expect("remove the stand-in folder");
    for place in HELD {
        let held = places
            .iter()
            .any(|(p, t)| p.contains(place) && !t.is_empty());
        assert!(held, "{place} holds nothing, so the scan proves nothing");
    }
    let found: Vec<String> = places.iter().flat_map(|(p, t)| leaks(p, t)).collect();
    assert!(found.is_empty(), "sentinels in clear: {found:?}");
}

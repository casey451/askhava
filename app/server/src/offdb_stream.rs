//! The off-database stream writer: audit rows, web request lines and AI readings to object storage
//! as they happen, the audit stream as a hash chain checked nightly (OPS-010, OPS-011, PRIV-029).
//!
//! `ops` only enqueues an `ops.offdb_stream` task in the writing transaction
//! (`ops::api::stream_event`); this `server` handler copies it to the storage adapter, since `ops`
//! may call nothing (D-491, D-734). Audit row `n` becomes `audit/<n>.json` holding the row's JSON
//! line, the ID of the row before it and the sha256 of that row's line, so editing, removing or
//! inserting an object breaks a link that [`verify_audit_chain`] finds; the nightly
//! `ops.offdb_verify` task runs it, raises a High alert on a break and, on the local stand-in,
//! purges request lines older than 30 days (the bucket lifecycle rule in Phase 2). Request lines go
//! to `request/<date>/<task id>.json`, AI readings to `ai_reading/<id>.json` (D-734, D-735).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use askhava_adapters::api::{IdempotencyKey, LocalStorage, StorageAdapter};
use askhava_core::api::{
    Claimed, Clock, HandlerError, NewTask, Registry, Settings, SettingsError, arizona,
    check_base_url, enqueue,
};
use askhava_ops::api::{
    AuditRow, EXPORT_PAGE_MAX, NewAlert, STREAM_KIND, Severity, StreamEvent, export_audit,
    raise_alert, stream_event,
};
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use jiff::{SignedDuration, Timestamp, ToSpan};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The `core.task` kind of the nightly hash-chain check and request-log purge (PRIV-029).
pub const VERIFY_KIND: &str = "ops.offdb_verify";
/// The alert kind raised when the audit chain does not verify (PRIV-029).
pub const CHAIN_ALERT: &str = "ops.audit_chain_broken";
/// How long request lines are kept (PRIV-029).
pub const REQUEST_LOG_DAYS: i64 = 30;
/// How long an audit row waits for a missing lower ID to commit before it is linked past it.
const SETTLE: SignedDuration = SignedDuration::from_mins(2);
/// Rows newer than this are not yet checked by the nightly run (their copy may be retrying).
const VERIFY_LAG: SignedDuration = SignedDuration::from_hours(1);
const BASE_URL: &str = "ASKHAVA_STORAGE_BASE_URL";
const ID_SECRET: &str = "ASKHAVA_ID_SECRET";
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// What the stream handlers need: the store, the `ID_SECRET` for idempotency keys (SEC-018), the
/// clock, and the stand-in's directory for the 30-day purge (none for a real bucket).
pub struct OffDbStream<S> {
    /// Where objects are written.
    pub storage: S,
    /// The key under which each object's idempotency key is derived.
    pub id_secret: SecretString,
    /// Where every "now" comes from.
    pub clock: Arc<dyn Clock>,
    /// The local stand-in directory, purged nightly; none for a bucket with a lifecycle rule.
    pub purge_root: Option<PathBuf>,
}

impl OffDbStream<LocalStorage> {
    /// Reads `ASKHAVA_STORAGE_BASE_URL` (a `file://` stand-in directory in Phase 1, D-726) and
    /// `ASKHAVA_ID_SECRET` from the pairs `Settings::from_pairs` took (SEC-002, SEC-032).
    ///
    /// # Errors
    ///
    /// A missing key, or a base URL that is invalid or a stand-in in live.
    pub fn from_settings<I>(
        settings: &Settings,
        pairs: I,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let (mut base, mut secret) = (None, None);
        for (name, value) in pairs.into_iter().filter(|(_, v)| !v.trim().is_empty()) {
            match name.as_str() {
                BASE_URL => base = Some(value.trim().to_owned()),
                ID_SECRET => secret = Some(SecretString::from(value)),
                _ => {}
            }
        }
        let base = base.ok_or(SettingsError::Missing(BASE_URL))?;
        check_base_url(settings.environment(), BASE_URL, &base)?;
        let storage = LocalStorage::new(&base).map_err(|_| SettingsError::Invalid(BASE_URL))?;
        let purge_root = base.strip_prefix("file://").map(PathBuf::from);
        let id_secret = secret.ok_or(SettingsError::Missing(ID_SECRET))?;
        Ok(Self {
            storage,
            id_secret,
            clock,
            purge_root,
        })
    }
}

/// Registers the stream writer and the nightly check on `registry` (OPS-011, PRIV-029, D-491).
pub fn register<S>(registry: &mut Registry, stream: Arc<OffDbStream<S>>)
where
    S: StorageAdapter + Send + Sync + 'static,
{
    let writer = Arc::clone(&stream);
    registry.register(STREAM_KIND, move |pool, task| {
        let stream = Arc::clone(&writer);
        async move { stream.write(&pool, &task).await }
    });
    registry.register(VERIFY_KIND, move |pool, _| {
        let stream = Arc::clone(&stream);
        async move { stream.nightly(&pool).await }
    });
}

impl<S: StorageAdapter + Send + Sync> OffDbStream<S> {
    /// Copies one queued stream event to storage (OPS-011). A retry writes the same bytes under
    /// the same name, which the write-once store accepts (SEC-018).
    async fn write(&self, pool: &PgPool, task: &Claimed) -> Result<(), HandlerError> {
        let p = &task.payload;
        let (name, body) = match p["stream"].as_str() {
            Some("audit") => {
                let id = p["id"].as_i64().ok_or(HandlerError("bad stream payload"))?;
                let mut conn = pool.acquire().await.map_err(|_| HandlerError(DB))?;
                (audit_name(id), self.chain_link(&mut conn, id).await?)
            }
            Some("request") => {
                let at: Timestamp = p["at"].as_str().and_then(|a| a.parse().ok()).ok_or(BAD)?;
                let day = at.to_zoned(jiff::tz::TimeZone::UTC).date();
                (format!("request/{day}/{}.json", task.id), p.clone())
            }
            Some("ai_reading") => {
                let id = p["reading_id"].as_str().ok_or(BAD)?;
                (format!("ai_reading/{id}.json"), p.clone())
            }
            _ => return Err(BAD),
        };
        let bytes = serde_json::to_vec(&body).map_err(|_| BAD)?;
        let key = IdempotencyKey::derive(&self.id_secret, "offdb.put", &[&name]);
        let put = self.storage.put_object(&key, &name, bytes).await;
        put.map_err(|_| HandlerError("the stream object could not be stored"))
    }

    /// The chain object of audit row `id`: its line, and the ID and line hash of the row before
    /// it (PRIV-029). While the row before is missing and `id` is younger than [`SETTLE`], it
    /// fails so the task retries: identity IDs can commit out of order (D-729, D-735).
    async fn chain_link(&self, conn: &mut PgConnection, id: i64) -> Result<Value, HandlerError> {
        let rows = export_audit(conn, id.saturating_sub(EXPORT_PAGE_MAX), EXPORT_PAGE_MAX)
            .await
            .map_err(|_| HandlerError(DB))?;
        let row = rows
            .iter()
            .find(|r| r.id == id)
            .ok_or(HandlerError("audit row missing"))?;
        let prev = rows.iter().rev().find(|r| r.id < id);
        let settled = self.clock.now().duration_since(row.at) >= SETTLE;
        if id > 1 && prev.is_none_or(|p| p.id != id - 1) && !settled {
            return Err(HandlerError("an earlier audit row may still commit"));
        }
        Ok(link(row, prev))
    }

    /// The nightly run: checks the audit chain (a High alert on a break), purges request lines
    /// older than 30 days from the stand-in, and queues tomorrow's run (PRIV-029).
    async fn nightly(&self, pool: &PgPool) -> Result<(), HandlerError> {
        let now = self.clock.now();
        let mut conn = pool.acquire().await.map_err(|_| HandlerError(DB))?;
        let checked = verify_audit_chain(&mut conn, &self.storage, now).await;
        if checked == Err(ChainError::Database) {
            return Err(HandlerError(DB));
        }
        if checked.is_err() {
            let alert = NewAlert {
                kind: CHAIN_ALERT,
                severity: Severity::High,
                subject: None,
            };
            raise_alert(&mut conn, &alert)
                .await
                .map_err(|_| HandlerError(DB))?;
        }
        if let Some(root) = &self.purge_root {
            purge_request_log(root, now).map_err(|_| HandlerError("request log purge failed"))?;
        }
        schedule_verify(
            &mut conn,
            now.checked_add(SignedDuration::from_hours(24))
                .unwrap_or(now),
        )
        .await
        .map_err(|_| HandlerError(DB))
    }
}

const DB: &str = "the database could not be reached";
const BAD: HandlerError = HandlerError("bad stream payload");

/// The object name of audit row `id`, zero-padded so names sort in ID order.
#[must_use]
pub fn audit_name(id: i64) -> String {
    format!("audit/{id:019}.json")
}

/// The sha256, in hex, of a row's JSON line.
fn line_hash(row: &AuditRow) -> String {
    let bytes = serde_json::to_vec(&row.to_json()).unwrap_or_default();
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The chain object for `row` linked to `prev` (none: the start of the chain).
fn link(row: &AuditRow, prev: Option<&AuditRow>) -> Value {
    json!({"line": row.to_json(), "prev_id": prev.map_or(0, |p| p.id),
        "prev_sha256": prev.map_or_else(|| GENESIS.to_owned(), line_hash)})
}

/// Why the chain check did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainError {
    /// The object of this audit row is missing, differs from the row, or does not link to the
    /// row before it (PRIV-029).
    Broken(i64),
    /// The database could not be read.
    Database,
}

/// Checks every audit row older than an hour against its stored object, in ID order: each object
/// must hold exactly the row's line and link to the row before it by ID and line hash (PRIV-029,
/// TD-OPS-19). Returns how many rows passed. The Phase 2 run reads with its own read key, never
/// the app's write-only token (D-735).
///
/// # Errors
///
/// [`ChainError::Broken`] naming the first row that fails, or [`ChainError::Database`].
pub async fn verify_audit_chain<S: StorageAdapter>(
    db: &mut PgConnection,
    storage: &S,
    now: Timestamp,
) -> Result<u64, ChainError> {
    let horizon = now.checked_sub(VERIFY_LAG).unwrap_or(now);
    let (mut prev, mut checked): (Option<AuditRow>, u64) = (None, 0);
    loop {
        let after = prev.as_ref().map_or(0, |p| p.id);
        let page = export_audit(db, after, EXPORT_PAGE_MAX)
            .await
            .map_err(|_| ChainError::Database)?;
        for row in page.iter().take_while(|r| r.at < horizon) {
            let stored = storage.read_object(&audit_name(row.id)).await.ok();
            let object: Option<Value> = stored.and_then(|b| serde_json::from_slice(&b).ok());
            if object != Some(link(row, prev.as_ref())) {
                return Err(ChainError::Broken(row.id));
            }
            prev = Some(row.clone());
            checked += 1;
        }
        if page.iter().any(|r| r.at >= horizon) || page.len() < EXPORT_PAGE_MAX as usize {
            return Ok(checked);
        }
    }
}

/// Removes the stand-in's request-log days older than [`REQUEST_LOG_DAYS`] under `root`
/// (PRIV-029); a real bucket does this with its lifecycle rule on `request/`. Returns how many
/// day folders went.
///
/// # Errors
///
/// A file-system error while listing or removing.
pub fn purge_request_log(root: &Path, now: Timestamp) -> std::io::Result<usize> {
    let today = now.to_zoned(jiff::tz::TimeZone::UTC).date();
    let cutoff = today.checked_sub(REQUEST_LOG_DAYS.days()).unwrap_or(today);
    let dir = root.join("request");
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let day = path
            .file_name()
            .and_then(|n| n.to_str()?.parse::<jiff::civil::Date>().ok());
        if day.is_some_and(|d| d < cutoff) {
            std::fs::remove_dir_all(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Queues the nightly check for 3 a.m. Arizona time on or after `after`'s day, once per day
/// (dedupe key by date); the worker calls it at start and each run queues the next (PRIV-029).
///
/// # Errors
///
/// The queue's error when the insert fails.
pub async fn schedule_verify(
    db: &mut PgConnection,
    after: Timestamp,
) -> Result<(), askhava_core::api::TaskError> {
    let day = after.to_zoned(arizona()).date();
    let run_at = day
        .at(3, 0, 0, 0)
        .to_zoned(arizona())
        .map_or(after, |z| z.timestamp());
    let mut task = NewTask::new(VERIFY_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("offdb.verify.{day}"));
    enqueue(db, &task).await.map(|_| ())
}

/// Middleware: queues one request-log line per request with its request ID, route template and
/// status, never the path or a header value (OPS-011, PRIV-016, rust brain R15). A queue failure
/// is logged once and the response still goes out.
pub async fn log_request(State(pool): State<PgPool>, request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let event = StreamEvent::Request {
        request_id,
        route: &route,
        status,
        at: Timestamp::now(),
    };
    let queued = match pool.acquire().await {
        Ok(mut conn) => stream_event(&mut conn, &event).await.is_ok(),
        Err(_) => false,
    };
    if !queued {
        tracing::warn!("the request log line could not be queued");
    }
    response
}

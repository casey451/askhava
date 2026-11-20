//! Re-keying the suppression list and retiring an index key version (PRIV-024, CON-035, CON-013;
//! D-079, D-4418, D-4748).
//!
//! Proves, against PostgreSQL as `askhava_app`: a v1-only opt-out gains its v2 row with the first
//! row's event and time, v1 is then retired and dropped, the start check passes and `send_checked`
//! still refuses the number; a retirement is refused for the active version and while one listing
//! (one no contact's phone matches, which the batch counts and leaves) lacks its v2 row; an
//! opt-out arriving during the run on a second connection waits for the number's lock and leaves
//! exactly one listing per version; and no log line, error or audit row holds a phone or a digest
//! in hex, base64 or `Debug` form. Fake 555-01xx numbers and keys only.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    INDEX_VERSION_RETIRED_ACTION, NewText, OutboxError, RekeyError, Relation, RevokeChannel, To,
    check_suppression_versions, enqueue, listing_key, rekey_suppression_batch,
    retire_index_version, send_checked, still_unlisted, suppress,
};
use askhava_ops::api::export_audit;
use askhava_privacy::api::{IndexPurpose, NewContact, Vault, rotate_batch, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;
use sqlx::{Connection as _, PgConnection, PgPool, Postgres};
use tokio::task::JoinHandle;

const DB_URL: &str = "postgres://askhava_app@db.invalid:5432/askhava";
const PHONE: &str = "+19285550171";
const ORPHAN: &str = "+19285550172";

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// A vault whose `VAULT_INDEX_KEY` holds `versions` (the first active), version `n` being 32
/// copies of byte `0x20 + n`; the seal key is v1 throughout.
fn vault(versions: &[u8]) -> Vault {
    let key = |v: u8, byte: u8| format!("{v}:{}", STANDARD.encode([byte; 32]));
    let index: Vec<String> = versions.iter().map(|v| key(*v, 0x20 + v)).collect();
    Vault::new(
        &secret(&key(1, 0x11)),
        &secret(&index.join(",")),
        &secret(DB_URL),
    )
    .expect("keys")
}

fn now() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

async fn app_conn(pool: &PgPool) -> PoolConnection<Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn store(conn: &mut PgConnection, phone: &str) -> Uuid {
    let new = NewContact {
        phone: Some(secret(phone)),
        ..NewContact::default()
    };
    store_contact(conn, &vault(&[1]), &new)
        .await
        .expect("store")
}

async fn list(conn: &mut PgConnection, vault: &Vault, phone: &str) -> bool {
    suppress(conn, vault, phone, (RevokeChannel::Portal, "portal"), now())
        .await
        .expect("suppress")
}

/// The list as `(version, event, since)` text rows, in version order: no digest is read out.
async fn rows(conn: &mut PgConnection) -> Vec<(i16, Uuid, String)> {
    sqlx::query_as(
        "SELECT index_version, event_id, since::text FROM messaging.suppression \
         ORDER BY index_version, digest",
    )
    .fetch_all(conn)
    .await
    .expect("rows")
}

/// A subscriber keeping every event's fields as text.
struct Capture(Arc<Mutex<Vec<String>>>);
struct Line(String);

impl tracing::field::Visit for Line {
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={v:?} ", f.name());
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, s: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut line = Line(String::new());
        s.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, r: &tracing::span::Record<'_>) {
        let mut line = Line(String::new());
        r.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut line = Line(String::new());
        e.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Every form a phone or one of its digests under v1 and v2 could leak in: the number with and
/// without its `+1`, and each digest as hex (either case), base64 (standard and URL-safe) and the
/// `Debug` decimal byte list of an array, slice or `Vec` (`[17, 34, ...]`).
fn needles() -> Vec<String> {
    let mut all = Vec::new();
    for phone in [PHONE, ORPHAN] {
        all.extend([phone.to_owned(), phone[2..].to_owned()]);
        for (_, digest) in vault(&[2, 1]).blind_index(IndexPurpose::HomeownerPhone, phone) {
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            let list = format!("{digest:?}");
            all.extend([hex.to_uppercase(), hex, list[1..list.len() - 1].to_owned()]);
            all.extend([STANDARD.encode(digest), URL_SAFE_NO_PAD.encode(digest)]);
        }
    }
    all
}

/// Fails when a captured log line, an error's text or an audit row holds a phone or a digest.
async fn assert_clean(conn: &mut PgConnection, logs: &Mutex<Vec<String>>, errors: &[RekeyError]) {
    let mut seen: Vec<String> = logs.lock().expect("lock").clone();
    seen.extend(errors.iter().map(|e| format!("{e} {e:?}")));
    let audit = export_audit(conn, 0, 1000).await.expect("audit");
    seen.extend(audit.iter().map(|row| format!("{row:?}")));
    for needle in needles() {
        let hit = seen.iter().find(|text| text.contains(&needle));
        assert!(hit.is_none(), "a phone or digest leaked: {hit:?}");
    }
}

#[test]
fn the_leak_check_knows_every_digest_form() {
    let digest = vault(&[1]).blind_index(IndexPurpose::HomeownerPhone, PHONE)[0].1;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let forms = [
        hex,
        format!("{digest:?}"),
        format!("{:?}", &digest[..]),
        format!("{:?}", digest.to_vec()),
        STANDARD.encode(digest),
        format!("to {PHONE}"),
    ];
    for form in forms {
        assert!(needles().iter().any(|n| form.contains(n)), "missed {form}");
    }
}

/// A carrier that counts its calls.
#[derive(Default)]
struct FakeCarrier(Mutex<usize>);

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        *self.0.lock().expect("lock") += 1;
        Err(AdapterError::Vendor {
            status: 500,
            code: None,
        })
    }
}

/// Queues a text for `her` and sends it through `send_checked` under `vault`.
async fn try_send(
    conn: &mut PgConnection,
    vault: &Vault,
    her: Uuid,
) -> (Option<OutboxError>, usize) {
    let id_secret = secret("fake-id-secret-for-tests");
    let new = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(1)],
        recipient: her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: fake body",
    };
    let id = enqueue(conn, &id_secret, &new).await.expect("queued").0;
    let carrier = FakeCarrier::default();
    let to = To::Homeowner {
        vault,
        relation: Relation::OpenRequest,
    };
    let from = (to, "+19285550100");
    let sent = send_checked(conn, (&id_secret, &carrier, vault), id, from, now()).await;
    (sent.err(), *carrier.0.lock().expect("lock"))
}

/// PRIV-024, CON-013, CON-035: a v1-only opt-out gains its v2 row, v1 is retired and dropped, and
/// the number is still refused every text; nothing in the trail holds her number or a digest.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_v1_only_opt_out_is_rekeyed_and_still_refused_after_v1_is_dropped(pool: PgPool) {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let _guard = tracing::subscriber::set_default(Capture(Arc::clone(&logs)));
    let mut conn = app_conn(&pool).await;
    let her = store(&mut conn, PHONE).await;
    assert!(list(&mut conn, &vault(&[1]), PHONE).await);
    let before = rows(&mut conn).await;
    let both = vault(&[2, 1]);
    assert_eq!(rotate_batch(&mut conn, &both, 10).await.expect("rotate"), 1);
    assert_eq!(rekey_suppression_batch(&mut conn, &both, 10).await, Ok(1));
    assert_eq!(rekey_suppression_batch(&mut conn, &both, 10).await, Ok(0));
    assert_eq!(rotate_batch(&mut conn, &both, 10).await.expect("rotate"), 0);
    let (_, event, since) = before[0].clone();
    let listed = vec![(1, event, since.clone()), (2, event, since.clone())];
    assert_eq!(
        rows(&mut conn).await,
        listed,
        "the copy keeps event and time"
    );
    assert_eq!(retire_index_version(&mut conn, &both, 1).await, Ok(1));
    assert_eq!(rows(&mut conn).await, vec![(2, event, since)]);
    let dropped = vault(&[2]);
    assert_eq!(
        check_suppression_versions(&mut conn, &dropped).await,
        Ok(())
    );
    let audit = export_audit(&mut conn, 0, 1000).await.expect("audit");
    let retired: Vec<_> = audit
        .iter()
        .filter(|r| r.action == INDEX_VERSION_RETIRED_ACTION)
        .collect();
    assert_eq!(retired.len(), 1, "one audit row for the retirement");
    assert_eq!(retired[0].subject_id, Uuid::from_u128(1));
    let reads = audit.iter().filter(|r| r.action.starts_with("vault_read."));
    assert_eq!(reads.count(), 6, "5 fields re-sealed, then the phone alone");
    let (refused, calls) = try_send(&mut conn, &dropped, her).await;
    assert_eq!((refused, calls), (Some(OutboxError::Suppressed), 0));
    assert_clean(&mut conn, &logs, &[]).await;
}

/// PRIV-024: retiring is refused for the active version and while one listing lacks its v2 row;
/// a listing no contact's phone matches is counted and left, and nothing is removed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn retiring_is_refused_while_a_listing_lacks_its_active_row(pool: PgPool) {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let _guard = tracing::subscriber::set_default(Capture(Arc::clone(&logs)));
    let mut conn = app_conn(&pool).await;
    store(&mut conn, PHONE).await;
    for phone in [PHONE, ORPHAN] {
        assert!(list(&mut conn, &vault(&[1]), phone).await);
    }
    let both = vault(&[2, 1]);
    let unlisted = |count| RekeyError::Unlisted {
        version: 1,
        active: 2,
        count,
    };
    let mut errors = Vec::new();
    for step in 0..2 {
        let refused = retire_index_version(&mut conn, &both, 1).await;
        assert_eq!(refused, Err(unlisted(2 - step)), "refused at step {step}");
        errors.extend(refused.err());
        assert_eq!(
            rows(&mut conn).await.len(),
            usize::try_from(2 + step).expect("small")
        );
        rotate_batch(&mut conn, &both, 10).await.expect("rotate");
        let moved = rekey_suppression_batch(&mut conn, &both, 1).await;
        assert_eq!(
            moved,
            Ok(usize::from(step == 0)),
            "the orphan never stalls it"
        );
    }
    let active = retire_index_version(&mut conn, &both, 2).await;
    assert_eq!(active, Err(RekeyError::Active(2)));
    errors.extend(active.err());
    let warned = logs
        .lock()
        .expect("lock")
        .iter()
        .any(|l| l.contains("left=1"));
    assert!(warned, "the orphan is counted in a warning");
    assert_clean(&mut conn, &logs, &errors).await;
}

/// PRIV-003, D-752: the batch's vault read is refused inside a caller's transaction, whose
/// rollback could undo its audit rows; the error names nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_batch_refuses_a_connection_inside_a_transaction(pool: PgPool) {
    let logs = Mutex::new(Vec::new());
    let mut conn = app_conn(&pool).await;
    assert!(list(&mut conn, &vault(&[1]), ORPHAN).await);
    let mut tx = conn.begin().await.expect("begin");
    let refused = rekey_suppression_batch(&mut tx, &vault(&[2, 1]), 10).await;
    assert_eq!(refused, Err(RekeyError::Vault));
    tx.rollback().await.expect("rollback");
    assert_clean(&mut conn, &logs, &[RekeyError::Vault]).await;
}

/// On a new `askhava_app` connection, runs the batch, or with `opt_out` a portal opt-out of
/// `PHONE`, in its own task, and returns once its backend waits on the number's lock.
async fn spawn_waiting(
    pool: &PgPool,
    observer: &mut PgConnection,
    opt_out: bool,
) -> JoinHandle<usize> {
    let mut conn = app_conn(pool).await;
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *conn)
        .await
        .expect("pid");
    let task = tokio::spawn(async move {
        let both = vault(&[2, 1]);
        if opt_out {
            return usize::from(list(&mut conn, &both, PHONE).await);
        }
        let moved = rekey_suppression_batch(&mut conn, &both, 10).await;
        moved.expect("batch")
    });
    seen_waiting(observer, pid).await;
    task
}

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

/// CON-035, D-4418: while the number's lock is held, the batch and an opt-out arriving on a second
/// connection both wait; after the commit each finishes and the number has one row per version.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_opt_out_during_the_run_leaves_one_listing_per_version(pool: PgPool) {
    let mut observer = app_conn(&pool).await;
    store(&mut observer, PHONE).await;
    assert!(list(&mut observer, &vault(&[1]), PHONE).await);
    let both = vault(&[2, 1]);
    rotate_batch(&mut observer, &both, 10)
        .await
        .expect("rotate");
    let first = rows(&mut observer).await[0].clone();
    let mut holder = app_conn(&pool).await;
    let mut tx = holder.begin().await.expect("begin");
    let digests = both.blind_index(IndexPurpose::HomeownerPhone, PHONE);
    let key = listing_key(&both, &digests).expect("key");
    assert!(!still_unlisted(&mut tx, &key).await.expect("lock"));
    let tasks = [
        spawn_waiting(&pool, &mut observer, false).await,
        spawn_waiting(&pool, &mut observer, true).await,
    ];
    assert!(
        tasks.iter().all(|task| !task.is_finished()),
        "both wait for the lock"
    );
    tx.commit().await.expect("commit");
    let mut done = Vec::new();
    for task in tasks {
        let finished = tokio::time::timeout(Duration::from_secs(10), task).await;
        done.push(finished.expect("finished in time").expect("task"));
    }
    assert_eq!(
        done,
        [1, 0],
        "the batch moved her; the opt-out listed nothing new"
    );
    let listed = vec![(1, first.1, first.2.clone()), (2, first.1, first.2)];
    assert_eq!(
        rows(&mut observer).await,
        listed,
        "one row per version, the first event's"
    );
    assert_eq!(
        rekey_suppression_batch(&mut observer, &both, 10).await,
        Ok(0)
    );
    assert_eq!(retire_index_version(&mut observer, &both, 1).await, Ok(1));
}

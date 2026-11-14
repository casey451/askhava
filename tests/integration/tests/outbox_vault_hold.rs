//! A passing vault error holds an outbox row; it never expires it (CON-009, CON-013, TXT-021;
//! D-3149, D-3581).
//!
//! On fresh databases, the `messaging.send_due` task runs through `core`'s registry as the worker
//! runs it, as `askhava_app`, on a manual clock at 10:00 Arizona time with a fake carrier. A test
//! trigger on `ops.audit_log` stands in for the vault: it fails the audit row of her second phone
//! read (the one in `optout::send_checked`), which `privacy` reports as `VaultError::Audit`; the row
//! stays `queued`, held 30 minutes (`RETRY_LAST`), and the run after the hold sends it, while the
//! row behind it in the batch still goes. A contact deleted after the first read still expires its
//! row. The next run is queued before the batch is worked: the carrier sees it. All data is fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_core::api::{Clock, Lease, ManualClock, Registry, WorkerId, claim};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    NewText, OutboxSender, Relation, SEND_EVERY, SendingNumber, enqueue, register_send_due,
    schedule_send_due,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgPool};

/// The stand-in's valid sending number.
const FROM: &str = "+15005550006";
/// The sender's private `RETRY_LAST`: how long a row whose step failed is held.
const HELD_FOR: SignedDuration = SignedDuration::from_mins(30);

/// 10:00 Arizona time on July 15, 2026 (UTC-7), inside the texting window.
fn morning() -> Timestamp {
    jiff::civil::date(2026, 7, 15)
        .at(10, 0, 0, 0)
        .in_tz("America/Phoenix")
        .expect("zone")
        .timestamp()
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

/// A carrier that accepts every text and, at each call, records whether the next run (due at
/// `next`) was already queued, read through `watch`.
#[derive(Clone)]
struct Fake {
    seen: Arc<Mutex<Vec<i64>>>,
    watch: PgPool,
    next: Timestamp,
}

impl Fake {
    fn calls(&self) -> usize {
        self.seen.lock().expect("lock").len()
    }
}

impl SmsAdapter for Fake {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM core.task WHERE kind = 'messaging.send_due' AND run_at = $1",
        )
        .bind(self.next.to_sqlx())
        .fetch_one(&self.watch)
        .await
        .expect("next run");
        let n = {
            let mut seen = self.seen.lock().expect("lock");
            seen.push(queued);
            seen.len()
        };
        Ok(SentText {
            sid: format!("SM{n:032}"),
            status: "queued".to_owned(),
        })
    }
}

/// The worker's pieces: an `askhava_app` pool, the manual clock, the registry, the carrier and
/// two fake homeowners.
struct World {
    app: PgPool,
    clock: Arc<ManualClock>,
    registry: Registry,
    carrier: Fake,
    her: Uuid,
    other: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let app = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let clock = Arc::new(ManualClock::new(morning()));
    let carrier = Fake {
        seen: Arc::default(),
        watch: pool.clone(),
        next: morning() + SEND_EVERY,
    };
    let worker_clock: Arc<dyn Clock> = clock.clone();
    let number = SendingNumber::parse(FROM).ok();
    let sender = OutboxSender::new(
        (secret(), vault()),
        carrier.clone(),
        (number, None),
        worker_clock,
    )
    .expect("sender");
    let mut registry = Registry::new();
    register_send_due(&mut registry, Arc::new(sender));
    let her = contact(&app, "928-555-0142").await;
    let other = contact(&app, "928-555-0143").await;
    World {
        app,
        clock,
        registry,
        carrier,
        her,
        other,
    }
}

/// Stores a fake homeowner with `phone`.
async fn contact(app: &PgPool, phone: &str) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let mut db = app.acquire().await.expect("connection");
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

/// Queues a `first_quote` text about `job` for `recipient`, recorded as queued `ago` minutes back.
async fn queue(w: &World, recipient: Uuid, job: u128, ago: i64) -> Uuid {
    let text = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(job)],
        recipient,
        relation: Relation::OpenRequest,
        body: "Laftly: fake body",
    };
    let mut db = w.app.acquire().await.expect("connection");
    let (id, _) = enqueue(&mut db, &secret(), &text).await.expect("queued");
    let at = morning() - SignedDuration::from_mins(ago);
    sqlx::query("UPDATE messaging.outbox SET created_at = $2 WHERE id = $1")
        .bind(id)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("queued at");
    id
}

/// Installs a trigger on `ops.audit_log` that runs `act` when the vault audits the read of
/// contact `who`'s phone for the `nth` time (1-based); `act` is SQL run inside the trigger.
async fn on_read(pool: &PgPool, who: Uuid, nth: i64, act: &str) {
    let ddl = [
        "CREATE SEQUENCE public.test_phone_reads".to_owned(),
        format!(
            "CREATE FUNCTION public.test_on_read() RETURNS trigger LANGUAGE plpgsql \
             SECURITY DEFINER AS $$ BEGIN \
             IF NEW.subject_kind = 'privacy.contact' AND NEW.subject_id = '{who}' \
             AND NEW.field = 'phone' AND nextval('public.test_phone_reads') = {nth} \
             THEN {act}; END IF; RETURN NEW; END $$"
        ),
        "CREATE TRIGGER test_on_read BEFORE INSERT ON ops.audit_log FOR EACH ROW \
         EXECUTE FUNCTION public.test_on_read()"
            .to_owned(),
    ];
    for sql in ddl {
        sqlx::raw_sql(AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("trigger");
    }
}

/// Runs every task due at the clock's now (queuing a run first), as the worker would.
async fn run_due(w: &World) {
    let mut db = w.app.acquire().await.expect("connection");
    schedule_send_due(&mut db, w.clock.now())
        .await
        .expect("schedule");
    drop(db);
    let owner = WorkerId::new(&w.app).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let tasks = claim(&w.app, owner, w.clock.now(), lease, 50)
        .await
        .expect("claim");
    for task in tasks {
        w.registry
            .run(&w.app, w.clock.as_ref(), owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

/// A row's status and `not_before`.
async fn row(w: &World, id: Uuid) -> (String, Option<Timestamp>) {
    let (status, not_before): (String, Option<jiff_sqlx::Timestamp>) =
        sqlx::query_as("SELECT status, not_before FROM messaging.outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&w.app)
            .await
            .expect("row");
    (status, not_before.map(|t| t.to_jiff()))
}

async fn expired_audits(w: &World) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = 'messaging.text_expired'")
        .fetch_one(&w.app)
        .await
        .expect("count")
}

/// CON-009, CON-013, D-3581: a passing audit failure on her second phone read (in `send_checked`)
/// leaves her row queued and held, not expired, and the run after the hold sends it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_passing_vault_error_on_the_send_read_holds_the_row(pool: PgPool) {
    let w = world(&pool).await;
    let held = queue(&w, w.her, 1, 30).await;
    on_read(
        &pool,
        w.her,
        2,
        "RAISE EXCEPTION 'fake passing audit failure'",
    )
    .await;
    run_due(&w).await;
    assert_eq!(
        row(&w, held).await,
        ("queued".to_owned(), Some(morning() + HELD_FOR))
    );
    assert_eq!(expired_audits(&w).await, 0, "a passing error never expires");
    assert_eq!(w.carrier.calls(), 0);
    w.clock.advance(HELD_FOR + SEND_EVERY);
    run_due(&w).await;
    assert_eq!(
        row(&w, held).await.0,
        "sent",
        "the run after the hold sends it"
    );
    assert_eq!(w.carrier.calls(), 1);
}

/// D-3149: a contact gone by the send's read still expires its row, with one audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_row_whose_contact_is_gone_still_expires(pool: PgPool) {
    let w = world(&pool).await;
    let gone = queue(&w, w.her, 1, 30).await;
    let act = "DELETE FROM privacy.contact WHERE id = NEW.subject_id";
    on_read(&pool, w.her, 1, act).await;
    run_due(&w).await;
    assert_eq!(row(&w, gone).await.0, "expired");
    assert_eq!(expired_audits(&w).await, 1);
    assert_eq!(w.carrier.calls(), 0);
}

/// D-2628, D-3149: the next run is queued before the batch is worked, so the carrier already sees
/// it at the first send.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_next_run_is_queued_before_the_batch_is_worked(pool: PgPool) {
    let w = world(&pool).await;
    let id = queue(&w, w.her, 1, 30).await;
    run_due(&w).await;
    assert_eq!(row(&w, id).await.0, "sent");
    assert_eq!(*w.carrier.seen.lock().expect("lock"), vec![1]);
}

/// D-3149: a row whose send fails is held while the row behind it in the same batch still goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_send_is_held_and_the_rest_of_the_batch_goes(pool: PgPool) {
    let w = world(&pool).await;
    let failed = queue(&w, w.her, 1, 30).await;
    let next = queue(&w, w.other, 2, 20).await;
    on_read(
        &pool,
        w.her,
        2,
        "RAISE EXCEPTION 'fake passing audit failure'",
    )
    .await;
    run_due(&w).await;
    assert_eq!(
        row(&w, failed).await,
        ("queued".to_owned(), Some(morning() + HELD_FOR))
    );
    assert_eq!(row(&w, next).await.0, "sent", "the row behind it goes");
    assert_eq!(w.carrier.calls(), 1);
}

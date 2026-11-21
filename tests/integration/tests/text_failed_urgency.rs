//! A failed text's call-back urgency (INT-023, TXT-015, OPS-032; D-226, D-277, D-4052).
//!
//! On fresh databases, the `messaging.send_due` task runs through `core`'s registry as the worker
//! runs it, as `askhava_app`, on a manual clock, with a fake carrier that refuses every send; her
//! fake contact has no email. After the last TXT-014 retry the text is given up and Casey gets one
//! `messaging` call-back alert: with `intake::api::contact_urgent` as the sender's urgency source,
//! an emergency job's and a "Today, it's urgent" job's is High and due 24 hours after, and a
//! regular job's is Normal and due after the no-rush setting; with no source set it is no-rush, as
//! before D-4052; a failing urgency read holds the row and raises nothing. All data is fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_core::api::{Clock, Lease, ManualClock, Registry, WorkerId, claim};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    CALL_BACK_ALERT, MailboxKind, NO_RUSH_CALL_BACK, NewText, OutboxError, OutboxSender,
    PostalAddress, Relation, SendingNumber, Urgency, UrgencyRead, UrgencySource, enqueue,
    register_send_due, schedule_send_due,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The stand-in's one valid sending number.
const FROM: &str = "+15005550006";

/// An instant given as Arizona wall time on July 15, 2026 (UTC-7, no daylight saving).
fn az(hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 7, 15)
        .at(hour, minute, 0, 0)
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

/// A carrier that refuses every send (a 400 with no code: a definite refusal, retried).
#[derive(Clone, Default)]
struct Refusing(Arc<Mutex<usize>>);

impl SmsAdapter for Refusing {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        *self.0.lock().expect("lock") += 1;
        Err(AdapterError::Vendor {
            status: 400,
            code: None,
        })
    }
}

/// The composition's source: `intake::api::contact_urgent`.
struct FromIntake;

impl UrgencySource for FromIntake {
    fn urgency<'a>(
        &'a self,
        conn: &'a mut PgConnection,
        contact: Uuid,
        now: Timestamp,
    ) -> UrgencyRead<'a> {
        Box::pin(async move {
            match askhava_intake::api::contact_urgent(conn, contact, now).await {
                Ok(true) => Ok(Urgency::Urgent),
                Ok(false) => Ok(Urgency::NoRush),
                Err(_) => Err(OutboxError::Database),
            }
        })
    }
}

/// A source whose read always fails.
struct Failing;

impl UrgencySource for Failing {
    fn urgency<'a>(&'a self, _: &'a mut PgConnection, _: Uuid, _: Timestamp) -> UrgencyRead<'a> {
        Box::pin(async { Err(OutboxError::Database) })
    }
}

/// The worker's pieces: an `askhava_app` pool, the manual clock, the registry and her contact.
struct World {
    app: PgPool,
    clock: Arc<ManualClock>,
    registry: Registry,
    her: Uuid,
}

async fn world(pool: &PgPool, source: Option<Arc<dyn UrgencySource>>) -> World {
    let app = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let clock = Arc::new(ManualClock::new(az(10, 0)));
    let postal = PostalAddress::new(
        "PO Box 4417, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    );
    let worker_clock: Arc<dyn Clock> = clock.clone();
    let numbers = (SendingNumber::parse(FROM).ok(), postal.ok());
    let sender = OutboxSender::new(
        (secret(), vault()),
        Refusing::default(),
        numbers,
        worker_clock,
    )
    .expect("sender");
    let sender = match source {
        Some(source) => sender.with_urgency(source),
        None => sender,
    };
    let mut registry = Registry::new();
    register_send_due(&mut registry, Arc::new(sender));
    let contact = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from("928-555-0142".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let mut db = app.acquire().await.expect("connection");
    let her = store_contact(&mut db, &vault(), &contact)
        .await
        .expect("contact");
    World {
        app,
        clock,
        registry,
        her,
    }
}

/// Her open request, as the superuser, with one stored answer `(question, choice)`.
async fn request(pool: &PgPool, her: Uuid, (question, choice): (&str, &str)) {
    let at = az(9, 0).to_sqlx();
    let homeowner: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $2) RETURNING id",
    )
    .bind(her)
    .bind(at)
    .fetch_one(pool)
    .await
    .expect("homeowner");
    let conversation: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at) VALUES ($1, 'en', $2, 'consent', $3, $3) \
         RETURNING id",
    )
    .bind(homeowner)
    .bind(Uuid::from_u128(0xfeed))
    .bind(at)
    .fetch_one(pool)
    .await
    .expect("conversation");
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(conversation)
    .bind(question)
    .bind(choice)
    .bind(at)
    .execute(pool)
    .await
    .expect("answer");
}

/// Queues one text for her.
async fn queue(w: &World) -> Uuid {
    let text = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(21)],
        recipient: w.her,
        relation: Relation::OpenRequest,
        body: "Fake body",
    };
    let mut db = w.app.acquire().await.expect("connection");
    enqueue(&mut db, &secret(), &text).await.expect("queued").0
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

/// Runs the first send and its three retries, each at its `not_before`; the instant of the last.
async fn send_until_given_up(w: &World, id: Uuid) -> Timestamp {
    let mut last = w.clock.now();
    for _ in 0..4 {
        run_due(w).await;
        last = w.clock.now();
        if let Some(at) = not_before(w, id).await {
            w.clock.set(at);
        }
    }
    last
}

async fn not_before(w: &World, id: Uuid) -> Option<Timestamp> {
    sqlx::query_scalar::<_, Option<jiff_sqlx::Timestamp>>(
        "SELECT not_before FROM messaging.outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&w.app)
    .await
    .expect("row")
    .map(|t| t.to_jiff())
}

/// The call-back alerts raised for text `id`: their severities.
async fn alerts(w: &World, id: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT severity FROM ops.alert WHERE kind = $1 AND subject_kind = 'messaging.outbox' \
         AND subject_id = $2",
    )
    .bind(CALL_BACK_ALERT)
    .bind(id)
    .fetch_all(&w.app)
    .await
    .expect("alerts")
}

/// The call-back due time recorded on text `id`'s retry row.
async fn due(w: &World, id: Uuid) -> Option<Timestamp> {
    sqlx::query_scalar::<_, Option<jiff_sqlx::Timestamp>>(
        "SELECT call_back_due FROM messaging.text_retry WHERE outbox_id = $1",
    )
    .bind(id)
    .fetch_one(&w.app)
    .await
    .expect("retry row")
    .map(|t| t.to_jiff())
}

/// Her text about a job answered `(question, choice)` is given up; the alert's severity and due.
async fn given_up(
    pool: &PgPool,
    source: Option<Arc<dyn UrgencySource>>,
    answer: (&str, &str),
) -> (Vec<String>, SignedDuration) {
    let w = world(pool, source).await;
    request(pool, w.her, answer).await;
    let id = queue(&w).await;
    let at = send_until_given_up(&w, id).await;
    let due = due(&w, id).await.expect("a call-back is due");
    (alerts(&w, id).await, due.duration_since(at))
}

/// INT-023, TXT-015, OPS-032: an emergency job's given-up text raises one High call-back due in
/// 24 hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_emergency_jobs_failed_text_is_a_high_call_back_due_in_24_hours(pool: PgPool) {
    let source: Arc<dyn UrgencySource> = Arc::new(FromIntake);
    let answer = ("emergency_choice", "emergency");
    let (alerts, after) = given_up(&pool, Some(source), answer).await;
    assert_eq!(alerts, ["high"]);
    assert_eq!(after, SignedDuration::from_hours(24));
}

/// INT-023, TXT-015, OPS-032: a "Today, it's urgent" job's given-up text raises one High call-back
/// due in 24 hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_today_jobs_failed_text_is_a_high_call_back_due_in_24_hours(pool: PgPool) {
    let source: Arc<dyn UrgencySource> = Arc::new(FromIntake);
    let (alerts, after) = given_up(&pool, Some(source), ("how_soon", "today")).await;
    assert_eq!(alerts, ["high"]);
    assert_eq!(after, SignedDuration::from_hours(24));
}

/// INT-023, TXT-015: a regular job's given-up text raises one Normal call-back due after the
/// no-rush setting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_regular_jobs_failed_text_is_a_normal_call_back_due_after_the_setting(pool: PgPool) {
    let source: Arc<dyn UrgencySource> = Arc::new(FromIntake);
    let (alerts, after) = given_up(&pool, Some(source), ("how_soon", "this_week")).await;
    assert_eq!(alerts, ["normal"]);
    assert_eq!(after, NO_RUSH_CALL_BACK);
}

/// D-4052: a sender with no urgency source keeps today's no-rush call-back, even for an emergency.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn with_no_source_the_call_back_is_no_rush(pool: PgPool) {
    let (alerts, after) = given_up(&pool, None, ("emergency_choice", "emergency")).await;
    assert_eq!(alerts, ["normal"]);
    assert_eq!(after, NO_RUSH_CALL_BACK);
}

/// D-4052: a failing urgency read holds the row for its retry; no call-back is raised or guessed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failing_urgency_read_holds_the_row(pool: PgPool) {
    let w = world(&pool, Some(Arc::new(Failing))).await;
    let id = queue(&w).await;
    let at = send_until_given_up(&w, id).await;
    let status: String = sqlx::query_scalar("SELECT status FROM messaging.outbox WHERE id = $1")
        .bind(id)
        .fetch_one(&w.app)
        .await
        .expect("row");
    assert_eq!(status, "queued", "the row waits for its retry");
    assert!(not_before(&w, id).await.is_some_and(|t| t > at));
    assert!(alerts(&w, id).await.is_empty());
    assert_eq!(due(&w, id).await, None);
}

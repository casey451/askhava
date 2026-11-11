//! EMERGENCY job-card notices and charge receipts through the production worker registry (T313;
//! TD-CARD-15: CARD-020, CARD-021, QH-004, BIZ-037; TD-FEE-55: FEE-031; D-1826 (1), D-2203,
//! D-2287, D-3095, D-3096).
//!
//! The registry is `askhava_server::worker_registry`, built from dev stand-in settings with the
//! footer postal setting, so `messaging.business_notice` and `messaging.charge_receipt` run with
//! the composition's own senders and send-time checks. The layer table lets a test crate depend on
//! modules but not on `askhava-server` (D-736), so the composition root is compiled in place, as
//! `task_registry.rs` does (D-2853, D-3096). Due tasks are claimed on a manual clock and run
//! through `Registry::run`; every stored email is then handed to a fake email provider through
//! `messaging::api::send_email`. Every ID, mailbox and address is fake.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    Acting, Checks, EMERGENCY_YES_LIFE, Event, JobTypeChoice, Roles, add_member, add_owner, apply,
    create, record_alert_answer, request_welcome, send_emergency_test, set_after_hours,
    set_emergency_emails, set_job_types, set_towns, welcome_sent,
};
use askhava_core::api::{
    Clock, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, claim, enqueue,
};
use askhava_messaging::api::{
    BUSINESS_NOTICE_KIND, DailyLimit, EmailTo, RECEIPT_EMAIL_KIND, send_email,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{RECEIPT_KIND, queue_receipt};
use askhava_picks::api::ConfirmSettings;
use askhava_quotes::api::{
    CardNotice, EMERGENCY_FIRST_LINE, emergency_card_subject, queue_card_notice,
    queue_emergency_card,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t313-not-a-real-key";
const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";
const TOWN: &str = "lake_havasu_city";
const EMERGENCY_JOB: Uuid = Uuid::from_u128(0x313_0001);
const ORDINARY_JOB: Uuid = Uuid::from_u128(0x313_0002);
const NOTICE: CardNotice<'static> = CardNotice {
    number: 1042,
    category: "HVAC",
    town: "Lake Havasu City",
    card: "AC not cooling. How soon: today.",
};

fn az(time: &str) -> Timestamp {
    time.parse().expect("time")
}

/// 10:40 p.m. Arizona time, inside the night.
fn night() -> Timestamp {
    az("2026-11-02T22:40:00-07:00")
}

/// 8:00:00 a.m. Arizona time the next morning.
fn eight() -> Timestamp {
    az("2026-11-03T08:00:00-07:00")
}

/// One email the fake provider took: (address, subject, plain text).
type Taken = (String, String, String);

/// A fake email provider: what it took, by idempotency key (one email per key).
#[derive(Clone, Default)]
struct FakeMail {
    sent: Arc<Mutex<HashMap<String, Taken>>>,
}

impl EmailAdapter for FakeMail {
    async fn send_email(
        &self,
        key: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        let value = (
            email.to.to_owned(),
            email.subject.to_owned(),
            email.text.to_owned(),
        );
        sent.entry(key.as_str().to_owned()).or_insert(value);
        Ok(format!("re_{:08}", sent.len()))
    }

    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

impl FakeMail {
    /// Every email taken: (address, subject, plain text), sorted.
    fn taken(&self) -> Vec<Taken> {
        let mut all: Vec<_> = self.sent.lock().expect("lock").values().cloned().collect();
        all.sort();
        all
    }
}

/// The fake sign-in address of mailbox `id`.
fn address(id: Uuid) -> String {
    format!("mailbox-{}@example.invalid", id.simple())
}

/// The production registry from dev stand-in settings with the postal setting, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t313-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
        ("ASKHAVA_POSTAL_ADDRESS", POSTAL.into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let parts = (None, ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            registry
                .run(pool, clock, owner, task, Lease::WORKER)
                .await
                .expect("run");
        }
    }
}

/// Hands every unsent email row to `mail` at `now`, addressed to its mailbox's fake address.
async fn deliver(pool: &PgPool, mail: &FakeMail, now: Timestamp) {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, recipient_id FROM messaging.email_outbox WHERE status <> 'sent' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .expect("outbox");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let limit = DailyLimit::new(100, 90).expect("limit");
    let mut db = pool.acquire().await.expect("conn");
    for (id, mailbox) in rows {
        let to = address(mailbox);
        send_email(
            &mut db,
            (&secret, mail, limit),
            id,
            EmailTo::Business(&to),
            now,
        )
        .await
        .expect("send");
    }
}

async fn email_rows(pool: &PgPool, template: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.email_outbox WHERE template_id = $1")
        .bind(template)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Queues every `kind` task's payload again under a new dedupe key, due at `at`: a repeat run.
async fn requeue(pool: &PgPool, kind: &'static str, at: Timestamp) {
    let payloads: Vec<Value> =
        sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(kind)
            .fetch_all(pool)
            .await
            .expect("payloads");
    let mut db = pool.acquire().await.expect("conn");
    for (n, payload) in payloads.into_iter().enumerate() {
        let mut task = NewTask::new(kind, payload, at);
        task.dedupe_key = Some(format!("t313.repeat.{kind}.{n}.{}", at.as_second()));
        enqueue(&mut db, &task).await.expect("requeue");
    }
}

/// An active, after-hours HVAC business: the owner (mailbox `seed`) has "Emergency emails at any
/// hour" on and answered the test; the "Cards and quotes" member (`seed + 1`) has it off; the
/// "Confirm and billing" member (`seed + 2`) gets no cards.
async fn after_hours_business(pool: &PgPool, seed: u128) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let t = az("2026-11-02T09:00:00-07:00");
    let b = create(&mut db, false, t).await.expect("business");
    let owner = add_owner(&mut db, b, Uuid::from_u128(seed), t).await;
    let owner = Acting {
        business: b,
        member: owner.expect("owner"),
    };
    for (n, roles) in [(1, (true, false)), (2, (false, true))] {
        let roles = Roles::new(roles.0, roles.1).expect("roles");
        let m = add_member(&mut db, owner, Uuid::from_u128(seed + n), roles, t).await;
        m.expect("member");
    }
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(&mut db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    request_welcome(&mut db, b, t).await.expect("welcome");
    welcome_sent(&mut db, b, t).await.expect("sent");
    let actor = Actor::Business(b);
    set_towns(&mut db, b, &[TOWN], actor).await.expect("towns");
    let job = JobTypeChoice::new("hvac_repair", "no cooling");
    let allowed: BTreeSet<_> = [job.clone()].into();
    set_job_types(&mut db, b, &[job], &allowed, actor)
        .await
        .expect("job types");
    let at = az("2026-11-02T10:00:00-07:00");
    set_emergency_emails(&mut db, owner, true, at)
        .await
        .expect("on");
    send_emergency_test(&mut db, owner, at).await.expect("test");
    record_alert_answer(&mut db, owner, true, at)
        .await
        .expect("yes");
    set_after_hours(&mut db, b, true, (at, EMERGENCY_YES_LIFE))
        .await
        .expect("tick");
    b
}

/// TD-CARD-15 (CARD-020, CARD-021, QH-004, QH-005, BIZ-037): at 22:40 the EMERGENCY card notice
/// reaches the fake provider at once, with CARD-021's subject and first line, for the switched-on
/// owner of an after-hours business only; the member without the switch gets it at 08:00:00; an
/// ordinary card notice at 22:40 is held to 08:00:00 with the ordinary subject; a repeat run of
/// either sends nothing twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_15_emergency_card_goes_at_night_and_ordinary_waits(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(night()));
    let registry = registry(Arc::clone(&clock) as Arc<dyn Clock>);
    let b = after_hours_business(&pool, 0xe0).await;
    let mut db = pool.acquire().await.expect("conn");
    let queued = queue_emergency_card(&mut db, (EMERGENCY_JOB, b), &NOTICE, night()).await;
    assert!(queued.expect("emergency"));
    let ordinary = queue_card_notice(&mut db, (ORDINARY_JOB, b), &NOTICE, (false, night())).await;
    assert!(ordinary.expect("ordinary"));
    drop(db);
    let mail = FakeMail::default();
    run_due(&pool, &registry, &clock).await;
    deliver(&pool, &mail, night()).await;
    let subject = emergency_card_subject("HVAC", "Lake Havasu City");
    assert_eq!(
        subject,
        "Ask Hava: EMERGENCY HVAC job in Lake Havasu City. Quote as soon as you can."
    );
    let at_night = mail.taken();
    assert_eq!(at_night.len(), 1, "only the switched-on owner at 22:40");
    let (to, got_subject, text) = &at_night[0];
    assert_eq!(*to, address(Uuid::from_u128(0xe0)));
    assert_eq!(*got_subject, subject);
    assert!(
        text.starts_with(&format!("{EMERGENCY_FIRST_LINE}\n")),
        "{text}"
    );
    assert_eq!(
        EMERGENCY_FIRST_LINE,
        "EMERGENCY: the homeowner chose emergency. Quote as soon as you can."
    );

    clock.set(eight() - SignedDuration::from_secs(1));
    run_due(&pool, &registry, &clock).await;
    deliver(&pool, &mail, clock.now()).await;
    assert_eq!(mail.taken().len(), 1, "nothing more before 08:00:00");

    clock.set(eight());
    run_due(&pool, &registry, &clock).await;
    deliver(&pool, &mail, eight()).await;
    let all = mail.taken();
    let ordinary_subject =
        "Ask Hava: New HVAC job in Lake Havasu City. Quote by 8 a.m. Arizona time.";
    let member = address(Uuid::from_u128(0xe1));
    let owner = address(Uuid::from_u128(0xe0));
    let mut got: Vec<(&str, &str)> = all
        .iter()
        .map(|(to, s, _)| (to.as_str(), s.as_str()))
        .collect();
    got.sort_unstable();
    let mut want = vec![
        (owner.as_str(), subject.as_str()),
        (owner.as_str(), ordinary_subject),
        (member.as_str(), subject.as_str()),
        (member.as_str(), ordinary_subject),
    ];
    want.sort_unstable();
    assert_eq!(
        got, want,
        "the rest of the EMERGENCY card and the held card at 08:00:00"
    );
    let ordinary_texts = all.iter().filter(|(_, s, _)| s == ordinary_subject);
    assert!(
        ordinary_texts
            .into_iter()
            .all(|(_, _, t)| !t.contains("EMERGENCY"))
    );

    let before = email_rows(&pool, "job_card_notice").await;
    assert_eq!(before, 4);
    requeue(&pool, BUSINESS_NOTICE_KIND, eight()).await;
    run_due(&pool, &registry, &clock).await;
    deliver(&pool, &mail, eight()).await;
    assert_eq!(
        email_rows(&pool, "job_card_notice").await,
        before,
        "nothing stored twice"
    );
    assert_eq!(mail.taken(), all, "nothing sent twice");
}

/// A job with one succeeded $25 charge for `pick` by `business`, written as T062 and T189 store
/// it (as `receipts.rs` does), and `business` active with its owner's mailbox `owner`.
async fn charged(pool: &PgPool, (pick, business, owner): (Uuid, Uuid, Uuid)) {
    let mut conn = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut conn, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let press = Uuid::from_u128(0x313_0f04);
    let label = format!("pick-{pick}-try1");
    let statements = [
        "INSERT INTO payments.pick_intent VALUES ($1, $2, $3, 2500, 'pi_FakeT313', false, now(), NULL)",
        "INSERT INTO payments.charge_attempt VALUES ($1, 1, $4, 'succeeded', now(), now())",
        "INSERT INTO payments.charge_evidence VALUES ($1, $2, $3, $4, 1, $6, \
         'cus_FakeT313', 'pi_FakeT313', NULL, 2500, now(), now())",
        "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
         VALUES ($2, 'active', now(), now())",
        "INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, \
         cards_and_quotes, confirm_and_billing, added_at) VALUES ($2, $5, true, true, true, now())",
    ];
    for sql in statements {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(press)
            .bind(owner)
            .bind(&label)
            .execute(&mut *conn)
            .await
            .expect("seed");
    }
}

/// TD-FEE-55 (FEE-031, D-2287): through the worker with `messaging.charge_receipt` registered, a
/// receipt event stores one receipt email per money recipient, which reaches the fake provider
/// once; a repeat run stores nothing new.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_55_a_receipt_reaches_the_provider_once(pool: PgPool) {
    // `picks` queues the receipt email at the system clock's now, so the worker's clock is ahead.
    let now = Timestamp::now() + SignedDuration::from_mins(5);
    let clock = Arc::new(ManualClock::new(now));
    let registry = registry(Arc::clone(&clock) as Arc<dyn Clock>);
    let [pick, business, owner] = [0x313_0f01, 0x313_0f02, 0x313_0f03].map(Uuid::from_u128);
    charged(&pool, (pick, business, owner)).await;
    let mut db = pool.acquire().await.expect("conn");
    assert!(queue_receipt(&mut db, pick, now).await.expect("event"));
    drop(db);
    let mail = FakeMail::default();
    run_due(&pool, &registry, &clock).await;
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(RECEIPT_KIND)
        .fetch_one(&pool)
        .await
        .expect("events");
    assert_eq!(events, 1);
    assert_eq!(
        email_rows(&pool, "charge_receipt").await,
        1,
        "one per recipient"
    );
    deliver(&pool, &mail, now).await;
    let taken = mail.taken();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].0, address(owner));
    assert!(
        taken[0].1.starts_with("Ask Hava: Receipt for job "),
        "{}",
        taken[0].1
    );
    assert!(taken[0].2.contains("Tax included."));

    requeue(&pool, RECEIPT_EMAIL_KIND, now).await;
    run_due(&pool, &registry, &clock).await;
    deliver(&pool, &mail, now).await;
    assert_eq!(
        email_rows(&pool, "charge_receipt").await,
        1,
        "nothing new stored"
    );
    assert_eq!(mail.taken(), taken, "reached the provider once");
}

/// TD-CARD-15 (CARD-021): an `emergency: true` notice without its subject or first line is a
/// malformed notice: the worker stores nothing and the task fails with the fixed sentence, never
/// an EMERGENCY email under another subject.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_15_an_emergency_without_its_words_sends_nothing(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(night()));
    let registry = registry(Arc::clone(&clock) as Arc<dyn Clock>);
    let b = after_hours_business(&pool, 0xf0).await;
    let payload = serde_json::json!({
        "template": "job_card_notice", "business": b, "job": EMERGENCY_JOB,
        "to": [Uuid::from_u128(0xf0)], "texts": [], "emergency": true,
        "first_line": EMERGENCY_FIRST_LINE,
        "values": {"trade": "HVAC", "town": "Lake Havasu City", "job": 1042, "quote_hours": 24},
    });
    let mut db = pool.acquire().await.expect("conn");
    let task = NewTask::new(BUSINESS_NOTICE_KIND, payload, night());
    enqueue(&mut db, &task).await.expect("queue");
    drop(db);
    run_due(&pool, &registry, &clock).await;
    assert_eq!(email_rows(&pool, "job_card_notice").await, 0);
    let error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM core.task WHERE kind = $1")
            .bind(BUSINESS_NOTICE_KIND)
            .fetch_one(&pool)
            .await
            .expect("task");
    assert_eq!(error.as_deref(), Some("business notice is malformed"));
}

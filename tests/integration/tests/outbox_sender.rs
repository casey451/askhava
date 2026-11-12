//! The outbox sender through the worker (TD-QH-05; QH-001, QH-006, QH-008, TXT-011, TXT-014,
//! TXT-021, CON-009, CON-030; D-558, D-2188, D-2628).
//!
//! On fresh databases, the `messaging.send_due` task runs through `core`'s registry as the worker
//! runs it, as `askhava_app`, on a manual clock: a `caller_missed` row queued at 21:30 Arizona time
//! for a 928 number is not sent at 21:31 and is sent at 08:00, and a consent text queued at 14:00
//! goes at once, both through the Twilio stand-in (`python3 -m stand_ins.server`); a suppressed
//! number gets only the HELP reply; a row queued while the line was suspended is expired, not
//! sent, when it reopens; a CON-009 confirmation goes within 5 minutes or never; with a fake
//! carrier that does not dedupe, a worker killed after the carrier accepted leaves one delivery and
//! the row `unknown` after 5 minutes, two workers racing for one row make one carrier call, and a
//! timeout gives `unknown`, the email copy of an important text and one alert. All data is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, SentText, SmsAdapter, TwilioConfig, twilio_clients,
};
use askhava_core::api::{
    Clock, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, claim,
    enqueue as enqueue_task,
};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    Audience, MailboxKind, NewText, OutboxSender, PostalAddress, Relation, RevokeChannel,
    SEND_DUE_KIND, SEND_OUTCOME_UNKNOWN_ALERT, SendingNumber, enqueue, record_restore,
    record_suspension, register_send_due, schedule_send_due, suppress, switch_on,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;

/// Her fake 928 number, and the stand-in's one valid sending number.
const TO: &str = "+19285550142";
const FROM: &str = "+15005550006";
const CASEY: Uuid = Uuid::from_u128(0xca5e);

/// An instant given as Arizona wall time on July 15, 2026 (UTC-7, no daylight saving).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 7, day)
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

/// What a fake carrier does with a send.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Accept,
    DieAfterAccept,
    Timeout,
}

/// A carrier that does not dedupe by key (Twilio does not, D-558): every call is a delivery.
#[derive(Clone)]
struct Fake {
    calls: Arc<Mutex<Vec<String>>>,
    mode: Mode,
    delay: Duration,
}

fn fake(mode: Mode) -> Fake {
    Fake {
        calls: Arc::default(),
        mode,
        delay: Duration::ZERO,
    }
}

impl Fake {
    fn calls(&self) -> usize {
        self.calls.lock().expect("lock").len()
    }
}

impl SmsAdapter for Fake {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        tokio::time::sleep(self.delay).await;
        let n = {
            let mut calls = self.calls.lock().expect("lock");
            calls.push(body.to_owned());
            calls.len()
        };
        assert!(
            self.mode != Mode::DieAfterAccept,
            "worker killed after accept"
        );
        if self.mode == Mode::Timeout {
            return Err(AdapterError::OutcomeUnknown);
        }
        Ok(SentText {
            sid: format!("SM{n:032}"),
            status: "queued".to_owned(),
        })
    }
}

/// The worker's pieces: an `askhava_app` pool, the manual clock and the registry.
struct World {
    app: PgPool,
    clock: Arc<ManualClock>,
    registry: Arc<Registry>,
    her: Uuid,
}

async fn world<C: SmsAdapter + Send + Sync + 'static>(
    pool: &PgPool,
    carrier: C,
    at: Timestamp,
) -> World {
    let app = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let clock = Arc::new(ManualClock::new(at));
    let postal = PostalAddress::new(
        "PO Box 4417, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    );
    let number = SendingNumber::parse(FROM).ok();
    let worker_clock: Arc<dyn Clock> = clock.clone();
    let sender = OutboxSender::new(
        (secret(), vault()),
        carrier,
        (number, postal.ok()),
        worker_clock,
    )
    .expect("sender");
    let mut registry = Registry::new();
    register_send_due(&mut registry, Arc::new(sender));
    let mut db = app.acquire().await.expect("connection");
    let contact = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from("928-555-0142".to_owned())),
        email: Some(SecretString::from("fake.homeowner@example.com".to_owned())),
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &contact)
        .await
        .expect("contact");
    World {
        app,
        clock,
        registry: Arc::new(registry),
        her,
    }
}

/// Queues `template` for her about `job`, recorded as queued at `at`.
async fn queue(w: &World, template: &str, job: u128, at: Timestamp) -> Uuid {
    let text = NewText {
        template_id: template,
        subject: &[Uuid::from_u128(job)],
        recipient: w.her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: fake body",
    };
    let mut db = w.app.acquire().await.expect("connection");
    let (id, _) = enqueue(&mut db, &secret(), &text).await.expect("queued");
    sqlx::query("UPDATE messaging.outbox SET created_at = $2 WHERE id = $1")
        .bind(id)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("queued at");
    id
}

/// Runs every task due at the clock's now (queuing a run first), as the worker would.
async fn run_due(w: &World) {
    let mut db = w.app.acquire().await.expect("connection");
    schedule_send_due(&mut db, w.clock.now())
        .await
        .expect("schedule");
    drop(db);
    let owner = WorkerId::new(&w.app).await.expect("worker");
    let tasks = claim(
        &w.app,
        owner,
        w.clock.now(),
        SignedDuration::from_secs(60),
        50,
    )
    .await
    .expect("claim");
    for task in tasks {
        w.registry
            .run(&w.app, w.clock.as_ref(), owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

async fn status(w: &World, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM messaging.outbox WHERE id = $1")
        .bind(id)
        .fetch_one(&w.app)
        .await
        .expect("row")
}

async fn count(w: &World, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&w.app)
        .await
        .expect("count")
}

/// TD-QH-05 (QH-001, QH-006): through the Twilio stand-in, a night `caller_missed` row waits for
/// 08:00 and a consent text queued at 14:00 goes at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_night_row_waits_for_eight_and_a_day_consent_text_goes_at_once(pool: PgPool) {
    let stand_in = stand_in();
    let w = world(&pool, texting(stand_in.1), az(15, 14, 0)).await;
    let consent = queue(&w, "consent", 1, az(15, 14, 0)).await;
    run_due(&w).await;
    assert_eq!(
        status(&w, consent).await,
        "sent",
        "a day consent text goes at once"
    );
    let missed = queue(&w, "caller_missed", 2, az(15, 21, 30)).await;
    w.clock.set(az(15, 21, 31));
    run_due(&w).await;
    assert_eq!(status(&w, missed).await, "queued", "not sent at 21:31");
    let held: Timestamp = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT not_before FROM messaging.outbox WHERE id = $1",
    )
    .bind(missed)
    .fetch_one(&w.app)
    .await
    .expect("held")
    .to_jiff();
    assert_eq!(held, az(16, 8, 0), "held to the next window (QH-006)");
    w.clock.set(az(16, 7, 59));
    run_due(&w).await;
    assert_eq!(status(&w, missed).await, "queued");
    w.clock.set(az(16, 8, 0));
    run_due(&w).await;
    assert_eq!(status(&w, missed).await, "sent", "sent at 08:00");
}

/// TD-QH-05 (CON-013, CON-035): a suppressed number gets only the HELP reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_suppressed_number_gets_only_help(pool: PgPool) {
    let carrier = fake(Mode::Accept);
    let w = world(&pool, carrier.clone(), az(15, 10, 0)).await;
    let mut db = w.app.acquire().await.expect("connection");
    let listed = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), TO, listed, az(15, 9, 0))
        .await
        .expect("listed");
    drop(db);
    let help = queue(&w, "help_reply", 3, az(15, 9, 59)).await;
    let quote = queue(&w, "first_quote", 4, az(15, 9, 59)).await;
    run_due(&w).await;
    assert_eq!(status(&w, help).await, "sent");
    assert_eq!(
        status(&w, quote).await,
        "expired",
        "never sent to a listed number"
    );
    assert_eq!(
        carrier.calls(),
        1,
        "only the HELP reply reached the carrier"
    );
}

/// TD-QH-05 (TXT-021, D-2188): a row queued while the line was suspended is expired, with one audit
/// row, when the line reopens; a row queued after it reopened goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_row_queued_while_suspended_is_expired_when_the_line_reopens(pool: PgPool) {
    let carrier = fake(Mode::Accept);
    let w = world(&pool, carrier.clone(), az(15, 9, 0)).await;
    let mut db = w.app.acquire().await.expect("connection");
    record_suspension(
        &mut db,
        Audience::Homeowner,
        az(15, 9, 0),
        (CASEY, az(15, 9, 0)),
    )
    .await
    .expect("suspended");
    let stale = queue(&w, "first_quote", 5, az(15, 9, 30)).await;
    w.clock.set(az(15, 9, 31));
    run_due(&w).await;
    assert_eq!(carrier.calls(), 0, "nothing goes while suspended");
    record_restore(&mut db, Audience::Homeowner, (CASEY, az(15, 10, 0)))
        .await
        .expect("restored");
    switch_on(&mut db, Audience::Homeowner, (CASEY, az(15, 10, 1)))
        .await
        .expect("on");
    drop(db);
    let fresh = queue(&w, "first_quote", 6, az(15, 10, 2)).await;
    w.clock.set(az(15, 10, 3));
    run_due(&w).await;
    assert_eq!(status(&w, stale).await, "expired");
    assert_eq!(status(&w, fresh).await, "sent");
    assert_eq!(
        carrier.calls(),
        1,
        "the suspended row never reached the carrier"
    );
    let audits = count(
        &w,
        "SELECT count(*) FROM ops.audit_log WHERE action = 'messaging.text_expired'",
    )
    .await;
    assert_eq!(audits, 1);
}

/// TD-QH-05 (CON-009, CON-030): a confirmation goes within 5 minutes or never.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_confirmation_goes_within_five_minutes_or_never(pool: PgPool) {
    let carrier = fake(Mode::Accept);
    let w = world(&pool, carrier.clone(), az(15, 23, 0)).await;
    let late = queue(&w, "opt_out_confirmation", 7, az(15, 22, 54)).await;
    let fresh = queue(&w, "opt_out_confirmation", 8, az(15, 22, 57)).await;
    run_due(&w).await;
    assert_eq!(
        status(&w, fresh).await,
        "sent",
        "within 5 minutes, at any hour"
    );
    assert_eq!(status(&w, late).await, "expired", "never after 5 minutes");
    assert_eq!(carrier.calls(), 1);
}

/// TD-QH-05, TD-TXT-04 (TXT-011, D-558): a worker killed after the carrier accepted leaves one
/// delivery and, 5 minutes on, the row `unknown`; never a second call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_killed_worker_delivers_once_and_the_row_is_unknown_after_five_minutes(
    pool: PgPool,
) {
    let carrier = fake(Mode::DieAfterAccept);
    let w = world(&pool, carrier.clone(), az(15, 10, 0)).await;
    let id = queue(&w, "first_quote", 9, az(15, 10, 0)).await;
    let (registry, app, clock) = (Arc::clone(&w.registry), w.app.clone(), Arc::clone(&w.clock));
    let mut db = app.acquire().await.expect("connection");
    schedule_send_due(&mut db, clock.now())
        .await
        .expect("schedule");
    drop(db);
    let killed = tokio::spawn(async move {
        let owner = WorkerId::new(&app).await.expect("worker");
        let lease = SignedDuration::from_secs(60);
        for task in claim(&app, owner, clock.now(), lease, 50)
            .await
            .expect("claim")
        {
            let _ = registry
                .run(&app, clock.as_ref(), owner, task, Lease::WORKER)
                .await;
        }
    });
    assert!(killed.await.is_err(), "the worker dies mid-send");
    assert_eq!(status(&w, id).await, "sending");
    w.clock.set(az(15, 10, 4));
    run_due(&w).await;
    assert_eq!(
        status(&w, id).await,
        "sending",
        "not resent and not given up early"
    );
    w.clock.set(az(15, 10, 6));
    run_due(&w).await;
    assert_eq!(status(&w, id).await, "unknown");
    assert_eq!(carrier.calls(), 1, "one delivery, never two");
}

/// TD-QH-05 (TXT-011): two workers racing for one row make one carrier call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_two_workers_racing_for_one_row_make_one_carrier_call(pool: PgPool) {
    let carrier = Fake {
        delay: Duration::from_millis(300),
        ..fake(Mode::Accept)
    };
    let w = world(&pool, carrier.clone(), az(15, 10, 0)).await;
    let id = queue(&w, "first_quote", 10, az(15, 10, 0)).await;
    let mut db = w.app.acquire().await.expect("connection");
    for _ in 0..2 {
        let task = NewTask::new(SEND_DUE_KIND, json!({}), w.clock.now());
        enqueue_task(&mut db, &task).await.expect("task");
    }
    drop(db);
    let owner = WorkerId::new(&w.app).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let tasks = claim(&w.app, owner, w.clock.now(), lease, 50)
        .await
        .expect("claim");
    assert_eq!(tasks.len(), 2);
    let runs = tasks.into_iter().map(|task| {
        let (registry, app, clock) = (Arc::clone(&w.registry), w.app.clone(), Arc::clone(&w.clock));
        tokio::spawn(async move {
            registry
                .run(&app, clock.as_ref(), owner, task, Lease::WORKER)
                .await
                .expect("run");
        })
    });
    for run in runs.collect::<Vec<_>>() {
        run.await.expect("worker");
    }
    assert_eq!(carrier.calls(), 1, "one carrier call");
    assert_eq!(status(&w, id).await, "sent");
}

/// TD-QH-05, TD-TXT-04 (TXT-011, TXT-014): a timeout gives `unknown` with no body, the email copy
/// of an important text, its portal line and one alert; nothing is resent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_timeout_gives_unknown_the_email_copy_and_one_alert(pool: PgPool) {
    let carrier = fake(Mode::Timeout);
    let w = world(&pool, carrier.clone(), az(15, 10, 0)).await;
    let id = queue(&w, "caller_missed", 11, az(15, 10, 0)).await;
    run_due(&w).await;
    w.clock.set(az(15, 10, 10));
    run_due(&w).await;
    assert_eq!(status(&w, id).await, "unknown");
    assert_eq!(carrier.calls(), 1, "never resent blind");
    let body = count(
        &w,
        "SELECT count(*) FROM messaging.outbox WHERE body IS NOT NULL",
    )
    .await;
    assert_eq!(body, 0, "the unknown row keeps no body");
    let emails = count(
        &w,
        "SELECT count(*) FROM messaging.email_outbox WHERE template_id = 'text_failed'",
    )
    .await;
    assert_eq!(emails, 1, "the important text's email copy");
    let portal = count(&w, "SELECT count(*) FROM messaging.portal_notice").await;
    assert_eq!(portal, 1);
    let alerts: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, severity FROM ops.alert ORDER BY kind")
            .fetch_all(&w.app)
            .await
            .expect("alerts");
    assert_eq!(
        alerts,
        vec![(SEND_OUTCOME_UNKNOWN_ALERT.to_owned(), "normal".to_owned())]
    );
}

/// TD-QH-05 (TXT-014, QH-008): a definite refusal puts the row back to `queued` for its retry
/// 10 minutes on, and the retry is sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_05_a_refused_send_is_retried_later(pool: PgPool) {
    let stand_in = stand_in();
    let w = world(&pool, texting(stand_in.1), az(15, 10, 0)).await;
    let mut db = w.app.acquire().await.expect("connection");
    let refused = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(12)],
        recipient: w.her,
        relation: Relation::OpenRequest,
        body: &"x".repeat(1601),
    };
    let (id, _) = enqueue(&mut db, &secret(), &refused).await.expect("queued");
    drop(db);
    run_due(&w).await;
    assert_eq!(
        status(&w, id).await,
        "queued",
        "a 4xx refusal is queued again"
    );
    let retries = count(
        &w,
        "SELECT count(*) FROM messaging.text_retry WHERE next_at IS NOT NULL",
    )
    .await;
    assert_eq!(retries, 1);
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

/// The real Twilio texting client pointed at the stand-in.
fn texting(port: u16) -> askhava_adapters::api::TwilioSms {
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
    sms
}

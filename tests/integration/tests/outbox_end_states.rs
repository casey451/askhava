//! Outbox end states and carrier rate limits (D-4052, D-4327; TXT-011, TXT-013, TXT-014, INT-041,
//! SEC-028).
//!
//! On fresh databases: a `blocked`, `failed` or `expired` row reads `Failed` and an `unknown` one
//! `Sent`; a watching card timer on a failed card text stops and queues no new timer; through the
//! Twilio stand-in behind a carrier that first answers a set number of rate limits (the stand-in
//! has no 429 of its own), a 429 holds the row with no counted failure and the next try sends it,
//! three in a row back off 1, 2 and 4 minutes and the text still goes once, and a 400 is still a
//! counted failure; an important text given up after its retries queues an email copy whose
//! manage link is the absolute portal link in both parts. All data is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, SentText, SmsAdapter, TwilioConfig, TwilioSms, twilio_clients,
};
use askhava_core::api::{Clock, Lease, ManualClock, Registry, Settings, WorkerId, brand, claim};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, CARD_TIMER_KIND, JobStart, Opened, Preview, Shown, TextIn, begin_job,
    card_timer_step, receive_text, show_card,
};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    DeliveryStatus, MailboxKind, NewText, OutboxSender, PostalAddress, Relation, SendingNumber,
    delivery_status, enqueue, register_send_due, schedule_send_due,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{CardParts, DEFAULT_MIN_WORDS, Place, Timing, build_card};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;

const FROM: &str = "+15005550006";

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

/// The stand-in Twilio client behind `limits` scripted rate-limit answers, counting real sends.
#[derive(Clone)]
struct Limited {
    sms: Arc<TwilioSms>,
    limits: Arc<Mutex<Vec<AdapterError>>>,
    sent: Arc<Mutex<usize>>,
}

impl SmsAdapter for Limited {
    async fn send_text(
        &self,
        key: &IdempotencyKey,
        to: &str,
        from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        let next = self.limits.lock().expect("lock").pop();
        if let Some(limit) = next {
            return Err(limit);
        }
        let sent = self.sms.send_text(key, to, from, body).await?;
        *self.sent.lock().expect("lock") += 1;
        Ok(sent)
    }
}

fn limited(port: u16, mut limits: Vec<AdapterError>) -> Limited {
    limits.reverse();
    Limited {
        sms: Arc::new(texting(port)),
        limits: Arc::new(Mutex::new(limits)),
        sent: Arc::default(),
    }
}

struct World {
    app: PgPool,
    clock: Arc<ManualClock>,
    registry: Registry,
    her: Uuid,
}

async fn world(pool: &PgPool, carrier: Limited, at: Timestamp) -> World {
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
    let worker_clock: Arc<dyn Clock> = clock.clone();
    let numbers = (SendingNumber::parse(FROM).ok(), postal.ok());
    let sender = OutboxSender::new((secret(), vault()), carrier, numbers, worker_clock);
    let mut registry = Registry::new();
    register_send_due(&mut registry, Arc::new(sender.expect("sender")));
    let contact = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from("928-555-0142".to_owned())),
        email: Some(SecretString::from("fake.homeowner@example.com".to_owned())),
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

/// Queues an important `first_quote` text for her about `job`, with `body`, at the clock's now.
async fn queue(w: &World, job: u128, body: &str) -> Uuid {
    let text = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(job)],
        recipient: w.her,
        relation: Relation::OpenRequest,
        body,
    };
    let mut db = w.app.acquire().await.expect("connection");
    let id = enqueue(&mut db, &secret(), &text).await.expect("queued").0;
    sqlx::query("UPDATE messaging.outbox SET created_at = $2 WHERE id = $1")
        .bind(id)
        .bind(w.clock.now().to_sqlx())
        .execute(&mut *db)
        .await
        .expect("queued at the clock's now");
    id
}

/// Runs every task due at `at`, as the worker would.
async fn run_at(w: &World, at: Timestamp) {
    w.clock.set(at);
    let mut db = w.app.acquire().await.expect("connection");
    schedule_send_due(&mut db, at).await.expect("schedule");
    drop(db);
    let owner = WorkerId::new(&w.app).await.expect("worker");
    let tasks = claim(&w.app, owner, at, SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    for task in tasks {
        let run = w
            .registry
            .run(&w.app, w.clock.as_ref(), owner, task, Lease::WORKER);
        run.await.expect("run");
    }
}

/// The row's (status, not_before, counted failures).
async fn row(w: &World, id: Uuid) -> (String, Option<Timestamp>, i64) {
    let (status, held, failures): (String, Option<jiff_sqlx::Timestamp>, i64) = sqlx::query_as(
        "SELECT status, not_before, (SELECT COALESCE(sum(failures), 0)::bigint FROM \
         messaging.text_retry WHERE outbox_id = $1) FROM messaging.outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&w.app)
    .await
    .expect("row");
    (status, held.map(|t| t.to_jiff()), failures)
}

fn rate_limit(status: u16, code: Option<i64>) -> AdapterError {
    AdapterError::Vendor { status, code }
}

/// TXT-013, INT-041 (D-4052 B4): every end state reads as failed, and `unknown` as sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn blocked_failed_and_expired_read_failed_and_unknown_reads_sent(pool: PgPool) {
    let stand_in = stand_in();
    let w = world(&pool, limited(stand_in.1, Vec::new()), az(10, 0)).await;
    let cases = [
        ("blocked", DeliveryStatus::Failed),
        ("failed", DeliveryStatus::Failed),
        ("expired", DeliveryStatus::Failed),
        ("unknown", DeliveryStatus::Sent),
    ];
    for (n, (status, want)) in cases.into_iter().enumerate() {
        let id = queue(&w, 100 + n as u128, "fake body").await;
        sqlx::query("UPDATE messaging.outbox SET status = $2, body = NULL WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .expect("end state");
        let mut db = w.app.acquire().await.expect("connection");
        assert_eq!(delivery_status(&mut db, id).await, Ok(want), "{status}");
    }
}

/// TXT-011, TXT-014 (D-4052 B20): a 429 holds the row a minute with no counted failure, and the
/// next try sends it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_429_holds_the_row_uncounted_and_the_next_try_sends(pool: PgPool) {
    let stand_in = stand_in();
    let carrier = limited(stand_in.1, vec![rate_limit(429, None)]);
    let w = world(&pool, carrier.clone(), az(10, 0)).await;
    let id = queue(&w, 1, "fake body").await;
    run_at(&w, az(10, 0)).await;
    assert_eq!(row(&w, id).await, ("queued".to_owned(), Some(az(10, 1)), 0));
    run_at(&w, az(10, 1)).await;
    assert_eq!(row(&w, id).await.0, "sent");
    assert_eq!(*carrier.sent.lock().expect("lock"), 1);
}

/// D-4052 B20: three rate limits in a row (429, and Twilio 20429 on a 429 and on a 503) back off 1,
/// 2 and 4 minutes, count nothing, and the text still goes once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn three_429s_then_a_send_send_once(pool: PgPool) {
    let stand_in = stand_in();
    let limits = vec![
        rate_limit(429, None),
        rate_limit(429, Some(20429)),
        rate_limit(503, Some(20429)),
    ];
    let carrier = limited(stand_in.1, limits);
    let w = world(&pool, carrier.clone(), az(10, 0)).await;
    let id = queue(&w, 2, "fake body").await;
    for (at, until) in [
        (az(10, 0), az(10, 1)),
        (az(10, 1), az(10, 3)),
        (az(10, 3), az(10, 7)),
    ] {
        run_at(&w, at).await;
        assert_eq!(row(&w, id).await, ("queued".to_owned(), Some(until), 0));
    }
    run_at(&w, az(10, 7)).await;
    assert_eq!(row(&w, id).await.0, "sent");
    assert_eq!(*carrier.sent.lock().expect("lock"), 1);
}

/// TXT-014, SEC-028 (D-4327): a 400 from the stand-in is still a counted failure, and once the
/// retries are spent the email copy's manage link is the absolute portal link in both parts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_400_counts_and_the_given_up_copy_links_the_portal_absolutely(pool: PgPool) {
    let stand_in = stand_in();
    let w = world(&pool, limited(stand_in.1, Vec::new()), az(10, 0)).await;
    let id = queue(&w, 3, &"x".repeat(1601)).await;
    run_at(&w, az(10, 0)).await;
    assert_eq!(
        row(&w, id).await,
        ("queued".to_owned(), Some(az(10, 10)), 1)
    );
    for minute in [10, 20, 30, 40] {
        run_at(&w, az(10, minute)).await;
    }
    let (html, plain): (String, String) =
        sqlx::query_as("SELECT html, plain FROM messaging.email_outbox WHERE recipient_id = $1")
            .bind(w.her)
            .fetch_one(&pool)
            .await
            .expect("the email copy");
    let link = format!("https://{}/h", brand().domain());
    assert!(html.contains(&format!("href=\"{link}\"")) && !html.contains("href=\"/h\""));
    assert!(plain.contains(&link), "{plain}");
}

/// INT-041, D-1278 (D-4052 B4): a watching card timer whose card text failed stops: no new timer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_card_timer_on_a_failed_text_stops(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let at = |m: i64| Timestamp::from_second(1_793_000_000 + m * 60).expect("time");
    let inbound = Inbound {
        words: "my kitchen sink is leaking under the cabinet",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x5_8301),
        contact: Uuid::from_u128(0x8301),
        inbound,
        received_at: at(0),
    };
    let handled = receive_text(&mut db, &secret(), &msg, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: msg.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let parts = CardParts {
        category: "Plumbing".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "The kitchen sink drips under the cabinet.".to_owned(),
        answers: Vec::new(),
        timing: Some(Timing::NoRush),
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let link = format!("{}/r/fake-token", brand().domain());
    let p = Preview {
        conversation,
        job,
        card: &card,
        link: &link,
        wait: AUTO_SEND_AFTER,
        at: at(2),
    };
    let Shown::Texted(sent) = show_card(&mut db, &secret(), &p).await.expect("show") else {
        panic!("texted");
    };
    sqlx::query("UPDATE messaging.outbox SET status = 'failed', body = NULL WHERE id = $1")
        .bind(sent.outbox_id)
        .execute(&mut *db)
        .await
        .expect("failed");
    let (payload, before) = timers(&pool, job).await;
    assert_eq!(payload["watch"], Value::Bool(true));
    let timer = askhava_intake::api::CardTimer {
        job,
        conversation,
        card: payload["card"].as_str().expect("card").to_owned(),
        show: Timestamp::from_microsecond(payload["show"].as_i64().expect("show")).expect("t"),
        outbox: sent.outbox_id,
        wait: AUTO_SEND_AFTER,
        due: at(2),
        watch: true,
    };
    assert!(!card_timer_step(&mut db, &timer, at(2)).await.expect("step"));
    assert_eq!(timers(&pool, job).await.1, before, "no new timer");
}

/// The job's latest card timer payload and how many card timers it has.
async fn timers(pool: &PgPool, job: Uuid) -> (Value, i64) {
    sqlx::query_as(
        "SELECT (SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         ORDER BY run_at DESC LIMIT 1), (SELECT count(*) FROM core.task WHERE kind = $1 \
         AND payload->>'job' = $2)",
    )
    .bind(CARD_TIMER_KIND)
    .bind(job.to_string())
    .fetch_one(pool)
    .await
    .expect("timers")
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
fn texting(port: u16) -> TwilioSms {
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
    twilio_clients(&config).expect("clients").0
}

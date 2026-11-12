//! TD-CALL-23 (CALL-011, CALL-003; D-078, D-1696, D-2247, D-3079, D-3167 to D-3169): a blocked
//! call (an unlinked phone made to enter the PIN) that connects line L queues one LINK notice for L
//! and one pending link holding no number, last 4 digits or PIN, live for 24 hours; a wrong-PIN
//! hang-up, a homeowner, a linked business phone and a second unlinked phone on L store nothing;
//! a failed link step still answers the dial. Signed fake Twilio requests through the voice route
//! (`app/server/src/inbound_voice.rs`, compiled in with `#[path]`, as `inbound_text_route.rs`
//! does, since a test crate may not depend on `server`). Fake numbers, names and CallSids only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::create;
use askhava_calls::api::{Connected, LINK_REQUEST_LIFE, PinWait, assign_number, stored_pin};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::sender_add;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::Webhooks;
use inbound_voice::{VOICE_PATH, VoiceHooks, routes};
use jiff::Timestamp;
use jiff_sqlx::Timestamp as SqlTime;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgPool};
use tower::ServiceExt as _;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// The shared Twilio webhook state the voice route builds on, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
#[allow(
    dead_code,
    reason = "only `Webhooks` is used; the text routes are not served here"
)]
mod inbound_text;
/// The voice route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_voice.rs"]
mod inbound_voice;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const HER: &str = "+19285550191";
const SHOP: &str = "+19285550192";
const BLOCKED: &str = "+19285550187";
const OTHER: &str = "+19285550164";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", NUMBER),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

fn hooks(db: &PgPool) -> Arc<VoiceHooks> {
    let text = Webhooks {
        pool: db.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    };
    let (pool, wait) = (pool(), PinWait::default());
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool,
        wait,
    })
}

/// Her contact, a business with `SHOP` linked, and one line between them; the line and its PIN.
async fn setup(db: &PgPool) -> (Uuid, String) {
    let mut db = db.acquire().await.expect("connection");
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let business = create(&mut db, false, at()).await.expect("business");
    sender_add(&mut db, &vault(), (business, Uuid::new_v4()), SHOP)
        .await
        .expect("link");
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: her,
        business,
    };
    let line = assign_number(&mut db, &pool(), job, at())
        .await
        .expect("line")
        .id();
    let pin = stored_pin(&mut db, line).await.expect("read").expect("pin");
    (line, pin.digits())
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// One signed turn of call `n` from `from`: `query` names the turn, `digits` what was keyed.
async fn turn(
    v: &Arc<VoiceHooks>,
    (n, from): (u32, &str),
    query: &str,
    digits: Option<&str>,
) -> String {
    let sid = format!("CA{n:032}");
    let mut fields = vec![("CallSid", sid.as_str()), ("From", from), ("To", NUMBER)];
    fields.extend(digits.map(|d| ("Digits", d)));
    let path = format!("{VOICE_PATH}{query}");
    let mut sorted = fields.clone();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(v))
        .oneshot(request)
        .await
        .expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

/// A blocked call `n` from `from` entering `pin` at the first try; the TwiML of that try.
async fn blocked_call(v: &Arc<VoiceHooks>, (n, from): (u32, &str), pin: &str) -> String {
    let first = turn(v, (n, from), "", None).await;
    assert!(
        first.contains("<Gather"),
        "an unlinked phone hears the PIN prompt"
    );
    turn(v, (n, from), "?try=1", Some(pin)).await
}

async fn count(db: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

async fn notices(db: &PgPool) -> i64 {
    count(
        db,
        "SELECT count(*) FROM core.task WHERE payload->>'template' = 'link_this_phone'",
    )
    .await
}

async fn pending(db: &PgPool) -> i64 {
    count(db, "SELECT count(*) FROM calls.link_request").await
}

/// How many rows of `table` (whole row as text) hold `needle`.
async fn rows_holding(db: &PgPool, table: &'static str, needle: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table} t WHERE t::text LIKE '%' || $1 || '%'");
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(needle)
        .fetch_one(db)
        .await
        .expect("scan")
}

/// Every app (non-sqlx) log line's message and fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<(Level, String)>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Lines {
    fn push(&self, level: Level, line: String) {
        self.0.lock().expect("lines").push((level, line));
    }
    fn warnings(&self) -> usize {
        self.0
            .lock()
            .expect("lines")
            .iter()
            .filter(|(l, _)| *l <= Level::WARN)
            .count()
    }
    fn holding(&self, needle: &str) -> usize {
        self.0
            .lock()
            .expect("lines")
            .iter()
            .filter(|(_, s)| s.contains(needle))
            .count()
    }
}

impl Subscriber for Lines {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        !meta.target().starts_with("sqlx")
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.push(*span.metadata().level(), line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.push(Level::TRACE, line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.push(*event.metadata().level(), line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// TD-CALL-23 (CALL-011, CALL-003, PRIV-002): an unlinked phone entering L's stored PIN is dialed
/// and queues one LINK notice for L and one pending link for L, expiring 24 hours after the
/// request, whose rows hold no number, last 4 digits or PIN; no log line, task payload other than
/// the notice or audit row holds the number, its last 4 digits or the PIN.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_23_a_blocked_call_that_connects_queues_one_notice_and_one_pending_link(
    db: PgPool,
) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (line, pin) = setup(&db).await;
    let v = hooks(&db);
    assert!(blocked_call(&v, (1, BLOCKED), &pin).await.contains("<Dial"));
    let named: (String, String) = sqlx::query_as(
        "SELECT payload->>'line', payload->'values'->>'job' FROM core.task \
         WHERE payload->>'template' = 'link_this_phone'",
    )
    .fetch_one(&db)
    .await
    .expect("one notice");
    assert_eq!(named, (line.to_string(), pin.clone()));
    let (on, requested, expires): (Uuid, SqlTime, SqlTime) =
        sqlx::query_as("SELECT line_id, requested_at, expires_at FROM calls.link_request")
            .fetch_one(&db)
            .await
            .expect("one pending link");
    assert_eq!((on, requested.to_jiff()), (line, at()));
    let day_later = at().checked_add(LINK_REQUEST_LIFE).expect("expiry");
    assert_eq!(expires.to_jiff(), day_later);
    assert_eq!(
        count(&db, "SELECT count(*) FROM calls.link_request_digest").await,
        1
    );
    for needle in [BLOCKED, &BLOCKED[BLOCKED.len() - 4..], pin.as_str()] {
        for table in [
            "calls.link_request",
            "calls.link_request_digest",
            "ops.audit_log",
        ] {
            assert_eq!(
                rows_holding(&db, table, needle).await,
                0,
                "{table} holds a value"
            );
        }
        let others = "(SELECT * FROM core.task WHERE payload->>'template' IS DISTINCT FROM 'link_this_phone')";
        assert_eq!(
            rows_holding(&db, others, needle).await,
            0,
            "a task payload holds a value"
        );
        assert_eq!(lines.holding(needle), 0, "a log line holds a value");
    }
    assert_eq!(
        rows_holding(&db, "core.task", BLOCKED).await,
        0,
        "the notice holds the number"
    );
}

/// TD-CALL-23 (CALL-003): three wrong PINs hang up; nothing is queued or stored until a later call
/// from the same phone connects the line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_23_a_wrong_pin_hang_up_queues_and_stores_nothing(db: PgPool) {
    let (_, pin) = setup(&db).await;
    let wrong = if pin == "0000" { "1111" } else { "0000" };
    let v = hooks(&db);
    assert!(
        blocked_call(&v, (2, BLOCKED), wrong)
            .await
            .contains("<Gather")
    );
    turn(&v, (2, BLOCKED), "?try=2", Some(wrong)).await;
    assert!(
        turn(&v, (2, BLOCKED), "?try=3", Some(wrong))
            .await
            .contains("<Hangup")
    );
    assert_eq!((notices(&db).await, pending(&db).await), (0, 0));
    assert!(blocked_call(&v, (8, BLOCKED), &pin).await.contains("<Dial"));
    assert_eq!(
        (notices(&db).await, pending(&db).await),
        (1, 1),
        "a later connect stores one"
    );
}

/// TD-CALL-23 (CALL-011): the business's linked phone is dialed on its one line without a PIN and
/// the homeowner's own call is answered without one (D-2093); neither queues a notice nor stores a
/// pending link, while a blocked call after them does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_23_a_homeowner_or_linked_business_phone_stores_nothing(db: PgPool) {
    let (_, pin) = setup(&db).await;
    let v = hooks(&db);
    assert!(!turn(&v, (3, HER), "", None).await.contains("<Gather"));
    assert!(turn(&v, (4, SHOP), "", None).await.contains("<Dial"));
    assert_eq!((notices(&db).await, pending(&db).await), (0, 0));
    assert!(blocked_call(&v, (9, BLOCKED), &pin).await.contains("<Dial"));
    assert_eq!(
        (notices(&db).await, pending(&db).await),
        (1, 1),
        "a blocked call stores one"
    );
}

/// TD-CALL-23 (CALL-011, D-1696): a second unlinked phone connecting L is dialed but queues no
/// second notice and stores no second pending link, and the step logs no failure.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_23_a_second_unlinked_phone_on_the_line_stores_nothing(db: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (_, pin) = setup(&db).await;
    let v = hooks(&db);
    assert!(blocked_call(&v, (5, BLOCKED), &pin).await.contains("<Dial"));
    assert!(blocked_call(&v, (6, OTHER), &pin).await.contains("<Dial"));
    assert_eq!((notices(&db).await, pending(&db).await), (1, 1));
    assert_eq!(
        lines.warnings(),
        0,
        "the repeat is refused by the once-per-line rule"
    );
}

/// TD-CALL-23 (CALL-011): a forced failure of the link step still answers the dial, rolls back
/// the notice with the pending link, and logs one line naming no number, line or PIN.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_23_a_failed_link_step_still_answers_the_dial(db: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (line, pin) = setup(&db).await;
    sqlx::query("ALTER TABLE calls.link_request_digest RENAME TO link_request_digest_gone")
        .execute(&db)
        .await
        .expect("a planted failure");
    let v = hooks(&db);
    assert!(blocked_call(&v, (7, BLOCKED), &pin).await.contains("<Dial"));
    assert_eq!((notices(&db).await, pending(&db).await), (0, 0));
    assert_eq!(lines.warnings(), 1);
    for needle in [
        BLOCKED,
        &BLOCKED[BLOCKED.len() - 4..],
        pin.as_str(),
        &line.to_string(),
    ] {
        assert_eq!(lines.holding(needle), 0, "a log line holds a value");
    }
}

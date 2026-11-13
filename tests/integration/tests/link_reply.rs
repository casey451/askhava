//! TD-CALL-24 (CALL-011, CALL-003, CALL-012, CALL-025; D-311, D-1696, D-3079, T391): the business
//! main phone's "LINK <PIN>" reply to the LINK notice links the blocked phone as a staff phone, so
//! its next call connects without the PIN prompt; every other LINK fails closed with no staff
//! phone, no sender-directory row, no audit row and no auto-reply, and at most 3 refused LINK
//! replies per phone per Arizona day are compared. Signed fake Twilio voice and text requests
//! through the served routes (`app/server/src/inbound_voice.rs` and `inbound_text.rs`, compiled
//! in with `#[path]`, as `link_request.rs` does), each pending link made by a blocked call through
//! the voice route (T390). Fake numbers, CallSids and MessageSids only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{
    AUTO_REPLY_TEMPLATE, Acting, SenderLink, TeamError, add_owner, create, link_phone,
};
use askhava_calls::api::{Connected, LINK_REQUEST_LIFE, PinWait, assign_number, stored_pin};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::{OutboxError, sender_add};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks};
use inbound_voice::{VOICE_PATH, VoiceHooks};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};
use tower::ServiceExt as _;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// The inbound text route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
#[allow(
    dead_code,
    reason = "only the inbound route and `Webhooks` are used; the retry items are the worker's"
)]
mod inbound_text;
/// The voice route that makes each pending link, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_voice.rs"]
mod inbound_voice;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const ASK_HAVA: &str = "+15005550006";
const HER: &str = "+19285550191";
const SHOP: &str = "+19285550192";
const STAFF: &str = "+19285550193";
const RIVAL: &str = "+19285550194";
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

/// The served routes and the clock both read.
struct Served {
    voice: Arc<VoiceHooks>,
    clock: Arc<ManualClock>,
}

fn served(db: &PgPool) -> Served {
    let clock = Arc::new(ManualClock::new(at()));
    let text = Webhooks {
        pool: db.clone(),
        clock: clock.clone(),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    };
    let voice = Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool: pool(),
        wait: PinWait::default(),
    });
    Served { voice, clock }
}

struct Directory;

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &vault(), business_line, number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::SenderTaken => TeamError::PhoneTaken,
                _ => TeamError::Database,
            })
    }
}

/// A business whose owner linked `main` as its main phone, with one line to her; the business,
/// the owner and the line's PIN.
async fn business(db: &PgPool, her: Uuid, main: &str) -> (Uuid, Uuid, String) {
    let mut db = db.acquire().await.expect("connection");
    let business = create(&mut db, false, at()).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::new_v4(), at())
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let phone = SecretString::from(main.to_owned());
    link_phone(&mut db, &Directory, (by, true), &phone, at())
        .await
        .expect("main phone");
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
    (business, owner, pin.digits())
}

/// Her contact and one business with `SHOP` as its main phone; the business, owner and PIN.
async fn setup(db: &PgPool) -> (Uuid, Uuid, String) {
    let mut conn = db.acquire().await.expect("connection");
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let her = store_contact(&mut conn, &vault(), &new)
        .await
        .expect("contact");
    business(db, her, SHOP).await
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `fields` signed for `path` as Twilio signs a webhook.
fn signed(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let mut sorted = fields.to_vec();
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
    Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request")
}

/// One signed voice turn of call `n` from `from`; the TwiML.
async fn turn(s: &Served, (n, from): (u32, &str), query: &str, digits: Option<&str>) -> String {
    let sid = format!("CA{n:032}");
    let mut fields = vec![("CallSid", sid.as_str()), ("From", from), ("To", NUMBER)];
    fields.extend(digits.map(|d| ("Digits", d)));
    let request = signed(&format!("{VOICE_PATH}{query}"), &fields);
    let answer = inbound_voice::routes(Arc::clone(&s.voice))
        .oneshot(request)
        .await
        .expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

/// A blocked call `n` from `from` entering `pin`, which must connect, leaving a pending link.
async fn blocked_call(s: &Served, (n, from): (u32, &str), pin: &str) {
    s.clock.set(at());
    assert!(turn(s, (n, from), "", None).await.contains("<Gather"));
    assert!(
        turn(s, (n, from), "?try=1", Some(pin))
            .await
            .contains("<Dial")
    );
}

/// Text `n` from `from` saying `words`, delivered at `now`.
async fn text(s: &Served, (n, from): (u32, &str), words: &str, now: Timestamp) {
    s.clock.set(now);
    let sid = format!("SM{n:032}");
    let fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    let answer = inbound_text::routes(Arc::clone(&s.voice.hooks))
        .oneshot(signed(INBOUND_PATH, &fields))
        .await
        .expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
}

/// Staff phones (not main), sender-directory rows, `phone_linked` audit rows and auto-replies.
async fn state(db: &PgPool) -> [i64; 4] {
    let count = |sql: &'static str| sqlx::query_scalar(sql).fetch_one(db);
    let replies =
        sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
            .bind(AUTO_REPLY_TEMPLATE)
            .fetch_one(db);
    [
        count("SELECT count(*) FROM businesses.staff_phone WHERE NOT is_main")
            .await
            .expect("staff"),
        count("SELECT count(*) FROM messaging.sender_directory")
            .await
            .expect("directory"),
        count("SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.phone_linked'")
            .await
            .expect("audit"),
        replies.await.expect("replies"),
    ]
}

fn wrong(pin: &str) -> &'static str {
    if pin == "0000" { "1111" } else { "0000" }
}

/// A refused LINK leaves everything as it was.
async fn refused(s: &Served, db: &PgPool, (n, from): (u32, &str), words: &str, now: Timestamp) {
    let before = state(db).await;
    text(s, (n, from), words, now).await;
    assert_eq!(state(db).await, before, "{words:?} changed something");
    let counted: i64 = sqlx::query_scalar("SELECT count(*) FROM calls.link_reply_refusal")
        .fetch_one(db)
        .await
        .expect("refusals");
    assert!(counted > 0, "the LINK was handled and refused");
}

/// Links the blocked phone with `  lInK <PIN> ` from the main phone `offset` seconds after the
/// pending link was made, and checks the outcome (CALL-011, CALL-012, D-3079).
async fn links_after(db: &PgPool, offset: i64) {
    let (business, owner, pin) = setup(db).await;
    let s = served(db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let before = state(db).await;
    let now = at() + SignedDuration::from_secs(offset);
    text(&s, (1, SHOP), &format!("  lInK {pin} "), now).await;
    let after = state(db).await;
    let added: Vec<i64> = after.iter().zip(before).map(|(a, b)| a - b).collect();
    assert_eq!(
        added,
        [1, 1, 1, 0],
        "one staff phone, directory row and audit row, no auto-reply"
    );
    let linked: (Uuid, Uuid) = sqlx::query_as(
        "SELECT business_id, linked_by FROM businesses.staff_phone WHERE NOT is_main",
    )
    .fetch_one(db)
    .await
    .expect("staff phone");
    assert_eq!(linked, (business, owner), "linked as by the owner");
    let answer = turn(&s, (2, BLOCKED), "", None).await;
    assert!(answer.contains("<Dial") && !answer.contains("<Gather"));
}

/// TD-CALL-24 (CALL-011, CALL-012, D-3079): `link <PIN>` from the main phone, any case with spaces
/// around, adds one staff phone linked by the owner with one audit row and no auto-reply, and the
/// blocked phone's next call is dialed with no PIN prompt.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_the_main_phones_link_reply_links_the_blocked_phone(db: PgPool) {
    links_after(&db, 60).await;
}

/// TD-CALL-24 (CALL-011): the same holds one second before the pending link's expiry time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_one_second_before_the_expiry_still_links(db: PgPool) {
    links_after(&db, LINK_REQUEST_LIFE.as_secs() - 1).await;
}

/// TD-CALL-24 (CALL-003, CALL-025): a wrong PIN, then three refused LINKs make the right PIN as
/// the fourth of the Arizona day refused too; the next Arizona day it links.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_a_wrong_pin_and_the_daily_cap_refuse(db: PgPool) {
    let (_, _, pin) = setup(&db).await;
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let now = at() + SignedDuration::from_secs(60);
    for n in 1..=3 {
        refused(&s, &db, (n, SHOP), &format!("LINK {}", wrong(&pin)), now).await;
    }
    refused(&s, &db, (4, SHOP), &format!("LINK {pin}"), now).await;
    let next_day = at() + SignedDuration::from_hours(15);
    text(&s, (5, SHOP), &format!("LINK {pin}"), next_day).await;
    assert_eq!(state(&db).await[0], 1, "the next Arizona day links");
}

/// TD-CALL-24 (CALL-012): the right PIN from a staff phone of the business that is not its main
/// phone is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_a_non_main_phone_is_refused(db: PgPool) {
    let (business, owner, pin) = setup(&db).await;
    let by = Acting {
        business,
        member: owner,
    };
    let mut conn = db.acquire().await.expect("connection");
    link_phone(
        &mut conn,
        &Directory,
        (by, false),
        &SecretString::from(STAFF),
        at(),
    )
    .await
    .expect("staff phone");
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let now = at() + SignedDuration::from_secs(60);
    refused(&s, &db, (1, STAFF), &format!("LINK {pin}"), now).await;
}

/// TD-CALL-24 (CALL-011): the PIN of another business's line, which has its own pending link, is
/// refused; the lookup is by the replying business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_another_business_lines_pin_is_refused(db: PgPool) {
    let (_, _, pin) = setup(&db).await;
    let her: Uuid = sqlx::query_scalar("SELECT homeowner_id FROM calls.number_pool")
        .fetch_one(&db)
        .await
        .expect("her");
    let (_, _, rival_pin) = business(&db, her, RIVAL).await;
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    blocked_call(&s, (2, OTHER), &rival_pin).await;
    let now = at() + SignedDuration::from_secs(60);
    refused(&s, &db, (1, SHOP), &format!("LINK {rival_pin}"), now).await;
}

/// TD-CALL-24 (CALL-011): a pending link exactly 24 hours old and one on a closed line are
/// refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_an_expired_link_or_a_closed_line_is_refused(db: PgPool) {
    let (_, _, pin) = setup(&db).await;
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let expiry = at() + LINK_REQUEST_LIFE;
    refused(&s, &db, (1, SHOP), &format!("LINK {pin}"), expiry).await;
    let (_, _, pin) = setup_again(&db).await;
    blocked_call(&s, (2, OTHER), &pin).await;
    sqlx::query("UPDATE calls.number_pool SET closed_at = $1")
        .bind((at() + SignedDuration::from_secs(30)).to_sqlx())
        .execute(&db)
        .await
        .expect("close");
    let now = at() + SignedDuration::from_secs(60);
    refused(&s, &db, (2, RIVAL), &format!("LINK {pin}"), now).await;
}

/// A second business (main phone `RIVAL`) with a line to the same homeowner.
async fn setup_again(db: &PgPool) -> (Uuid, Uuid, String) {
    let her: Uuid = sqlx::query_scalar("SELECT homeowner_id FROM calls.number_pool LIMIT 1")
        .fetch_one(db)
        .await
        .expect("her");
    business(db, her, RIVAL).await
}

/// TD-CALL-24 (CALL-011): the same LINK sent again after it linked adds nothing: one staff phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_a_replayed_link_adds_nothing(db: PgPool) {
    let (_, _, pin) = setup(&db).await;
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let now = at() + SignedDuration::from_secs(60);
    text(&s, (1, SHOP), &format!("LINK {pin}"), now).await;
    assert_eq!(state(&db).await[0], 1);
    refused(&s, &db, (2, SHOP), &format!("LINK {pin}"), now).await;
    assert_eq!(state(&db).await[0], 1, "exactly one staff phone");
}

/// Every app (non-sqlx) log line, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Lines {
    fn holding(&self, needle: &str) -> usize {
        let lines = self.0.lock().expect("lines");
        lines.iter().filter(|s| s.contains(needle)).count()
    }
}

impl Subscriber for Lines {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        !meta.target().starts_with("sqlx")
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// How many rows of `table` matching `filter` hold `needle` in a text or json column (D-3324).
async fn rows_where(db: &PgPool, (table, filter): (&str, &str), needle: &str) -> i64 {
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT quote_ident(attname) FROM pg_attribute WHERE attrelid = $1::text::regclass \
         AND attnum > 0 AND NOT attisdropped \
         AND atttypid = ANY (ARRAY['text', 'varchar', 'bpchar', 'json', 'jsonb']::regtype[])",
    )
    .bind(table)
    .fetch_all(db)
    .await
    .expect("text columns");
    if columns.is_empty() {
        return 0;
    }
    let held: Vec<String> = columns
        .iter()
        .map(|c| format!("t.{c}::text LIKE '%' || $1 || '%'"))
        .collect();
    let sql = format!(
        "SELECT count(*) FROM {table} t WHERE ({filter}) AND ({})",
        held.join(" OR ")
    );
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(needle)
        .fetch_one(db)
        .await
        .expect("scan")
}

/// TD-CALL-24 (PRIV-002, PRIV-016): after a refused and a linking LINK, no log line, task payload
/// (but the LINK notice T390 queued), audit row or link, phone or refusal row holds the blocked
/// number, its last 4 digits or the PIN.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_24_no_row_or_log_line_holds_the_number_or_the_pin(db: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let (_, _, pin) = setup(&db).await;
    let s = served(&db);
    blocked_call(&s, (1, BLOCKED), &pin).await;
    let now = at() + SignedDuration::from_secs(60);
    text(&s, (1, SHOP), &format!("LINK {}", wrong(&pin)), now).await;
    text(&s, (2, SHOP), &format!("LINK {pin}"), now).await;
    assert_eq!(state(&db).await[0], 1, "linked");
    let notice = "payload->>'template' IS DISTINCT FROM 'link_this_phone'";
    for needle in [BLOCKED, &BLOCKED[BLOCKED.len() - 4..], pin.as_str()] {
        for (table, filter) in [
            ("core.task", notice),
            ("ops.audit_log", "TRUE"),
            ("calls.link_request", "TRUE"),
            ("calls.link_reply_refusal", "TRUE"),
            ("businesses.staff_phone", "TRUE"),
            ("messaging.sender_directory", "TRUE"),
        ] {
            assert_eq!(rows_where(&db, (table, filter), needle).await, 0, "{table}");
        }
        assert_eq!(lines.holding(needle), 0, "a log line holds a value");
    }
}

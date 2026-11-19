//! TD-TXT-26's refusal clause and TD-TXT-28's first clause (T480; TXT-039, TXT-043, SEC-043;
//! D-427, D-490, D-3753, D-3844): the stored owner-portal "Spanish conversations" switch. Every
//! password, code, reviewer, reference, sample, phone and ID is fake; texts go only to the
//! stand-in Twilio.
//!
//! With no stored row the switch reads off. Turning it on with no review, with a review missing
//! one Spanish template or naming an older version, with no campaign note, with a note and no
//! sample, or with an empty sample is refused, names what is missing and stores nothing; so is a
//! turn-on without a fresh second factor. With every record it turns on and the next Spanish text
//! gets the Spanish consent text; turned off, the next Spanish text gets English. The inbound
//! route and its retry read the switch per message. Each change is one new row and one audit row
//! holding no reviewer, reference or sample text, and the table is append-only for the app role.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_adapters::api::{TwilioConfig, TwilioMessageReader};
use askhava_core::api::{
    Clock, Environment, Lease, ManualClock, Registry, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Settings as IntakeSettings};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    Missing, SPANISH_SWITCH_ACTION, SpanishChange, SpanishRecords, spanish_switch,
    spanish_template_versions, turn_spanish_off, turn_spanish_on,
};
use askhava_portals::api::{
    CookieJar, OwnerAuth, SecondFactor, bootstrap_owner, csrf_token, owner_sign_in,
};
use askhava_privacy::api::Vault;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

/// The composition root, compiled in for the router, the inbound route and its retry.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the router, the inbound route and the retry are used here"
)]
mod server;
#[allow(
    clippy::wildcard_imports,
    reason = "the included files name their siblings as `crate::`"
)]
use server::*;

const PASSWORD: &str = "fake t480 owner pass phrase";
const TOKEN: &str = "fake-t480-bootstrap-token-0123456789";
const HOST: &str = "laftly.test";
const AUTH: &str = "fake-auth-token";
const BASE: &str = "https://hooks.example.test";
const MAIN: &str = "+19285550100";
const HER: &str = "+19285550181";
const NOT_US: &str = "+4420";
const REVIEWER: &str = "FAKE-T480-REVIEWER";
const NOTE: &str = "FAKE-T480-CAMPAIGN";
const SAMPLE: &str = "FAKE-T480-SAMPLE Hola, recibimos su solicitud.";
const SPANISH: &str = "Hola, mi lavabo gotea";
/// 10:00 in Arizona.
const MORNING: &str = "2026-11-12T17:00:00Z";

type Jar = BTreeMap<String, String>;

fn vault() -> Vault {
    let key = |b: u8| format!("1:{}", STANDARD.encode([b; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x48).into(), &key(0x49).into(), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-for-t480-not-a-real-key".to_owned())
}

/// Every record a turn-on needs, the review covering each Spanish template's current version.
fn records() -> SpanishRecords {
    SpanishRecords {
        reviewed_on: Some("2026-11-02".parse().expect("date")),
        reviewer: REVIEWER.to_owned(),
        reviewed: spanish_template_versions().expect("versions"),
        note_on: Some("2026-11-03".parse().expect("date")),
        note_reference: NOTE.to_owned(),
        sample: SAMPLE.to_owned(),
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The served router after the owner was bootstrapped.
async fn app(pool: &PgPool) -> Router {
    let mut db = pool.acquire().await.expect("db");
    bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now())
        .await
        .expect("bootstrap");
    let parts = server::RouterParts::from((None, None, None, None)).with_vault(Some(vault()));
    server::build_router(pool.clone(), Router::new(), parts)
}

/// The owner signed in at `at` with a TOTP code (fresh for 5 minutes from `at`).
async fn owner_at(pool: &PgPool, at: Timestamp) -> Jar {
    let mut db = pool.acquire().await.expect("db");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let auth = OwnerAuth::new(HOST, &format!("https://{HOST}")).expect("relying party");
    let code = totp_code(&secret, at);
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    jar.expect("signed in")
        .iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

/// A same-origin `method` to `path` with `jar`'s cookies and CSRF token: status and page.
async fn send(
    app: &Router,
    (method, path): (&str, &str),
    jar: &Jar,
    fields: &str,
) -> (u16, String) {
    let line: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut headers = HeaderMap::new();
    let cookie = HeaderValue::from_str(&line.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, cookie.clone());
    let csrf = csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{fields}")))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let answer = app.clone().oneshot(request).await.expect("answer");
    let status = answer.status().as_u16();
    let bytes = to_bytes(answer.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The served "on" form for `r`.
fn on_form(r: &SpanishRecords) -> String {
    let date = |d: Option<jiff::civil::Date>| d.map(|d| d.to_string()).unwrap_or_default();
    let versions: Vec<String> = r.reviewed.iter().map(|(i, v)| format!("{i} {v}")).collect();
    format!(
        "&to=on&reviewed_on={}&reviewer={}&versions={}&note_on={}&note_reference={}&sample={}",
        date(r.reviewed_on),
        encode(&r.reviewer),
        encode(&versions.join("\n")),
        date(r.note_on),
        encode(&r.note_reference),
        encode(&r.sample)
    )
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

/// Switch rows, switch audit rows, and audit rows anywhere holding a fake record's text.
async fn stored(pool: &PgPool) -> (i64, i64, i64) {
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(SPANISH_SWITCH_ACTION)
        .fetch_one(pool)
        .await
        .expect("audits");
    let leaked = "SELECT count(*) FROM ops.audit_log a WHERE to_jsonb(a)::text LIKE '%FAKE-T480%'";
    (
        count(pool, "SELECT count(*) FROM messaging.spanish_switch").await,
        audits,
        count(pool, leaked).await,
    )
}

/// Consent texts queued in `language`.
async fn consents(pool: &PgPool, language: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE template_id = $1 AND language = $2",
    )
    .bind(CONSENT_TEMPLATE)
    .bind(language)
    .fetch_one(pool)
    .await
    .expect("outbox")
}

/// TD-TXT-26, TD-TXT-28 (TXT-039, TXT-043, D-427): each missing record refuses the turn-on,
/// names what is missing and stores nothing; the switch stays off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_26_td_txt_28_each_missing_record_refuses_and_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    assert!(!spanish_switch(&mut db).await.expect("read").is_on());
    let (owner, now) = (Uuid::from_u128(0x480), Timestamp::now());
    let (first, _) = records()
        .reviewed
        .into_iter()
        .next()
        .expect("a Spanish template");
    let no_review = SpanishRecords {
        reviewed_on: None,
        reviewer: String::new(),
        reviewed: BTreeMap::new(),
        ..records()
    };
    let mut one_short = records();
    one_short.reviewed.remove(&first);
    let mut older = records();
    older.reviewed.insert(first, "0".repeat(64));
    let cases = [
        (
            no_review,
            vec![
                Missing::ReviewDate,
                Missing::Reviewer,
                Missing::ReviewedTemplates,
            ],
        ),
        (one_short, vec![Missing::ReviewedTemplates]),
        (older, vec![Missing::ReviewedTemplates]),
        (
            SpanishRecords {
                note_on: None,
                note_reference: " ".to_owned(),
                ..records()
            },
            vec![Missing::NoteDate, Missing::NoteReference],
        ),
        (
            SpanishRecords {
                sample: String::new(),
                ..records()
            },
            vec![Missing::Sample],
        ),
        (
            SpanishRecords {
                sample: " \n ".to_owned(),
                ..records()
            },
            vec![Missing::Sample],
        ),
    ];
    for (given, want) in cases {
        let said = turn_spanish_on(&mut db, owner, &given, now).await;
        assert_eq!(said, Ok(SpanishChange::Refused(want)));
    }
    assert_eq!(stored(&pool).await, (0, 0, 0), "nothing stored");
    assert!(!spanish_switch(&mut db).await.expect("read").is_on());
    let off = turn_spanish_off(&mut db, owner, now).await;
    assert_eq!(off, Ok(SpanishChange::Unchanged), "already off");
}

/// TD-TXT-26, TD-TXT-28 (TXT-039, TXT-043, SEC-043): the settings page shows the switch off; a
/// stale factor gets the step-up form and a turn-on with no records is a 400 naming each missing
/// record; neither stores anything. With every record the served route turns it on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_28_the_served_switch_needs_a_fresh_factor_and_every_record(pool: PgPool) {
    let app = app(&pool).await;
    let now = Timestamp::now();
    let path = "/owner/spanish-conversations";
    let stale = owner_at(&pool, now - SignedDuration::from_mins(6)).await;
    let (status, _) = send(&app, ("POST", path), &stale, &on_form(&records())).await;
    assert_eq!(status, 401, "a stale factor gets the step-up form");
    let fresh = owner_at(&pool, now).await;
    let (status, page) = send(&app, ("GET", "/owner/settings"), &fresh, "").await;
    assert_eq!(status, 200);
    assert!(
        page.contains("Spanish conversations are off."),
        "off at launch"
    );
    let (status, page) = send(&app, ("POST", path), &fresh, "&to=on").await;
    assert_eq!(status, 400, "{page}");
    for named in [
        "The date of the professional translation review.",
        "name (200 characters at most)",
        "A review that lists every Spanish text",
        "The date you updated the campaign description",
        "The reference for that campaign update",
        "A Spanish sample message",
    ] {
        assert!(page.contains(named), "names {named}");
    }
    assert_eq!(stored(&pool).await, (0, 0, 0), "nothing stored");
    let (status, page) = send(&app, ("POST", path), &fresh, &on_form(&records())).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Saved. Spanish conversations are on."));
    let mut db = pool.acquire().await.expect("db");
    assert!(spanish_switch(&mut db).await.expect("read").is_on());
    assert_eq!(stored(&pool).await, (1, 1, 0));
}

/// The inbound route over `pool`, its clock `clock`.
fn hooks(pool: &PgPool, clock: &Arc<ManualClock>) -> Arc<server::inbound_text::Webhooks> {
    Arc::new(server::inbound_text::Webhooks {
        pool: pool.clone(),
        clock: Arc::clone(clock) as _,
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(AUTH),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

/// POSTs `fields` to the inbound route, signed by the fake token; the status.
async fn post(h: &Arc<server::inbound_text::Webhooks>, fields: &[(String, String)]) -> u16 {
    let path = server::inbound_text::INBOUND_PATH;
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(AUTH.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in &sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let routes = server::inbound_text::routes(Arc::clone(h));
    routes
        .oneshot(request)
        .await
        .expect("answer")
        .status()
        .as_u16()
}

/// A signed delivery of `words` from `from` to the main number; its status.
async fn text(h: &Arc<server::inbound_text::Webhooks>, sid: u32, from: &str, words: &str) -> u16 {
    let fields = [
        ("Body", words),
        ("From", from),
        ("MessageSid", &format!("SM{sid:032}")),
        ("To", MAIN),
    ];
    let fields: Vec<(String, String)> = fields
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect();
    post(h, &fields).await
}

/// TD-TXT-26, TD-TXT-28 (TXT-039, CON-031): the route reads the stored switch per message: a
/// Spanish text gets the English consent text while off, the Spanish one once on, and English
/// again once off. Each change is one row and one audit row; the table is append-only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_28_the_inbound_route_follows_the_stored_switch(pool: PgPool) {
    let now: Timestamp = MORNING.parse().expect("instant");
    let h = hooks(&pool, &Arc::new(ManualClock::new(now)));
    let owner = Uuid::from_u128(0x480);
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(text(&h, 1, "+19285550191", SPANISH).await, 200);
    assert_eq!(
        (consents(&pool, "en").await, consents(&pool, "es").await),
        (1, 0)
    );
    let on = turn_spanish_on(&mut db, owner, &records(), now).await;
    assert_eq!(on, Ok(SpanishChange::Changed));
    assert_eq!(text(&h, 2, "+19285550192", SPANISH).await, 200);
    assert_eq!(consents(&pool, "es").await, 1, "Spanish from the next text");
    let off = turn_spanish_off(&mut db, owner, now + SignedDuration::from_secs(1)).await;
    assert_eq!(off, Ok(SpanishChange::Changed));
    assert_eq!(text(&h, 3, "+19285550193", SPANISH).await, 200);
    assert_eq!(
        (consents(&pool, "en").await, consents(&pool, "es").await),
        (2, 1)
    );
    assert_eq!(stored(&pool).await, (2, 2, 0));
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("app role");
    for sql in [
        "UPDATE messaging.spanish_switch SET is_on = false",
        "DELETE FROM messaging.spanish_switch",
    ] {
        assert!(
            sqlx::query(sql).execute(&mut *db).await.is_err(),
            "append-only: {sql}"
        );
    }
}

/// The stand-in server on a port it picked itself; killed on drop.
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

/// She texts `words` to the stand-in; the webhook fields Twilio would post, `From` replaced by a
/// number that is not a US one, so the route's sender lookup fails and queues the retry.
async fn unreadable_sms(port: u16, words: &str) -> Vec<(String, String)> {
    let body = json!({ "from": HER, "to": MAIN, "body": words }).to_string();
    let request = format!(
        "POST /_stand_in/twilio/inbound HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        STANDARD.encode(format!("AC{}:{AUTH}", "0".repeat(32))),
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, json) = answer.split_once("\r\n\r\n").expect("http answer");
    let parsed: serde_json::Value = serde_json::from_str(json).expect("json");
    ["MessageSid", "From", "To", "Body"]
        .iter()
        .filter_map(|n| {
            let value = if *n == "From" {
                Some(NOT_US)
            } else {
                parsed["params"][n].as_str()
            };
            value.map(|v| ((*n).to_owned(), v.to_owned()))
        })
        .collect()
}

/// The worker's registry with the retry handler over the stand-in.
fn worker(port: u16, clock: &Arc<ManualClock>) -> Registry {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", AUTH),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", "+19285550110,+19285550111"),
        ("ASKHAVA_MASKED_POOL_SIZE", "2"),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("twilio config");
    let reader = TwilioMessageReader::new(&config).expect("reader");
    let keys = server::inbound_retry::RetryKeys {
        id_secret: secret(),
        vault: vault(),
        intake: IntakeSettings::default(),
        clock: Arc::clone(clock) as _,
    };
    let mut registry = Registry::new();
    server::inbound_retry::register(&mut registry, Arc::new(reader), Arc::new(keys));
    registry
}

/// Runs every task due at the clock's now through `registry` on an `askhava_app` pool (R9).
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let app = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let owner = WorkerId::new(&app).await.expect("worker");
    let tasks = claim(&app, owner, clock.now(), SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    for task in tasks {
        registry
            .run(&app, clock, owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

/// TD-TXT-28 (TXT-039, D-3889): the retry reads the stored switch when it runs, not when the text
/// came in: a Spanish text stopped while the switch was off and retried after it was turned on
/// gets the Spanish consent text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_28_the_retry_reads_the_switch_when_it_runs(pool: PgPool) {
    let (fake, clock) = (
        stand_in(),
        Arc::new(ManualClock::new(MORNING.parse().expect("instant"))),
    );
    let fields = unreadable_sms(fake.1, SPANISH).await;
    assert_eq!(post(&hooks(&pool, &clock), &fields).await, 200);
    let retries = "SELECT count(*) FROM core.task WHERE kind = 'messaging.inbound_retry'";
    assert_eq!(
        count(&pool, retries).await,
        1,
        "the stopped text is queued for the retry"
    );
    assert_eq!(consents(&pool, "es").await + consents(&pool, "en").await, 0);
    let mut db = pool.acquire().await.expect("db");
    let on = turn_spanish_on(&mut db, Uuid::from_u128(0x480), &records(), clock.now()).await;
    assert_eq!(on, Ok(SpanishChange::Changed));
    clock.advance(SignedDuration::from_mins(5));
    run_due(&pool, &worker(fake.1, &clock), &clock).await;
    assert_eq!(
        (consents(&pool, "es").await, consents(&pool, "en").await),
        (1, 0)
    );
}

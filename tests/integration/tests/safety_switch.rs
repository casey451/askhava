//! TD-CALL-26 (T407; CALL-019, CALL-022, SEC-043, CON-031, TXT-024; D-275, D-384, D-3134,
//! D-3890): the stored owner-portal "safety voice call" switch. Every password, code, approval,
//! contact, phone and ID is fake; calls go only to the stand-in Twilio.
//!
//! With no stored row the switch reads off. Turning it on without an approval reference, without
//! a fresh second factor, or while a CALL-022 item is false stores nothing, and the page names
//! what is missing. Turned on through the served route after the worker started, the next
//! `calls.safety_call` task places exactly one call and a repeat places none; turned off, the
//! next task places none; with no main number set the task completes with no call. The inbound
//! text route's consent text carries CALL-022's call sentence, and a 911-list text during a
//! suspension queues the safety call, only while the stored switch is on. Each change is one new
//! row and one audit row holding no reference text.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_calls::api::{
    Approval, Conditions, SAFETY_CALL_KIND, SAFETY_CONDITIONS, SAFETY_SWITCH_ACTION, safety_switch,
    turn_safety_switch_off, turn_safety_switch_on,
};
use askhava_core::api::{
    Clock, Environment, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, claim, enqueue,
};
use askhava_intake::api::rules::{CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE};
use askhava_messaging::api::{Audience, record_suspension};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_picks::api::ConfirmSettings;
use askhava_portals::api::{
    CookieJar, OwnerAuth, SafetyCallForm, SecondFactor, SettingsNotice, bootstrap_owner,
    csrf_token, owner_principal, owner_safety_call, owner_settings_page_with, owner_sign_in,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
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
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The composition root, compiled in for the router, the worker and the inbound route.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the router, worker and inbound route are used here"
)]
mod server;
#[allow(
    clippy::wildcard_imports,
    reason = "the included files name their siblings as `crate::`"
)]
use server::*;

const PASSWORD: &str = "fake t407 owner pass phrase";
const TOKEN: &str = "fake-t407-bootstrap-token-0123456789";
const HOST: &str = "laftly.test";
const SID: &str = "AC00000000000000000000000000000000";
const AUTH: &str = "fake-auth-token";
const BASE: &str = "https://hooks.example.test";
const MAIN: &str = "+19285550100";
const POOL: &str = "+19285550110";
const REFERENCE: &str = "FAKE-T407-APPROVAL";
const ON: &str = "&to=on&approved_on=2026-11-02&reference=FAKE-T407-APPROVAL";

type Jar = BTreeMap<String, String>;

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x47).into(), &key(0x48).into(), &url).expect("vault")
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

/// The served router (owner tools over the pool) after the owner was bootstrapped.
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

fn cookies(jar: &Jar) -> CookieJar {
    let line: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&line.join("; ")).expect("cookie"),
    );
    CookieJar::from_headers(&headers)
}

/// A same-origin `method` to `path` with `jar`'s cookies (and CSRF token for a `POST`): status and
/// page.
async fn send(
    app: &Router,
    (method, path): (&str, &str),
    jar: &Jar,
    fields: &str,
) -> (u16, String) {
    let line: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let csrf = csrf_token(&cookies(jar)).unwrap_or_default();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, line.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{fields}")))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let answer = app.clone().oneshot(request).await.expect("answer");
    let status = answer.status().as_u16();
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

/// Switch rows, switch audit rows, and audit rows anywhere holding the reference text.
async fn stored(pool: &PgPool) -> (i64, i64, i64) {
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(SAFETY_SWITCH_ACTION)
        .fetch_one(pool)
        .await
        .expect("audits");
    let leaked = "SELECT count(*) FROM ops.audit_log a WHERE to_jsonb(a)::text LIKE '%FAKE-T407%'";
    (
        count(pool, "SELECT count(*) FROM calls.safety_switch").await,
        audits,
        count(pool, leaked).await,
    )
}

/// TD-CALL-26 (CALL-019, CALL-022, SEC-043, D-275, D-384): no row reads off; a turn-on without a
/// fresh factor, without a reference or with a CALL-022 item false stores nothing, and the page
/// says what is missing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_26_the_switch_reads_off_and_each_refusal_stores_nothing(pool: PgPool) {
    let app = app(&pool).await;
    let mut db = pool.acquire().await.expect("db");
    assert!(!safety_switch(&mut db).await.expect("read").is_on());
    let now = Timestamp::now();
    let stale = owner_at(&pool, now - SignedDuration::from_mins(6)).await;
    let (status, _) = send(&app, ("POST", "/owner/safety-call"), &stale, ON).await;
    assert_eq!(status, 401, "a stale factor gets the step-up form");
    let fresh = owner_at(&pool, now).await;
    let (status, page) = send(&app, ("GET", "/owner/settings"), &fresh, "").await;
    assert_eq!(status, 200);
    assert!(
        page.contains("The safety voice call is off."),
        "off at launch"
    );
    let blank = "&to=on&approved_on=2026-11-02&reference=%20";
    let (status, page) = send(&app, ("POST", "/owner/safety-call"), &fresh, blank).await;
    assert_eq!(status, 400, "{page}");
    assert!(page.contains("Enter the date and reference of your attorney"));
    let principal = owner_principal(&mut db, &cookies(&fresh), now)
        .await
        .expect("read")
        .expect("live");
    let form = SafetyCallForm {
        to: "on".to_owned(),
        approved_on: "2026-11-02".to_owned(),
        reference: REFERENCE.to_owned(),
    };
    let unmet = Conditions {
        logged: false,
        ..SAFETY_CONDITIONS
    };
    let notice = owner_safety_call(&mut db, &principal, (&form, &unmet), now).await;
    let notice = notice.expect("a notice");
    assert_eq!(notice, SettingsNotice::SafetyCallUnmet(unmet));
    assert!(notice.refused());
    let page = owner_settings_page_with(&mut db, "fake-csrf", Some(notice))
        .await
        .expect("page");
    assert!(
        page.contains("Each call is logged: the text that started it"),
        "names it"
    );
    assert!(
        !page.contains("Before every call, we check"),
        "names only the unmet item"
    );
    assert_eq!(stored(&pool).await, (0, 0, 0), "nothing stored");
    assert!(!safety_switch(&mut db).await.expect("read").is_on());
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

/// How many calls the stand-in was asked to place.
async fn placed(port: u16) -> usize {
    let basic = STANDARD.encode(format!("{SID}:{AUTH}"));
    let url = format!("http://127.0.0.1:{port}/twilio/2010-04-01/Accounts/{SID}/Calls.json");
    let request = format!(
        "GET {url} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {basic}\r\n\
         Connection: close\r\n\r\n"
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, body) = answer.split_once("\r\n\r\n").expect("http answer");
    let list: serde_json::Value = serde_json::from_str(body).expect("json");
    list["calls"].as_array().expect("calls").len()
}

/// The production worker registry in dev, Twilio at the stand-in on `port`, with `main` as
/// `ASKHAVA_MAIN_NUMBER` when given.
fn worker(port: u16, main: Option<&str>, clock: Arc<dyn Clock>) -> Registry {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t407-{port}"));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let twilio = format!("http://127.0.0.1:{port}/twilio");
    let storage = format!("file://{}", dir.display());
    let mut pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_STORAGE_BASE_URL", &storage),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-for-t407-not-a-real-key",
        ),
        ("VAULT_KEY", &key(0x47)),
        ("VAULT_INDEX_KEY", &key(0x48)),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", SID),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", AUTH),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &format!("{MAIN},{POOL}")),
        ("ASKHAVA_MASKED_POOL_SIZE", "2"),
        ("ASKHAVA_TWILIO_BASE_URL", &twilio),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &twilio),
        ("ASKHAVA_PUBLIC_BASE_URL", BASE),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    if let Some(main) = main {
        pairs.push(("ASKHAVA_MAIN_NUMBER".to_owned(), main.to_owned()));
    }
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream = server::offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock)
        .expect("stream");
    let parts = (None, ConfirmSettings::default());
    server::worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// A stored fake contact with an open critical alert about her, queued as one safety call task
/// arriving at `now`.
async fn queue_call(pool: &PgPool, phone: &str, now: Timestamp) -> serde_json::Value {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let alert = NewAlert {
        kind: "messaging.opted_out_emergency",
        severity: Severity::Critical,
        subject: Some(("privacy.contact", contact)),
    };
    let alert = raise_alert(&mut db, &alert).await.expect("alert");
    let payload = json!({
        "contact": contact, "message": Uuid::new_v4(), "arrived_at": now.to_string(), "alert": alert,
    });
    requeue(pool, &payload, now).await;
    payload
}

async fn requeue(pool: &PgPool, payload: &serde_json::Value, now: Timestamp) {
    let mut db = pool.acquire().await.expect("db");
    let task = NewTask::new(SAFETY_CALL_KIND, payload.clone(), now);
    enqueue(&mut db, &task).await.expect("queue");
}

/// Runs every task due now through `registry`; whether each finished done.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) -> Vec<bool> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    let mut done = Vec::new();
    for task in tasks {
        let id = task.id;
        registry
            .run(pool, clock, owner, task, Lease::WORKER)
            .await
            .expect("run");
        let sql = "SELECT done_at IS NOT NULL FROM core.task WHERE id = $1";
        let finished = sqlx::query_scalar(sql).bind(id).fetch_one(pool).await;
        done.push(finished.expect(sql));
    }
    done
}

/// TD-CALL-26 (CALL-019, CALL-022, SEC-043, TXT-024): turned on through the served route after
/// the worker started, the next task places one call and a repeat none; turned off, the next task
/// places none; each change is one row and one audit row holding no reference text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_26_turned_on_after_start_places_one_call_and_off_stops_the_next(pool: PgPool) {
    let fake = stand_in();
    let now = Timestamp::now();
    let clock = Arc::new(ManualClock::new(now));
    let registry = worker(fake.1, Some(MAIN), clock.clone());
    let app = app(&pool).await;
    let first = queue_call(&pool, "+19285550171", now).await;
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    assert_eq!(placed(fake.1).await, 0, "off: no call");
    let jar = owner_at(&pool, now).await;
    let (status, page) = send(&app, ("POST", "/owner/safety-call"), &jar, ON).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Saved. The safety voice call is on."));
    assert!(
        page.contains(REFERENCE) && page.contains("November 2, 2026"),
        "the approval"
    );
    requeue(&pool, &first, now).await;
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    assert_eq!(placed(fake.1).await, 1, "exactly one call");
    requeue(&pool, &first, now).await;
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    assert_eq!(placed(fake.1).await, 1, "a repeat places none");
    let (status, page) = send(&app, ("POST", "/owner/safety-call"), &jar, "&to=off").await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Saved. The safety voice call is off."));
    queue_call(&pool, "+19285550172", now).await;
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    assert_eq!(placed(fake.1).await, 1, "off stops the next call");
    assert_eq!(
        stored(&pool).await,
        (2, 2, 0),
        "two rows, two audits, no reference"
    );
}

/// TD-CALL-26 (CALL-022, D-3890): switched on with no main number set, the task completes with no
/// call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_26_with_no_main_number_the_task_completes_with_no_call(pool: PgPool) {
    let fake = stand_in();
    let now = Timestamp::now();
    let clock = Arc::new(ManualClock::new(now));
    let registry = worker(fake.1, None, clock.clone());
    let mut db = pool.acquire().await.expect("db");
    let approval = Approval {
        date: "2026-11-02".parse().expect("date"),
        reference: REFERENCE.to_owned(),
    };
    let on = turn_safety_switch_on(
        &mut db,
        Uuid::new_v4(),
        (&approval, &SAFETY_CONDITIONS),
        now,
    );
    assert_eq!(on.await, Ok(askhava_calls::api::SwitchChange::Changed));
    assert!(safety_switch(&mut db).await.expect("read").is_on());
    queue_call(&pool, "+19285550173", now).await;
    assert_eq!(
        run_due(&pool, &registry, &clock).await,
        vec![true],
        "completes"
    );
    assert_eq!(placed(fake.1).await, 0, "no call");
}

/// The inbound route over `pool`, its clock at `now`.
fn hooks(pool: &PgPool, now: Timestamp) -> Arc<server::inbound_text::Webhooks> {
    Arc::new(server::inbound_text::Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(now)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(AUTH),
        id_secret: SecretString::from("test-only-id-secret-for-t407-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: askhava_intake::api::rules::Settings::default(),
    })
}

/// A signed Twilio delivery of `words` from `from` to the main number; its status.
async fn text(h: &Arc<server::inbound_text::Webhooks>, sid: u32, from: &str, words: &str) -> u16 {
    let sid = format!("SM{sid:032}");
    let mut fields = [
        ("Body", words),
        ("From", from),
        ("MessageSid", sid.as_str()),
        ("To", MAIN),
    ];
    fields.sort_unstable();
    let path = server::inbound_text::INBOUND_PATH;
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(AUTH.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", urlencoding(v)))
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

fn urlencoding(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn consents(pool: &PgPool, template: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
        .bind(template)
        .fetch_one(pool)
        .await
        .expect("outbox")
}

/// TD-CALL-26 (CON-031, CALL-022, TXT-024): the inbound route reads the stored switch per
/// message: the consent text carries the call sentence, and a 911-list text during a suspension
/// queues the safety call, only while it is on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_26_the_inbound_route_follows_the_stored_switch(pool: PgPool) {
    let now = Timestamp::now();
    let h = hooks(&pool, now);
    let owner = Uuid::new_v4();
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(
        text(&h, 1, "+19285550181", "my kitchen sink is leaking").await,
        200
    );
    assert_eq!(consents(&pool, CONSENT_TEMPLATE).await, 1);
    assert_eq!(
        consents(&pool, CONSENT_CALL_TEMPLATE).await,
        0,
        "never before"
    );
    let approval = Approval {
        date: "2026-11-02".parse().expect("date"),
        reference: REFERENCE.to_owned(),
    };
    let on = turn_safety_switch_on(&mut db, owner, (&approval, &SAFETY_CONDITIONS), now).await;
    assert_eq!(on, Ok(askhava_calls::api::SwitchChange::Changed));
    assert_eq!(
        text(&h, 2, "+19285550182", "my water heater is leaking").await,
        200
    );
    assert_eq!(
        consents(&pool, CONSENT_CALL_TEMPLATE).await,
        1,
        "from the moment it is on"
    );
    let by = (Uuid::from_u128(0x407), now - SignedDuration::from_mins(5));
    let since = now - SignedDuration::from_mins(5);
    let suspended = record_suspension(&mut db, Audience::Homeowner, since, by).await;
    assert_eq!(suspended, Ok(true));
    let calls = "SELECT count(*) FROM core.task WHERE kind = 'calls.safety_call'";
    assert_eq!(
        text(&h, 3, "+19285550183", "I smell gas in my house").await,
        200
    );
    assert_eq!(count(&pool, calls).await, 1, "queued while on");
    let off = turn_safety_switch_off(&mut db, owner, now + SignedDuration::from_secs(1)).await;
    assert_eq!(off, Ok(askhava_calls::api::SwitchChange::Changed));
    assert_eq!(
        text(&h, 4, "+19285550184", "I smell gas in my house").await,
        200
    );
    assert_eq!(count(&pool, calls).await, 1, "none while off");
    assert_eq!(stored(&pool).await, (2, 2, 0));
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("app role");
    for sql in [
        "UPDATE calls.safety_switch SET is_on = false",
        "DELETE FROM calls.safety_switch",
    ] {
        assert!(
            sqlx::query(sql).execute(&mut *db).await.is_err(),
            "append-only: {sql}"
        );
    }
}

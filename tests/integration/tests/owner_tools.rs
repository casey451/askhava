//! The owner tools T484 adds (TD-OPS-10's call case, TD-CON-17's owner case through the settings
//! page, TD-FEE-30's filed-answer precondition; OPS-028, OPS-035, CON-035, PICK-048, SEC-043;
//! D-3753, D-3880, D-3890). Every password, code, contact, phone and ID is fake.
//!
//! The bridge press queues one `calls.owner_bridge` task per press and none for a second press of
//! the same form; the worker's handler rings Casey through the stand-in Twilio and bridges him to
//! the homeowner with the pool number shown, never his own. OPS-035's automated call
//! (`calls.owner_alert_call`, queued by T204) names the alert kind and job number with no homeowner
//! detail, at 02:00 and while the texting number is suspended; with no owner phone it dials
//! nothing. An owner-recorded opt-out lists the number at once with channel `owner` and queues no
//! text; a Stripe answer filing is stored once and read by `quotes::api::stripe_answer_filed`.
//! Each owner action is refused without a fresh second factor.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_calls::api::OWNER_BRIDGE_KIND;
use askhava_core::api::{Clock, Lease, ManualClock, NewTask, Registry, Settings, WorkerId};
use askhava_core::api::{claim, enqueue};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{Audience, OWNER_ALERT_CALL_KIND, record_suspension};
use askhava_ops::api::{NewAlert, Severity, raise_alert, record_push};
use askhava_picks::api::ConfirmSettings;
use askhava_portals::api::{
    BridgeForm, CookieJar, OptOutForm, OwnerAuth, OwnerPrincipal, OwnerToolError, SecondFactor,
    SettingsNotice, StripeAnswerForm, bootstrap_owner, owner_bridge_press,
    owner_file_stripe_answer, owner_home_page, owner_principal, owner_record_opt_out,
    owner_settings_page_with, owner_sign_in,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::stripe_answer_filed;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The composition root, compiled in for `worker_registry`.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
const SID: &str = "AC00000000000000000000000000000000";
const AUTH: &str = "fake-auth-token";
const MAIN: &str = "+19285550100";
const POOL: &str = "+19285550110";
const CASEY: &str = "+19285550199";
const HER: &str = "+19285550176";
const KIND: &str = "messaging.paused_emergency";

/// 02:00 Arizona time (09:00 UTC) on a fake day, plus `secs`.
fn at(secs: i64) -> Timestamp {
    "2026-11-03T09:00:00Z".parse::<Timestamp>().expect("time") + SignedDuration::from_secs(secs)
}

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x48).into(), &key(0x49).into(), &url).expect("vault")
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

/// The bootstrapped owner signed in at 0 with a TOTP code; the jar of that session.
async fn signed_in(db: &mut PgConnection) -> CookieJar {
    bootstrap_owner(db, Some(TOKEN), PASSWORD, at(0))
        .await
        .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let code = totp_code(&secret, at(0));
    let auth = OwnerAuth::new("laftly.test", "https://laftly.test").expect("relying party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    owner_sign_in(db, CookieJar::new(), factor, &auth, at(0))
        .await
        .expect("signed in")
}

/// The session's principal at `t` seconds: its factor is fresh until 300.
async fn principal(db: &mut PgConnection, jar: &CookieJar, t: i64) -> OwnerPrincipal {
    owner_principal(db, jar, at(t))
        .await
        .expect("read")
        .expect("live")
}

async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect(sql)
}

/// A homeowner (stored in the vault with [`HER`]) whose plumbing text opened job #n, a critical
/// alert about her inbound text and its `messaging.inbox_contact` row: the alert and job number.
async fn alerted(db: &mut PgConnection) -> (Uuid, i64) {
    let phone = Some(SecretString::from(HER));
    let new = NewContact {
        phone,
        ..NewContact::default()
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my water heater is leaking all over the garage";
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x484),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(-600),
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &msg, IntakeSettings::default())
        .await
        .expect("text");
    let start = JobStart {
        conversation: handled.conversation.expect("request"),
        words,
        emergency: false,
        trade: "plumbing",
        at: at(-540),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    sqlx::query("INSERT INTO messaging.inbox_contact (inbox_id, contact_id, received_at) VALUES ($1, $2, $3)")
        .bind(msg.inbox_id)
        .bind(contact)
        .bind(at(-600).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("inbox contact");
    let alert = NewAlert {
        kind: KIND,
        severity: Severity::Critical,
        subject: Some(("messaging.inbox", msg.inbox_id)),
    };
    let alert = raise_alert(db, &alert).await.expect("alert");
    record_push(db, alert, at(-300)).await.expect("pushed");
    (alert, job_number(db, job).await.expect("number"))
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

/// One HTTP/1.1 exchange with the stand-in; the JSON answer.
async fn http(port: u16, head: &str, body: &str) -> Value {
    let request = format!(
        "{head}\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, json) = answer.split_once("\r\n\r\n").expect("http answer");
    serde_json::from_str(json).expect("json")
}

/// Every call the stand-in was asked to place, each with the script its create gave.
async fn calls(port: u16) -> Vec<(Value, String)> {
    let basic = STANDARD.encode(format!("{SID}:{AUTH}"));
    let head = format!(
        "GET /twilio/2010-04-01/Accounts/{SID}/Calls.json HTTP/1.1\r\nAuthorization: Basic {basic}"
    );
    let list = http(port, &head, "").await;
    let mut out = Vec::new();
    for call in list["calls"].as_array().expect("calls").clone() {
        let body = json!({ "sid": call["sid"], "outcome": "human" }).to_string();
        let given = http(port, "POST /_stand_in/twilio/call-progress HTTP/1.1", &body).await;
        out.push((call, given["twiml"].as_str().unwrap_or_default().to_owned()));
    }
    out
}

/// The `worker` binary's registry in dev, Twilio at the stand-in on `port`, with `owner` as
/// `ASKHAVA_OWNER_PHONE` when given.
fn worker(port: u16, owner: Option<&str>, clock: Arc<dyn Clock>) -> Registry {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t484-{port}"));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let base = format!("http://127.0.0.1:{port}/twilio");
    let mut pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            &format!("file://{}", dir.display()),
        ),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-for-t484-not-a-real-key",
        ),
        ("VAULT_KEY", &key(0x48)),
        ("VAULT_INDEX_KEY", &key(0x49)),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", SID),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", AUTH),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &format!("{MAIN},{POOL}")),
        ("ASKHAVA_MASKED_POOL_SIZE", "2"),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
        ("ASKHAVA_MAIN_NUMBER", MAIN),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://hooks.example.test"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    if let Some(owner) = owner {
        pairs.push(("ASKHAVA_OWNER_PHONE".to_owned(), owner.to_owned()));
    }
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    worker_registry(&settings, stream, (None, ConfirmSettings::default()), pairs).expect("registry")
}

/// Runs each task due now through `registry`, as the worker would; whether each finished done.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) -> Vec<bool> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let tasks = claim(pool, owner, clock.now(), lease, 50)
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
        let finished: bool = sqlx::query_scalar(sql)
            .bind(id)
            .fetch_one(pool)
            .await
            .expect(sql);
        done.push(finished);
    }
    done
}

/// OPS-028, SEC-043 (D-3753): one press queues one bridge task with
/// the alert and press IDs only and one audit row; the same form again queues none; a stale
/// factor or a non-critical alert queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_028_one_press_queues_one_bridge_task(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (alert, _) = alerted(&mut db).await;
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 60).await;
    let page = owner_home_page(&mut db, "fake-csrf", 4, at(60))
        .await
        .expect("home");
    assert!(
        page.contains("/owner/call-homeowner"),
        "the press is offered"
    );
    assert!(
        !page.contains(HER) && !page.contains("5550176"),
        "no contact detail"
    );
    let form = BridgeForm {
        alert: alert.to_string(),
        press: Uuid::from_u128(0x0b1d).to_string(),
    };
    assert_eq!(
        owner_bridge_press(&mut db, &owner, &form, at(60)).await,
        Ok(true)
    );
    assert_eq!(
        owner_bridge_press(&mut db, &owner, &form, at(61)).await,
        Ok(false)
    );
    let tasks = "SELECT count(*) FROM core.task WHERE kind = 'calls.owner_bridge'";
    assert_eq!(count(&mut db, tasks).await, 1, "one task per press");
    let payload: Value = sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
        .bind(OWNER_BRIDGE_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("payload");
    assert_eq!(
        payload,
        json!({ "alert": alert, "press": Uuid::from_u128(0x0b1d) })
    );
    let logged = "SELECT count(*) FROM ops.audit_log WHERE action = 'portals.owner_bridge_press'";
    assert_eq!(count(&mut db, logged).await, 1);
    let stale = principal(&mut db, &jar, 400).await;
    let other = BridgeForm {
        press: Uuid::from_u128(0x0b1e).to_string(),
        ..form
    };
    let refused = owner_bridge_press(&mut db, &stale, &other, at(400)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    let normal = NewAlert {
        kind: "businesses.complaint",
        severity: Severity::Normal,
        subject: None,
    };
    let normal = raise_alert(&mut db, &normal).await.expect("alert");
    let wrong = BridgeForm {
        alert: normal.to_string(),
        ..other
    };
    let refused = owner_bridge_press(&mut db, &owner, &wrong, at(62)).await;
    assert_eq!(refused, Err(OwnerToolError::Refused));
    assert_eq!(count(&mut db, tasks).await, 1, "nothing more queued");
}

/// OPS-028, CALL-006 (D-3753, D-3890): the press's handler rings Casey through the stand-in from
/// the pool number, reads the 911 words and bridges him to her with the pool number shown, never
/// his own number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_028_the_bridge_rings_through_the_stand_in(pool: PgPool) {
    let fake = stand_in();
    let clock = Arc::new(ManualClock::new(at(60)));
    let registry = worker(fake.1, Some(CASEY), clock.clone());
    let mut db = pool.acquire().await.expect("conn");
    let (alert, _) = alerted(&mut db).await;
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 60).await;
    let form = BridgeForm {
        alert: alert.to_string(),
        press: Uuid::from_u128(0x0b1f).to_string(),
    };
    assert_eq!(
        owner_bridge_press(&mut db, &owner, &form, at(60)).await,
        Ok(true)
    );
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    let placed = calls(fake.1).await;
    assert_eq!(placed.len(), 1, "one call");
    let (call, twiml) = &placed[0];
    assert_eq!(
        (call["to"].as_str(), call["from"].as_str()),
        (Some(CASEY), Some(POOL))
    );
    assert!(twiml.contains(&format!("callerId=\"{POOL}\"")), "{twiml}");
    assert!(
        twiml.contains(&format!("<Number>{HER}</Number>")),
        "her leg"
    );
    assert!(twiml.contains("911"), "the INT-004 words first");
    assert!(
        !twiml.contains(CASEY) && !twiml.contains(MAIN),
        "his number never shown"
    );
}

/// TD-OPS-10's call case (OPS-035, OPS-004, QH-004; D-3880, D-3890): at 02:00 and with the texting
/// number suspended, `calls.owner_alert_call` places one call to Casey naming the alert kind and
/// her job number, holding no homeowner detail; with no owner phone the run fails and dials
/// nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_10_the_automated_call_names_kind_and_job_at_night(pool: PgPool) {
    let fake = stand_in();
    let clock = Arc::new(ManualClock::new(at(0)));
    let mut db = pool.acquire().await.expect("conn");
    let (alert, number) = alerted(&mut db).await;
    let texts = "SELECT count(*) FROM messaging.outbox";
    let before = count(&mut db, texts).await;
    let by = (Uuid::from_u128(0x0e), at(-60));
    let suspended = record_suspension(&mut db, Audience::Homeowner, at(-60), by).await;
    assert_eq!(suspended, Ok(true), "the texting number is suspended");
    queue_call(&mut db, alert).await;
    let registry = worker(fake.1, Some(CASEY), clock.clone());
    assert_eq!(run_due(&pool, &registry, &clock).await, vec![true]);
    let placed = calls(fake.1).await;
    assert_eq!(placed.len(), 1, "one call at 02:00");
    let (call, twiml) = &placed[0];
    assert_eq!(
        (call["to"].as_str(), call["from"].as_str()),
        (Some(CASEY), Some(POOL))
    );
    assert!(
        twiml.contains("messaging paused emergency"),
        "names the kind: {twiml}"
    );
    assert!(
        twiml.contains(&format!("on job {number}.")),
        "names the job number"
    );
    assert!(
        !twiml.contains(HER) && !twiml.contains("5550176"),
        "no homeowner detail"
    );
    assert!(!twiml.contains("<Dial"), "say only");
    assert_eq!(count(&mut db, texts).await, before, "no alert goes by text");
    queue_call(&mut db, Uuid::from_u128(0x0a1e)).await;
    let closed = worker(fake.1, None, clock.clone());
    assert_eq!(
        run_due(&pool, &closed, &clock).await,
        vec![false],
        "fails closed"
    );
    assert_eq!(
        calls(fake.1).await.len(),
        1,
        "nothing dialled without the owner phone"
    );
}

/// Queues `alert`'s automated call as T204 does: the alert ID only, once per alert.
async fn queue_call(db: &mut PgConnection, alert: Uuid) {
    let mut task = NewTask::new(OWNER_ALERT_CALL_KIND, json!({ "alert": alert }), at(0));
    task.dedupe_key = Some(format!("{OWNER_ALERT_CALL_KIND}:{alert}"));
    enqueue(db, &task).await.expect("queued");
}

/// TD-CON-17's owner case through the settings page (CON-035, CON-022, D-3753): an owner-recorded
/// opt-out lists the number at once with channel `owner`, is confirmed on screen, queues no text;
/// a repeat changes nothing; a stale factor or a bad number lists nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_035_an_owner_recorded_opt_out_lists_at_once_with_no_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 60).await;
    let form = OptOutForm {
        phone: "+19285550184".to_owned(),
    };
    assert!(
        !format!("{form:?}").contains("5550184"),
        "the form's Debug hides the number"
    );
    let done = owner_record_opt_out(&mut db, &vault(), &owner, &form, at(60)).await;
    assert_eq!(done, Ok(SettingsNotice::OptOutListed));
    let (channel, since): (String, jiff_sqlx::Timestamp) = sqlx::query_as(
        "SELECT e.channel, s.since FROM messaging.suppression s \
         JOIN messaging.optout_event e ON e.id = s.event_id",
    )
    .fetch_one(&mut *db)
    .await
    .expect("listed and logged");
    assert_eq!((channel.as_str(), since.to_jiff()), ("owner", at(60)));
    let again = owner_record_opt_out(&mut db, &vault(), &owner, &form, at(61)).await;
    assert_eq!(again, Ok(SettingsNotice::OptOutAlready));
    let page = owner_settings_page_with(&mut db, "fake-csrf", again.ok())
        .await
        .expect("page");
    assert!(page.contains("already off our texts") && !page.contains("5550184"));
    assert_eq!(
        count(&mut db, "SELECT count(*) FROM messaging.outbox").await,
        0,
        "no text"
    );
    let other = OptOutForm {
        phone: "+19285550185".to_owned(),
    };
    let stale = principal(&mut db, &jar, 400).await;
    let refused = owner_record_opt_out(&mut db, &vault(), &stale, &other, at(400)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    let bad = OptOutForm {
        phone: "not a number".to_owned(),
    };
    let refused = owner_record_opt_out(&mut db, &vault(), &owner, &bad, at(62)).await;
    assert_eq!(refused, Err(OwnerToolError::Refused));
    let listed = "SELECT count(*) FROM messaging.suppression";
    assert_eq!(
        count(&mut db, listed).await,
        1,
        "only the first number is listed"
    );
}

/// TD-FEE-30's precondition (PICK-048, FEE-014, D-2783, D-3753): no filing reads `false`; a filing
/// with the fresh factor is stored once and `stripe_answer_filed` reads it; the page shows the
/// newest; a stale factor or a blank reference files nothing; a later "no" filing is in force.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_a_filed_stripe_answer_is_stored_once_and_read(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(
        stripe_answer_filed(&mut db).await,
        Ok(false),
        "no record, no box"
    );
    let jar = signed_in(&mut db).await;
    let owner = principal(&mut db, &jar, 60).await;
    let form = StripeAnswerForm {
        answered_on: "2026-10-30".to_owned(),
        reference: "FAKE-CASE-0484".to_owned(),
        allows: "yes".to_owned(),
    };
    let filed = owner_file_stripe_answer(&mut db, &owner, &form, at(60)).await;
    assert_eq!(filed, Ok(SettingsNotice::StripeAnswerFiled));
    let rows = "SELECT count(*) FROM quotes.stripe_written_answer";
    assert_eq!(count(&mut db, rows).await, 1, "stored once");
    assert_eq!(stripe_answer_filed(&mut db).await, Ok(true));
    let page = owner_settings_page_with(&mut db, "fake-csrf", filed.ok())
        .await
        .expect("page");
    assert!(page.contains("FAKE-CASE-0484") && page.contains("2026-10-30"));
    let stale = principal(&mut db, &jar, 400).await;
    let refused = owner_file_stripe_answer(&mut db, &stale, &form, at(400)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    let blank = StripeAnswerForm {
        reference: "  ".to_owned(),
        ..form.clone()
    };
    let refused = owner_file_stripe_answer(&mut db, &owner, &blank, at(61)).await;
    assert_eq!(refused, Err(OwnerToolError::Refused));
    assert_eq!(count(&mut db, rows).await, 1, "nothing more filed");
    let no = StripeAnswerForm {
        allows: "no".to_owned(),
        ..form
    };
    let filed = owner_file_stripe_answer(&mut db, &owner, &no, at(62)).await;
    assert_eq!(filed, Ok(SettingsNotice::StripeAnswerFiled));
    assert_eq!(
        stripe_answer_filed(&mut db).await,
        Ok(false),
        "the newest is in force"
    );
}

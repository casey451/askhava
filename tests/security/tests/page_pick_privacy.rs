//! The page pick's code never shows her number (TD-PICK-46; PICK-013, PICK-055, PRIV-003,
//! CON-029, D-1516). Fake numbers (555-01xx), IDs, codes and words only.
//!
//! The code goes only to the vaulted phone of the job's homeowner, read by `picks` alone through
//! `privacy::api::read_for_pick_code` (one `vault_read.picks.pick_code` audit row per read; the
//! reader scan is `vault_readers.rs`); the number's digits appear in no response body, header,
//! log line, alert or audit value; a paused job or a number that can't be texted gets no code
//! and nothing is picked; the daily send cap holds; without the keys the page sends nothing and
//! keeps today's "Nothing was picked" lines.

use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_portals::api::{PRE_SESSION_COOKIE, PickCodeKeys, pick_flow_routes};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const PHONE: &str = "+19285550161";
const DIGITS: [&str; 3] = ["5550161", "555-0161", "555 0161"];
const CODE: &str = "123456";
const COOKIE: &str = "fake-pre-session-cookie-value-0046";
const DAY: &str = "2026-11-02T17:00:00Z";
const NO_CODE: &str =
    "We can&#39;t text your phone right now, so no code was sent and nothing was picked.";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x47), &key(0x57), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-046".to_owned())
}

/// A Verify stand-in that records each destination and passes only [`CODE`].
#[derive(Clone, Default)]
struct FakeVerify {
    sent: Arc<Mutex<Vec<String>>>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sent.lock().expect("lock").push(to.to_owned());
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// Every log line written while a case runs.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn routes(pool: &PgPool, verify: Option<&FakeVerify>) -> Router {
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let terms = (
        "2026-11-01".to_owned(),
        "Sample homeowner terms for tests only.",
    );
    let keys = verify.map(|v| PickCodeKeys::new(v.clone(), vault(), secret(), terms));
    pick_flow_routes(pool.clone(), clock, keys)
}

/// Posts `pairs` to `/q/<token><tail>` as the browser would; returns the headers and body text.
async fn post(app: &Router, token: &str, tail: &str, pairs: &[(&str, &str)]) -> String {
    let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut body = format!("csrf={csrf}");
    for (k, v) in pairs {
        body.push_str(&format!("&{k}={v}"));
    }
    let request = Request::post(format!("/q/{token}{tail}"))
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let headers = format!("{:?}", response.headers());
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    format!(
        "{headers}\n{}",
        String::from_utf8(bytes.to_vec()).expect("utf8")
    )
}

/// A homeowner texts in; her plumbing job reaches Quotes ready with one quote and a token.
async fn ready(db: &mut PgConnection) -> (Uuid, String) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    one_quote(db, job).await;
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    (job, token.as_str().to_owned())
}

/// One confirmed quote on `job` from a fake business (fake wording).
async fn one_quote(db: &mut PgConnection, job: Uuid) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording', false, true, \
         now())",
    )
    .bind(job)
    .execute(db)
    .await
    .expect("quote");
}

async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

fn no_digits(text: &str) {
    for digits in DIGITS {
        assert!(!text.contains(digits), "the number leaked: {digits}");
    }
}

/// TD-PICK-46 (PICK-013, PRIV-003): the code goes to her vaulted phone only, each read is one
/// `vault_read.picks.pick_code` audit row, and the digits reach no body, header, log line, alert
/// or audit value; the pick is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_46_the_code_goes_to_her_phone_and_her_digits_go_nowhere(pool: PgPool) {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let verify = FakeVerify::default();
    let app = routes(&pool, Some(&verify));
    let reads = "SELECT count(*) FROM ops.audit_log WHERE action = 'vault_read.picks.pick_code'";
    let sent = post(&app, &token, "/pick", &[("quote", "1")]).await;
    assert!(
        sent.contains("We texted a 6-digit code to your phone."),
        "{sent}"
    );
    assert_eq!(*verify.sent.lock().expect("lock"), vec![PHONE.to_owned()]);
    assert_eq!(count(&mut db, reads).await, 1);
    let form = [
        ("quote", "1"),
        ("code", CODE),
        ("reach", "call"),
        ("send", "pick"),
    ];
    let done = post(&app, &token, "/code", &form).await;
    assert!(done.contains("Done. The business has your pick."), "{done}");
    assert_eq!(count(&mut db, reads).await, 2);
    let picked = "SELECT count(*) FROM picks.pick";
    assert_eq!(count(&mut db, picked).await, 1, "{job}");
    let audit: String =
        sqlx::query_scalar("SELECT coalesce(string_agg(a::text, ' '), '') FROM ops.audit_log a")
            .fetch_one(&mut *db)
            .await
            .expect("audit");
    let alerts: String =
        sqlx::query_scalar("SELECT coalesce(string_agg(a::text, ' '), '') FROM ops.alert a")
            .fetch_one(&mut *db)
            .await
            .expect("alerts");
    let logged = String::from_utf8(lines.0.lock().expect("lock").clone()).expect("utf8");
    for text in [&sent, &done, &audit, &alerts, &logged] {
        no_digits(text);
    }
}

/// TD-PICK-46 (CON-029, QH-003): a job paused by the homeowner, or a number on the suppression
/// list, gets no code and a line saying nothing was picked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_46_a_paused_job_or_an_opted_out_number_gets_no_code(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let verify = FakeVerify::default();
    let app = routes(&pool, Some(&verify));
    sqlx::query(
        "INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at) \
         SELECT id, $1, gen_random_uuid(), now() FROM intake.conversation",
    )
    .bind(job)
    .execute(&mut *db)
    .await
    .expect("fixture: pause");
    let paused = post(&app, &token, "/pick", &[("quote", "1")]).await;
    assert!(paused.contains(NO_CODE), "{paused}");
    assert!(!paused.contains("Finish my pick"), "no form");
    sqlx::query("UPDATE intake.homeowner_pause SET ended_at = now()")
        .execute(&mut *db)
        .await
        .expect("fixture: end the pause");
    let channel = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), PHONE, channel, ts(DAY))
        .await
        .expect("suppress");
    let opted = post(&app, &token, "/pick", &[("quote", "1")]).await;
    assert!(opted.contains(NO_CODE), "{opted}");
    assert!(verify.sent.lock().expect("lock").is_empty(), "no code");
    assert_eq!(count(&mut db, "SELECT count(*) FROM picks.pick").await, 0);
}

/// TD-PICK-46 (PICK-055): five codes to her number in a day, then the cap line and no sixth.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_46_the_daily_cap_holds_on_the_page(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, token) = ready(&mut db).await;
    let verify = FakeVerify::default();
    let app = routes(&pool, Some(&verify));
    post(&app, &token, "/pick", &[("quote", "1")]).await;
    let again = [("quote", "1"), ("reach", "call"), ("send", "new")];
    for _ in 0..4 {
        let body = post(&app, &token, "/code", &again).await;
        assert!(body.contains("We texted a 6-digit code"), "{body}");
    }
    let capped = post(&app, &token, "/code", &again).await;
    assert!(
        capped.contains("We&#39;ve sent as many codes as we can today."),
        "{capped}"
    );
    assert_eq!(verify.sent.lock().expect("lock").len(), 5);
}

/// TD-PICK-46 (D-1264, D-1516): without the keys the page sends nothing, reads no phone and keeps
/// today's "Nothing was picked" lines word for word.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_46_without_keys_nothing_is_sent_or_read(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, token) = ready(&mut db).await;
    let app = routes(&pool, None);
    let review = post(&app, &token, "/pick", &[("quote", "1")]).await;
    assert!(
        review.contains(
            "Nothing was picked. Picking on this page isn't open yet, so no code was sent."
        )
    );
    let code = post(&app, &token, "/code", &[("quote", "1")]).await;
    assert!(code.contains(
        "Nothing was picked, and no code was sent. Picking on this page isn&#39;t open yet."
    ));
    let reads = "SELECT count(*) FROM ops.audit_log WHERE action LIKE 'vault_read.picks.%'";
    assert_eq!(count(&mut db, reads).await, 0);
    assert_eq!(count(&mut db, "SELECT count(*) FROM picks.pick").await, 0);
}

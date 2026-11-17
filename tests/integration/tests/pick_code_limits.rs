//! TD-SEC-63 (SEC-014, SEC-020, D-2634, D-3144, T363): the served pick's code step spends the
//! per-IP code limits. Pick code sends from one client IP across different jobs stop at the
//! eleventh, which shows the daily-cap line and sends nothing; shaped code tries past the per-IP
//! try limit are refused before Verify is asked; another client IP is unaffected. The client IP is
//! the edge header the default `ClientIpPolicy` reads (`x-real-ip`). Fake numbers and words only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{ClientIpPolicy, IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_portals::api::{PRE_SESSION_COOKIE, PickCodeKeys, pick_flow_routes};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const CODE: &str = "123456";
const WRONG: &str = "654321";
const COOKIE: &str = "fake-pre-session-cookie-value-0363";
const DAY: &str = "2026-11-02T17:00:00Z";
const TERMS_TEXT: &str = "Sample homeowner terms for tests only.";
const SENT: &str = "We texted a 6-digit code to your phone.";
const CAPPED: &str = "as many codes as we can today";
const DONE: &str = "Done. The business has your pick.";
const IP_A: &str = "203.0.113.63";
const IP_B: &str = "198.51.100.63";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x46), &key(0x56), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-363".to_owned())
}

/// A Verify stand-in: counts sends and checks, and passes only [`CODE`].
#[derive(Clone, Default)]
struct FakeVerify {
    sent: Arc<Mutex<usize>>,
    checked: Arc<Mutex<usize>>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        *self.sent.lock().expect("lock") += 1;
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        *self.checked.lock().expect("lock") += 1;
        Ok(code == CODE)
    }
}

/// The pick routes with keys over one Verify stand-in, the client IP from the default policy.
struct Site {
    app: Router,
    verify: FakeVerify,
}

impl Site {
    fn new(pool: &PgPool) -> Self {
        let verify = FakeVerify::default();
        let terms = ("2026-11-01".to_owned(), TERMS_TEXT);
        let policy = ClientIpPolicy::from_pairs(std::iter::empty()).expect("policy");
        let keys = PickCodeKeys::new(verify.clone(), vault(), secret(), terms);
        let clock = Arc::new(ManualClock::new(ts(DAY)));
        let app = pick_flow_routes(pool.clone(), clock, Some(keys.with_client_ip(policy)));
        Self { app, verify }
    }

    fn sends(&self) -> usize {
        *self.verify.sent.lock().expect("lock")
    }

    fn checks(&self) -> usize {
        *self.verify.checked.lock().expect("lock")
    }

    /// Posts `pairs` to `/q/<token><tail>` from client `ip`; returns status and body.
    async fn post(&self, ip: &str, path: String, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut body = format!("csrf={csrf}");
        for (k, v) in pairs {
            body.push_str(&format!("&{k}={v}"));
        }
        let request = Request::post(path)
            .header(header::HOST, "askhava.test")
            .header(header::ORIGIN, "https://askhava.test")
            .header("sec-fetch-site", "same-origin")
            .header("x-real-ip", ip)
            .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
    }

    /// The final Pick of quote 1 behind `token` from `ip`: the page body.
    async fn pick(&self, ip: &str, token: &str) -> String {
        let form = [("quote", "1"), ("share", "yes")];
        let (status, body) = self.post(ip, format!("/q/{token}/pick"), &form).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// "Finish my pick" on quote 1 behind `token` from `ip` with `code`: the page body.
    async fn finish(&self, ip: &str, token: &str, code: &str) -> String {
        let form = [
            ("quote", "1"),
            ("code", code),
            ("reach", "call"),
            ("send", "pick"),
        ];
        let (status, body) = self.post(ip, format!("/q/{token}/code"), &form).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

/// Homeowner `n` (fake number) texts in and gets a plumbing job in Quotes ready with one confirmed
/// quote; returns its quote-page token.
async fn ready(db: &mut PgConnection, n: u16) -> String {
    let phone = format!("+1928555{:04}", 100 + n);
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone)),
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
    open_with_quote(db, job).await;
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    token.as_str().to_owned()
}

/// Records `job`'s facts, walks it to Quotes ready and stores one confirmed fake quote.
async fn open_with_quote(db: &mut PgConnection, job: Uuid) {
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0xb363))
    .bind(ts(DAY).to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

/// `count` jobs, one fake homeowner each: their tokens.
async fn jobs(pool: &PgPool, count: u16) -> Vec<String> {
    let mut db = pool.acquire().await.expect("db");
    let mut tokens = Vec::new();
    for n in 0..count {
        tokens.push(ready(&mut db, n).await);
    }
    tokens
}

/// TD-SEC-63 (SEC-014, D-2634, D-3144): 11 pick code sends from one IP across 11 jobs give
/// exactly one refusal, the eleventh, with the daily-cap line and no eleventh Verify send; another
/// IP still gets its code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_63_eleven_sends_from_one_ip_give_one_refusal(pool: PgPool) {
    let tokens = jobs(&pool, 12).await;
    let site = Site::new(&pool);
    let mut refused = 0;
    for token in &tokens[..11] {
        let body = site.pick(IP_A, token).await;
        if body.contains(CAPPED) {
            refused += 1;
            assert!(!body.contains(SENT), "{body}");
        } else {
            assert!(body.contains(SENT), "{body}");
        }
    }
    assert_eq!(refused, 1, "exactly one refusal");
    assert_eq!(site.sends(), 10, "no eleventh send");
    let refused_last = site.pick(IP_A, &tokens[10]).await;
    assert!(
        refused_last.contains(CAPPED),
        "still refused: {refused_last}"
    );
    assert_eq!(site.sends(), 10, "a refused send never reaches Verify");
    let other = site.pick(IP_B, &tokens[11]).await;
    assert!(other.contains(SENT), "{other}");
    assert_eq!(site.sends(), 11, "another IP is unaffected");
}

/// TD-SEC-63 (SEC-014, SEC-020, D-2634): 20 shaped code tries from one IP reach Verify; the 21st
/// is refused before Verify with the daily-cap line and nothing picked; another IP's right code
/// on that same job finishes the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_63_tries_over_the_ip_limit_are_refused_before_verify(pool: PgPool) {
    let tokens = jobs(&pool, 6).await;
    let site = Site::new(&pool);
    for token in &tokens {
        assert!(site.pick(IP_A, token).await.contains(SENT));
    }
    for token in &tokens[..5] {
        for _ in 0..4 {
            let body = site.finish(IP_A, token, WRONG).await;
            assert!(body.contains("That code didn"), "{body}");
        }
    }
    assert_eq!(
        site.checks(),
        20,
        "every try under the limit reached Verify"
    );
    let refused = site.finish(IP_A, &tokens[5], CODE).await;
    assert!(refused.contains(CAPPED), "{refused}");
    assert!(!refused.contains(DONE), "nothing picked: {refused}");
    assert_eq!(site.checks(), 20, "the refused try never reached Verify");
    let other = site.finish(IP_B, &tokens[5], CODE).await;
    assert!(other.contains(DONE), "another IP is unaffected: {other}");
    assert_eq!(site.checks(), 21);
}

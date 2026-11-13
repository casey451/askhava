//! TD-PICK-52 page-pick cases (T266; PICK-018, PICK-042, PRIV-001, PRIV-003; D-1264, D-1761):
//! the homeowner's contact choice and shared email are stored with the page pick, through the
//! served pick routes against PostgreSQL. Fake numbers (555-01xx), addresses, IDs and codes only.
//!
//! A request with its consent text and a plumbing job in Quotes ready with one quote (its facts
//! recorded, D-3118); `portals`' pick routes run with the page pick's keys over a Verify adapter
//! that passes only [`CODE`]. Each case posts the final Pick and "Finish my pick" as a browser
//! would and reads `picks.contact_choice`, the vault contact and the `shared_at_pick` audit rows.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_ops::api::{Actor, export_audit};
use askhava_picks::api::{ContactChoice, contact_choice};
use askhava_portals::api::{PRE_SESSION_COOKIE, PickCodeKeys, pick_flow_routes};
use askhava_privacy::api::{
    Field, NewContact, Purpose, Reader, SHARED_AT_PICK, Vault, read_contact, store_contact,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};
use tower::ServiceExt;
use tracing::field::{Field as LogField, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const CODE: &str = "123456";
const COOKIE: &str = "fake-pre-session-cookie-value-0266";
const DAY: &str = "2026-11-02T17:00:00Z";
const EMAIL: &str = "testa.t266@example.com";
const DONE: &str = "Done. The business has your pick.";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x66), &key(0x76), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-266".to_owned())
}

/// A Verify stand-in that passes only [`CODE`].
#[derive(Clone)]
struct FakeVerify;

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// The pick routes with keys, on a manual clock, for the quote page `token`.
struct Page {
    app: Router,
    token: String,
}

impl Page {
    fn new(pool: &PgPool, token: String) -> Self {
        let terms = ("2026-11-01".to_owned(), "Sample homeowner terms for tests only.");
        let keys = PickCodeKeys::new(FakeVerify, vault(), secret(), terms);
        let clock = Arc::new(ManualClock::new(ts(DAY)));
        let app = pick_flow_routes(pool.clone(), clock, Some(keys));
        Self { app, token }
    }

    async fn post(&self, tail: &str, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut body = format!("csrf={csrf}");
        for (k, v) in pairs {
            body.push_str(&format!("&{k}={}", v.replace('@', "%40")));
        }
        let request = Request::post(format!("/q/{}{tail}", self.token))
            .header(header::HOST, "askhava.test")
            .header(header::ORIGIN, "https://askhava.test")
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
    }

    /// The final Pick of quote 1, then "Finish my pick" with `code` and the choice `reach`.
    async fn finish(&self, code: &str, reach: &str) -> (StatusCode, String) {
        let (status, body) = self.post("/pick", &[("quote", "1"), ("share", "yes")]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let mut form = vec![("quote", "1"), ("code", code), ("reach", reach)];
        if reach == "email" {
            form.push(("email", EMAIL));
        }
        form.push(("send", "pick"));
        self.post("/code", &form).await
    }
}

/// A homeowner texts in; her plumbing job reaches Quotes ready with one confirmed quote and a
/// quote-page token. Returns her contact, the job and the token.
async fn ready(db: &mut PgConnection) -> (Uuid, Uuid, String) {
    let new = NewContact {
        phone: Some(SecretString::from("+19285550166".to_owned())),
        ..NewContact::default()
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
    .bind(Uuid::from_u128(0xb266))
    .bind(ts(DAY).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    (her, job, token.as_str().to_owned())
}

/// The job's stored contact choices: pick and word.
async fn choices(db: &mut PgConnection, job: Uuid) -> Vec<(Uuid, String)> {
    sqlx::query_as(
        "SELECT c.pick_id, c.choice FROM picks.contact_choice c \
         JOIN picks.pick p ON p.id = c.pick_id WHERE p.job_id = $1",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("choices")
}

/// Her vault email, read as `messaging` would, and the `shared_at_pick` audit rows.
async fn vaulted(db: &mut PgConnection, her: Uuid) -> (Option<String>, Vec<(String, String)>) {
    let gate = (Reader::Messaging, Purpose::Send);
    let read = read_contact(db, &vault(), gate, her, &[Field::Email]).await;
    let email = read.expect("read").email;
    let rows = export_audit(db, 0, 10_000).await.expect("audit");
    let shared = rows
        .into_iter()
        .filter(|r| r.action == SHARED_AT_PICK)
        .map(|r| (r.actor_kind, r.field.unwrap_or_default()))
        .collect();
    (email.map(|e| e.expose_secret().to_owned()), shared)
}

/// Every table outside `privacy` whose rows, as text, hold [`EMAIL`].
async fn tables_with_the_email(pool: &PgPool) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT format('%I.%I', schemaname, tablename) FROM pg_tables \
         WHERE schemaname NOT IN ('pg_catalog', 'information_schema', 'privacy')",
    )
    .fetch_all(pool)
    .await
    .expect("tables");
    let mut found = Vec::new();
    for table in tables {
        let sql = format!("SELECT count(*) FROM {table} t WHERE strpos(t::text, $1) > 0");
        let hits: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(EMAIL)
            .fetch_one(pool)
            .await
            .expect("scan");
        if hits > 0 {
            found.push(table);
        }
    }
    found
}

/// TD-PICK-52 (PICK-018, PICK-042, PRIV-001, PRIV-003): choosing email stores `email` for the
/// pick and the address only in her vault contact (sealed, one audit row naming her), and
/// `picks::api::contact_choice` answers email; the address is in no table outside the vault, no
/// task payload and no log line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_52_an_email_choice_is_stored_and_sealed_in_her_vault_only(pool: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let mut db = pool.acquire().await.expect("db");
    let (her, job, token) = ready(&mut db).await;
    let (status, body) = Page::new(&pool, token).finish(CODE, "email").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(DONE), "{body}");
    let stored = choices(&mut db, job).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].1, "email");
    let choice = contact_choice(&mut db, stored[0].0).await.expect("choice");
    assert_eq!(choice, ContactChoice::Email);
    let (email, shared) = vaulted(&mut db, her).await;
    assert_eq!(email.as_deref(), Some(EMAIL));
    assert_eq!(shared, [("homeowner".to_owned(), "email".to_owned())]);
    assert!(tables_with_the_email(&pool).await.is_empty());
    let logged = lines.0.lock().expect("lines").clone();
    assert!(logged.iter().all(|l| !l.contains("t266@")), "{logged:?}");
}

/// TD-PICK-52 (PICK-018): choosing a call stores `call` and no email; a pick with no stored choice
/// (a text pick) answers a call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_52_a_call_choice_stores_call_and_no_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, job, token) = ready(&mut db).await;
    let (status, body) = Page::new(&pool, token).finish(CODE, "call").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(DONE), "{body}");
    let stored = choices(&mut db, job).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].1, "call");
    let choice = contact_choice(&mut db, stored[0].0).await.expect("choice");
    assert!(!choice.email_chosen());
    assert_eq!(vaulted(&mut db, her).await, (None, Vec::new()));
    let none = contact_choice(&mut db, Uuid::from_u128(0x5266)).await;
    assert_eq!(none.expect("no row"), ContactChoice::Call);
}

/// TD-PICK-52 (PICK-018, A11Y-012): a wrong code keeps both choices on the page and stores
/// nothing; a pick rolled back (its terms acceptance refused) stores neither the choice nor the
/// email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_52_a_code_error_or_a_rollback_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, job, token) = ready(&mut db).await;
    let page = Page::new(&pool, token);
    let (_, wrong) = page.finish("654321", "email").await;
    assert!(wrong.contains("value=\"email\" checked"), "{wrong}");
    assert!(wrong.contains(EMAIL), "{wrong}");
    sqlx::raw_sql(
        "CREATE FUNCTION pg_temp.fail() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'fixture: refused'; END $$; \
         CREATE TRIGGER fail_once BEFORE INSERT ON intake.terms_acceptance \
         FOR EACH ROW EXECUTE FUNCTION pg_temp.fail();",
    )
    .execute(&mut *db)
    .await
    .expect("fixture trigger");
    let (status, _) = page.finish(CODE, "email").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(choices(&mut db, job).await.is_empty());
    assert_eq!(vaulted(&mut db, her).await, (None, Vec::new()));
}

/// Every log line's message and fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &LogField, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Lines {
    fn push(&self, line: String) {
        self.0.lock().expect("lines").push(line);
    }
}

impl Subscriber for Lines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

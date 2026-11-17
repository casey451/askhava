//! TD-PICK-64 (T441; PICK-008, PRIV-005, D-2632, D-3497, D-3878, D-3879): the page half of
//! TD-PICK-60 on a fresh database. Fake numbers (555-01xx), IDs, codes and words only.
//!
//! Business A's quote arrives first and B's second, but B's is confirmed (shown) first, so B keeps
//! shown number 1 and A number 2 while their arrival places are the other way round. The quote page
//! numbers by the shown number and every pick form carries the business that number was shown for;
//! a form naming another business stores, notifies and charges nothing.

use std::sync::Arc;

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_picks::api::PICK_MADE_KIND;
use askhava_portals::api::{PRE_SESSION_COOKIE, PickCodeKeys, pick_flow_routes, quote_page_routes};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const PHONE: &str = "+19285550164";
const CODE: &str = "123456";
const COOKIE: &str = "fake-pre-session-cookie-value-0441";
const DAY: &str = "2026-11-02T17:00:00Z";
const A: Uuid = Uuid::from_u128(0xa441);
const B: Uuid = Uuid::from_u128(0xb441);

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x41), &key(0x44), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-441".to_owned())
}

/// A Verify stand-in that passes only [`CODE`].
#[derive(Clone)]
struct FakeVerify;

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// The quote page and the pick routes with keys, behind link `token`.
struct Page {
    app: Router,
    token: String,
}

impl Page {
    fn new(pool: &PgPool, token: String) -> Self {
        let keys = PickCodeKeys::new(FakeVerify, vault(), secret(), ("2026-11-01".into(), "x"));
        let clock = Arc::new(ManualClock::new(ts(DAY)));
        let app = pick_flow_routes(pool.clone(), clock.clone(), Some(keys))
            .merge(quote_page_routes(pool.clone(), clock));
        Self { app, token }
    }

    /// Sends `pairs` to `/q/<token><tail>` as the browser would (GET when `pairs` is empty).
    async fn send(&self, tail: &str, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut body = format!("csrf={csrf}");
        for (k, v) in pairs {
            body.push_str(&format!("&{k}={v}"));
        }
        let uri = format!("/q/{}{tail}", self.token);
        let request = if pairs.is_empty() {
            Request::get(uri)
        } else {
            Request::post(uri)
        };
        let request = request
            .header(header::HOST, "askhava.test")
            .header(header::ORIGIN, "https://askhava.test")
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(if pairs.is_empty() {
                Body::empty()
            } else {
                Body::from(body)
            })
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
    }

    /// The final Pick then "Finish my pick" of shown number `n`, the forms carrying `business`.
    async fn pick(&self, n: &str, business: &str) -> (StatusCode, String) {
        let (_, code_step) = self
            .send(
                "/pick",
                &[("quote", n), ("share", "yes"), ("business", business)],
            )
            .await;
        let form = [
            ("quote", n),
            ("code", CODE),
            ("reach", "call"),
            ("send", "pick"),
            ("business", business),
        ];
        let (status, done) = self.send("/code", &form).await;
        (status, format!("{code_step}\n{done}"))
    }
}

/// A plumbing job in Quotes ready with A's quote first in, B's second, B's shown first.
async fn ready(db: &mut PgConnection) -> (Uuid, String) {
    let phone = Some(SecretString::from(PHONE.to_owned()));
    let new = NewContact {
        name: None,
        phone,
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
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    quotes_ready(db, job).await;
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    (job, token.as_str().to_owned())
}

/// Moves `job` to Quotes ready; A's quote arrives first, B's second, and B's is shown first.
async fn quotes_ready(db: &mut PgConnection, job: Uuid) {
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    for (business, mins) in [(A, 1), (B, 2)] {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, false, $3)",
        )
        .bind(job)
        .bind(business)
        .bind((ts(DAY) + SignedDuration::from_mins(mins)).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    for business in [B, A] {
        sqlx::query(
            "UPDATE quotes.quote SET confirmed = true WHERE job_id = $1 AND business_id = $2",
        )
        .bind(job)
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("confirm");
    }
}

/// The job's picks (business, status), the pick notices queued and the fees stored.
async fn stored(db: &mut PgConnection, job: Uuid) -> (Vec<(Uuid, String)>, i64, i64) {
    let picks = sqlx::query_as("SELECT business_id, status FROM picks.pick WHERE job_id = $1")
        .bind(job)
        .fetch_all(&mut *db)
        .await
        .expect("picks");
    let count = |sql: &'static str| sqlx::query_scalar::<_, i64>(sql);
    let notices = count("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(PICK_MADE_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    let fees = count(
        "SELECT (SELECT count(*) FROM payments.pick_fee) + \
         (SELECT count(*) FROM picks.pick WHERE status IN ('paid', 'free'))",
    )
    .fetch_one(&mut *db)
    .await
    .expect("fees");
    (picks, notices, fees)
}

/// TD-PICK-64 (PICK-008, D-2632): the page numbers each quote by its kept shown number, its Pick
/// form carrying the business; a withdrawn quote keeps its place and number as "Withdrawn by the
/// business", with no Pick form.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_64_the_page_shows_the_kept_number(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let page = Page::new(&pool, token);
    let (_, shown) = page.send("", &[]).await;
    let one = shown.find("id=\"quote-1\"").expect("#1");
    let two = shown.find("id=\"quote-2\"").expect("#2");
    let carried = |b: Uuid| {
        shown
            .find(&format!("name=\"business\" value=\"{b}\""))
            .expect("carried")
    };
    assert!(
        one < carried(B) && carried(B) < two && two < carried(A),
        "#1 is B, #2 is A"
    );
    sqlx::query(
        "UPDATE quotes.quote SET withdrawn_at = now() WHERE job_id = $1 AND business_id = $2",
    )
    .bind(job)
    .bind(B)
    .execute(&mut *db)
    .await
    .expect("withdraw");
    let (_, shown) = page.send("", &[]).await;
    let one = shown.find("id=\"quote-1\"").expect("#1 keeps its place");
    let gone = shown
        .find("Withdrawn by the business.")
        .expect("withdrawn line");
    let two = shown.find("id=\"quote-2\"").expect("#2");
    assert!(one < gone && gone < two);
    assert!(
        !shown.contains(&format!("value=\"{B}\"")),
        "no Pick form for the withdrawn quote"
    );
    assert!(shown.contains(&format!("value=\"{A}\"")), "A stays #2");
}

/// TD-PICK-64 (PICK-008, PRIV-005, D-2632): the review and code forms carry the business number 1
/// was shown for, and finishing number 1 picks B, never A, the first to arrive.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_64_the_forms_carry_the_business_and_pick_it(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let page = Page::new(&pool, token);
    let carried = format!("name=\"business\" value=\"{B}\"");
    let (_, review) = page.send("", &[("quote", "1")]).await;
    assert!(review.contains(&carried), "{review}");
    let (status, body) = page.pick("1", &B.to_string()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&carried), "the code form carries B");
    assert!(body.contains("Done. The business has your pick."), "{body}");
    let (picks, notices, _) = stored(&mut db, job).await;
    assert_eq!(picks, vec![(B, "awaiting_confirm".to_owned())]);
    assert_eq!(notices, 1);
}

/// TD-PICK-64 (PICK-008, PRIV-005, D-3879): forms whose number no longer maps to the business they
/// carry store, notify and charge nothing; a post with no business resolves the number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_64_a_form_for_another_business_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let page = Page::new(&pool, token);
    for posted in [A.to_string(), "not-a-business".to_owned()] {
        let (status, _) = page.pick("1", &posted).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "back to the card");
        assert_eq!(stored(&mut db, job).await, (Vec::new(), 0, 0));
    }
    let (status, body) = page.pick("1", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Done. The business has your pick."), "{body}");
    assert_eq!(
        stored(&mut db, job).await.0,
        vec![(B, "awaiting_confirm".to_owned())]
    );
}

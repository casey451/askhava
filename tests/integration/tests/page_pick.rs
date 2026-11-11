//! The quote page's pick end to end on a fresh database (TD-PICK-45, TD-PICK-47, TD-PICK-30,
//! TD-PICK-14 on the page; PICK-012, PICK-013, PICK-015, PICK-016, PICK-032, PICK-045, TERMS-016,
//! D-1516). Fake numbers (555-01xx), IDs, codes and words only.
//!
//! A request is opened with its consent text and a job that walks to Quotes ready with two
//! quotes; `portals`' pick routes run with the page pick's keys over a Verify adapter that records
//! each send and passes only [`CODE`] (the HTTP stand-in is `adapters`' own test, D-453, D-1605).
//! Each case posts the final Pick and "Finish my pick" as a browser would, with the pre-session
//! cookie and its CSRF token, and reads the stored picks, events and terms acceptances.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text, terms_acceptances};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{PICK_MADE_KIND, PICK_WITHDRAWN_KIND};
use askhava_portals::api::{PRE_SESSION_COOKIE, PickCodeKeys, pick_flow_routes, quote_page_routes};
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

const PHONE: &str = "+19285550151";
const CODE: &str = "123456";
const COOKIE: &str = "fake-pre-session-cookie-value-0001";
const DAY: &str = "2026-11-02T17:00:00Z";
const TERMS_TEXT: &str = "Sample homeowner terms for tests only.";
const DONE: &str = "Done. The business has your pick.";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x46), &key(0x56), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-246".to_owned())
}

/// A Verify stand-in: counts sends and passes only [`CODE`].
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

/// The page under test: the pick routes with keys over `verify`, and the quote page.
struct Page {
    app: Router,
    verify: FakeVerify,
    clock: Arc<ManualClock>,
    token: String,
}

impl Page {
    fn new(pool: &PgPool, token: String) -> Self {
        let verify = FakeVerify::default();
        let terms = ("2026-11-01".to_owned(), TERMS_TEXT);
        let keys = PickCodeKeys::new(verify.clone(), vault(), secret(), terms);
        let clock = Arc::new(ManualClock::new(ts(DAY)));
        let app = pick_flow_routes(pool.clone(), clock.clone(), Some(keys))
            .merge(quote_page_routes(pool.clone(), clock.clone()));
        Self {
            app,
            verify,
            clock,
            token,
        }
    }

    fn sends(&self) -> usize {
        self.verify.sent.lock().expect("lock").len()
    }

    /// Posts `pairs` to `/q/<token><tail>` as the browser would; returns status and body.
    async fn post(&self, tail: &str, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut body = format!("csrf={csrf}");
        for (k, v) in pairs {
            body.push_str(&format!("&{k}={v}"));
        }
        let request = Request::post(format!("/q/{}{tail}", self.token))
            .header(header::HOST, "askhava.test")
            .header(header::ORIGIN, "https://askhava.test")
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("request");
        read(self.app.clone().oneshot(request).await.expect("response")).await
    }

    /// The final Pick of quote `n` (the code step).
    async fn pick(&self, n: &str) -> String {
        self.clock.advance(jiff::SignedDuration::from_secs(1));
        let (status, body) = self.post("/pick", &[("quote", n), ("share", "yes")]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// "Finish my pick" on quote `n` with `code` and a call as the contact choice.
    async fn finish(&self, n: &str, code: &str) -> (StatusCode, String) {
        let form = [
            ("quote", n),
            ("code", code),
            ("reach", "call"),
            ("send", "pick"),
        ];
        self.post("/code", &form).await
    }

    /// The quote page as she sees it.
    async fn quote_page(&self) -> String {
        let request = Request::get(format!("/q/{}", self.token))
            .header(header::HOST, "askhava.test")
            .body(Body::empty())
            .expect("request");
        read(self.app.clone().oneshot(request).await.expect("response"))
            .await
            .1
    }
}

async fn read(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

/// A homeowner texts in, gets a plumbing job in Quotes ready with `quotes` confirmed quotes and a
/// quote-page token; returns the request, the job and the token.
async fn ready(db: &mut PgConnection, quotes: u128) -> (Uuid, Uuid, String) {
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
    for n in 1..=quotes {
        quote(db, job, n).await;
    }
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    (conversation, job, token.as_str().to_owned())
}

/// Business `n` quotes on `job`, confirmed, arriving `n` minutes in (fake wording).
async fn quote(db: &mut PgConnection, job: Uuid, n: u128) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(business(n))
    .bind((ts(DAY) + jiff::SignedDuration::from_mins(i64::try_from(n).expect("small"))).to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0xb246 + n)
}

/// The job's picks: business, quote number, channel, inbox ID set, status, end reason.
async fn picks(
    db: &mut PgConnection,
    job: Uuid,
) -> Vec<(Uuid, i32, String, bool, String, Option<String>)> {
    sqlx::query_as(
        "SELECT business_id, quote_number, channel, inbox_id IS NOT NULL, status, end_reason \
         FROM picks.pick WHERE job_id = $1 ORDER BY picked_at, id",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("picks")
}

async fn tasks(db: &mut PgConnection, kind: &str) -> usize {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(kind)
        .fetch_one(db)
        .await
        .map(|n| usize::try_from(n).expect("count"))
        .expect("tasks")
}

async fn status(db: &mut PgConnection, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

/// Picks quote `n` on the page with the right code; asserts the result page.
async fn page_pick(page: &Page, n: &str) {
    let body = page.pick(n).await;
    assert!(
        body.contains("We texted a 6-digit code to your phone."),
        "{body}"
    );
    let (code, body) = page.finish(n, CODE).await;
    assert_eq!(code, StatusCode::OK);
    assert!(body.contains(DONE), "{body}");
}

/// TD-PICK-45, TD-PICK-14 on the page (PICK-012, PICK-032, D-147, D-1516): a page pick stores one
/// pick with channel `page` and no inbox and moves the job to Awaiting confirm; the quote page
/// still shows Pick on the other available quote; a re-pick before Confirm ends the first as
/// `replaced` with its withdrawal notice, stores the new one, and charges, frees and releases
/// nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_45_a_page_repick_replaces_the_waiting_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, job, token) = ready(&mut db, 2).await;
    let page = Page::new(&pool, token);
    page_pick(&page, "1").await;
    let first = picks(&mut db, job).await;
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].0, first[0].1), (business(1), 1));
    assert_eq!((first[0].2.as_str(), first[0].3), ("page", false));
    assert_eq!(status(&mut db, job).await, "awaiting_confirm");
    let shown = page.quote_page().await;
    assert!(
        shown.contains("Pick #2</button>"),
        "PICK-032: Pick on the other quote"
    );
    page_pick(&page, "2").await;
    let both = picks(&mut db, job).await;
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].4, "cancelled");
    assert_eq!(both[0].5.as_deref(), Some("replaced"));
    assert_eq!(
        (both[1].0, both[1].4.as_str()),
        (business(2), "awaiting_confirm")
    );
    assert_eq!((both[1].2.as_str(), both[1].3), ("page", false));
    assert_eq!(tasks(&mut db, PICK_WITHDRAWN_KIND).await, 1);
    assert_eq!(tasks(&mut db, PICK_MADE_KIND).await, 2);
    assert_eq!(status(&mut db, job).await, "awaiting_confirm");
    let irreversible: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM picks.release) + \
         (SELECT count(*) FROM picks.pick WHERE status IN ('paid', 'free'))",
    )
    .fetch_one(&mut *db)
    .await
    .expect("count");
    assert_eq!(
        irreversible, 0,
        "no charge, free win or release before Confirm"
    );
}

/// TD-PICK-45 (PICK-032, D-1516): from Payment pending on the final Pick stores nothing, sends no
/// code and shows the confirmed line, and the quote page shows no Pick button.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_45_after_the_business_confirmed_nothing_is_picked(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, job, token) = ready(&mut db, 2).await;
    let page = Page::new(&pool, token);
    page_pick(&page, "1").await;
    let paying = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let moved = move_to(&mut db, job, paying, ts(DAY), Actor::System).await;
    assert!(moved.expect("move").applied().is_some());
    let sends = page.sends();
    let body = page.pick("2").await;
    assert!(body.contains("that business has accepted it"), "{body}");
    assert_eq!(page.sends(), sends, "no code");
    assert_eq!(picks(&mut db, job).await.len(), 1);
    assert!(!page.quote_page().await.contains("Pick #"));
}

/// TD-PICK-47 (PICK-013, PICK-015, PICK-016): picking the waiting quote again sends no code and
/// stores nothing new; a wrong code shows the tries left and stores nothing; a malformed one
/// counts no try.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_47_same_quote_wrong_and_malformed_codes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, job, token) = ready(&mut db, 2).await;
    let page = Page::new(&pool, token);
    page_pick(&page, "1").await;
    let sends = page.sends();
    assert!(page.pick("1").await.contains(DONE), "the result page");
    assert_eq!(page.sends(), sends, "no code for the same quote");
    assert_eq!(picks(&mut db, job).await.len(), 1);
    page.pick("2").await;
    let (_, wrong) = page.finish("2", "654321").await;
    assert!(wrong.contains("You have 4 tries left."), "{wrong}");
    let (_, malformed) = page.finish("2", "12ab").await;
    assert!(
        malformed.contains("The code must be 6 numbers"),
        "{malformed}"
    );
    let tries: i32 = sqlx::query_scalar(
        "SELECT tries FROM picks.code_attempt WHERE job_id = $1 ORDER BY sent_at DESC LIMIT 1",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .expect("tries");
    assert_eq!(tries, 1, "a malformed code counts no try");
    assert_eq!(picks(&mut db, job).await.len(), 1, "nothing new stored");
}

/// CON-029, D-1602: a CANCEL pause committing while "Finish my pick" stores is waited for under
/// the homeowner lock and seen, so no pick lands on the paused job. The CANCEL's transaction is
/// played by hand: it holds the homeowner row and writes the pause, and commits only after the
/// finish has started.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_029_a_cancel_committing_during_the_store_blocks_the_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (conversation, job, token) = ready(&mut db, 2).await;
    let page = Arc::new(Page::new(&pool, token));
    page.pick("1").await;
    let mut cancel = pool.begin().await.expect("cancel tx");
    sqlx::query(
        "SELECT h.id FROM intake.homeowner h JOIN intake.conversation c \
         ON c.homeowner_id = h.id WHERE c.id = $1 FOR UPDATE OF h",
    )
    .bind(conversation)
    .fetch_one(&mut *cancel)
    .await
    .expect("homeowner lock");
    sqlx::query(
        "INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(conversation)
    .bind(job)
    .bind(Uuid::new_v4())
    .bind(ts(DAY).to_sqlx())
    .execute(&mut *cancel)
    .await
    .expect("pause");
    let finishing = Arc::clone(&page);
    let finish = tokio::spawn(async move { finishing.finish("1", CODE).await });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !finish.is_finished(),
        "the store waits for the homeowner lock"
    );
    cancel.commit().await.expect("cancel commit");
    let (code, body) = finish.await.expect("finish");
    assert_eq!(code, StatusCode::OK);
    assert!(
        body.contains("so your pick didn&#39;t go through"),
        "{body}"
    );
    assert!(
        picks(&mut db, job).await.is_empty(),
        "no pick on a paused job"
    );
    assert_eq!(status(&mut db, job).await, "quotes_ready");
}

/// TD-PICK-30, TD-PICK-47 (PICK-016, TERMS-016): the right code stores one pick and one terms
/// acceptance (channel `quote_page`, the published version and the request) together; a failed
/// acceptance insert leaves no pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_30_one_acceptance_with_the_pick_or_neither(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (conversation, job, token) = ready(&mut db, 2).await;
    let page = Page::new(&pool, token);
    sqlx::query(
        "CREATE FUNCTION pg_temp_fail() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'fixture: refused'; END $$",
    )
    .execute(&mut *db)
    .await
    .expect("fixture function");
    sqlx::query(
        "CREATE TRIGGER fail_once BEFORE INSERT ON intake.terms_acceptance \
         FOR EACH ROW EXECUTE FUNCTION pg_temp_fail()",
    )
    .execute(&mut *db)
    .await
    .expect("fixture trigger");
    page.pick("1").await;
    let (code, _) = page.finish("1", CODE).await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        picks(&mut db, job).await.is_empty(),
        "no pick without its acceptance"
    );
    assert_eq!(status(&mut db, job).await, "quotes_ready");
    sqlx::query("DROP TRIGGER fail_once ON intake.terms_acceptance")
        .execute(&mut *db)
        .await
        .expect("fixture drop");
    page_pick(&page, "1").await;
    let accepted = terms_acceptances(&mut db, conversation)
        .await
        .expect("rows");
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].channel, "quote_page");
    assert_eq!(accepted[0].version, "2026-11-01");
    assert_eq!(
        accepted[0].assent_line,
        "By picking, you agree to the Terms"
    );
    assert_eq!(picks(&mut db, job).await.len(), 1);
}

//! SEC-014's new-request limit and TXT-031's LIST names through the served inbound text route
//! (TD-SEC-52; SEC-014, SEC-015, TXT-031; D-1310, D-1951, D-2206, D-3083; T317).
//!
//! The route under test is `app/server/src/inbound_text.rs`, compiled into this crate with
//! `#[path]` (a test crate may not depend on `server`), driven through `tower::ServiceExt` with
//! deliveries signed by a fake Twilio token over the fake `ASKHAVA_PUBLIC_BASE_URL`. Proves: a
//! phone's sixth new request within an hour opens nothing, is answered by nothing and raises one
//! Normal alert (the lowest severity `ops` has) with no subject, so no body or phone; Twilio's
//! resend of that text is a repeat and counts once; a delivery rolled back by a database error
//! counts nothing; a reply inside an open request and HELP, STOP and START are never limited; a
//! LIST reply names each quoted business through `businesses::api::public_names`, read at the
//! time of the text, and keeps D-1310's fallback for a business with no name. All data is fake.

use std::sync::Arc;

use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::REQUESTS_PER_PHONE;
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{
    DayPart, Format, Price, QuoteForm, QuoteRules, parse_quote, submit_quote,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HOME: &str = "+19285550142";
const OTHER: &str = "+19285550143";
const ASK_HAVA: &str = "+19285550100";
const LEAK: &str = "my kitchen sink is leaking";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: Vault::new(&key(0x53), &key(0x63), &url).expect("vault"),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Text `words` from `from` as MessageSid number `sid`, signed; the route's status.
async fn text(hooks: &Arc<Webhooks>, sid: u32, from: &str, words: &str) -> StatusCode {
    text_with(hooks, (sid, from, words), &[]).await
}

/// [`text`] with the `extra` form fields Twilio adds (such as `OptOutType`).
async fn text_with(
    hooks: &Arc<Webhooks>,
    (sid, from, words): (u32, &str, &str),
    extra: &[(&'static str, &str)],
) -> StatusCode {
    let sid = format!("SM{sid:032}");
    let mut fields = vec![
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    fields.extend_from_slice(extra);
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(hooks)).oneshot(request).await;
    answer.expect("answer").status()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// The per-phone bucket's hits this hour, summed over every phone (0 with no row).
async fn phone_hits(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(hits), 0)::bigint FROM core.rate_limit WHERE bucket LIKE $1",
    )
    .bind(format!("{}:id:%", REQUESTS_PER_PHONE.scope))
    .fetch_one(pool)
    .await
    .expect("hits")
}

/// Closes every open request, so her next text is a new request.
async fn close_all(pool: &PgPool) {
    sqlx::query("UPDATE intake.conversation SET closed_at = $1 WHERE closed_at IS NULL")
        .bind(at().to_sqlx())
        .execute(pool)
        .await
        .expect("closed");
}

/// Five new requests from `HOME` within the hour (MessageSids 101 to 105), each closed after it
/// opened.
async fn five_requests(pool: &PgPool, h: &Arc<Webhooks>) {
    for sid in 101..=105 {
        assert_eq!(text(h, sid, HOME, LEAK).await, StatusCode::OK);
        close_all(pool).await;
    }
    let requests = "SELECT count(*) FROM intake.conversation";
    assert_eq!(count(pool, requests).await, 5);
}

/// TD-SEC-52 (SEC-014, SEC-015, D-2206): the sixth new request in an hour opens nothing, queues
/// nothing and raises one Normal alert with no subject; its resend is a repeat counted once; the
/// seventh raises no second alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_52_a_sixth_new_request_in_an_hour_is_refused_once(pool: PgPool) {
    let h = hooks(&pool);
    five_requests(&pool, &h).await;
    let queued = count(&pool, "SELECT count(*) FROM messaging.outbox").await;
    assert_eq!(text(&h, 6, HOME, LEAK).await, StatusCode::OK);
    assert_eq!(text(&h, 6, HOME, LEAK).await, StatusCode::OK, "resend");
    assert_eq!(phone_hits(&pool).await, 6, "the resend is a repeat");
    let inbox = "SELECT count(*) FROM messaging.inbox";
    assert_eq!(
        count(&pool, inbox).await,
        6,
        "the refused text keeps its inbox row"
    );
    assert_eq!(
        text(&h, 7, HOME, "the sink is still leaking").await,
        StatusCode::OK
    );
    let requests = "SELECT count(*) FROM intake.conversation";
    assert_eq!(count(&pool, requests).await, 5, "opens nothing");
    let outbox = count(&pool, "SELECT count(*) FROM messaging.outbox").await;
    assert_eq!(outbox, queued, "answered by nothing");
    let alerts: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT severity, subject_id FROM ops.alert WHERE kind = $1")
            .bind(REQUESTS_PER_PHONE.scope)
            .fetch_all(&pool)
            .await
            .expect("alerts");
    assert_eq!(alerts, vec![("normal".to_owned(), None)]);
    assert_eq!(text(&h, 8, OTHER, LEAK).await, StatusCode::OK);
    assert_eq!(
        count(&pool, requests).await,
        6,
        "another phone is not limited"
    );
}

/// TD-SEC-52 (SEC-014, TXT-012, D-2041 (2)): a delivery rolled back by a database error after the
/// count counts nothing; Twilio's resend then counts once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_52_a_rolled_back_delivery_counts_nothing(pool: PgPool) {
    let h = hooks(&pool);
    let fail = "CREATE FUNCTION public.t317_fail() RETURNS trigger LANGUAGE plpgsql AS \
                $$ BEGIN RAISE EXCEPTION 'fake failure'; END $$";
    let trigger = "CREATE TRIGGER t317_fail BEFORE INSERT ON intake.conversation \
                   FOR EACH ROW EXECUTE FUNCTION public.t317_fail()";
    for sql in [fail, trigger] {
        sqlx::query(sql).execute(&pool).await.expect("trigger");
    }
    let failed = text(&h, 1, HOME, LEAK).await;
    assert_eq!(failed, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(phone_hits(&pool).await, 0, "rolled back with the delivery");
    let inbox = "SELECT count(*) FROM messaging.inbox";
    assert_eq!(count(&pool, inbox).await, 0);
    sqlx::query("DROP TRIGGER t317_fail ON intake.conversation")
        .execute(&pool)
        .await
        .expect("drop");
    assert_eq!(text(&h, 1, HOME, LEAK).await, StatusCode::OK);
    assert_eq!(phone_hits(&pool).await, 1);
    let requests = "SELECT count(*) FROM intake.conversation";
    assert_eq!(count(&pool, requests).await, 1);
}

/// TD-SEC-52 (SEC-014, CON-008, CON-018, D-1951): replies inside an open request are never
/// counted, and HELP, STOP and START from a phone at its limit still run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_52_replies_help_stop_and_start_are_never_limited(pool: PgPool) {
    let h = hooks(&pool);
    five_requests(&pool, &h).await;
    let before = count(&pool, "SELECT count(*) FROM messaging.outbox").await;
    assert_eq!(text(&h, 20, HOME, "HELP").await, StatusCode::OK);
    let helped = count(&pool, "SELECT count(*) FROM messaging.outbox").await;
    assert!(helped > before, "the HELP reply is queued");
    let stop = text_with(&h, (21, HOME, "STOP"), &[("OptOutType", "STOP")]).await;
    assert_eq!(stop, StatusCode::OK);
    let listed = "SELECT count(*) FROM messaging.suppression";
    assert_eq!(count(&pool, listed).await, 1, "the opt-out is listed");
    let start = text_with(&h, (22, HOME, "START"), &[("OptOutType", "START")]).await;
    assert_eq!(start, StatusCode::OK);
    assert_eq!(count(&pool, listed).await, 0, "START resubscribes");
    let requests = "SELECT count(*) FROM intake.conversation";
    assert_eq!(
        count(&pool, requests).await,
        6,
        "START's new request is not refused"
    );
    let hits = "HELP, STOP and START count nothing";
    assert_eq!(phone_hits(&pool).await, 5, "{hits}");
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = 'intake.requests_per_phone'";
    assert_eq!(count(&pool, alerts).await, 0);
    assert_eq!(text(&h, 1, OTHER, LEAK).await, StatusCode::OK);
    for sid in 2..=9 {
        let reply = text(&h, sid, OTHER, "it is under the sink").await;
        assert_eq!(reply, StatusCode::OK);
    }
    assert_eq!(phone_hits(&pool).await, 6, "replies are not new requests");
}

/// A request from `HOME` with one plumbing job, moved to Open, holding one shown $190 quote from
/// business `business`.
async fn quoted_job(pool: &PgPool, h: &Arc<Webhooks>, business: Uuid) {
    assert_eq!(text(h, 1, HOME, LEAK).await, StatusCode::OK);
    let mut db = pool.acquire().await.expect("connection");
    let id: Uuid = sqlx::query_scalar("SELECT id FROM intake.conversation")
        .fetch_one(pool)
        .await
        .expect("request");
    let start = askhava_intake::api::JobStart {
        conversation: id,
        words: LEAK,
        emergency: false,
        trade: "plumbing_repair",
        at: at() - jiff::SignedDuration::from_hours(1),
    };
    let opened = askhava_intake::api::begin_job(&mut db, &start).await;
    let Ok(askhava_intake::api::Opened::Created(job)) = opened else {
        panic!("a new job");
    };
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        at(),
        Actor::System,
    );
    assert!(moved.await.expect("open").applied().is_some());
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(business)
        .bind(at().to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    submit_firm_190(&mut db, job, business).await;
}

/// Business `business`'s confirmed $190 firm quote on `job`.
async fn submit_firm_190(db: &mut sqlx::PgConnection, job: Uuid, business: Uuid) {
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: "Sample wording".to_owned(),
        ai_read: false,
        earliest_visit: Some((jiff::civil::date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    let submitted = submit_quote(db, (job, business), &checked, at()).await;
    assert!(submitted.is_ok());
}

/// TD-SEC-52 (TXT-031, PICK-041, D-1310, D-2206): LIST reads the quoted business's public name at
/// the time of the text: with no name stored no list goes (D-1310's fallback, one name alert);
/// once its name is stored the next LIST names it, and the LIST itself is never limited.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_52_list_names_each_quoted_business_at_the_time_of_the_text(pool: PgPool) {
    let h = hooks(&pool);
    let business = Uuid::from_u128(0x317);
    sqlx::query(
        "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
         VALUES ($1, 'active', now(), now())",
    )
    .bind(business)
    .execute(&pool)
    .await
    .expect("business");
    quoted_job(&pool, &h, business).await;
    let lists = "SELECT count(*) FROM messaging.outbox WHERE template_id LIKE 'quote_list%'";
    assert_eq!(text(&h, 2, HOME, "LIST").await, StatusCode::OK);
    assert_eq!(count(&pool, lists).await, 0, "no name: D-1310's fallback");
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = 'businesses.public_name_unusable'";
    assert_eq!(count(&pool, alerts).await, 1);
    sqlx::query(
        "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
         VALUES ($1, 'Sample Valley Plumbing', now())",
    )
    .bind(business)
    .execute(&pool)
    .await
    .expect("name");
    assert_eq!(text(&h, 3, HOME, "LIST").await, StatusCode::OK);
    let body: String =
        sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = 'quote_list'")
            .fetch_one(&pool)
            .await
            .expect("list");
    assert!(body.contains("1. Sample Valley Plumbing: $190."), "{body}");
    assert_eq!(
        phone_hits(&pool).await,
        1,
        "only the first text opened a request"
    );
}

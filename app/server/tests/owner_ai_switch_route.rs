//! TD-AI-32 (AI-008, AI-018, SEC-043, SEC-006, A11Y-030; T737, D-4727, D-5534): the owner's AI
//! page and its three `POST`s served by the assembled router with T303's owner session against
//! PostgreSQL. `GET /owner/ai` lists each missing record in plain words; turning on is refused
//! naming the first missing record; after the provider terms and a policy version effective today
//! with the AI-018 sentence are recorded, turning on writes one history row and one `ai.switch_on`
//! audit row and `ai_mode` follows; off always works; each recording writes one IDs-only audit
//! row; a stale factor answers the step-up form and records nothing; a bad field shows the
//! A11Y-030 error with the values kept. The owner's pass phrase, token and secret are fake.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_ai::api::{AiMode, ai_mode};
use askhava_core::api::{Environment, MIGRATOR};
use askhava_ops::api::{AuditRow, EXPORT_PAGE_MAX, export_audit};
use askhava_portals::api::{
    CookieJar, OwnerAuth, SecondFactor, bootstrap_owner, csrf_token, owner_sign_in,
};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use hmac::{Hmac, KeyInit, Mac};
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t737-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t737 owner pass phrase";
const PAGE: &str = "/owner/ai";
const TERMS: &str = "/owner/ai/terms";
const POLICY: &str = "/owner/ai/policy";
const SWITCH: &str = "/owner/ai/switch";
const MISSING: [&str; 5] = [
    "The AI provider&#39;s terms aren&#39;t recorded yet. Record them below.",
    "The newest terms don&#39;t say API inputs are kept out of training.",
    "The newest terms don&#39;t say how long flagged text may be kept.",
    "No spend limit is recorded, or it&#39;s more than 3 times what you expect to spend.",
    "Today&#39;s privacy policy doesn&#39;t have the AI wording yet.",
];

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let parts = RouterParts::from((None, None, None, None));
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        secret: secret.as_bytes().to_vec(),
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `at`.
fn totp_code(secret: &[u8], at: Timestamp) -> String {
    let step = u64::try_from(at.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The owner signed in at `at` with a TOTP code: the factor is fresh for 5 minutes from `at`.
async fn owner_at(w: &World, at: Timestamp) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let code = totp_code(&w.secret, at);
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    let jar = jar.expect("owner signed in");
    jar.iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

fn cookies(jar: &Jar) -> String {
    let all: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    all.join("; ")
}

fn csrf(jar: &Jar) -> String {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&cookies(jar)).expect("cookie");
    headers.insert(header::COOKIE, value);
    csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default()
}

/// A same-origin request with `jar`'s cookies; its status and body, after asserting `no-store`.
async fn send(w: &World, (method, path): (&str, &str), jar: &Jar, body: String) -> (u16, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies(jar))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = w.app.clone().oneshot(request).await.expect("response");
    let cache = response.headers().get(header::CACHE_CONTROL).cloned();
    assert!(
        cache.is_some_and(|v| v.to_str().is_ok_and(|s| s.contains("no-store"))),
        "{path} no-store"
    );
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn post(w: &World, path: &str, jar: &Jar, fields: &str) -> (u16, String) {
    let body = format!("csrf={}&{fields}", csrf(jar));
    send(w, ("POST", path), jar, body).await
}

async fn audit_rows(w: &World) -> Vec<AuditRow> {
    let mut db = w.pool.acquire().await.expect("db");
    export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit")
}

/// The switch history rows, each seen through the audit row its transaction wrote with it (this
/// file runs no SQL of its own, TD-SEC-10): distinct `ai.switch_history` subjects.
async fn history_len(w: &World) -> usize {
    let mut ids: Vec<Uuid> = audit_rows(w)
        .await
        .iter()
        .filter(|r| r.subject_kind == "ai.switch_history")
        .map(|r| r.subject_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

async fn live_mode(w: &World) -> AiMode {
    let mut db = w.pool.acquire().await.expect("db");
    ai_mode(&mut db, Environment::Live).await.expect("mode")
}

/// Today in Arizona (UTC-7, no daylight time), as the gate reads it.
fn today() -> String {
    let arizona = TimeZone::fixed(offset(-7));
    Timestamp::now().to_zoned(arizona).date().to_string()
}

fn good_terms() -> String {
    format!(
        "url=https%3A%2F%2Fprovider.example%2Fterms&checked_on={}&training=yes&retention_days=30\
         &flagged_days=730&spend_limit=60&expected_cost=20&zero_retention_on=",
        today()
    )
}

/// The audit rows naming `action`, each checked IDs-only: the owner as actor, no field or status.
fn ids_only<'a>(rows: &'a [AuditRow], action: &str) -> Vec<&'a AuditRow> {
    let found: Vec<_> = rows.iter().filter(|r| r.action == action).collect();
    for row in &found {
        assert_eq!(row.actor_kind, "owner");
        assert!(row.field.is_none() && row.from_status.is_none() && row.to_status.is_none());
    }
    found
}

/// D-5534(b): a terms row's subject is `from_u64_pair(0, id)` of its bigint key: a zero high half
/// and a non-zero low half, so never `Uuid::nil()` nor a random UUID. Returns the row ID.
fn assert_terms_subject(row: &AuditRow) -> u64 {
    assert_eq!(row.subject_kind, "ai.provider_terms");
    let (high, low) = row.subject_id.as_u64_pair();
    assert_eq!(high, 0, "{}", row.subject_id);
    assert_ne!(low, 0, "{}", row.subject_id);
    low
}

/// TD-AI-32: the page lists every missing record; on is refused naming the first; after both
/// records, on writes one history and one `ai.switch_on` row and `ai_mode` follows; off works.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ai_32_records_then_the_switch_turns_on_and_off(pool: PgPool) {
    let w = world(&pool).await;
    let owner = owner_at(&w, Timestamp::now()).await;
    let (status, page) = send(&w, ("GET", PAGE), &owner, String::new()).await;
    assert_eq!(status, 200, "{page}");
    assert!(MISSING.iter().all(|m| page.contains(m)), "{page}");
    let (status, refused) = post(&w, SWITCH, &owner, "to=on").await;
    assert_eq!(status, 400, "{refused}");
    assert!(refused.contains("<title>Error: ") && refused.contains("href=\"#url\""));
    assert!(refused.contains(&format!("AI can&#39;t be turned on yet. {}", MISSING[0])));
    assert_eq!(
        (history_len(&w).await, live_mode(&w).await),
        (0, AiMode::KeywordOnly)
    );
    let (status, text) = post(&w, TERMS, &owner, &good_terms()).await;
    assert_eq!(status, 200, "{text}");
    let (status, refused) = post(&w, SWITCH, &owner, "to=on").await;
    assert_eq!(status, 400, "{refused}");
    assert!(refused.contains("href=\"#version\"") && refused.contains(MISSING[4]));
    let policy = format!("version=2026-fake.1&effective_on={}&ai018=yes", today());
    let (status, text) = post(&w, POLICY, &owner, &policy).await;
    assert_eq!(status, 200, "{text}");
    let (status, text) = post(&w, SWITCH, &owner, "to=on").await;
    assert_eq!(status, 200, "{text}");
    assert!(
        text.contains("AI on scrubbed text is now on.")
            && text.contains("Nothing. You can turn it on.")
    );
    assert_eq!(
        (history_len(&w).await, live_mode(&w).await),
        (1, AiMode::Ai)
    );
    let rows = audit_rows(&w).await;
    assert_eq!(ids_only(&rows, "ai.switch_on").len(), 1);
    let terms = ids_only(&rows, "ai.provider_terms_recorded");
    assert_eq!(terms.len(), 1);
    assert_eq!(terms[0].subject_kind, "ai.provider_terms");
    assert_terms_subject(terms[0]);
    let policy = ids_only(&rows, "ai.privacy_policy_recorded");
    assert_eq!(policy.len(), 1);
    assert_eq!(policy[0].subject_id, Uuid::nil());
    let (status, text) = post(&w, SWITCH, &owner, "to=off").await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(
        (history_len(&w).await, live_mode(&w).await),
        (2, AiMode::KeywordOnly)
    );
    assert_eq!(ids_only(&audit_rows(&w).await, "ai.switch_off").len(), 1);
}

/// TD-AI-32, SEC-043: with a factor 6 minutes old every change answers the step-up form (401)
/// and records nothing, not even turning off.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ai_32_a_stale_factor_records_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let stale = owner_at(&w, Timestamp::now() - SignedDuration::from_mins(6)).await;
    let (status, page) = send(&w, ("GET", PAGE), &stale, String::new()).await;
    assert_eq!(status, 200, "{page}");
    let before = audit_rows(&w).await.len();
    let policy = format!("version=2026-fake.2&effective_on={}&ai018=yes", today());
    let presses = [
        (TERMS, good_terms()),
        (POLICY, policy),
        (SWITCH, "to=off".to_owned()),
    ];
    for (path, fields) in &presses {
        let (status, text) = post(&w, path, &stale, fields).await;
        assert_eq!(status, 401, "{path}: {text}");
        assert!(
            text.contains("need a new code from your authenticator app"),
            "{path}"
        );
        assert!(!text.contains("/owner/sign-in\""), "{path}");
    }
    let rows = audit_rows(&w).await;
    assert!(rows[before..].iter().all(|r| !r.action.starts_with("ai.")));
    assert_eq!(history_len(&w).await, 0);
}

/// TD-AI-32, A11Y-030: a bad field is a 400 whose title starts "Error: ", whose focused summary
/// links each message to its field, and which keeps every typed value; nothing is recorded. A
/// version name already recorded is refused the same way.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ai_32_a_bad_field_keeps_the_values(pool: PgPool) {
    let w = world(&pool).await;
    let owner = owner_at(&w, Timestamp::now()).await;
    let fields = "url=http%3A%2F%2Fprovider.example&checked_on=2026-11-02&training=yes\
                  &retention_days=lots&flagged_days=730&spend_limit=60&expected_cost=20";
    let (status, text) = post(&w, TERMS, &owner, fields).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("<title>Error: ") && text.contains("id=\"error-summary\""));
    assert!(text.contains("href=\"#url\"") && text.contains("href=\"#retention_days\""));
    for kept in ["http://provider.example", "lots", "2026-11-02", "730"] {
        assert!(text.contains(&format!("value=\"{kept}\"")), "{kept}");
    }
    assert!(text.contains("aria-invalid=\"true\"") && text.contains(" checked"));
    assert!(!text.contains(" required"));
    assert!(
        audit_rows(&w)
            .await
            .iter()
            .all(|r| !r.action.starts_with("ai."))
    );
    let policy = "version=2026-fake.3&effective_on=2026-11-02&ai018=yes";
    assert_eq!(post(&w, POLICY, &owner, policy).await.0, 200);
    let (status, text) = post(&w, POLICY, &owner, policy).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("already recorded") && text.contains("value=\"2026-fake.3\""));
    let rows = audit_rows(&w).await;
    assert_eq!(ids_only(&rows, "ai.privacy_policy_recorded").len(), 1);
}

/// TD-AI-32, AI-008, D-841, D-5690: a zero-retention date on or before today keeps live AI on
/// whatever the switch says, and the page and the "off" answer say so; recording the terms again
/// without the date closes the gate and the notice goes. Each terms row's audit subject is its
/// own bigint ID, the newer one higher (D-5534(b)).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ai_32_a_zero_retention_date_is_shown_as_keeping_ai_on(pool: PgPool) {
    const NOTICE: &str = "A signed zero-retention deal is on record. So the AI stays on";
    let w = world(&pool).await;
    let owner = owner_at(&w, Timestamp::now()).await;
    let signed = good_terms() + &today();
    assert_eq!(post(&w, TERMS, &owner, &signed).await.0, 200);
    let (status, page) = send(&w, ("GET", PAGE), &owner, String::new()).await;
    assert_eq!(status, 200, "{page}");
    assert!(
        page.contains("The switch is off.") && page.contains(NOTICE),
        "{page}"
    );
    let (status, text) = post(&w, SWITCH, &owner, "to=off").await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("The switch is off, but the AI still runs on scrubbed text"));
    assert!(!text.contains("AI on scrubbed text is now off.") && text.contains(NOTICE));
    assert_eq!(live_mode(&w).await, AiMode::Ai);
    let (status, text) = post(&w, TERMS, &owner, &good_terms()).await;
    assert_eq!(status, 200, "{text}");
    assert!(!text.contains(NOTICE), "{text}");
    assert_eq!(live_mode(&w).await, AiMode::KeywordOnly);
    let (status, text) = post(&w, SWITCH, &owner, "to=off").await;
    assert!(
        status == 200 && text.contains("AI on scrubbed text is now off."),
        "{text}"
    );
    let rows = audit_rows(&w).await;
    let ids: Vec<u64> = ids_only(&rows, "ai.provider_terms_recorded")
        .into_iter()
        .map(assert_terms_subject)
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().min() < ids.iter().max(), "{ids:?}");
}

//! TD-SEC-81 (A11Y-030, CON-035, PICK-048, SEC-006, SEC-043; T738, D-4727, D-5657): the owner
//! settings refusals and the opt-out and Stripe-answer audit rows, served by the assembled router
//! against PostgreSQL with T303's owner session.
//!
//! Proves: a malformed opt-out number and a bad Stripe-answer date, reference or choice each
//! answer 400 with "Error: " in the title, a focused summary linking the field, the message by it
//! with `aria-invalid` and every typed value kept; the four refusal notices (no approval, unmet
//! safety items, Spanish records missing, business texting refused) show the same error form; a
//! recorded opt-out writes one `Actor::Owner` audit row naming its opt-out event, a repeat one
//! more, and a filed Stripe answer one naming its row, each in the record's transaction (a failed
//! audit insert leaves no record) and holding no phone, digest or reference. The owner's pass
//! phrase, authenticator secret, vault keys, phone numbers and references are fake.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_calls::api::Conditions;
use askhava_catalog::api::load_embedded;
use askhava_core::api::MIGRATOR;
use askhava_ops::api::{EXPORT_PAGE_MAX, export_audit};
use askhava_portals::api::{
    CookieJar, OWNER_OPT_OUT_ACTION, OWNER_STRIPE_ANSWER_ACTION, OwnerAuth, SecondFactor,
    SettingsNotice, bootstrap_owner, csrf_token, owner_settings_page_with, owner_sign_in,
};
use askhava_privacy::api::{IndexPurpose, Vault};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t738-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t738 owner pass phrase";
const PHONE: &str = "+15205550142";
const OPT_OUT: &str = "/owner/opt-out";
const STRIPE: &str = "/owner/stripe-answer";
const OPT_OUT_ACTION: &str = OWNER_OPT_OUT_ACTION;

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let parts = RouterParts::from((None, None, None, None)).with_vault(Some(vault()));
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

/// The owner signed in now with a TOTP code: the factor is fresh for 5 minutes.
async fn owner(w: &World) -> Jar {
    let at = Timestamp::now();
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

fn csrf(jar: &Jar) -> String {
    let mut headers = HeaderMap::new();
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let value = HeaderValue::from_str(&cookies.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, value);
    csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default()
}

/// A same-origin `POST` of `fields` with `jar`'s cookies and CSRF token: status and page.
async fn post(w: &World, path: &str, jar: &Jar, fields: &str) -> (u16, String) {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={}{fields}", csrf(jar))))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = w.app.clone().oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The one text value `v` of `sql` (one row), which reads other modules' tables, run through
/// `query_to_xml` as `web_chat_stops.rs` does: `server` names no other schema in a query macro.
async fn value(w: &World, sql: &str) -> String {
    sqlx::query_scalar!(
        r#"SELECT coalesce((xpath('/row/v/text()', query_to_xml($1::text, false, true, '')))[1]::text, '') AS "v!""#,
        sql
    )
    .fetch_one(&w.pool)
    .await
    .expect("value")
}

async fn count(w: &World, from: &str) -> i64 {
    let v = value(w, &format!("SELECT count(*) AS v FROM {from}")).await;
    v.parse().expect("count")
}

/// The A11Y-030 error form naming control `id`: "Error: " title, the focused summary first in
/// `main` linking `#id`, and the message by the control.
fn assert_error_form(page: &str, id: &str, case: &str) {
    assert!(
        page.contains("<title>Error: Owner settings"),
        "{case}: title"
    );
    let summary = page.find("id=\"error-summary\"").expect("summary");
    let h1 = page.find("<h1>").expect("h1");
    assert!(summary > h1, "{case}: summary right under the heading");
    assert!(
        page.contains("role=\"alert\" tabindex=\"-1\" autofocus"),
        "{case}: summary focused without JavaScript"
    );
    assert!(page.contains(&format!("href=\"#{id}\"")), "{case}: link");
    assert!(
        page.contains(&format!("id=\"{id}-error\">Error: ")),
        "{case}"
    );
    assert!(
        !page.contains("id=\"settings-done\""),
        "{case}: no done line"
    );
}

/// The control `id`'s tag: its `aria-invalid` and `aria-describedby` are on it.
fn control<'a>(page: &'a str, id: &str) -> &'a str {
    let at = page.find(&format!("id=\"{id}\"")).expect("control");
    let start = page[..at].rfind('<').expect("tag start");
    let end = at + page[at..].find('>').expect("tag end");
    &page[start..=end]
}

/// TD-SEC-81 (A11Y-030, CON-035): a malformed opt-out number is a 400 with the error form, the
/// number kept as typed; nothing is listed or logged.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_a_malformed_opt_out_number_shows_the_error_form(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    let (status, page) = post(&w, OPT_OUT, &jar, "&phone=555-01x").await;
    assert_eq!(status, 400);
    assert_error_form(&page, "opt-out-phone", "phone");
    let tag = control(&page, "opt-out-phone");
    assert!(tag.contains("aria-invalid=\"true\""), "{tag}");
    assert!(tag.contains("opt-out-hint opt-out-phone-error"), "{tag}");
    assert!(tag.contains("value=\"555-01x\""), "the value kept: {tag}");
    assert_eq!(count(&w, "messaging.optout_event").await, 0);
    assert!(
        audit_rows(&w, OPT_OUT_ACTION).await.is_empty(),
        "nothing logged"
    );
}

/// TD-SEC-81 (A11Y-030, PICK-048): a bad date, reference or choice each answer 400 naming that
/// field, every typed value kept; nothing is filed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_a_bad_stripe_answer_names_its_field_and_keeps_the_values(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    let cases = [
        (
            "&answered_on=not-a-day&reference=fake-case-0041&allows=no",
            "stripe-date",
        ),
        (
            "&answered_on=2026-03-02&reference=+&allows=yes",
            "stripe-reference",
        ),
        (
            "&answered_on=2026-03-02&reference=fake-case-0042&allows=maybe",
            "stripe-yes",
        ),
    ];
    for (fields, id) in cases {
        let (status, page) = post(&w, STRIPE, &jar, fields).await;
        assert_eq!(status, 400, "{id}");
        assert_error_form(&page, id, id);
        assert!(control(&page, id).contains("aria-invalid=\"true\""), "{id}");
    }
    let (_, page) = post(&w, STRIPE, &jar, cases[0].0).await;
    assert!(control(&page, "stripe-date").contains("value=\"not-a-day\""));
    assert!(control(&page, "stripe-reference").contains("value=\"fake-case-0041\""));
    assert!(control(&page, "stripe-no").contains("checked"));
    let (_, page) = post(&w, STRIPE, &jar, cases[1].0).await;
    assert!(control(&page, "stripe-date").contains("value=\"2026-03-02\""));
    assert!(control(&page, "stripe-yes").contains("checked"));
    let (_, page) = post(&w, STRIPE, &jar, cases[2].0).await;
    assert!(control(&page, "stripe-reference").contains("value=\"fake-case-0042\""));
    let filed = count(&w, "quotes.stripe_written_answer").await;
    assert_eq!(filed, 0, "nothing filed");
}

/// TD-SEC-81 (A11Y-030, D-5657): the four refusal notices answer 400 with the error form linking
/// their control; the unmet-items notice is rendered directly, as every item is met in the build.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_the_refusal_notices_show_the_error_form(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    for (path, id) in [
        ("/owner/safety-call", "safety-date"),
        ("/owner/spanish-conversations", "spanish-reviewed-on"),
        ("/owner/business-texting", "business-on"),
    ] {
        let (status, page) = post(&w, path, &jar, "&to=on").await;
        assert_eq!(status, 400, "{path}");
        assert_error_form(&page, id, path);
        assert!(
            control(&page, id).contains(&format!("{id}-error")),
            "{path}"
        );
    }
    let unmet = Conditions {
        narrowed_trigger: false,
        recent_only: true,
        script: true,
        other_number: true,
        list_checked: true,
        consent_text: true,
        logged: true,
    };
    let mut db = w.pool.acquire().await.expect("db");
    let notice = Some(SettingsNotice::SafetyCallUnmet(unmet));
    let page = owner_settings_page_with(&mut db, "fake-csrf", notice)
        .await
        .expect("page");
    assert_error_form(&page, "safety-on", "unmet");
    assert!(control(&page, "safety-on").contains("safety-on-error safety-call-missing"));
}

/// The owner's audit rows of `action`, oldest first, as their `Debug` text and subject IDs.
async fn audit_rows(w: &World, action: &str) -> Vec<(String, Uuid)> {
    let mut db = w.pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX).await;
    rows.expect("audit rows")
        .into_iter()
        .filter(|r| r.action == action && r.actor_kind == "owner")
        .map(|r| (format!("{r:?}"), r.subject_id))
        .collect()
}

/// Every form the phone's digests could take in a row: hex and base64.
fn digest_forms(phone: &str) -> Vec<String> {
    let mut forms = Vec::new();
    for (_, digest) in vault().blind_index(IndexPurpose::HomeownerPhone, phone) {
        let mut hex = String::new();
        for b in digest {
            write!(hex, "{b:02x}").expect("hex");
        }
        forms.push(hex);
        forms.push(STANDARD.encode(digest));
    }
    forms
}

/// TD-SEC-81 (SEC-006, CON-035): a recorded opt-out writes one owner audit row naming its event,
/// a repeat one more naming its own; no row holds the phone or a digest.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_each_recorded_opt_out_writes_one_owner_audit_row(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    let (first, _) = post(&w, OPT_OUT, &jar, "&phone=%2B15205550142").await;
    let (again, page) = post(&w, OPT_OUT, &jar, "&phone=%2B15205550142").await;
    assert_eq!((first, again), (200, 200));
    assert!(page.contains("already off our texts") && !page.contains("Error: "));
    let events = value(
        &w,
        "SELECT string_agg(id::text, ',' ORDER BY id) AS v FROM messaging.optout_event",
    )
    .await;
    let events: Vec<Uuid> = events
        .split(',')
        .map(|id| id.parse().expect("id"))
        .collect();
    let rows = audit_rows(&w, OPT_OUT_ACTION).await;
    assert_eq!(
        rows.len(),
        2,
        "one row per recorded opt-out, the repeat too"
    );
    let mut named: Vec<Uuid> = rows.iter().map(|(_, id)| *id).collect();
    let mut expected = events.clone();
    named.sort();
    expected.sort();
    assert_eq!(named, expected, "each row names its opt-out event");
    let mut needles = digest_forms(PHONE);
    needles.push("5205550142".to_owned());
    for (row, _) in &rows {
        assert!(
            row.contains("subject_kind: \"messaging.optout_event\""),
            "{row}"
        );
        assert!(needles.iter().all(|n| !row.contains(n.as_str())), "{row}");
    }
}

/// TD-SEC-81 (SEC-006, PICK-048): a filed Stripe answer writes one owner audit row naming its row,
/// holding no reference.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_a_filed_stripe_answer_writes_one_owner_audit_row(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    let fields = "&answered_on=2026-03-02&reference=fake-case-0043&allows=yes";
    let (status, _) = post(&w, STRIPE, &jar, fields).await;
    assert_eq!(status, 200);
    let filed = value(&w, "SELECT id::text AS v FROM quotes.stripe_written_answer").await;
    let filed: Uuid = filed.parse().expect("filed");
    let rows = audit_rows(&w, OWNER_STRIPE_ANSWER_ACTION).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, filed, "the row names the filing");
    assert!(
        rows[0]
            .0
            .contains("subject_kind: \"quotes.stripe_written_answer\"")
    );
    assert!(
        !rows[0].0.contains("fake-case"),
        "no reference: {}",
        rows[0].0
    );
}

/// TD-SEC-81 (SEC-006): the audit row is in the record's transaction, so a failed audit insert
/// (the log's ID sequence spent, a fault injected without DDL) leaves no opt-out and no Stripe
/// answer.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_81_a_failed_audit_insert_leaves_no_record(pool: PgPool) {
    let w = world(&pool).await;
    let jar = owner(&w).await;
    let spent = "SELECT setval(pg_get_serial_sequence('ops.audit_log', 'id'), \
                 9223372036854775807) AS v";
    assert_eq!(value(&w, spent).await, "9223372036854775807");
    let (opted, _) = post(&w, OPT_OUT, &jar, "&phone=%2B15205550142").await;
    let fields = "&answered_on=2026-03-02&reference=fake-case-0044&allows=no";
    let (filed, _) = post(&w, STRIPE, &jar, fields).await;
    assert_eq!((opted, filed), (500, 500));
    for table in [
        "messaging.optout_event",
        "messaging.suppression",
        "quotes.stripe_written_answer",
    ] {
        assert_eq!(count(&w, table).await, 0, "{table} is empty");
    }
}

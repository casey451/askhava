//! TD-SEC-45's and TD-BIZ-49's sign-up cases (BIZ-001, BIZ-002, BIZ-016, BIZ-021, CON-019,
//! SEC-014, SEC-015, SEC-034, SEC-042, SEC-048; T302, T459, D-2128, D-3641, D-3676): the
//! assembled router with the sign-up part serves `GET /b/signup` without a session (the
//! pre-session cookie and its CSRF token, the box unticked, the channels sealed for that token,
//! the stored categories, `no-store`), and `POST /b/signup/start`: the limits first, the
//! cross-site and CSRF refusals with their security event, the consent seal, the minted mailbox,
//! the binding cookie and its MAC, the names, the pest and agency writers, the license binding and
//! name flag, the city-list contact, `no_record_contact`, an unreadable record, and no record
//! name or contact stored in any table. T460's identity steps (BIZ-002, BIZ-026, SEC-019,
//! D-3641, D-3779): refused without a verified binding cookie, out of order or after the code,
//! each code to the bound record's own contact through a fake Verify, never a posted license
//! number. Fake records are read from a temporary `file://` directory. No setting is a real key;
//! all data is fake.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{
    Step, business_names, businesses_of_mailbox, leave, review_due, signup_progress,
};

use askhava_catalog::api::{load, parse};
use askhava_core::api::{IdSecret, MIGRATOR, Settings, TestControl};
use askhava_licensing::api::{
    Agency, HolderInputs, PestRecordCategory, RECORD_HEADER, bound_roc_number, holder_for,
    import_city_list, import_roc_list,
};
use askhava_portals::api::{
    CODE_SEND_ROUTE, CONSENT_AGAIN, ChannelPost, CodeProblem, CookieJar, IDENTITY_CHECK_ROUTE,
    LETTER_ROUTE, NUMBER_OLD_ROUTE, SESSION_COOKIE, SIGNUP_FORM_ROUTE, SIGNUP_ROUTE, csrf_token,
};
use askhava_privacy::api::{Vault, mailboxes_of};
use askhava_server::business_signup::{SIGNUP_COOKIE, ServedSignUp, bound_signup};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const DATABASE: &str = "postgres://fake@127.0.0.1:1/none";
/// Two distinct fake 32-byte vault keys, version 1.
const SEAL: &str = "1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const INDEX: &str = "1:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ=";

/// The fake record of `ROC100001` and the city-list row's record.
const FULL: &str = "ROC100001,Example Plumbing Co,Sample Pipes,928-555-0142,mobile,office@example.test,100 Sample Way,Lake Havasu City,86403";
const CITY_RECORD: &str = "CL-100001,Sample Yard Care LLC,Sample Yards,928-555-0150,mobile,,,,";
const ROC_LIST: &str = "license_number,class,status,issued_on\n\
ROC100001,R-37,Active,2015-06-01\nROC200001,R-37,Active,2016-01-02\n";
const CITY_LIST: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-100001,Sample Yard Care LLC,Sample Yards,Fake Owner Person,2020-02-03\n\
CL-100002,Example Window Wash,,,2021-07-08\nCL-100003,Example Window Wash,,,2022-01-01\n";

fn pairs(records: Option<&str>) -> Vec<(String, String)> {
    let base = records.map(|dir| ("ASKHAVA_LICENSE_BASE_URL", format!("file://{dir}")));
    [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        ("ASKHAVA_DATABASE_URL", DATABASE.to_owned()),
        ("ASKHAVA_ID_SECRET", SECRET.to_owned()),
        ("VAULT_KEY", SEAL.to_owned()),
        ("VAULT_INDEX_KEY", INDEX.to_owned()),
    ]
    .into_iter()
    .chain(base)
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// The router as `main` assembles it, with only the sign-up part added and no license records.
fn app(pool: &PgPool) -> Router {
    app_reading(pool, None)
}

/// The router reading license records from the `file://` directory `records`, if any.
fn app_reading(pool: &PgPool, records: Option<&Records>) -> Router {
    app_coding(pool, records, None)
}

/// As [`app_reading`], the identity codes through `verify` when given (TXT-038).
fn app_coding(pool: &PgPool, records: Option<&Records>, verify: Option<&FakeVerify>) -> Router {
    let dir = records.map(|r| r.0.display().to_string());
    let pairs = pairs(dir.as_deref());
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let signup = match verify {
        Some(fake) => ServedSignUp::with_verify(pool.clone(), pairs, fake.clone()),
        None => ServedSignUp::from_settings(pool.clone(), pairs),
    };
    let signup = signup.expect("sign-up part");
    let parts = RouterParts::from((None, None, None, None)).with_sign_up(signup);
    build_router(pool.clone(), control.router(), parts)
}

/// Stores the reviewed catalog, its categories switched on as the file says (LIC-003).
async fn store_catalog(pool: &PgPool) {
    let catalog = parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog");
    let mut conn = pool.acquire().await.expect("connection");
    load(&mut conn, &catalog).await.expect("catalog stored");
}

/// `GET /b/signup` with no cookie: the status, the `Set-Cookie` lines, `Cache-Control` and the page.
async fn open(app: &Router) -> (StatusCode, Vec<String>, String, String) {
    let request = Request::get(SIGNUP_ROUTE)
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status();
    let headers = response.headers();
    let cookies = headers.get_all(header::SET_COOKIE).iter();
    let cookies = cookies.filter_map(|v| v.to_str().ok().map(str::to_owned));
    let cache = headers
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    let cache = cache.unwrap_or_default().to_owned();
    let cookies = cookies.collect();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let html = String::from_utf8(body.to_vec()).expect("utf-8");
    (status, cookies, cache, html)
}

/// The value of the first hidden field `name` on `html`.
fn hidden(html: &str, name: &str) -> String {
    let at = format!("name=\"{name}\" value=\"");
    let rest = &html[html.find(&at).expect("field") + at.len()..];
    rest[..rest.find('"').expect("end")].to_owned()
}

/// The one pre-session `Set-Cookie` line, checked for every `__Host-` attribute; its `name=value`.
fn pre_session_cookie(cookies: &[String]) -> String {
    let line = cookies
        .iter()
        .find(|c| c.starts_with("__Host-askhava-pre="))
        .expect("the pre-session cookie");
    let attributes: Vec<&str> = line.split(';').map(str::trim).collect();
    for wanted in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"] {
        assert!(
            attributes.iter().any(|a| a.eq_ignore_ascii_case(wanted)),
            "SEC-042: {wanted} on {line}"
        );
    }
    assert!(
        !attributes
            .iter()
            .any(|a| a.to_ascii_lowercase().starts_with("domain=")),
        "a __Host- cookie has no Domain: {line}"
    );
    attributes[0].to_owned()
}

/// A cookie jar holding only the `name=value` pair, as the browser would send it back.
fn jar_with(pair: &str) -> CookieJar {
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, pair.parse().expect("cookie header"));
    CookieJar::from_headers(&headers)
}

fn secret() -> IdSecret {
    IdSecret::new(1, SecretString::from(SECRET.to_owned())).expect("secret")
}

/// The form's shown channels as a post would carry them, checked for `csrf`.
fn shown(html: &str, csrf: &str) -> Option<Vec<askhava_businesses::api::ConsentChannel>> {
    let fields: Vec<(String, String)> = ["consent_channels", "consent_seal"]
        .iter()
        .map(|n| ((*n).to_owned(), hidden(html, n)))
        .collect();
    ChannelPost::parse(&fields, false, (&secret(), csrf)).shown
}

/// TD-SEC-45 (BIZ-021, CON-019, SEC-042): served without a session, with the pre-session cookie,
/// its CSRF token, the box unticked, the channels sealed for that token alone and `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_the_form_is_served_without_a_session(pool: PgPool) {
    store_catalog(&pool).await;
    let (status, cookies, cache, html) = open(&app(&pool)).await;
    assert_eq!(status, StatusCode::OK);
    let pre = pre_session_cookie(&cookies);
    assert!(
        !cookies
            .iter()
            .any(|c| c.starts_with("__Host-askhava-session"))
    );
    assert_eq!(cache, "no-store");
    assert!(html.contains("Sign up your business"));
    let csrf = hidden(&html, "csrf");
    assert_eq!(
        Some(csrf.clone()),
        csrf_token(&jar_with(&pre)),
        "SEC-042: the CSRF token derived from the pre-session cookie"
    );
    let tag = "<input type=\"checkbox\" id=\"consent\" name=\"consent\" value=\"yes\"";
    let tag_at = html.find(tag).expect("the E-SIGN box");
    let tag_end = tag_at + html[tag_at..].find('>').expect("tag end");
    assert!(
        !html[tag_at..tag_end].contains("checked"),
        "CON-019: unticked"
    );
    assert_eq!(hidden(&html, "consent_channels"), "email");
    let sealed = shown(&html, &csrf).expect("the seal verifies for the page's token");
    assert_eq!(sealed, vec![askhava_businesses::api::ConsentChannel::Email]);
    assert!(
        shown(&html, &"0".repeat(64)).is_none(),
        "not for another token"
    );
}

/// TD-SEC-45 (BIZ-021): each visit gets its own pre-session token; the stored switched-on
/// categories are listed with their "Open now" or "Coming later" label.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_the_form_lists_the_stored_categories(pool: PgPool) {
    let app = app(&pool);
    let (_, _, _, empty) = open(&app).await;
    assert!(
        !empty.contains("Open now"),
        "no stored category, none listed"
    );
    store_catalog(&pool).await;
    let (status, _, _, first) = open(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert!(first.contains("Open now") || first.contains("Coming later"));
    let (_, _, _, second) = open(&app).await;
    assert_ne!(hidden(&first, "csrf"), hidden(&second, "csrf"));
}

/// A temporary `file://` directory of fake `record/<list>/<number>` files (D-2954).
struct Records(PathBuf);

impl Records {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("askhava-signup-{}", Uuid::new_v4()));
        let files = [
            ("roc", "ROC100001", format!("{RECORD_HEADER}\n{FULL}\n")),
            (
                "roc",
                "ROC200001",
                "license_number,business_name\n".to_owned(),
            ),
            (
                "city",
                "CL-100001",
                format!("{RECORD_HEADER}\n{CITY_RECORD}\n"),
            ),
        ];
        for (list, number, text) in files {
            let at = dir.join("record").join(list);
            std::fs::create_dir_all(&at).expect("fixture dir");
            std::fs::write(at.join(number), text).expect("record");
        }
        Self(dir)
    }
}

impl Drop for Records {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The catalog and the fake ROC and city lists, stored.
async fn store_lists(pool: &PgPool) {
    store_catalog(pool).await;
    let mut conn = pool.acquire().await.expect("connection");
    let day = date(2026, 11, 12);
    import_roc_list(&mut conn, day, ROC_LIST)
        .await
        .expect("roc");
    import_city_list(&mut conn, day, CITY_LIST)
        .await
        .expect("city");
}

/// What the browser holds after `GET /b/signup`: the pre-session cookie and the form's tokens.
struct Browser {
    pre: String,
    fields: Vec<(String, String)>,
}

async fn browser(app: &Router) -> Browser {
    let (_, cookies, _, html) = open(app).await;
    let fields = ["csrf", "consent_channels", "consent_seal"];
    let fields = fields.iter().map(|n| ((*n).to_owned(), hidden(&html, n)));
    Browser {
        pre: pre_session_cookie(&cookies),
        fields: fields.collect(),
    }
}

/// A typed form: an R-37 plumber with the box ticked, plus `extra`, minus any field in `drop`.
fn typed(b: &Browser, extra: &[(&str, &str)], drop: &[&str]) -> Vec<(String, String)> {
    let base = [
        ("business_name", "Example Plumbing Co"),
        ("email", "owner@example.test"),
        ("category", "plumbing_repair"),
        ("roc_number", "ROC100001"),
        ("consent", "yes"),
    ];
    let base = base.iter().filter(|(k, _)| !drop.contains(k));
    let typed = base
        .chain(extra)
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()));
    let kept = b.fields.iter().filter(|(k, _)| !drop.contains(&k.as_str()));
    kept.cloned().chain(typed).collect()
}

/// `POST /b/signup/start` with `fields`, same-origin unless `site` names another; the answer as
/// [`open`] reads it.
async fn post(
    app: &Router,
    b: &Browser,
    fields: &[(String, String)],
    site: &str,
) -> (StatusCode, Vec<String>, String, String) {
    post_to(app, (SIGNUP_FORM_ROUTE, &b.pre), fields, site).await
}

/// `POST route` with the `Cookie` header `cookie` and `fields`, as [`post`].
async fn post_to(
    app: &Router,
    (route, cookie): (&str, &str),
    fields: &[(String, String)],
    site: &str,
) -> (StatusCode, Vec<String>, String, String) {
    let enc = |s: &str| {
        let keep = |c: &u8| c.is_ascii_alphanumeric() || b"-._".contains(c);
        s.bytes()
            .map(|c| {
                if keep(&c) {
                    (c as char).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect::<String>()
    };
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect();
    let request = Request::post(route)
        .header(header::HOST, "askhava.test")
        .header("sec-fetch-site", site)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status();
    let cookies = response.headers().get_all(header::SET_COOKIE).iter();
    let cookies = cookies
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    let cache = response.headers().get(header::CACHE_CONTROL);
    let cache = cache
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (
        status,
        cookies,
        cache,
        String::from_utf8_lossy(&body).into_owned(),
    )
}

/// The sign-up a response's binding cookie names, when its MAC verifies (SEC-019).
fn signed_up(cookies: &[String]) -> Option<Uuid> {
    let line = cookies.iter().find(|c| c.starts_with(SIGNUP_COOKIE))?;
    let pair = line.split(';').next()?;
    bound_signup(&jar_with(pair), &secret())
}

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(k.to_owned());
    Vault::new(&key(SEAL), &key(INDEX), &key(DATABASE)).expect("vault")
}

/// The mailboxes the vault's blind index finds for `address` (D-3262).
async fn mailboxes(pool: &PgPool, address: &str) -> Vec<Uuid> {
    let mut conn = pool.acquire().await.expect("connection");
    let found = mailboxes_of(&mut conn, &vault(), address).await;
    found.expect("lookup")
}

/// Whether `business` waits in needs review (BIZ-016, BIZ-020).
async fn in_review(pool: &PgPool, business: Uuid) -> bool {
    let mut conn = pool.acquire().await.expect("connection");
    let due = review_due(&mut conn, Timestamp::now()).await;
    due.expect("review list").contains(&business)
}

/// Every log event's fields on the test's thread (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Subscriber for Events {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Fields(&mut line));
        self.0.lock().expect("events").push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// TD-SEC-45 (SEC-014, SEC-015, SEC-042, SEC-048, D-1951): the limit comes first, so a day's
/// refused posts still count; a cross-site post and a wrong CSRF token are 403s that store
/// nothing and are each logged as a `csrf_rejected` security event; once the limit is full a good
/// post stores nothing either.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_limits_and_cross_site_and_csrf_refusals(pool: PgPool) {
    store_lists(&pool).await;
    let app = app(&pool);
    let b = browser(&app).await;
    let good = typed(&b, &[], &[]);
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    let (status, ..) = post(&app, &b, &good, "cross-site").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "cross-site");
    let mut forged = typed(&b, &[], &["csrf"]);
    forged.push(("csrf".to_owned(), "0".repeat(64)));
    for _ in 0..4 {
        let (status, ..) = post(&app, &b, &forged, "same-origin").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "wrong CSRF token");
    }
    drop(guard);
    let logged = events.0.lock().expect("events").clone();
    let rejected = logged.iter().filter(|l| l.contains("\"csrf_rejected\""));
    assert_eq!(rejected.count(), 5, "SEC-048");
    let (status, cookies, ..) = post(&app, &b, &good, "same-origin").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "SEC-014: the sixth"
    );
    assert_eq!(signed_up(&cookies), None);
    assert!(mailboxes(&pool, "owner@example.test").await.is_empty());
}

/// TD-SEC-45 (CON-019, D-2943, D-3725): a seal not made for this token, an untick, a refused
/// name or a category the class map refuses shows the form again (with its reason where it has
/// one) and stores nothing, not even the typed address's mailbox.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_consent_seal_and_names_refused(pool: PgPool) {
    store_lists(&pool).await;
    let app = app(&pool);
    let b = browser(&app).await;
    let other = browser(&app).await;
    let seal = other.fields.iter().find(|(n, _)| n == "consent_seal");
    let mut swapped = typed(&b, &[], &["consent_seal"]);
    swapped.push(seal.cloned().expect("the other page's seal"));
    let cases = [
        (swapped, CONSENT_AGAIN.split(',').next().expect("text")),
        (typed(&b, &[], &["consent"]), "Tick the box"),
        (
            typed(&b, &[], &["business_name"]),
            "Enter your business name.",
        ),
    ];
    for (fields, reason) in cases {
        let (status, cookies, cache, html) = post(&app, &b, &fields, "same-origin").await;
        assert_eq!((status, cache.as_str()), (StatusCode::OK, "no-store"));
        assert!(html.contains(reason), "{reason}");
        assert!(
            html.contains("value=\"ROC100001\""),
            "what was typed is kept"
        );
        assert_eq!(signed_up(&cookies), None);
    }
    let unknown = typed(&b, &[("category", "not_a_category")], &["category"]);
    let (status, cookies, cache, html) = post(&app, &b, &unknown, "same-origin").await;
    assert_eq!((status, cache.as_str()), (StatusCode::OK, "no-store"));
    assert!(html.contains("value=\"ROC100001\""), "the form again");
    assert_eq!(signed_up(&cookies), None);
    assert!(mailboxes(&pool, "owner@example.test").await.is_empty());
}

/// TD-SEC-45, TD-BIZ-49 (BIZ-001, BIZ-002, BIZ-016, SEC-019, SEC-034, D-3262, D-3641): an R-37
/// sign-up whose name agrees with its record is stored unflagged under the mailbox
/// `mailboxes_of` finds for the typed address, shows the record phone masked, `no-store`, and
/// binds the browser with a `__Host-` cookie whose MAC verifies only unaltered.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_49_r37_sign_up_binds_and_shows_the_masked_record_phone(pool: PgPool) {
    store_lists(&pool).await;
    let records = Records::new();
    let app = app_reading(&pool, Some(&records));
    let b = browser(&app).await;
    let (status, cookies, cache, html) = post(&app, &b, &typed(&b, &[], &[]), "same-origin").await;
    assert_eq!((status, cache.as_str()), (StatusCode::OK, "no-store"));
    assert!(
        html.contains("(•••) •••-0142"),
        "BIZ-026: the record phone, masked"
    );
    let business = signed_up(&cookies).expect("the binding verifies");
    assert!(!in_review(&pool, business).await, "BIZ-016: not flagged");
    let found = mailboxes(&pool, "owner@example.test").await;
    let mut conn = pool.acquire().await.expect("connection");
    let owners = businesses_of_mailbox(&mut conn, found[0]).await;
    assert_eq!(
        (found.len(), owners.expect("owners")),
        (1, vec![business]),
        "D-3262"
    );
    let line = cookies
        .iter()
        .find(|c| c.starts_with(SIGNUP_COOKIE))
        .expect("binding");
    let attributes: Vec<&str> = line.split(';').map(str::trim).collect();
    for wanted in [
        "Secure",
        "HttpOnly",
        "SameSite=Lax",
        "Path=/",
        "Max-Age=2592000",
    ] {
        assert!(attributes.contains(&wanted), "SEC-019: {wanted}");
    }
    assert!(!line.to_ascii_lowercase().contains("domain="));
    let other = Uuid::new_v4().to_string();
    let value = attributes[0].replacen(&business.to_string(), &other, 1);
    assert_eq!(
        bound_signup(&jar_with(&value), &secret()),
        None,
        "another ID"
    );
    let wrong = IdSecret::new(1, SecretString::from(format!("{SECRET}-x"))).expect("secret");
    assert_eq!(bound_signup(&jar_with(attributes[0]), &wrong), None);
    let made_up = format!(
        "{SIGNUP_COOKIE}={business}.{}.{}",
        "0".repeat(32),
        "0".repeat(64)
    );
    assert_eq!(bound_signup(&jar_with(&made_up), &secret()), None);
}

/// TD-BIZ-49 (BIZ-016, LIC-011, LIC-048, D-2954): a name agreeing with neither record name waits
/// in needs review (`name_mismatch`); a city-list business shows its matched row's record
/// contact; a city match with no single number waits in needs review (`no_record_contact`).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_49_name_flag_city_contact_and_no_record_contact(pool: PgPool) {
    store_lists(&pool).await;
    let records = Records::new();
    let app = app_reading(&pool, Some(&records));
    let city = ["business_name", "category", "roc_number"];
    let cases = [
        (
            ("Fake Fixers", "plumbing_repair"),
            &["business_name"][..],
            "-0142",
        ),
        (("Sample Yards", "house_cleaning"), &city[..], "-0150"),
        (
            ("Example Window Wash", "house_cleaning"),
            &city[..],
            "Your sign-up is saved",
        ),
    ];
    for (((name, trade), drop, shown), review) in cases.into_iter().zip([true, false, true]) {
        let b = browser(&app).await;
        let extra = [("business_name", name), ("category", trade)];
        let (status, cookies, _, html) =
            post(&app, &b, &typed(&b, &extra, drop), "same-origin").await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains(shown), "{shown}");
        let business = signed_up(&cookies).expect("signed up");
        assert_eq!(in_review(&pool, business).await, review, "{name}");
    }
}

/// TD-BIZ-49 (LIC-008, D-2954): an unreadable record, and no records configured at all, flag
/// nothing and answer 503 with `Retry-After`; the sign-up and its cookie stay.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_49_an_unreadable_record_changes_nothing(pool: PgPool) {
    store_lists(&pool).await;
    let records = Records::new();
    let bad = app_reading(&pool, Some(&records));
    for app in [bad, app(&pool)] {
        let b = browser(&app).await;
        let fields = typed(&b, &[("roc_number", "ROC200001")], &["roc_number"]);
        let (status, cookies, cache, _) = post(&app, &b, &fields, "same-origin").await;
        assert_eq!(
            (status, cache.as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, "no-store")
        );
        let business = signed_up(&cookies).expect("signed up");
        assert!(!in_review(&pool, business).await, "no flag");
    }
}

/// TD-BIZ-49 (LIC-015, LIC-016, LIC-047, D-2711, D-2712, D-3726, PRIV-016): the pest attestation
/// with its record categories and the BTR number's license are stored through T374's writers; the
/// BTR number is never a septage hauler's ADEQ license; the stored names are the typed ones, never
/// the record's, and the record's email is no mailbox.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_49_pest_and_agency_writers_and_no_record_value_stored(pool: PgPool) {
    store_lists(&pool).await;
    let records = Records::new();
    let app = app_reading(&pool, Some(&records));
    let b = browser(&app).await;
    let extra = [
        ("category", "pest_control"),
        ("category", "home_inspection"),
        ("category", "septic_pumping"),
        ("pest_license", "yes"),
        ("pest_number", "PM-1234"),
        ("pest_category", "wood_destroying"),
        ("btr_number", "HI-5678"),
    ];
    let (status, cookies, _, html) = post(&app, &b, &typed(&b, &extra, &[]), "same-origin").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("-0142"));
    let business = signed_up(&cookies).expect("signed up");
    let mut conn = pool.acquire().await.expect("connection");
    let parts = holder_for(&mut conn, business, HolderInputs::default()).await;
    let holder = parts.expect("holder").holder;
    assert!(holder.agencies.contains(&Agency::HomeInspector), "LIC-015");
    assert!(!holder.agencies.contains(&Agency::SeptageHauler), "LIC-016");
    let pest = holder.pest.expect("LIC-047: the attestation");
    assert_eq!(pest.number(), "PM-1234");
    assert!(pest.holds(PestRecordCategory::WoodDestroying));
    assert!(!pest.holds(PestRecordCategory::WeedControl));
    let names = business_names(&mut conn, business).await.expect("names");
    let names = names.expect("stored");
    assert_eq!(names.business_name, "Example Plumbing Co");
    assert_eq!(names.trade_name, None, "the record's DBA is never stored");
    drop(conn);
    assert!(mailboxes(&pool, "office@example.test").await.is_empty());
}

/// The one right identity code of [`FakeVerify`].
const RIGHT: &str = "246810";

/// Verify: records each send's channel and digits and counts checks; only [`RIGHT`] is right.
#[derive(Clone, Default)]
struct FakeVerify(Arc<Mutex<(Vec<String>, usize)>>);

impl FakeVerify {
    fn seen(&self) -> (Vec<String>, usize) {
        self.0.lock().expect("fake").clone()
    }
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let digits: String = to.chars().filter(char::is_ascii_digit).collect();
        let sent = format!("{channel:?}:{digits}");
        self.0.lock().expect("fake").0.push(sent);
        Ok("VE-fake".to_owned())
    }

    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.lock().expect("fake").1 += 1;
        Ok(code == RIGHT)
    }
}

/// A stored sign-up (an R-37 plumber, `extra` added, `drop` left out): the browser, its binding
/// cookie's `name=value` and the business.
async fn signed(app: &Router, extra: &[(&str, &str)], drop: &[&str]) -> (Browser, String, Uuid) {
    let b = browser(app).await;
    let (status, cookies, ..) = post(app, &b, &typed(&b, extra, drop), "same-origin").await;
    assert_eq!(status, StatusCode::OK);
    let line = cookies.iter().find(|c| c.starts_with(SIGNUP_COOKIE));
    let pair = line.and_then(|l| l.split(';').next()).expect("binding");
    (b, pair.to_owned(), signed_up(&cookies).expect("bound"))
}

/// A step's same-site `POST` from browser `b` holding `binding` (none when empty), with its
/// CSRF token and `fields`: the status, `Cache-Control` and the page.
async fn press(
    app: &Router,
    (b, binding): (&Browser, &str),
    route: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, String, String) {
    let csrf = b.fields.iter().find(|(n, _)| n == "csrf").cloned();
    let typed = fields
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()));
    let posted: Vec<(String, String)> = csrf.into_iter().chain(typed).collect();
    let cookie = [b.pre.as_str(), binding].join("; ");
    let (status, _, cache, html) = post_to(app, (route, &cookie), &posted, "same-origin").await;
    (status, cache, html)
}

/// The sign-up's stored identity step and whether its code is done (D-3779).
async fn progress(pool: &PgPool, business: Uuid) -> (Option<Step>, bool) {
    let mut conn = pool.acquire().await.expect("connection");
    let read = signup_progress(&mut conn, business)
        .await
        .expect("progress");
    (read.step(), read.code_done())
}

/// TD-SEC-45 (SEC-019, SEC-042, SEC-048, D-3641): every identity step with no binding cookie,
/// another sign-up's ID under this cookie's MAC or a made-up cookie is a bare `no-store` 403; a
/// cross-site post is refused as a `csrf_rejected` event; a removed business is refused however
/// good its cookie. None of them sends or checks a code or stores a step.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_identity_steps_refuse_a_bad_binding(pool: PgPool) {
    store_lists(&pool).await;
    let (records, verify) = (Records::new(), FakeVerify::default());
    let app = app_coding(&pool, Some(&records), Some(&verify));
    let (b, binding, business) = signed(&app, &[], &[]).await;
    let value = binding.split_once('=').expect("value").1;
    let other = value.replacen(&business.to_string(), &Uuid::new_v4().to_string(), 1);
    let zeros = format!("{business}.{}.{}", "0".repeat(32), "0".repeat(64));
    let bad = [
        String::new(),
        format!("{SIGNUP_COOKIE}={other}"),
        format!("{SIGNUP_COOKIE}={zeros}"),
    ];
    for route in [
        CODE_SEND_ROUTE,
        NUMBER_OLD_ROUTE,
        LETTER_ROUTE,
        IDENTITY_CHECK_ROUTE,
    ] {
        for cookie in &bad {
            let (status, cache, _) = press(&app, (&b, cookie), route, &[("code", RIGHT)]).await;
            let answer = (status, cache.as_str());
            assert_eq!(answer, (StatusCode::FORBIDDEN, "no-store"), "{route}");
        }
    }
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    let cookie = format!("{}; {binding}", b.pre);
    let csrf = b.fields.iter().find(|(n, _)| n == "csrf").cloned();
    let posted: Vec<(String, String)> = csrf.into_iter().collect();
    let (status, ..) = post_to(&app, (CODE_SEND_ROUTE, &cookie), &posted, "cross-site").await;
    drop(guard);
    assert_eq!(status, StatusCode::FORBIDDEN, "cross-site");
    let logged = events.0.lock().expect("events").clone();
    assert_eq!(
        logged
            .iter()
            .filter(|l| l.contains("\"csrf_rejected\""))
            .count(),
        1
    );
    let mut conn = pool.acquire().await.expect("connection");
    leave(&mut conn, business, Timestamp::now())
        .await
        .expect("removed");
    drop(conn);
    let (status, ..) = press(&app, (&b, &binding), CODE_SEND_ROUTE, &[]).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "D-3641: only Pending or NeedsReview"
    );
    assert_eq!((verify.seen().0.len(), verify.seen().1), (0, 0));
    assert_eq!(progress(&pool, business).await, (None, false));
}

/// TD-BIZ-49, TD-SEC-45 (BIZ-002, BIZ-016, BIZ-026, SEC-019, SEC-034, D-3641, D-3779): an R-37
/// sign-up's out-of-order letter or code check shows the contact screen and calls no Verify; after
/// a browser restart its code goes to the bound record's phone, shown masked, however another ROC
/// number is posted (no new check row); in a browser with a business session a wrong code is
/// refused and the right one goes on to the service area; then every step is refused.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_49_the_identity_code_goes_to_the_bound_record_phone(pool: PgPool) {
    store_lists(&pool).await;
    let (records, verify) = (Records::new(), FakeVerify::default());
    let app = app_coding(&pool, Some(&records), Some(&verify));
    let (b, binding, business) = signed(&app, &[], &[]).await;
    for route in [LETTER_ROUTE, IDENTITY_CHECK_ROUTE] {
        let (status, _, html) = press(&app, (&b, &binding), route, &[("code", RIGHT)]).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains("Send the code"),
            "{route}: the contact screen again"
        );
    }
    assert_eq!(
        (verify.seen().0.len(), verify.seen().1),
        (0, 0),
        "no try used"
    );
    let restarted = browser(&app).await;
    let other_roc = [("roc_number", "ROC200001")];
    let (status, cache, html) =
        press(&app, (&restarted, &binding), CODE_SEND_ROUTE, &other_roc).await;
    assert_eq!(
        (status, cache.as_str()),
        (StatusCode::OK, "no-store"),
        "SEC-034"
    );
    assert!(html.contains("(•••) •••-0142"), "BIZ-026: masked");
    let sent = verify.seen().0;
    assert!(sent.len() == 1 && sent[0].starts_with("Sms:") && sent[0].ends_with("5550142"));
    let mut conn = pool.acquire().await.expect("connection");
    let bound = bound_roc_number(&mut conn, business).await.expect("bound");
    assert_eq!(bound.as_deref(), Some("ROC100001"), "no new check row");
    drop(conn);
    assert_eq!(progress(&pool, business).await, (Some(Step::Text), false));
    let pair = format!("{SESSION_COOKIE}=fake-business-session-value");
    let token = csrf_token(&jar_with(&pair)).expect("session token");
    let fields = vec![("csrf".to_owned(), token)];
    let session = Browser { pre: pair, fields };
    let at = (&session, binding.as_str());
    let (status, _, html) = press(&app, at, IDENTITY_CHECK_ROUTE, &[("code", "000000")]).await;
    let refused = CodeProblem::Refused
        .message()
        .split('\'')
        .next()
        .expect("text");
    assert!(
        status == StatusCode::OK && html.contains(refused),
        "A11Y-030"
    );
    let (status, _, html) = press(&app, at, IDENTITY_CHECK_ROUTE, &[("code", RIGHT)]).await;
    assert!(
        status == StatusCode::OK && html.contains("How far do you travel"),
        "BIZ-058"
    );
    assert_eq!(progress(&pool, business).await, (Some(Step::Text), true));
    let (status, ..) = press(&app, (&b, &binding), CODE_SEND_ROUTE, &[]).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the steps close after the code"
    );
    assert_eq!((verify.seen().0.len(), verify.seen().1), (1, 2));
}

/// TD-SEC-45, TD-BIZ-49 (BIZ-002, BIZ-026, SEC-019): two sign-ups' code sends each reach only
/// their own record contact (the city-list row's phone, then the ROC record's); "That number is
/// old" shows the saved screen.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_a_code_send_reaches_only_its_own_record(pool: PgPool) {
    store_lists(&pool).await;
    let (records, verify) = (Records::new(), FakeVerify::default());
    let app = app_coding(&pool, Some(&records), Some(&verify));
    let city = [
        ("business_name", "Sample Yards"),
        ("category", "house_cleaning"),
    ];
    let drop = ["business_name", "category", "roc_number"];
    let (cb, city_binding, _) = signed(&app, &city, &drop).await;
    let (rb, roc_binding, _) = signed(&app, &[], &[]).await;
    let (status, ..) = press(&app, (&cb, &city_binding), CODE_SEND_ROUTE, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, ..) = press(&app, (&rb, &roc_binding), CODE_SEND_ROUTE, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let sent = verify.seen().0;
    assert!(sent.len() == 2 && sent[0].ends_with("5550150") && sent[1].ends_with("5550142"));
    let (status, _, html) = press(&app, (&cb, &city_binding), NUMBER_OLD_ROUTE, &[]).await;
    assert!(status == StatusCode::OK && html.contains("Your sign-up is saved"));
}

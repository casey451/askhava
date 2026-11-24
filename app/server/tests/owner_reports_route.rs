//! TD-SEC-48 (SEC-005, SEC-034, SEC-042, SEC-048, OPS-036, OPS-039, OPS-040; T305, D-2128,
//! D-3890, D-4897, D-5318): the owner's scorecard with its minutes form, the intake report, the
//! money view with its expense entry and CSV, and the owner help page with its reply box, served
//! by the assembled router against PostgreSQL. They show only behind T303's owner session; no
//! session or a business or homeowner session gets a 4xx linking to `/owner/sign-in`; every
//! answer is `no-store`; a cross-site `POST` (by `Sec-Fetch-Site` or `Origin`) or one with a
//! wrong CSRF token is a 403, changes nothing and is logged as a `csrf_rejected` security event.
//! The owner's pass phrase, token, secret, vendor and amounts are fake.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_core::api::{MIGRATOR, Settings};
use askhava_ops::api::minutes_for_week;
use askhava_portals::api::{
    CookieJar, OwnerAuth, Realm, SecondFactor, bootstrap_owner, csrf_token, ensure_pre_session,
    owner_sign_in, start_session,
};
use askhava_server::owner_reports::ReportSettings;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t305-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t305 owner pass phrase";
const SCORECARD: &str = "/owner/scorecard";
const MINUTES: &str = "/owner/scorecard/minutes";
const INTAKE: &str = "/owner/intake-report";
const MONEY: &str = "/owner/money";
const CSV: &str = "/owner/money/csv";
const EXPENSE: &str = "/owner/money/expense";
const HELP: &str = "/owner/help";
const REPLY: &str = "/owner/help/reply";
const SIGN_IN_LINK: &str = r#"<a href="/owner/sign-in">"#;
/// Each served `GET` and a line only the owner's page shows.
const PAGES: [(&str, &str); 5] = [
    (SCORECARD, "<h1>Pilot scorecard</h1>"),
    (INTAKE, "<h1>Intake report, "),
    (MONEY, "<h1>Money, "),
    (CSV, ""),
    (HELP, "No open requests"),
];
const VENDOR: &str = "Sample Software Co";

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    jar: Jar,
    events: Events,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Reply {
    status: u16,
    text: String,
    location: String,
}

/// The router with default report settings and the owner signed in now with a TOTP code.
async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, now).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let code = totp_code(secret.as_bytes(), now);
    let auth = OwnerAuth::new(HOST, "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, now).await;
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    let parts = RouterParts::from((None, None, None, None));
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        jar: own_jar(&jar.expect("owner signed in")),
        events,
        _guard: guard,
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

fn own_jar(jar: &CookieJar) -> Jar {
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

/// A fresh session of `realm` for a made-up actor, with its pre-session cookie.
async fn realm_session(pool: &PgPool, realm: Realm) -> Jar {
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(1);
    let who = (realm, Uuid::new_v4());
    let jar = start_session(&mut db, CookieJar::new(), who, life, Timestamp::now()).await;
    own_jar(&jar.expect("session"))
}

/// A same-origin request with `jar`'s cookies and `body`.
fn request(method: &str, path: &str, jar: &Jar, body: String) -> Request<Body> {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 48)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// `request`'s answer; every one must be `no-store` (SEC-034).
async fn answer(w: &World, request: Request<Body>) -> Reply {
    let what = format!("{} {}", request.method(), request.uri());
    let response = w.app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let value = |name| {
        let found = parts.headers.get(name).and_then(|v| v.to_str().ok());
        found.unwrap_or_default().to_owned()
    };
    assert!(value(header::CACHE_CONTROL).contains("no-store"), "{what}");
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        location: value(header::LOCATION),
    }
}

async fn get(w: &World, path: &str, jar: &Jar) -> Reply {
    answer(w, request("GET", path, jar, String::new())).await
}

/// A same-origin `POST` of `fields` with `jar`'s CSRF token.
async fn post(w: &World, path: &str, jar: &Jar, fields: &str) -> Reply {
    let body = format!("csrf={}&{fields}", csrf(jar));
    answer(w, request("POST", path, jar, body)).await
}

fn expense_fields() -> String {
    let today = jiff::Zoned::now()
        .with_time_zone(askhava_core::api::arizona())
        .date();
    format!(
        "spent_on={today}&vendor={}&amount=12.50&category=Software&note=",
        VENDOR.replace(' ', "+")
    )
}

/// This week's minutes as `(recruiting, other)`, through `ops`' reader.
async fn week_minutes(w: &World) -> Option<(u32, u32)> {
    let mut db = w.pool.acquire().await.expect("db");
    let week = minutes_for_week(&mut db, Timestamp::now()).await;
    week.expect("minutes").map(|m| (m.recruiting(), m.other()))
}

/// The `security event` log lines captured, `[kind, route]` (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<[String; 2]>>>);

#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl tracing::Subscriber for Events {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let get = |name: &str| {
            let found = fields.0.iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap_or_default()
        };
        if get("message") == "security event" {
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push([get("kind"), get("route")]);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn events(w: &World, kind: &str, route: &str) -> usize {
    let all = w.events.0.lock().unwrap_or_else(PoisonError::into_inner);
    all.iter().filter(|e| e[0] == kind && e[1] == route).count()
}

/// TD-SEC-48 (SEC-005, SEC-034, OPS-036, OPS-039, OPS-040): signed in, the owner gets each page:
/// the scorecard with its minutes form, the intake report, the money view with its expense form,
/// the month's CSV as a download, and the help page; each `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_48_the_owner_sees_each_report(pool: PgPool) {
    let w = world(&pool).await;
    for (path, shows) in PAGES {
        let page = get(&w, path, &w.jar).await;
        assert_eq!(page.status, 200, "{path}: {}", page.text);
        assert!(page.text.contains(shows), "{path}");
        assert!(!page.text.contains(SIGN_IN_LINK), "{path}");
    }
    let scorecard = get(&w, SCORECARD, &w.jar).await.text;
    assert!(scorecard.contains(&format!(r#"action="{MINUTES}""#)));
    assert!(scorecard.contains(&format!(r#"name="csrf" value="{}""#, csrf(&w.jar))));
    let money = get(&w, &format!("{MONEY}?month=2026-10"), &w.jar)
        .await
        .text;
    assert!(
        money.contains("<h1>Money, October 2026</h1>"),
        "the month query"
    );
    assert!(money.contains(&format!(r#"action="{EXPENSE}""#)));
    let report = get(&w, &format!("{INTAKE}?month=2026-10"), &w.jar).await;
    assert!(report.text.contains("<h1>Intake report, October 2026</h1>"));
    let unread = get(&w, &format!("{MONEY}?month=not-a-month"), &w.jar).await;
    assert_eq!(unread.status, 200, "an unreadable month shows this month");
}

/// TD-SEC-48, TD-OPS-15 (OPS-039, FEE-046, FEE-028, SEC-042): the owner's minutes are saved for
/// this week and a bad entry is refused with the values kept; an expense is stored and shown on
/// its month's page after a 303, and a bad one is refused with the values kept.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_48_the_owner_saves_minutes_and_an_expense(pool: PgPool) {
    let w = world(&pool).await;
    let saved = post(&w, MINUTES, &w.jar, "recruiting=90&other=45").await;
    assert_eq!(saved.status, 200, "{}", saved.text);
    assert!(saved.text.contains("Your minutes for this week are saved."));
    assert_eq!(week_minutes(&w).await, Some((90, 45)));
    let bad = post(&w, MINUTES, &w.jar, "recruiting=lots&other=45").await;
    assert_eq!(bad.status, 400);
    assert!(bad.text.contains(r#"value="lots""#), "the value is kept");
    assert_eq!(week_minutes(&w).await, Some((90, 45)), "nothing changed");
    let added = post(&w, EXPENSE, &w.jar, &expense_fields()).await;
    assert_eq!(added.status, 303, "{}", added.text);
    assert!(added.location.starts_with(&format!("{MONEY}?month=")));
    let month = get(&w, &added.location, &w.jar).await;
    assert!(month.text.contains(VENDOR), "the expense is listed");
    let wrong = expense_fields().replace("amount=12.50", "amount=abc");
    let refused = post(&w, EXPENSE, &w.jar, &wrong).await;
    assert_eq!(refused.status, 400);
    assert!(refused.text.contains(r#"value="abc""#), "the value is kept");
    let page = get(&w, &added.location, &w.jar).await.text;
    assert_eq!(page.matches(VENDOR).count(), 1, "one expense stored");
}

/// TD-SEC-48 (SEC-005, SEC-034): no session, a business session and a homeowner session get a
/// 401 linking to `/owner/sign-in` on every page and `POST`, see no report and save nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_48_other_doors_are_sent_to_the_owner_sign_in(pool: PgPool) {
    let w = world(&pool).await;
    let visitor = own_jar(&ensure_pre_session(CookieJar::new()).expect("pre-session"));
    let business = realm_session(&pool, Realm::Business).await;
    let homeowner = realm_session(&pool, Realm::Homeowner).await;
    for (who, jar) in [
        ("none", &visitor),
        ("business", &business),
        ("homeowner", &homeowner),
    ] {
        for (path, shows) in PAGES {
            let page = get(&w, path, jar).await;
            assert_eq!(page.status, 401, "{who} {path}");
            assert!(page.text.contains(SIGN_IN_LINK), "{who} {path}");
            assert!(
                shows.is_empty() || !page.text.contains(shows),
                "{who} {path}"
            );
        }
        let minutes = post(&w, MINUTES, jar, "recruiting=5&other=5").await;
        assert_eq!(minutes.status, 401, "{who} minutes");
        assert!(minutes.text.contains(SIGN_IN_LINK), "{who} minutes");
        let expense = post(&w, EXPENSE, jar, &expense_fields()).await;
        assert_eq!(expense.status, 401, "{who} expense");
        assert!(expense.text.contains(SIGN_IN_LINK), "{who} expense");
        let reply = post(&w, REPLY, jar, "request=&press=reply&text=Sample").await;
        assert!((400..500).contains(&reply.status), "{who} help reply");
    }
    assert_eq!(week_minutes(&w).await, None, "no minutes saved");
    let money = get(&w, MONEY, &w.jar).await.text;
    assert!(!money.contains(VENDOR), "no expense stored");
}

/// The three forged forms of one owner `POST` of `fields` to `path`: cross-site by
/// `Sec-Fetch-Site`, cross-site by `Origin`, and a wrong CSRF token.
fn forged(w: &World, path: &str, fields: &str) -> [(&'static str, Request<Body>); 3] {
    let good = format!("csrf={}&{fields}", csrf(&w.jar));
    let wrong = format!("csrf={}&{fields}", "0".repeat(64));
    let mut by_fetch = request("POST", path, &w.jar, good.clone());
    let cross = HeaderValue::from_static("cross-site");
    by_fetch.headers_mut().insert("sec-fetch-site", cross);
    let mut by_origin = request("POST", path, &w.jar, good);
    by_origin.headers_mut().remove("sec-fetch-site");
    let evil = HeaderValue::from_static("https://evil.example");
    by_origin.headers_mut().insert(header::ORIGIN, evil);
    let by_token = request("POST", path, &w.jar, wrong);
    [
        ("fetch", by_fetch),
        ("origin", by_origin),
        ("token", by_token),
    ]
}

/// TD-SEC-48 (SEC-042, SEC-048): from the owner, a cross-site (`Sec-Fetch-Site` or `Origin`) or
/// wrong-token `POST` to the minutes form, the expense entry or the help reply is a 403 logged as
/// `csrf_rejected` on its route, and none saves anything.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_48_forged_posts_are_refused_and_logged(pool: PgPool) {
    let w = world(&pool).await;
    let expense = expense_fields();
    let reply = format!("request={}&press=reply&text=Sample+reply", Uuid::new_v4());
    for (path, fields) in [
        (MINUTES, "recruiting=30&other=30"),
        (EXPENSE, expense.as_str()),
        (REPLY, reply.as_str()),
    ] {
        for (case, request) in forged(&w, path, fields) {
            assert_eq!(answer(&w, request).await.status, 403, "{path} {case}");
        }
        assert_eq!(events(&w, "csrf_rejected", path), 3, "{path} each logged");
    }
    assert_eq!(week_minutes(&w).await, None, "no minutes saved");
    let money = get(&w, MONEY, &w.jar).await.text;
    assert!(!money.contains(VENDOR), "no expense stored");
}

fn settings(extra: &[(&str, &str)]) -> Settings {
    let base = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
    ];
    let pairs = base.iter().chain(extra);
    Settings::from_pairs(pairs.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))).expect("settings")
}

/// OPS-047, D-5318: `ASKHAVA_PILOT_PUSHED_TRADES` is comma-separated code names, trimmed, empty
/// ones dropped, none by default; the report settings read it beside the scorecard lines, and
/// their defaults equal the settings with no key set.
#[test]
fn the_pushed_trades_and_report_settings_parse() {
    assert!(settings(&[]).pilot_pushed_trades().is_empty());
    let pushed = settings(&[("ASKHAVA_PILOT_PUSHED_TRADES", " plumbing, ,hvac ")]);
    assert_eq!(pushed.pilot_pushed_trades(), ["plumbing", "hvac"]);
    assert!(ReportSettings::from_settings(&pushed).is_ok());
    let defaults = format!("{:?}", ReportSettings::default());
    assert_eq!(
        defaults,
        format!(
            "{:?}",
            ReportSettings::from_settings(&settings(&[])).expect("defaults")
        ),
        "the defaults are the unset settings"
    );
}

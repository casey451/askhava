//! TD-SEC-50 (SEC-006, SEC-034, SEC-042, SEC-043, SEC-048, OPS-001, OPS-003; T310, D-2128,
//! D-3890): the owner home and settings pages and every switch `POST` served by the assembled
//! router against PostgreSQL. They work only behind the owner session: no session, a business or
//! homeowner session gets the 401 linking to `/owner/sign-in` and changes nothing. A pause and the
//! safe-mode acknowledgement need no fresh factor; the fee mode, a category, texting, the
//! Stripe-answer record and the opt-out need one from the last 5 minutes (a stale one is a 401
//! showing the step-up form with no sign-in link and an `authz_denied` event), and each change
//! writes its audit row. Every answer is `no-store`; a cross-site (`Sec-Fetch-Site` or `Origin`)
//! or wrong-token `POST` is a 403 that changes nothing and is a `csrf_rejected` event. The owner's
//! pass phrase, token, authenticator secret, vault keys and phone number are fake.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_catalog::api::{category_list, load_embedded};
use askhava_core::api::MIGRATOR;
use askhava_ops::api::{EXPORT_PAGE_MAX, OWNER_ACK_ACTION, Severity, Switch, export_audit};
use askhava_ops::api::{is_paused, raise_alert_once, record_push};
use askhava_portals::api::{
    CookieJar, OwnerAuth, Realm, SecondFactor, bootstrap_owner, csrf_token, ensure_pre_session,
    owner_sign_in, start_session,
};
use askhava_privacy::api::Vault;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t310-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t310 owner pass phrase";
const HOME: &str = "/owner/home";
const SETTINGS: &str = "/owner/settings";
const SIGN_IN: &str = "/owner/sign-in";

/// Every switch `POST` with a form it accepts.
const POSTS: [(&str, &str); 8] = [
    ("/owner/pause", "&change=pause_all&to=on"),
    ("/owner/fee-mode", "&change=fee_mode&target=flat"),
    ("/owner/category", "&change=category&target=plumbing&to=off"),
    ("/owner/texting", "&change=homeowner_texting&to=off"),
    ("/owner/banner", "&change=banner_off"),
    (
        "/owner/alert-ack",
        "&change=acknowledge&target=00000000-0000-0000-0000-0000000000a1",
    ),
    (
        "/owner/stripe-answer",
        "&answered_on=2026-11-02&reference=fake-ref-1&allows=no",
    ),
    ("/owner/opt-out", "&phone=%2B15205550142"),
];

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
    events: Events,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Reply {
    status: u16,
    text: String,
    no_store: bool,
    location: String,
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let parts = RouterParts::from((None, None, None, None)).with_vault(Some(vault()));
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        secret: secret.as_bytes().to_vec(),
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

/// The owner signed in at `at` with a TOTP code: the factor is fresh for 5 minutes from `at`.
async fn owner_at(w: &World, at: Timestamp) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let code = totp_code(&w.secret, at);
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    own_jar(&jar.expect("owner signed in"))
}

async fn realm_session(w: &World, realm: Realm) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let who = (realm, Uuid::new_v4());
    let life = SignedDuration::from_hours(1);
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
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let value = |name| {
        let found = parts.headers.get(name).and_then(|v| v.to_str().ok());
        found.unwrap_or_default().to_owned()
    };
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        no_store: value(header::CACHE_CONTROL).contains("no-store"),
        location: value(header::LOCATION),
    }
}

async fn get(w: &World, path: &str, jar: &Jar) -> Reply {
    let reply = answer(&w.app, request("GET", path, jar, String::new())).await;
    assert!(reply.no_store, "GET {path} is no-store (SEC-034)");
    reply
}

/// A same-origin `POST` of `fields` with `jar`'s CSRF token.
async fn post(w: &World, path: &str, jar: &Jar, fields: &str) -> Reply {
    let body = format!("csrf={}{fields}", csrf(jar));
    let reply = answer(&w.app, request("POST", path, jar, body)).await;
    assert!(reply.no_store, "POST {path} is no-store (SEC-034)");
    reply
}

/// How many audit rows name `action`.
async fn audited(w: &World, action: &str) -> usize {
    let mut db = w.pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    rows.iter().filter(|r| r.action == action).count()
}

/// How many audit rows there are.
async fn audit_len(w: &World) -> usize {
    let mut db = w.pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    rows.len()
}

async fn paused_all(w: &World) -> bool {
    let mut db = w.pool.acquire().await.expect("db");
    is_paused(&mut db, Switch::All).await.expect("read")
}

/// The fee-mode switch's audit action (`payments`' `FEE_MODE_ACTION`; this file names no
/// `payments` item, as TD-FEE-07's scan of homeowner-realm files requires).
const FEE_MODE_ACTION: &str = "payments.fee_mode_switched";

/// Whether the owner home, read with the owner's `jar`, shows the flat fee mode as stored.
async fn flat(w: &World, jar: &Jar) -> bool {
    let home = get(w, HOME, jar).await;
    assert_eq!(home.status, 200, "{}", home.text);
    home.text.contains(r#"value="flat" checked"#)
}

/// The `security event` log lines captured, `[kind, route]` (SEC-048), as `owner_signin_route.rs`
/// reads them: the row is `portals`' table, which `server` code may not name.
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

/// TD-SEC-50 (SEC-006, SEC-019, SEC-034, SEC-048): no session, a business session and a
/// homeowner session each get the 401 linking to `/owner/sign-in` on both pages and every switch
/// `POST` (with a good CSRF token), see no owner detail and change nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_50_no_owner_gets_the_owner_door(pool: PgPool) {
    let w = world(&pool).await;
    let visitor = own_jar(&ensure_pre_session(CookieJar::new()).expect("pre-session"));
    let before = audit_len(&w).await;
    let business = realm_session(&w, Realm::Business).await;
    let homeowner = realm_session(&w, Realm::Homeowner).await;
    for (who, jar) in [
        ("none", &visitor),
        ("business", &business),
        ("homeowner", &homeowner),
    ] {
        for path in [HOME, SETTINGS] {
            let reply = get(&w, path, jar).await;
            assert_eq!(reply.status, 401, "{who} GET {path}");
            assert!(
                reply.text.contains(SIGN_IN),
                "{who} GET {path} links to sign-in"
            );
            assert!(!reply.text.contains("Pause or resume"), "{who} {path}");
        }
        for (path, fields) in POSTS {
            let reply = post(&w, path, jar, fields).await;
            assert!((400..500).contains(&reply.status), "{who} POST {path}");
            assert!(!reply.text.contains("Pause or resume"), "{who} POST {path}");
        }
    }
    for (path, fields) in POSTS {
        let reply = post(&w, path, &business, fields).await;
        assert_eq!(reply.status, 401, "a business session is refused at {path}");
        assert!(reply.text.contains(SIGN_IN), "{path} links to sign-in");
        assert!(events(&w, "authz_denied", path) >= 1, "{path} is logged");
    }
    assert!(!paused_all(&w).await, "nothing paused");
    assert_eq!(audit_len(&w).await, before, "no change logged");
    let owner = owner_at(&w, Timestamp::now()).await;
    assert!(!flat(&w, &owner).await, "fee mode unchanged");
}

/// TD-SEC-50, TD-OPS-01 (OPS-003, SEC-006, SEC-043): the signed-in owner sees both pages; a pause
/// needs no fresh factor and is logged; a stale factor refuses the fee mode with the step-up form
/// (no sign-in link), changes nothing and is an `authz_denied` event.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_50_pages_and_a_pause_behind_the_owner_session(pool: PgPool) {
    let w = world(&pool).await;
    let stale = owner_at(&w, Timestamp::now() - SignedDuration::from_mins(6)).await;
    let home = get(&w, HOME, &stale).await;
    assert_eq!(home.status, 200, "{}", home.text);
    assert!(home.text.contains("Pause or resume") && home.text.contains("Sign-ups waiting"));
    let settings = get(&w, SETTINGS, &stale).await;
    assert_eq!(settings.status, 200, "{}", settings.text);
    assert!(settings.text.contains("Stripe"));
    for (to, paused) in [("on", true), ("off", false)] {
        let reply = post(
            &w,
            "/owner/pause",
            &stale,
            &format!("&change=pause_all&to={to}"),
        )
        .await;
        assert_eq!((reply.status, reply.location.as_str()), (303, HOME));
        assert_eq!(paused_all(&w).await, paused, "pause_all {to}");
    }
    assert_eq!(
        audited(&w, "switch_on").await + audited(&w, "switch_off").await,
        2
    );
    for (path, fields) in POSTS.iter().skip(1).filter(|p| !p.0.contains("ack")) {
        let reply = post(&w, path, &stale, fields).await;
        assert_eq!(
            reply.status, 401,
            "{path} needs a fresh factor: {}",
            reply.text
        );
        assert!(
            reply.text.contains("Confirm it"),
            "{path} shows the step-up form"
        );
        assert!(
            !reply.text.contains(SIGN_IN),
            "{path}: no sign-in link (D-3878)"
        );
        assert_eq!(events(&w, "authz_denied", path), 1, "{path} logged");
    }
    assert!(!flat(&w, &stale).await);
    assert_eq!(audited(&w, FEE_MODE_ACTION).await, 0, "nothing logged");
    let wrong = post(&w, "/owner/fee-mode", &stale, "&change=pause_all&to=on").await;
    assert_eq!(wrong.status, 400, "a form for another route is refused");
    assert!(!paused_all(&w).await);
}

/// TD-SEC-50, TD-BIZ-07 (FEE-007, LIC-002, OPS-001, CON-035, PICK-048, SEC-043): with a fresh
/// factor the fee mode, a category, texting, the alert acknowledgement, the Stripe answer and the
/// opt-out are made and logged.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_50_fresh_changes_are_made_and_logged(pool: PgPool) {
    let w = world(&pool).await;
    let fresh = owner_at(&w, Timestamp::now()).await;
    let reply = post(
        &w,
        "/owner/fee-mode",
        &fresh,
        "&change=fee_mode&target=flat",
    )
    .await;
    assert_eq!((reply.status, reply.location.as_str()), (303, HOME));
    assert!(flat(&w, &fresh).await);
    assert_eq!(audited(&w, FEE_MODE_ACTION).await, 1);
    let mut db = w.pool.acquire().await.expect("db");
    let list = category_list(&mut db).await.expect("list");
    let code = list
        .categories
        .iter()
        .find(|c| c.enabled)
        .expect("one on")
        .code
        .clone();
    let alert = raise_alert_once(
        &mut db,
        "fake_high",
        Severity::High,
        ("ops.test", Uuid::new_v4()),
    );
    let alert = alert.await.expect("alert").expect("raised");
    record_push(&mut db, alert, Timestamp::now())
        .await
        .expect("push");
    drop(db);
    let category = post(
        &w,
        "/owner/category",
        &fresh,
        &format!("&change=category&target={code}&to=off"),
    )
    .await;
    assert_eq!(
        (category.status, category.location.as_str()),
        (303, SETTINGS)
    );
    assert_eq!(audited(&w, "catalog.category_switched").await, 1);
    let texting = post(
        &w,
        "/owner/texting",
        &fresh,
        "&change=homeowner_texting&to=off",
    )
    .await;
    assert_eq!(texting.status, 303);
    let ack = post(
        &w,
        "/owner/alert-ack",
        &fresh,
        &format!("&change=acknowledge&target={alert}"),
    )
    .await;
    assert_eq!(ack.status, 303);
    assert_eq!(
        audited(&w, OWNER_ACK_ACTION).await,
        1,
        "the safe-mode acknowledgement"
    );
    let stripe = post(&w, POSTS[6].0, &fresh, POSTS[6].1).await;
    assert_eq!(stripe.status, 200, "{}", stripe.text);
    assert!(stripe.text.contains("Saved. Stripe"));
    let opt_out = post(&w, POSTS[7].0, &fresh, POSTS[7].1).await;
    assert_eq!(opt_out.status, 200, "{}", opt_out.text);
    assert!(opt_out.text.contains("Done.") && !opt_out.text.contains("5550142"));
    let bridge = post(
        &w,
        "/owner/call-homeowner",
        &fresh,
        &format!("&alert={alert}&press={}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(
        bridge.status, 400,
        "a High alert about no inbound text is not bridged"
    );
    let back = post(
        &w,
        "/owner/fee-mode",
        &fresh,
        "&change=fee_mode&target=tiers",
    )
    .await;
    assert_eq!(back.status, 303);
    assert!(!flat(&w, &fresh).await);
}

/// TD-SEC-50 (SEC-042, SEC-048): on every owner `POST`, a cross-site request (by
/// `Sec-Fetch-Site` or by `Origin`) or a wrong CSRF token is a 403 `csrf_rejected` event that
/// changes nothing, even for the signed-in owner with a fresh factor.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_50_cross_site_and_wrong_token_posts_change_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let owner = owner_at(&w, Timestamp::now()).await;
    let before = audit_len(&w).await;
    let mut all = POSTS.to_vec();
    all.push(("/owner/call-homeowner", "&alert=x&press=y"));
    for (path, fields) in all {
        let good = format!("csrf={}{fields}", csrf(&owner));
        let wrong = format!("csrf={}{fields}", "0".repeat(64));
        let mut by_fetch = request("POST", path, &owner, good.clone());
        by_fetch
            .headers_mut()
            .insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        let mut by_origin = request("POST", path, &owner, good);
        by_origin.headers_mut().remove("sec-fetch-site");
        let evil = HeaderValue::from_static("https://evil.example");
        by_origin.headers_mut().insert(header::ORIGIN, evil);
        let by_token = request("POST", path, &owner, wrong);
        for (case, request) in [
            ("Sec-Fetch-Site", by_fetch),
            ("Origin", by_origin),
            ("token", by_token),
        ] {
            let reply = answer(&w.app, request).await;
            assert_eq!(reply.status, 403, "{path} by {case}");
            assert!(reply.no_store, "{path} by {case} is no-store");
        }
        assert_eq!(events(&w, "csrf_rejected", path), 3, "{path}");
    }
    assert!(!paused_all(&w).await, "nothing paused");
    assert!(!flat(&w, &owner).await, "fee mode unchanged");
    assert_eq!(audit_len(&w).await, before, "no change logged");
}

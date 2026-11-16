//! TD-SEC-46 (SEC-005, SEC-007, SEC-014, SEC-034, SEC-042, SEC-043, SEC-044, SEC-046, SEC-048;
//! T303, D-2128, D-2206, D-3878): the owner sign-in, step-up and sign-out served by the assembled
//! router against PostgreSQL. Only the password plus a second factor signs in; a wrong password or
//! code signs no one in and says nothing about which was wrong; no session, a business or
//! homeowner session, an owner session idle 31 minutes and one older than 12 hours each get a 401
//! linking to `/owner/sign-in` on every owner route; sign-out and "Sign out everywhere" end the
//! sessions; every answer is `no-store`; a cross-site or wrong-token `POST` is a 403 that changes
//! nothing and is a `csrf_rejected` event; password tries past `admit_owner_password`'s limit are
//! refused before the password is checked. The passkey cases (D-3878): the challenge route
//! answers, a fake assertion signs no one in, and `passkey.js` is served. SEC-046's proof types
//! are proven by `portals`' trybuild case; here an owner session reads nothing outside its door.
//! The owner's pass phrase, token and authenticator secret are fake and made for this file.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use askhava_core::api::MIGRATOR;
use askhava_portals::api::{
    CookieJar, OwnerAuth, Realm, SecondFactor, bootstrap_owner, csrf_token, owner_sign_in,
    start_session,
};
use askhava_server::request_timeout::RequestTimeout;
use askhava_server::static_files::StaticRoot;
use askhava_server::{Hooks, RouterParts, build_router};
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
/// A fake bootstrap token (32 or more characters) and a fake pass phrase (15 or more, not breached).
const TOKEN: &str = "fake-t303-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t303 owner pass phrase";
const SIGN_IN: &str = "/owner/sign-in";
const STEP_UP: &str = "/owner/step-up";
const SIGN_OUT: &str = "/owner/sign-out";
const SIGN_OUT_ALL: &str = "/owner/sign-out-everywhere";
const PASSKEY: &str = "/owner/sign-in/passkey";

/// The assembled router, the pool and the owner's authenticator secret.
struct World {
    app: Router,
    pool: PgPool,
    secret: Vec<u8>,
}

/// A browser's cookies by name.
type Jar = BTreeMap<String, String>;

/// One answer: status, body, `Cache-Control`, `Location` and the cookies it set (empty: cleared).
struct Reply {
    status: u16,
    text: String,
    no_store: bool,
    location: String,
    set: Jar,
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now()).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let app = build_router(
        pool.clone(),
        Router::new(),
        RouterParts::from((None, None, None, None)),
    );
    World {
        app,
        pool: pool.clone(),
        secret: secret.as_bytes().to_vec(),
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `at` (30-second steps).
fn totp_code(secret: &[u8], at: Timestamp) -> String {
    let step = u64::try_from(at.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// A 6-digit code no step from a minute ago to a minute ahead uses.
fn wrong_code(secret: &[u8]) -> String {
    let now = Timestamp::now();
    let near: Vec<String> = (-2..=2)
        .map(|k| totp_code(secret, now + SignedDuration::from_secs(30 * k)))
        .collect();
    (0..100)
        .map(|n| format!("{:06}", 100_000 + n))
        .find(|code| !near.contains(code))
        .expect("a wrong code")
}

fn portals_jar(jar: &Jar) -> CookieJar {
    let mut headers = HeaderMap::new();
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&cookies.join("; ")).expect("cookie"),
    );
    CookieJar::from_headers(&headers)
}

fn own_jar(jar: &CookieJar) -> Jar {
    jar.iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

/// A same-origin request from 203.0.113.`ip` with `jar`'s cookies and, for a `POST`, `body`.
fn request(method: &str, path: &str, jar: &Jar, body: &str, ip: u8) -> Request<Body> {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, ip)), 40_000);
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
    let set = parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next()?.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        no_store: value(header::CACHE_CONTROL).contains("no-store"),
        location: value(header::LOCATION),
        set,
    }
}

/// `jar` after the cookies `reply` set (an empty value clears one).
fn absorb(mut jar: Jar, reply: &Reply) -> Jar {
    for (name, value) in &reply.set {
        if value.is_empty() {
            jar.remove(name);
        } else {
            jar.insert(name.clone(), value.clone());
        }
    }
    jar
}

fn csrf(jar: &Jar) -> String {
    csrf_token(&portals_jar(jar)).unwrap_or_default()
}

async fn get(w: &World, path: &str, jar: &Jar) -> Reply {
    answer(&w.app, request("GET", path, jar, "", 1)).await
}

/// A same-origin `POST` of `fields` with `jar`'s CSRF token from 203.0.113.`ip`.
async fn post(w: &World, path: &str, jar: &Jar, fields: &str, ip: u8) -> Reply {
    let body = format!("csrf={}{fields}", csrf(jar));
    answer(&w.app, request("POST", path, jar, &body, ip)).await
}

/// The anonymous browser: the sign-in page's pre-session cookie.
async fn visitor(w: &World) -> Jar {
    let page = get(w, SIGN_IN, &Jar::new()).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.no_store, "the sign-in page is no-store (SEC-034)");
    absorb(Jar::new(), &page)
}

/// A sign-in `POST` of `password` and `code` from 203.0.113.`ip`.
async fn sign_in(w: &World, jar: &Jar, (password, code): (&str, &str), ip: u8) -> Reply {
    let fields = format!("&password={}&code={code}", password.replace(' ', "+"));
    post(w, SIGN_IN, jar, &fields, ip).await
}

/// An owner session made through `portals`' `owner_sign_in` at `at` (a step before any the
/// served routes use in the same test).
async fn owner_at(w: &World, at: Timestamp) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let code = totp_code(&w.secret, at);
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    own_jar(&jar.expect("owner signed in"))
}

/// A business or homeowner session.
async fn realm_session(w: &World, realm: Realm) -> Jar {
    let mut db = w.pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(1);
    let jar = start_session(
        &mut db,
        CookieJar::new(),
        (realm, Uuid::new_v4()),
        life,
        Timestamp::now(),
    );
    own_jar(&jar.await.expect("session"))
}

/// Runs `sql` with `jar`'s session token as `$1`.
async fn on_session(w: &World, jar: &Jar, sql: &'static str) {
    let token = jar.get("__Host-askhava-session").expect("a session cookie");
    sqlx::query(sql)
        .bind(token)
        .execute(&w.pool)
        .await
        .expect("update");
}

async fn events(w: &World, kind: &str, route: &str) -> i64 {
    let sql = "SELECT count(*) FROM portals.security_event WHERE kind = $1 AND route = $2";
    let found = sqlx::query_scalar(sql).bind(kind).bind(route);
    found.fetch_one(&w.pool).await.expect("events")
}

async fn owner_sessions(w: &World) -> i64 {
    let sql = "SELECT count(*) FROM portals.owner_session";
    sqlx::query_scalar(sql)
        .fetch_one(&w.pool)
        .await
        .expect("sessions")
}

/// The owner door's refusal: a 401 linking to the owner sign-in, `no-store`, no owner page.
fn refused(reply: &Reply, case: &str) {
    assert_eq!(reply.status, 401, "{case}: {}", reply.text);
    assert!(reply.text.contains("href=\"/owner/sign-in\""), "{case}");
    assert!(reply.no_store, "{case}: no-store (SEC-034)");
    assert!(!reply.text.contains("Confirm it's you"), "{case}");
}

/// TD-SEC-46 (SEC-005, SEC-043, SEC-048): a wrong password or code signs no one in and the two
/// failures read the same; the password and a TOTP code sign in; the step-up takes a fresh code
/// once; sign-out ends the session on the server.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_password_and_code_sign_in_step_up_and_sign_out(pool: PgPool) {
    let w = world(&pool).await;
    let jar = visitor(&w).await;
    let now = Timestamp::now();
    let wrong_password = sign_in(
        &w,
        &jar,
        ("fake wrong pass phrase t303", &totp_code(&w.secret, now)),
        2,
    )
    .await;
    let wrong_code = sign_in(&w, &jar, (PASSWORD, &wrong_code(&w.secret)), 2).await;
    for (case, reply) in [
        ("wrong password", &wrong_password),
        ("wrong code", &wrong_code),
    ] {
        assert_eq!(reply.status, 401, "{case}");
        assert!(
            reply.no_store && reply.text.contains("That didn't work"),
            "{case}"
        );
        assert!(
            !reply.set.contains_key("__Host-askhava-session"),
            "{case}: no session"
        );
    }
    assert_eq!(
        wrong_password.text, wrong_code.text,
        "neither says which was wrong"
    );
    assert_eq!(events(&w, "auth_failed", SIGN_IN).await, 2);
    assert_eq!(owner_sessions(&w).await, 0);
    let code = totp_code(&w.secret, Timestamp::now());
    let signed = sign_in(&w, &jar, (PASSWORD, &code), 2).await;
    assert_eq!(
        (signed.status, signed.location.as_str()),
        (303, STEP_UP),
        "{}",
        signed.text
    );
    assert!(signed.no_store);
    let owner = absorb(jar, &signed);
    let page = get(&w, STEP_UP, &owner).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.no_store && page.text.contains("Confirm it's you"));
    let reused = post(&w, STEP_UP, &owner, &format!("&code={code}"), 2).await;
    assert_eq!(reused.status, 400, "a used code is refused");
    assert!(reused.text.contains("error-summary") && !reused.text.contains(SIGN_IN));
    let next = totp_code(&w.secret, Timestamp::now() + SignedDuration::from_secs(30));
    let fresh = post(&w, STEP_UP, &owner, &format!("&code={next}"), 2).await;
    assert_eq!(fresh.status, 200, "{}", fresh.text);
    assert!(fresh.no_store && fresh.text.contains("For the next 5 minutes"));
    let out = post(&w, SIGN_OUT, &owner, "", 2).await;
    assert_eq!((out.status, out.location.as_str()), (303, SIGN_IN));
    assert!(
        out.no_store
            && out
                .set
                .get("__Host-askhava-session")
                .is_some_and(String::is_empty)
    );
    refused(&get(&w, STEP_UP, &owner).await, "the signed-out cookie");
    assert_eq!(
        owner_sessions(&w).await,
        0,
        "logout ended the server session"
    );
}

/// TD-SEC-46 (SEC-005, SEC-006, SEC-043, SEC-048): no session, a business or homeowner session,
/// an owner session idle for 31 minutes and one older than 12 hours each get the 401 on every
/// owner route; a refused session is an `authz_denied` event; no owner-creation route exists.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_every_owner_route_refuses_all_but_a_live_owner(pool: PgPool) {
    let w = world(&pool).await;
    let start = Timestamp::now() - SignedDuration::from_mins(10);
    let idle = owner_at(&w, start).await;
    on_session(&w, &idle, "UPDATE portals.owner_session SET last_seen_at = now() - interval '31 minutes' WHERE session_id = (SELECT id FROM portals.session WHERE token_sha256 = sha256(convert_to($1, 'UTF8')))").await;
    let old = owner_at(&w, start + SignedDuration::from_secs(30)).await;
    on_session(&w, &old, "UPDATE portals.session SET created_at = now() - interval '12 hours 1 minute', expires_at = now() - interval '1 minute' WHERE token_sha256 = sha256(convert_to($1, 'UTF8'))").await;
    on_session(
        &w,
        &old,
        "UPDATE portals.owner_session SET last_seen_at = now() WHERE session_id = (SELECT id FROM portals.session WHERE token_sha256 = sha256(convert_to($1, 'UTF8')))",
    )
    .await;
    let live = owner_at(&w, start + SignedDuration::from_secs(60)).await;
    let actors = [
        ("no session", visitor(&w).await),
        ("business", realm_session(&w, Realm::Business).await),
        ("homeowner", realm_session(&w, Realm::Homeowner).await),
        ("owner idle 31 minutes", idle),
        ("owner older than 12 hours", old),
    ];
    for (who, jar) in &actors {
        refused(&get(&w, STEP_UP, jar).await, &format!("{who} GET step-up"));
        for path in [STEP_UP, SIGN_OUT, SIGN_OUT_ALL] {
            let fields = if path == STEP_UP { "&code=123456" } else { "" };
            let reply = post(&w, path, jar, fields, 3).await;
            refused(&reply, &format!("{who} POST {path}"));
        }
    }
    assert!(
        events(&w, "authz_denied", STEP_UP).await >= 2,
        "the realm sessions were logged"
    );
    assert_eq!(
        get(&w, STEP_UP, &live).await.status,
        200,
        "a live owner still passes"
    );
    for method in ["GET", "POST"] {
        let reply = answer(&w.app, request(method, "/owner/bootstrap", &live, "", 3)).await;
        assert_eq!(reply.status, 404, "no owner-creation route (SEC-044)");
    }
}

/// TD-SEC-46 (SEC-043): "Sign out everywhere" ends every owner session, this one included.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_sign_out_everywhere_ends_every_session(pool: PgPool) {
    let w = world(&pool).await;
    let start = Timestamp::now() - SignedDuration::from_mins(5);
    let laptop = owner_at(&w, start).await;
    let phone = owner_at(&w, start + SignedDuration::from_secs(30)).await;
    assert_eq!(owner_sessions(&w).await, 2);
    let out = post(&w, SIGN_OUT_ALL, &laptop, "", 4).await;
    assert_eq!(
        (out.status, out.location.as_str()),
        (303, SIGN_IN),
        "{}",
        out.text
    );
    assert!(out.no_store);
    refused(&get(&w, STEP_UP, &laptop).await, "the signed-out laptop");
    refused(&get(&w, STEP_UP, &phone).await, "the other device");
    assert_eq!(owner_sessions(&w).await, 0);
}

/// TD-SEC-46 (SEC-042, SEC-048): on every owner `POST`, a cross-site request (by
/// `Sec-Fetch-Site` or by `Origin`) or a wrong CSRF token is a 403 `csrf_rejected` event and
/// changes nothing: no sign-in, no sign-out.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_cross_site_and_wrong_token_posts_change_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let owner = owner_at(&w, Timestamp::now() - SignedDuration::from_mins(5)).await;
    let code = totp_code(&w.secret, Timestamp::now());
    let fields = format!("&password={}&code={code}", PASSWORD.replace(' ', "+"));
    for path in [SIGN_IN, PASSKEY, STEP_UP, SIGN_OUT, SIGN_OUT_ALL] {
        let extra = if path == SIGN_IN { fields.as_str() } else { "" };
        let good = format!("csrf={}{extra}", csrf(&owner));
        let wrong = format!("csrf={}{extra}", "0".repeat(64));
        let mut by_fetch = request("POST", path, &owner, &good, 5);
        by_fetch
            .headers_mut()
            .insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        let mut by_origin = request("POST", path, &owner, &good, 5);
        by_origin.headers_mut().remove("sec-fetch-site");
        by_origin.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        let by_token = request("POST", path, &owner, &wrong, 5);
        for (case, request) in [
            ("Sec-Fetch-Site", by_fetch),
            ("Origin", by_origin),
            ("token", by_token),
        ] {
            let reply = answer(&w.app, request).await;
            assert_eq!(reply.status, 403, "{path} by {case}");
            assert!(reply.set.is_empty(), "{path} by {case} set no cookie");
        }
        assert_eq!(events(&w, "csrf_rejected", path).await, 3, "{path}");
    }
    assert_eq!(owner_sessions(&w).await, 1, "nothing signed in or out");
    assert_eq!(
        get(&w, STEP_UP, &owner).await.status,
        200,
        "the session still works"
    );
}

/// TD-SEC-46 (SEC-005, D-3878): the passkey challenge route answers (no passkey registered: a
/// 404, `no-store`), a fake passkey answer signs no one in, and `passkey.js` is served.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_passkey_routes(pool: PgPool) {
    let w = world(&pool).await;
    let jar = visitor(&w).await;
    let issued = post(&w, PASSKEY, &jar, "", 6).await;
    assert_eq!(
        issued.status, 404,
        "no passkey is registered: {}",
        issued.text
    );
    assert!(issued.no_store);
    let fake = format!(
        "&password={}&challenge={}&passkey=%7B%22id%22%3A%22fake%22%7D",
        PASSWORD.replace(' ', "+"),
        Uuid::new_v4()
    );
    let signed = post(&w, SIGN_IN, &jar, &fake, 6).await;
    assert_eq!(signed.status, 401, "a fake assertion signs no one in");
    assert!(!signed.set.contains_key("__Host-askhava-session"));
    assert_eq!(owner_sessions(&w).await, 0);
    let root = std::env::temp_dir().join(format!("askhava-t303-{}", std::process::id()));
    std::fs::create_dir_all(root.join("portals")).expect("folder");
    std::fs::write(
        root.join("portals").join("passkey.js"),
        "export const fakeT303 = 1;\n",
    )
    .expect("script");
    let hooks: Hooks = (None, None, None, None);
    let parts = (
        hooks,
        RequestTimeout::DEFAULT,
        StaticRoot::Built(PathBuf::from(&root)),
    );
    let app = build_router(pool.clone(), Router::new(), parts);
    let script = answer(
        &app,
        request("GET", "/static/portals/passkey.js", &Jar::new(), "", 6),
    )
    .await;
    assert_eq!(script.status, 200);
    assert!(script.text.contains("fakeT303"));
    let page = get(&w, SIGN_IN, &jar).await;
    assert!(page.text.contains("action=\"/owner/sign-in\""));
}

/// TD-SEC-46 (SEC-014, SEC-020, D-1951, D-2206): past `admit_owner_password`'s per-IP limit a
/// password `POST` is refused before the password is checked: the right password and code sign
/// no one in, no `auth_failed` event is added, and the code stays unused for another address.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_46_password_tries_past_the_limit_are_refused_first(pool: PgPool) {
    let w = world(&pool).await;
    let jar = visitor(&w).await;
    let wrong = wrong_code(&w.secret);
    for _ in 0..5 {
        let reply = sign_in(&w, &jar, ("fake wrong pass phrase t303", &wrong), 7).await;
        assert_eq!(reply.status, 401);
    }
    assert_eq!(events(&w, "auth_failed", SIGN_IN).await, 5);
    let code = totp_code(&w.secret, Timestamp::now());
    let limited = sign_in(&w, &jar, (PASSWORD, &code), 7).await;
    assert_eq!(limited.status, 401, "refused past the limit");
    assert!(!limited.set.contains_key("__Host-askhava-session"));
    assert_eq!(
        events(&w, "auth_failed", SIGN_IN).await,
        5,
        "the password was not checked"
    );
    let elsewhere = sign_in(&w, &jar, (PASSWORD, &code), 8).await;
    assert_eq!(
        elsewhere.status, 303,
        "the code was never used: {}",
        elsewhere.text
    );
}

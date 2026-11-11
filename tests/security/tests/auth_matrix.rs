//! The authorization matrix over the route table (TD-SEC-01 matrix cases, TD-BIZ-31 and
//! TD-BIZ-13's push case; SEC-004, SEC-005, SEC-006, SEC-008, SEC-019, SEC-048, BIZ-041, D-1945
//! to D-1948). Every name, ID and token is fake.
//!
//! The route table is generated, not typed: every `.route(` call in `app/*/src` (constants
//! resolved, `#[cfg(test)]` parts and `src/tests/` left out, the `/__test/` control routes left to
//! TD-SEC-15) is a served route with its methods, and every `const *_ROUTE` or `*_PATH` naming a
//! path is a declared route, served or not. Each path falls through [`door`] to its door; a path
//! no door names fails the run, so a new kind of route cannot go untested. Six actors (anonymous,
//! homeowners A and B, businesses X and Y, the owner) try every route: a link route (`/q`, `/f`,
//! and `/c` with a valid check-in answer code, D-2429) over HTTP through the portals routers merged as `server::build_router` merges them, a webhook
//! through the adapter's signature check, a homeowner or business route through the realm guard
//! its handler must call first (the homeowner realm, the only one `homeowner_proof` mints for;
//! `business_principal` with `may`), since those routes are not served yet; one served fails the
//! run until the matrix probes it over HTTP. Homeowner A versus B isolation is proven for the `/q`,
//! `/f` and `/c` tokens only, not yet for `/h` (D-1945). Sign-in routes are open to everyone; their
//! rules are `business_signin.rs`', `owner_auth.rs`' and `homeowner_portal::tests`'; only the
//! owner has pause, refund and release (`owner_auth.rs`). Every refusal of a served route is one
//! `csrf_rejected` event with route template, request ID and session ID; webhook signature
//! failures and realm-guard refusals are not events yet, so SEC-048 is only partly met (D-1948).
//!
//! T348 (TD-SEC-61, SEC-019, SEC-034, SEC-048, D-2440) adds three doors: `Public` (`/healthz`,
//! `/help` and below, the five legal pages, and any route the `site` module serves outside a realm
//! prefix), `ActionLink` (`/b/a/`, T148's signed email links, before the business door) and
//! `Owner` (`/owner` and below, its sign-in paths staying `SignIn`). Where each door is probed:
//! `Link` and served `Asset` routes over HTTP here; `Webhook` through the signature check here;
//! `Homeowner` and `Business` through the realm guard here and, once served, over HTTP in
//! `app/server/tests/door_matrix.rs`; `Owner`, `ActionLink` and `Public` over HTTP in
//! `door_matrix.rs` only; `SignIn` by the sign-in tests named above. A served path of the
//! homeowner, business, owner, action-link or public door must have a `yes` row in
//! `docs/architecture.md` section 13 (which `route_wiring.rs` proves served), so `door_matrix.rs`
//! reaches it. Both files assert the shared case list `tests/security/door_cases.txt`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use askhava_adapters::api::{Delivery, TwilioVerify, WebhookVendor, verify_webhook};
use askhava_businesses::api::{add_owner, create as create_business};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, job_number, move_to};
use askhava_ops::api::Actor as AuditActor;
use askhava_portals::api::{
    CookieJar, Need, Realm, business_principal, channel_choices, checkin_routes, csrf_token,
    current_session, ensure_pre_session, feedback_routes, pick_flow_routes, quote_page_routes,
    start_session,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
/// A well-formed link token no job was ever given.
const UNISSUED: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
/// A valid answer code of the emailed check-in, filling `/c/{token}/{answer}` (TXT-034, D-2429).
const ANSWER: &str = "3-done";
/// Words a push web app would bring (BIZ-041, TD-BIZ-31).
const PUSH_WORDS: [&str; 7] = [
    "serviceworker",
    "service-worker",
    "manifest",
    "pushmanager",
    "push-subscri",
    "/push",
    "turn on job alerts",
];
/// The arrows from Intake to Done (JOB-002).
const TO_DONE: [JobStatus; 7] = [
    JobStatus::Intake,
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
    JobStatus::PaymentPending,
    JobStatus::Connected,
    JobStatus::Done,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Actor {
    Anonymous,
    HomeownerA,
    HomeownerB,
    BusinessX,
    BusinessY,
    Owner,
}

const ACTORS: [Actor; 6] = [
    Actor::Anonymous,
    Actor::HomeownerA,
    Actor::HomeownerB,
    Actor::BusinessX,
    Actor::BusinessY,
    Actor::Owner,
];

/// How a route decides who may pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Door {
    /// The path's link token is the key: only its homeowner, and only to her own job (SEC-019).
    Link,
    /// A public file.
    Asset,
    /// A signed vendor delivery; no person's session opens it (SEC-016).
    Webhook,
    /// A sign-in page, open to everyone (SEC-004, SEC-005, SEC-008).
    SignIn,
    /// The homeowner portal: a homeowner session on her own record (SEC-008, SEC-046).
    Homeowner,
    /// The business portal: a business session, scoped to its own business (SEC-004, D-1726).
    Business,
    /// A public page: every actor, no session needed, no personal data (SEC-019, D-2440).
    Public,
    /// A signed email action link: the link is the key, no session opens it (SEC-025, SEC-026).
    ActionLink,
    /// The owner portal: the owner's session only (SEC-005, SEC-006, SEC-007).
    Owner,
}

/// The paths of the public door besides `/help` and below and the `site` module's routes.
const PUBLIC_PATHS: [&str; 6] = [
    "/healthz",
    "/terms",
    "/privacy",
    "/sms-terms",
    "/your-data",
    "/business-data",
];

/// The door of `path`, which the `site` module serves when `site`; a path no door names fails the
/// run (SEC-019: every route is tested). A `site` route under a realm prefix keeps that realm's
/// door, the stricter one (D-3089).
fn door(path: &str, site: bool) -> Door {
    let under = |root: &str| path == root || path.starts_with(&format!("{root}/"));
    if under("/webhooks") {
        Door::Webhook
    } else if under("/static") {
        Door::Asset
    } else if path.contains("/sign-in") || path == "/b/signup" {
        Door::SignIn
    } else if ["/q/{token}", "/f/{token}", "/c/{token}"]
        .iter()
        .any(|root| path.starts_with(root))
    {
        Door::Link
    } else if PUBLIC_PATHS.contains(&path) || under("/help") {
        Door::Public
    } else if path.starts_with("/b/a/") {
        Door::ActionLink
    } else if under("/owner") {
        Door::Owner
    } else if under("/h") {
        Door::Homeowner
    } else if under("/b") {
        Door::Business
    } else if site {
        Door::Public
    } else {
        panic!("{path} has no door in auth_matrix.rs: add its matrix rule (SEC-019)")
    }
}

/// The expected answer of the matrix: whether `actor` passes `door`.
fn allowed(door: Door, actor: Actor) -> bool {
    match door {
        Door::Asset | Door::SignIn | Door::Public => true,
        Door::Webhook | Door::ActionLink => false,
        Door::Owner => actor == Actor::Owner,
        Door::Link | Door::Homeowner => matches!(actor, Actor::HomeownerA | Actor::HomeownerB),
        Door::Business => matches!(actor, Actor::BusinessX | Actor::BusinessY),
    }
}

/// The non-test Rust of every `app/*/src` file, with its module's folder name.
fn app_sources() -> Vec<(String, String)> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() && !path.ends_with("tests") {
                collect(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    let mut found = Vec::new();
    for module in fs::read_dir(root).expect("app") {
        let module = module.expect("module").path();
        let name = module
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let mut files = Vec::new();
        if module.join("src").is_dir() {
            collect(&module.join("src"), &mut files);
        }
        for file in files {
            let text = fs::read_to_string(file).expect("source");
            found.push((name.clone(), without_test_modules(&text)));
        }
    }
    found
}

/// `source` without its top-level `#[cfg(test)] mod name { ... }` blocks.
fn without_test_modules(source: &str) -> String {
    let mut kept = String::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        let opens = lines
            .peek()
            .is_some_and(|l| l.starts_with("mod ") && l.ends_with('{'));
        if line == "#[cfg(test)]" && opens {
            lines.by_ref().find(|l| *l == "}");
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

/// Every code line of `sources` (comments left out) with its module.
fn code_lines(sources: &[(String, String)]) -> impl Iterator<Item = (&str, &str)> {
    let lines = sources
        .iter()
        .flat_map(|(module, s)| s.lines().map(move |l| (module.as_str(), l)));
    lines.filter(|(_, l)| !l.trim_start().starts_with("//"))
}

/// Every `const NAME: &str = "...";` in `sources`.
fn constants(sources: &[(String, String)]) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for (_, line) in code_lines(sources) {
        let Some((head, value)) = line.split_once(": &str = \"") else {
            continue;
        };
        let (Some(name), Some(value)) = (head.split(' ').next_back(), value.strip_suffix("\";"))
        else {
            continue;
        };
        found.insert(name.to_owned(), value.to_owned());
    }
    found
}

/// The HTTP methods a `.route(` call's handler part names.
fn methods_in(rest: &str) -> BTreeSet<&'static str> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let names = |m: &str| {
        let call = format!("{m}(");
        rest.match_indices(&call)
            .any(|(i, _)| !rest[..i].chars().next_back().is_some_and(ident))
    };
    ["get", "post", "put", "patch", "delete", "any"]
        .into_iter()
        .filter(|m| names(m))
        .collect()
}

/// The route table: each path with the methods it is served on (none: declared, not served yet).
fn route_table() -> BTreeMap<String, BTreeSet<&'static str>> {
    routes().into_iter().map(|(p, (m, _))| (p, m)).collect()
}

/// [`route_table`] with, for each path, whether a `.route(` call of the `site` module serves it.
fn routes() -> BTreeMap<String, (BTreeSet<&'static str>, bool)> {
    let sources = app_sources();
    let consts = constants(&sources);
    let mut table: BTreeMap<String, (BTreeSet<&'static str>, bool)> = BTreeMap::new();
    for (name, path) in &consts {
        if (name.ends_with("_ROUTE") || name.ends_with("_PATH")) && path.starts_with('/') {
            table.entry(path.clone()).or_default();
        }
    }
    for (module, line) in code_lines(&sources) {
        let Some((_, call)) = line.split_once(".route(") else {
            continue;
        };
        let (arg, rest) = call.split_once(',').expect("a .route( call on one line");
        let arg = arg.trim();
        let literal = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"'));
        let path = literal
            .map(str::to_owned)
            .or_else(|| consts.get(arg).cloned());
        let path = path.unwrap_or_else(|| panic!("{arg}: route path not resolved"));
        if !path.starts_with("/__test/") {
            let methods = methods_in(rest);
            assert!(!methods.is_empty(), "{path}: no method read");
            let entry = table.entry(path).or_default();
            entry.0.extend(methods);
            entry.1 |= module == "site";
        }
    }
    table
}

/// One actor: its cookies, its session row, its link token and job number, and its business.
struct Who {
    actor: Actor,
    jar: CookieJar,
    session: Option<Uuid>,
    link: Option<(String, i64)>,
    business: Option<Uuid>,
}

struct World {
    app: Router,
    who: Vec<Who>,
}

fn now() -> Timestamp {
    "2026-11-02T18:00:00Z".parse().expect("time")
}

/// A job walked to Done an hour ago, with its link token and number.
async fn done_job(db: &mut PgConnection) -> (String, i64) {
    let at = |m: i64| now() - SignedDuration::from_mins(60 - m);
    let job = create(db, at(0), false, AuditActor::System)
        .await
        .expect("job");
    for (minute, pair) in (1..).zip(TO_DONE.windows(2)) {
        let moved = move_to(db, job, (pair[0], pair[1]), at(minute), AuditActor::System).await;
        assert!(moved.expect("move").applied().is_some(), "moved");
    }
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    let token = issue(db, &secret, job, at(10)).await.expect("token");
    let number = job_number(db, job).await.expect("number");
    (token.as_str().to_owned(), number)
}

/// `actor` signed in to its realm (anonymous: only the pre-session cookie a page hands out).
async fn signed_in(db: &mut PgConnection, actor: Actor, link: Option<(String, i64)>) -> Who {
    let (realm, id, business) = match actor {
        Actor::Anonymous => (None, Uuid::nil(), None),
        Actor::HomeownerA | Actor::HomeownerB => (Some(Realm::Homeowner), Uuid::new_v4(), None),
        Actor::BusinessX | Actor::BusinessY => {
            let business = create_business(db, false, now()).await.expect("business");
            let member = add_owner(db, business, Uuid::new_v4(), now()).await;
            (
                Some(Realm::Business),
                member.expect("member"),
                Some(business),
            )
        }
        Actor::Owner => (Some(Realm::Owner), Uuid::new_v4(), None),
    };
    let life = SignedDuration::from_hours(12);
    let jar = match realm {
        Some(realm) => start_session(db, CookieJar::new(), (realm, id), life, now()).await,
        None => ensure_pre_session(CookieJar::new()),
    };
    let jar = jar.expect("cookies");
    let session = current_session(db, &jar, now()).await.expect("read");
    let session = session.map(|s| s.id());
    if let (Some(session), Some(business)) = (session, business) {
        sqlx::query(
            "INSERT INTO portals.business_session (session_id, business_id) VALUES ($1, $2)",
        )
        .bind(session)
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("business session");
    }
    Who {
        actor,
        jar,
        session,
        link,
        business,
    }
}

/// Two homeowners with a Done job each, two businesses, the owner and anonymous, and the served
/// portal routers merged as `server::build_router` merges them.
async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let a = done_job(&mut db).await;
    let b = done_job(&mut db).await;
    let mut who = Vec::new();
    for actor in ACTORS {
        let link = match actor {
            Actor::HomeownerA => Some(a.clone()),
            Actor::HomeownerB => Some(b.clone()),
            _ => None,
        };
        who.push(signed_in(&mut db, actor, link).await);
    }
    let clock = Arc::new(ManualClock::new(now()));
    let app = Router::new()
        .merge(quote_page_routes(pool.clone(), clock.clone()))
        .merge(pick_flow_routes::<TwilioVerify>(
            pool.clone(),
            clock.clone(),
            None,
        ))
        .merge(feedback_routes(pool.clone(), clock.clone()))
        .merge(checkin_routes(pool.clone(), clock));
    World { app, who }
}

fn cookie_header(jar: &CookieJar) -> String {
    let pairs: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    pairs.join("; ")
}

/// How a request reaches the app: same-origin with the actor's own CSRF token, same-origin with
/// a wrong one, or from another site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    WrongToken,
    Cross,
}

/// Sends `method path` as `who`'s browser under a fresh request ID; the status, the body with any
/// `Location`, and the request ID.
async fn send(
    app: &Router,
    (method, path): (&str, &str),
    who: &Who,
    site: Site,
) -> (u16, String, Uuid) {
    let id = Uuid::new_v4();
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(&who.jar).expect("csrf"),
    };
    let fields = if path.starts_with("/f/") {
        "&body=Fake+words"
    } else if path.starts_with("/c/") {
        ""
    } else {
        "&quote=1"
    };
    let fetch = if site == Site::Cross {
        "cross-site"
    } else {
        "same-origin"
    };
    let request = Request::builder()
        .method(method.to_uppercase().as_str())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", fetch)
        .header("x-request-id", id.to_string())
        .header(header::COOKIE, cookie_header(&who.jar))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{fields}")))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let location = parts
        .headers
        .get(header::LOCATION)
        .map(|l| l.to_str().expect("ascii"));
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&bytes),
        location.unwrap_or_default()
    );
    (parts.status.as_u16(), text, id)
}

/// Whether `text` names request `number` (not merely a longer number starting with it).
fn names(text: &str, number: i64) -> bool {
    let word = format!("request {number}");
    text.match_indices(&word)
        .any(|(i, _)| !text[i + word.len()..].starts_with(|c: char| c.is_ascii_digit()))
}

/// A link route: the holder reaches her own job and never the other's; everyone else, holding no
/// token, gets a bare 404 for a token that opens nothing (SEC-019, SEC-037, D-1945).
async fn probe_link(w: &World, method: &str, template: &str, who: &Who) {
    let token = who.link.as_ref().map_or(UNISSUED, |(t, _)| t.as_str());
    let path = template
        .replace("{token}", token)
        .replace("{answer}", ANSWER);
    let (status, text, _) = send(&w.app, (method, &path), who, Site::Own).await;
    let case = format!("{method} {template} as {:?}", who.actor);
    let Some((own_token, own)) = &who.link else {
        assert!(!allowed(Door::Link, who.actor), "{case}");
        assert_eq!((status, text.as_str()), (404, ""), "{case}");
        return;
    };
    assert!(allowed(Door::Link, who.actor), "{case}");
    match status {
        200 => assert!(names(&text, *own), "{case}: her own job"),
        303 => assert!(
            text.starts_with(&format!("/q/{own_token}#")),
            "{case}: {text}"
        ),
        _ => panic!("{case}: {status}"),
    }
    for other in w.who.iter().filter(|o| o.actor != who.actor) {
        if let Some((other_token, number)) = &other.link {
            assert!(
                !names(&text, *number),
                "{case} reads {:?}'s job",
                other.actor
            );
            assert!(!text.contains(other_token.as_str()), "{case}");
        }
    }
}

/// A webhook: an actor's cookies never stand in for the vendor's signature (SEC-016).
fn probe_webhook(path: &str, who: &Who) {
    let vendor = if path.contains("twilio") {
        WebhookVendor::Twilio
    } else if path.contains("stripe") {
        WebhookVendor::Stripe
    } else {
        panic!("{path}: no vendor in auth_matrix.rs")
    };
    let (url, cookie) = (format!("https://{HOST}{path}"), cookie_header(&who.jar));
    let headers = [("cookie", cookie.as_str()), ("host", HOST)];
    let delivery = Delivery {
        url: &url,
        body: b"Fake=1",
        headers: &headers,
    };
    let secret = SecretString::from(format!("fake-webhook-secret-{}", "0".repeat(16)));
    let checked = verify_webhook(vendor, &secret, delivery, now().as_second());
    assert_eq!(checked.is_ok(), allowed(Door::Webhook, who.actor), "{path}");
}

/// A homeowner or business route: the realm guard its handler calls first (SEC-008, SEC-046,
/// SEC-004, D-1726); a business passes only for its own business.
async fn probe_realm(pool: &PgPool, w: &World, door: Door, who: &Who) {
    let mut db = pool.acquire().await.expect("db");
    let session = current_session(&mut db, &who.jar, now())
        .await
        .expect("read");
    let case = format!("{door:?} as {:?}", who.actor);
    assert_eq!(session.as_ref().map(|s| s.id()), who.session, "{case}");
    let homeowner = session.is_some_and(|s| s.realm() == Realm::Homeowner);
    let principal = business_principal(&mut db, &who.jar, now())
        .await
        .expect("read");
    let passed = if door == Door::Homeowner {
        homeowner
    } else {
        principal.is_some()
    };
    assert_eq!(passed, allowed(door, who.actor), "{case}");
    let Some(principal) = principal else { return };
    for other in &w.who {
        let Some(business) = other.business else {
            continue;
        };
        let own = other.actor == who.actor;
        assert_eq!(principal.may(business, Need::Member), own, "{case}");
    }
}

/// The architecture file, whose section 13 lists every route with `yes` once served (D-2269).
const ARCHITECTURE: &str = include_str!("../../../docs/architecture.md");
/// The shared path-to-door cases (TD-SEC-61, D-2440).
const DOOR_CASES: &str = include_str!("../door_cases.txt");

/// Every `(method, path)` row of `docs/architecture.md` section 13 whose `Wired` cell is `yes`,
/// the method lower-case as the route table holds it.
fn yes_rows() -> BTreeSet<(String, String)> {
    let start = ARCHITECTURE.find("\n## 13. Routes\n").expect("section 13");
    let body = &ARCHITECTURE[start + 1..];
    let body = body[1..].find("\n## ").map_or(body, |end| &body[..=end]);
    body.lines()
        .map(|l| l.split('|').map(str::trim).collect::<Vec<_>>())
        .filter(|c| c.len() == 6 && c[4] == "yes")
        .map(|c| (c[1].to_lowercase(), c[2].to_owned()))
        .collect()
}

/// The doors `door_matrix.rs` probes over HTTP once a path is served (TD-SEC-61).
fn probed_over_http(door: Door) -> bool {
    matches!(
        door,
        Door::Homeowner | Door::Business | Door::Owner | Door::ActionLink | Door::Public
    )
}

/// TD-SEC-01, TD-BIZ-31, TD-SEC-61 (SEC-019, BIZ-041, D-2440): the table is read from the source,
/// holds the served quote, pick and feedback routes, gives every path a door, and has no push
/// route. A served path of the homeowner, business, owner, action-link or public door without a
/// `yes` row in section 13 fails naming it: `door_matrix.rs` probes exactly those rows over HTTP.
#[test]
fn td_sec_01_route_table_is_generated_and_every_path_has_a_door() {
    let (table, yes) = (routes(), yes_rows());
    let served = |path: &str, methods: &[&str]| {
        let (found, _) = table.get(path).unwrap_or_else(|| panic!("{path} not read"));
        assert!(
            methods.iter().all(|m| found.contains(m)),
            "{path}: {found:?}"
        );
    };
    served("/q/{token}", &["get", "post"]);
    served("/q/{token}/pick", &["post"]);
    served("/f/{token}", &["get", "post"]);
    served("/c/{token}/{answer}", &["get", "post"]);
    served("/webhooks/stripe", &["post"]);
    assert!(
        table.contains_key("/h") && table.contains_key("/b"),
        "declared routes read"
    );
    for (path, (methods, site)) in &table {
        let probed = probed_over_http(door(path, *site));
        for method in methods.iter().filter(|_| probed) {
            assert!(
                yes.contains(&((*method).to_owned(), path.clone())),
                "{} {path} is served: add its `yes` row to docs/architecture.md section 13 so \
                 door_matrix.rs probes it (D-2440)",
                method.to_uppercase()
            );
        }
        let lower = path.to_lowercase();
        assert!(!PUSH_WORDS.iter().any(|w| lower.contains(w)), "{path}");
    }
}

/// TD-BIZ-31, TD-BIZ-13 push case (BIZ-041, D-491): no portal page or script registers a service
/// worker, links a web app manifest, offers "Turn on job alerts" or ships a manifest file, and
/// push is no notice choice.
#[test]
fn td_biz_31_no_service_worker_manifest_or_push() {
    fn scan(dir: &Path) {
        for entry in fs::read_dir(dir).expect("dir") {
            let path = entry.expect("entry").path();
            let name = path.to_string_lossy().to_lowercase();
            if path.is_dir() {
                scan(&path);
                continue;
            }
            assert!(!PUSH_WORDS[..3].iter().any(|w| name.contains(w)), "{name}");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let text = text.to_lowercase();
            assert!(!PUSH_WORDS.iter().any(|w| text.contains(w)), "{name}");
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    for module in fs::read_dir(root).expect("app") {
        let module = module.expect("module").path();
        for part in ["templates", "static"] {
            if module.join(part).is_dir() {
                scan(&module.join(part));
            }
        }
    }
    for texting_on in [false, true] {
        let offered = format!("{:?}", channel_choices(texting_on)).to_lowercase();
        assert!(!offered.contains("push"), "{offered}");
    }
}

/// TD-SEC-01 (SEC-004 to SEC-006, SEC-008, SEC-019): every route of the table by every actor gets
/// exactly its expected allow or deny, and homeowner A's token never reads B's job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_every_route_by_every_actor(pool: PgPool) {
    let w = world(&pool).await;
    for (path, (methods, site)) in routes() {
        let door = door(&path, site);
        for who in &w.who {
            match door {
                Door::Link => {
                    for method in &methods {
                        probe_link(&w, method, &path, who).await;
                    }
                }
                Door::Asset if !methods.is_empty() => {
                    let (status, ..) = send(&w.app, ("get", &path), who, Site::Own).await;
                    assert_eq!(status == 200, allowed(door, who.actor), "{path}");
                }
                Door::Webhook => probe_webhook(&path, who),
                Door::Homeowner | Door::Business => probe_realm(&pool, &w, door, who).await,
                // Probed over HTTP in app/server/tests/door_matrix.rs (TD-SEC-61, D-2440).
                Door::Asset | Door::SignIn | Door::Public | Door::ActionLink | Door::Owner => {}
            }
        }
    }
}

/// SEC-048 (TD-SEC-01, D-1946): every refused request to a served route, a cross-site post or a
/// wrong CSRF token, by every actor, is a 403 and one `csrf_rejected` event with the route
/// template (never the path or its token), the request ID and the actor's session ID.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_048_every_refusal_is_a_security_event(pool: PgPool) {
    let w = world(&pool).await;
    let posts = route_table().into_iter().filter(|(path, methods)| {
        methods.contains("post") && matches!(door(path, false), Door::Link)
    });
    for (path, _) in posts {
        for who in &w.who {
            for site in [Site::Cross, Site::WrongToken] {
                let token = who.link.as_ref().map_or(UNISSUED, |(t, _)| t.as_str());
                let target = path.replace("{token}", token).replace("{answer}", ANSWER);
                let (status, _, request) = send(&w.app, ("post", &target), who, site).await;
                assert_eq!(status, 403, "{path} as {:?}", who.actor);
                let rows: Vec<(String, String, Option<Uuid>)> = sqlx::query_as(
                    "SELECT kind, route, actor_ref FROM portals.security_event \
                     WHERE request_id = $1",
                )
                .bind(request)
                .fetch_all(&pool)
                .await
                .expect("event");
                let event = ("csrf_rejected".to_owned(), path.clone(), who.session);
                assert_eq!(rows, vec![event], "{path} as {:?}", who.actor);
            }
        }
    }
}

/// TD-SEC-61 (SEC-019, D-2440): `door` gives every case of the shared list
/// `tests/security/door_cases.txt` its listed door, every door has a case, and the list is the
/// one `app/server/tests/door_matrix.rs` asserts too, so the two files' rules cannot drift.
#[test]
fn td_sec_61_the_shared_door_cases_hold_here() {
    let mut seen = BTreeSet::new();
    for line in DOOR_CASES
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let parts: Vec<&str> = line.split(' ').collect();
        let (listed, path, site) = match parts[..] {
            [listed, path] => (listed, path, false),
            [listed, path, "site"] => (listed, path, true),
            _ => panic!("malformed door case: {line:?}"),
        };
        assert_eq!(format!("{:?}", door(path, site)), listed, "{line}");
        seen.insert(listed);
    }
    assert_eq!(seen.len(), 9, "every door has a case: {seen:?}");
}

/// TD-SEC-61: the section 13 reader finds the served rows, and the owner door admits the owner
/// alone, the action-link door no session, the public door everyone (D-2440).
#[test]
fn td_sec_61_yes_rows_and_new_door_rules() {
    let yes = yes_rows();
    assert!(
        yes.contains(&("get".to_owned(), "/q/{token}".to_owned())),
        "{yes:?}"
    );
    assert!(
        !yes.iter().any(|(_, p)| p == "/healthz"),
        "T106's row is not yes"
    );
    for actor in ACTORS {
        assert!(allowed(Door::Public, actor));
        assert!(!allowed(Door::ActionLink, actor));
        assert_eq!(allowed(Door::Owner, actor), actor == Actor::Owner);
    }
}

//! The HTTP probe of the authorization matrix's realm, owner, action-link and public doors
//! (TD-SEC-61, T348; SEC-019, SEC-048, SEC-034, SEC-004 to SEC-008, SEC-026; D-2440, D-736).
//!
//! `tests/security/tests/auth_matrix.rs` gives every route a door; a test crate may not depend on
//! `askhava-server` (D-736), so the doors whose routes it cannot reach are probed here, through
//! `build_router` over a real test database, every vendor setting a fake (unreachable base URLs,
//! fake keys, as `route_wiring.rs`' `router()`), so no probe reaches a vendor. The rows probed are
//! the `docs/architecture.md` section 13 rows reading `yes` whose path falls under the
//! `Homeowner`, `Business`, `Owner`, `ActionLink` or `Public` door (the prefix rules restated in
//! [`door`]; both files assert the shared list `tests/security/door_cases.txt`). Six actors
//! (anonymous, homeowners A and B, businesses X and Y, the owner) send every method of each row:
//! a realm or owner route admits only its own people and refuses everyone else with a 4xx linking
//! to its sign-in and no fixture detail; a homeowner or business route answers the other side's
//! `{pick}` or `{complaint}` with the one not-found answer; an action link opens only for its
//! minted kind, object and token (the path names no member, D-3093), and its `GET` changes nothing; a public `GET` needs no session and shows no
//! fixture's personal data; every non-public response is `no-store`; every `POST`, cross-site or
//! with a wrong CSRF token, is a 403 with one `csrf_rejected` security event (its log line,
//! captured here, since `server` may read no module's table, TD-SEC-10) naming the route
//! template. An admitted `GET` is a 200; an admitted CSRF-only `POST`, and any owner answer (a
//! step-up challenge counts, SEC-043), passes when it is neither the door's refusal nor a 5xx.
//! A row with a path parameter other than `{pick}`, `{complaint}`, `{kind}`, `{object}` or
//! `{token}` fails naming it. With no such row served the probe passes with zero routes. Planted
//! cases run the same judging code on a test-local router. Every name, phone and ID is fake.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    AccountStatus, NewComplaint, add_owner, create as create_business, record_complaint,
};
use askhava_core::api::{Guarded, IdSecret, Settings, SystemClock, TestControl};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, job_number, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor as AuditActor;
use askhava_picks::api::{PagePick, page_pick};
use askhava_portals::api::{
    ActionKind, ActionTarget, CookieJar, PortalsError, Realm, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE,
    SignIn, SignInAddress, SignInMail, csrf_token, current_session, ensure_pre_session,
    mint_action_link, open_action_link, signin_post, signin_request, start_session,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The fake host every request names, so a same-origin request passes the cross-site refusal.
const HOST: &str = "askhava.example";
/// The architecture file, whose section 13 is the route table.
const ARCHITECTURE: &str = include_str!("../../../docs/architecture.md");
/// The shared path-to-door cases, asserted by `auth_matrix.rs` too (D-2440).
const DOOR_CASES: &str = include_str!("../../../tests/security/door_cases.txt");
/// The path parameters the probe can fill from its fixtures.
const FILLS: [&str; 5] = ["{pick}", "{complaint}", "{kind}", "{object}", "{token}"];
/// A well-formed link token nobody was given.
const UNISSUED: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
/// A base URL nothing listens on, for every vendor client, so no probe reaches a vendor.
const UNREACHABLE: &str = "http://127.0.0.1:9";
/// Homeowners A's and B's fake names, phones and streets.
const NAMES: [&str; 2] = ["Dana Testperson", "Robin Fakename"];
const PHONES: [&str; 2] = ["+19285550190", "+19285550191"];
const STREETS: [&str; 2] = ["100 Example Way", "200 Sample Road"];

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

impl Actor {
    /// Which of the two fixture records is the actor's own: B and Y the second, everyone else the
    /// first.
    fn side(self) -> usize {
        usize::from(matches!(self, Self::HomeownerB | Self::BusinessY))
    }
}

/// The doors of `auth_matrix.rs`, with the same names (`door_cases.txt` holds them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Door {
    Link,
    Asset,
    Webhook,
    SignIn,
    Homeowner,
    Business,
    Public,
    ActionLink,
    Owner,
}

/// The public paths besides `/help` and below and the `site` module's routes.
const PUBLIC_PATHS: [&str; 6] = [
    "/healthz",
    "/terms",
    "/privacy",
    "/sms-terms",
    "/your-data",
    "/business-data",
];

/// The door of `path` (served by the `site` module when `site`), `auth_matrix.rs`' rules restated
/// (SEC-019, D-2440); `None` where that file fails the run. `sign-in` counts only as a whole path
/// segment, so `/owner/sign-in-history` is the owner's.
fn door(path: &str, site: bool) -> Option<Door> {
    let under = |root: &str| path == root || path.starts_with(&format!("{root}/"));
    let link = ["/q/{token}", "/f/{token}", "/c/{token}"];
    Some(if under("/webhooks") {
        Door::Webhook
    } else if under("/static") {
        Door::Asset
    } else if path.split('/').any(|seg| seg == "sign-in") || path == "/b/signup" {
        Door::SignIn
    } else if link.iter().any(|root| path.starts_with(root)) {
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
        return None;
    })
}

/// Whether `actor`'s session passes `door` (SEC-019; the link is the action link's key).
fn allowed(door: Door, actor: Actor) -> bool {
    match door {
        Door::Homeowner => matches!(actor, Actor::HomeownerA | Actor::HomeownerB),
        Door::Business => matches!(actor, Actor::BusinessX | Actor::BusinessY),
        Door::Owner => actor == Actor::Owner,
        Door::Public | Door::Asset | Door::SignIn => true,
        Door::Link | Door::Webhook | Door::ActionLink => false,
    }
}

/// The sign-in page a refusal of `door` links to.
fn sign_in(door: Door) -> &'static str {
    match door {
        Door::Homeowner => "/h/sign-in",
        Door::Owner => "/owner/sign-in",
        _ => "/b/sign-in",
    }
}

/// One row the probe takes: method, path template and door.
type Row = (Method, String, Door);

/// The section 13 rows reading `yes` under the five probed doors; a `yes` row no door names fails
/// naming it (as `auth_matrix.rs` fails the run).
fn probed_rows() -> Vec<Row> {
    let start = ARCHITECTURE.find("\n## 13. Routes\n").expect("section 13");
    let body = &ARCHITECTURE[start + 1..];
    let body = body[1..].find("\n## ").map_or(body, |end| &body[..=end]);
    let mut rows = Vec::new();
    for line in body.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() != 6 || cells[4] != "yes" {
            continue;
        }
        let Some(door) = door(cells[2], cells[3] == "site") else {
            panic!("served row {line:?} has no door: add its matrix rule (SEC-019)");
        };
        let probed = !matches!(
            door,
            Door::Link | Door::Asset | Door::Webhook | Door::SignIn
        );
        let method = Method::from_bytes(cells[1].as_bytes()).expect("a method");
        if probed {
            rows.push((method, cells[2].to_owned(), door));
        }
    }
    rows
}

/// One minted action link: its kind, object and token.
#[derive(Clone)]
struct Minted {
    kind: ActionKind,
    object: Uuid,
    token: String,
}

/// The fixtures' records, index 0 for A and X, 1 for B and Y: A's job is quoted by X and picked,
/// B's by Y.
struct Fixtures {
    jobs: [Uuid; 2],
    numbers: [i64; 2],
    tokens: [String; 2],
    picks: [Uuid; 2],
    complaints: [Uuid; 2],
    links: [Minted; 2],
}

impl Fixtures {
    /// Everything a page must not show the wrong person: names, phones, streets, quote and
    /// action-link tokens and job numbers (PRIV-016).
    fn personal(&self) -> Vec<String> {
        let fixed = [NAMES, PHONES, STREETS].concat();
        let mut all: Vec<String> = fixed.into_iter().map(str::to_owned).collect();
        all.extend(self.tokens.iter().cloned());
        all.extend(self.links.iter().map(|l| l.token.clone()));
        all.extend(self.numbers.iter().map(|n| format!("request {n}")));
        all
    }

    /// The `{pick}` and `{complaint}` of `side`.
    fn ids(&self, side: usize) -> (Uuid, Uuid) {
        (self.picks[side], self.complaints[side])
    }
}

/// `template` with `{pick}` and `{complaint}` from `ids` and the action-link parameters from
/// `link`; any other parameter fails naming it (D-2440).
fn fill(template: &str, (pick, complaint): (Uuid, Uuid), link: &Minted) -> Result<String, String> {
    let mut path = template.to_owned();
    for seg in template.split('/').filter(|s| s.starts_with('{')) {
        let value = match seg {
            "{pick}" => pick.to_string(),
            "{complaint}" => complaint.to_string(),
            "{kind}" => link.kind.slug().to_owned(),
            "{object}" => link.object.to_string(),
            "{token}" => link.token.clone(),
            other => {
                return Err(format!(
                    "{template}: no fixture fills {other} (only {FILLS:?}); the task serving \
                     it adds the fill in a plan change (D-2440)"
                ));
            }
        };
        path = path.replacen(seg, &value, 1);
    }
    Ok(path)
}

/// What came back: status, whether `Cache-Control` holds `no-store`, body and `Location`.
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    no_store: bool,
    text: String,
}

/// How a request reaches the app: same-origin with the jar's CSRF token, with a wrong one, or
/// from another site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    Own,
    WrongToken,
    Cross,
}

/// A form request for `method path` with `jar`'s cookies, request ID `id` and the body
/// `csrf=<token><extra>`.
fn request(
    (method, path, extra): (&Method, &str, &str),
    jar: &CookieJar,
    site: Site,
    id: Uuid,
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        Site::Own | Site::Cross => csrf_token(jar).unwrap_or_default(),
    };
    let fetch = if site == Site::Cross {
        "cross-site"
    } else {
        "same-origin"
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", fetch)
        .header("x-request-id", id.to_string())
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request")
}

/// The answer to `request`, with the `name=value` of every non-empty cookie it sets.
async fn answer(app: &Router, request: Request<Body>) -> (Reply, Vec<String>) {
    let response = app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let value = |name| {
        let found = parts.headers.get(name).and_then(|v| v.to_str().ok());
        found.unwrap_or_default().to_owned()
    };
    let set: Vec<String> = parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes);
    let reply = Reply {
        status: parts.status.as_u16(),
        no_store: value(header::CACHE_CONTROL).contains("no-store"),
        text: format!("{text}{}", value(header::LOCATION)),
    };
    (reply, set)
}

/// Sends `method path` as the holder of `jar`, with a CSRF-only form, under request ID `id`.
async fn send(
    app: &Router,
    (method, path): (&Method, &str),
    jar: &CookieJar,
    site: Site,
    id: Uuid,
) -> Reply {
    answer(app, request((method, path, ""), jar, site, id))
        .await
        .0
}

/// The first of `secrets` that `text` shows.
fn shows<'a>(text: &str, secrets: &'a [String]) -> Option<&'a String> {
    secrets.iter().find(|s| text.contains(s.as_str()))
}

/// Whether `reply` is `door`'s refusal: a 4xx linking to its sign-in page.
fn refusal(door: Door, reply: &Reply) -> bool {
    (400..500).contains(&reply.status) && reply.text.contains(sign_in(door))
}

/// SEC-019, SEC-034, D-2440: `actor`'s answer `reply` to `method` on a `door` route (not the
/// action-link door, [`judge_opened`]). Every non-public answer is `no-store`; a public `GET` is
/// a 200 with no fixture data; an admitted `GET` is a 200 (the owner's: neither refusal nor 5xx,
/// so a step-up challenge passes), an admitted `POST` neither refusal nor 5xx; homeowner A never
/// sees B's job or token; everyone else gets the door's refusal with no fixture detail.
fn judge(
    door: Door,
    (actor, method): (Actor, &Method),
    reply: &Reply,
    fx: &Fixtures,
) -> Result<(), String> {
    let status = reply.status;
    if door != Door::Public && !reply.no_store {
        return Err(format!("{status} is not no-store (SEC-034)"));
    }
    let personal = fx.personal();
    if door == Door::Public || !allowed(door, actor) {
        let ok = match door {
            Door::Public if method == Method::GET => status == 200,
            Door::Public => status < 500,
            _ => refusal(door, reply),
        };
        return match shows(&reply.text, &personal) {
            Some(s) => Err(format!("{status} shows fixture data {s:?}")),
            None if ok => Ok(()),
            None => Err(format!(
                "{status} is not the door's answer: {:?}",
                reply.text
            )),
        };
    }
    let strict = method == Method::GET && door != Door::Owner;
    let admitted = if strict {
        status == 200
    } else {
        !refusal(door, reply) && status < 500
    };
    if !admitted {
        return Err(format!("own record not reached: {status}"));
    }
    let other = 1 - actor.side();
    let foreign = [
        fx.tokens[other].clone(),
        format!("request {}", fx.numbers[other]),
    ];
    match shows(&reply.text, &foreign).filter(|_| door == Door::Homeowner) {
        Some(s) => Err(format!("names the other homeowner's {s:?}")),
        None => Ok(()),
    }
}

/// SEC-026, SEC-034: the answer to an action link's own token: `no-store`; a `GET` is a 200, a
/// `POST` anything but the not-found answer or a 5xx.
fn judge_opened(method: &Method, reply: &Reply) -> Result<(), String> {
    let status = reply.status;
    let opened = if method == Method::GET {
        status == 200
    } else {
        status != 404 && status < 500
    };
    match (reply.no_store, opened) {
        (false, _) => Err(format!("{status} is not no-store (SEC-034)")),
        (true, false) => Err(format!("its own link did not open: {status}")),
        (true, true) => Ok(()),
    }
}

/// SEC-019, SEC-037: `reply` is the one not-found answer `not_found`: a 404, `no-store` on the
/// realm doors, the same bytes.
fn judge_not_found(reply: &Reply, not_found: &Reply) -> Result<(), String> {
    if not_found.status == 404 && reply == not_found {
        Ok(())
    } else {
        Err(format!(
            "not the one not-found answer: {} {:?}",
            reply.status, reply.text
        ))
    }
}

/// SEC-026: a `GET` leaves the link as it found it (`open` before and after).
fn judge_unchanged(method: &Method, before: bool, after: bool) -> Result<(), String> {
    if method == Method::GET && before != after {
        Err("a GET changed the link's state (SEC-026)".to_owned())
    } else {
        Ok(())
    }
}

/// SEC-048: a refused `POST` is a 403 with exactly one `csrf_rejected` event naming `template`.
fn judge_csrf(reply: &Reply, events: &[[String; 2]], template: &str) -> Result<(), String> {
    let want = [["csrf_rejected".to_owned(), template.to_owned()]];
    if reply.status == 403 && events == want {
        Ok(())
    } else {
        Err(format!("refusal {} with events {events:?}", reply.status))
    }
}

/// The `security event` log lines captured: kind, route template, request ID (SEC-048). The
/// event's row is `portals`' table, which `server` code may not name (TD-SEC-10); the line is
/// written by the same `record_security_event` call.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<[String; 3]>>>);

/// The fields of one log event.
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
            let line = [get("kind"), get("route"), get("request_id")];
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push(line);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

impl Events {
    /// The `[kind, route]` of every event of request `id`.
    fn of(&self, id: Uuid) -> Vec<[String; 2]> {
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let id = id.to_string();
        let mine = all.iter().filter(|e| e[2] == id);
        mine.map(|e| [e[0].clone(), e[1].clone()]).collect()
    }
}

/// A business's owner member, the business and the owner's sign-in mailbox.
#[derive(Clone, Copy)]
struct Team {
    member: Uuid,
    business: Uuid,
    mailbox: Uuid,
}

/// One actor and its cookies.
struct Who {
    actor: Actor,
    jar: CookieJar,
}

struct World {
    app: Router,
    pool: PgPool,
    who: Vec<Who>,
    fx: Fixtures,
    teams: [Team; 2],
}

/// A fake vault key pair value (`<version>:<base64 of 32 bytes>`).
fn vault_key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// Dev settings with every optional part configured from fake values, as `route_wiring.rs`'.
fn pairs() -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-0001",
        ),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_STRIPE_WEBHOOK_SECRET", "whsec_fake_value_0001"),
        ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake_value_0001"),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake"),
        ("ASKHAVA_STRIPE_BASE_URL", UNREACHABLE),
        ("ASKHAVA_TWILIO_BASE_URL", UNREACHABLE),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", UNREACHABLE),
    ];
    let keys = [
        ("VAULT_KEY", vault_key(0x41)),
        ("VAULT_INDEX_KEY", vault_key(0x42)),
    ];
    let fixed = fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()));
    fixed
        .chain(keys.into_iter().map(|(k, v)| (k.to_owned(), v)))
        .collect()
}

/// The app's router as `route_wiring.rs:238` builds it, every optional part configured, over the
/// test database `pool`.
fn router(pool: PgPool) -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), pairs.clone()).expect("hooks");
    let voice = askhava_server::voice_hooks(&settings, webhooks.as_ref(), pairs.clone())
        .expect("voice hooks");
    let pick_keys = askhava_server::pick_code_keys(&settings, pairs.clone()).expect("pick keys");
    let stripe = askhava_server::release_sink::stripe_webhook(&settings, pool.clone(), pairs)
        .expect("stripe hooks");
    let all = webhooks.is_some() && voice.is_some() && pick_keys.is_some() && stripe.is_some();
    assert!(all, "every optional part of build_router is configured");
    askhava_server::build_router(pool, control.router(), (webhooks, voice, pick_keys, stripe))
}

/// An active, verified R-39 business serving Lake Havasu City that ticked the job type.
fn card_business(id: Uuid) -> CardBusiness {
    CardBusiness {
        id,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// An AC repair job in Lake Havasu City, as matching reads it.
fn card_job() -> CardJob {
    let job = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    CardJob {
        towns: vec![town],
        candidates: vec![job],
        text: String::new(),
    }
}

/// A job opened, carded to `business` and quoted by it, so it is in Quotes ready.
async fn quoted_job(db: &mut PgConnection, business: Uuid, now: Timestamp) -> Uuid {
    let job = create(db, now, false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(db, map, job, &card_job(), &[card_business(business)], now).await;
    assert_eq!(sent.expect("card").len(), 1);
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(db, (job, business), &quote, now).await;
    saved.expect("quoted");
    job
}

/// Homeowner `side`'s vault contact (fake name, phone, street) and her request naming `job`.
async fn her_request(db: &mut PgConnection, side: usize, job: Uuid, now: Timestamp) -> Uuid {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x59), &key(0x69), &url).expect("vault");
    let new = NewContact {
        name: Some(SecretString::from(NAMES[side])),
        phone: Some(SecretString::from(PHONES[side])),
        email: None,
        address: Some(SecretString::from(STREETS[side])),
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault, &new).await.expect("contact");
    let inbound = Inbound {
        words: "my sink leaks",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(db, &answer).await.expect("job named");
    contact
}

/// A Quote link for `object`, minted for `team`'s owner (SEC-025, `portals::api` as T148's tests).
async fn mint(db: &mut PgConnection, team: Team, object: Uuid, now: Timestamp) -> Minted {
    let kind = ActionKind::Quote;
    let target = ActionTarget {
        kind,
        object,
        member: team.member,
        business: team.business,
    };
    let ends = now + SignedDuration::from_hours(2);
    let link = mint_action_link(db, target, None, (now, ends)).await;
    let link = link.expect("minted");
    let token = link.expose_secret().rsplit('/').next().expect("token");
    Minted {
        kind,
        object,
        token: token.to_owned(),
    }
}

/// One side's records.
struct Side {
    contact: Uuid,
    team: Team,
    job: Uuid,
    number: i64,
    token: String,
    pick: Uuid,
    complaint: Uuid,
    link: Minted,
}

/// Side `side`: a business with its owner, her job it quoted, her request and page pick of the
/// quote, the job's quote token and number, a complaint and a Quote action link.
async fn side(pool: &PgPool, side: usize, now: Timestamp) -> Side {
    let mut db = pool.acquire().await.expect("db");
    let business = create_business(&mut db, false, now)
        .await
        .expect("business");
    let mailbox = Uuid::new_v4();
    let member = add_owner(&mut db, business, mailbox, now).await;
    let team = Team {
        member: member.expect("owner"),
        business,
        mailbox,
    };
    let job = quoted_job(&mut db, business, now).await;
    let contact = her_request(&mut db, side, job, now).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, AuditActor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    let token = issue(&mut db, &secret, job, now).await.expect("token");
    let number = job_number(&mut db, job).await.expect("number");
    let words = NewComplaint {
        business,
        job,
        kind: "no_show",
        words: "Fake words",
    };
    let complaint = record_complaint(&mut db, &words, now).await;
    let link = mint(&mut db, team, job, now).await;
    Side {
        contact,
        team,
        job,
        number,
        token: token.as_str().to_owned(),
        pick,
        complaint: complaint.expect("complaint").id,
        link,
    }
}

/// The one sign-in address of one business owner; the link it "emails" is kept.
struct OneMail {
    to: SignInAddress,
    sent: Mutex<Option<String>>,
}

impl SignInMail for OneMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(vec![self.to])
    }

    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let path = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = path;
        Ok(())
    }
}

/// `team`'s owner signed in through `portals`' emailed-link sign-in (D-1724), since `server`
/// code may not write the business session row itself (TD-SEC-10).
async fn business_jar(pool: &PgPool, team: Team) -> CookieJar {
    let to = SignInAddress {
        business: team.business,
        mailbox: team.mailbox,
    };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let state = SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail));
    let app = state.into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let asked = request(
        (&Method::POST, SIGNIN_ROUTE, "&email=owner%40x.example"),
        &jar,
        Site::Own,
        Uuid::new_v4(),
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = request((&Method::POST, &path, ""), &jar, Site::Own, Uuid::new_v4());
    let (_, set) = answer(&app, pressed).await;
    let mut headers = HeaderMap::new();
    let cookie = set.join("; ").parse().expect("cookie header");
    headers.insert(header::COOKIE, cookie);
    CookieJar::from_headers(&headers)
}

/// `actor` signed in as `auth_matrix.rs` signs it in (anonymous: only the pre-session cookie;
/// homeowners on their vault contact; the owner a 12-hour owner session; a business by its link).
async fn signed_in(pool: &PgPool, actor: Actor, sides: &[Side; 2], now: Timestamp) -> CookieJar {
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(12);
    let jar = match actor {
        Actor::Anonymous => ensure_pre_session(CookieJar::new()).expect("pre-session"),
        Actor::BusinessX | Actor::BusinessY => business_jar(pool, sides[actor.side()].team).await,
        Actor::HomeownerA | Actor::HomeownerB | Actor::Owner => {
            let login = match actor {
                Actor::Owner => (Realm::Owner, Uuid::new_v4()),
                _ => (Realm::Homeowner, sides[actor.side()].contact),
            };
            let jar = start_session(&mut db, CookieJar::new(), login, life, now).await;
            jar.expect("session")
        }
    };
    let session = current_session(&mut db, &jar, now).await.expect("read");
    let realm = session.map(|s| s.realm());
    let want = match actor {
        Actor::Anonymous => None,
        Actor::HomeownerA | Actor::HomeownerB => Some(Realm::Homeowner),
        Actor::BusinessX | Actor::BusinessY => Some(Realm::Business),
        Actor::Owner => Some(Realm::Owner),
    };
    assert_eq!(realm, want, "{actor:?} signed in");
    jar
}

/// Both sides' records, the six actors and the router `main` builds, over `pool`.
async fn world(pool: &PgPool) -> World {
    let now = Timestamp::now();
    let sides = [side(pool, 0, now).await, side(pool, 1, now).await];
    let mut who = Vec::new();
    for actor in ACTORS {
        let jar = signed_in(pool, actor, &sides, now).await;
        who.push(Who { actor, jar });
    }
    let [a, b] = sides;
    let fx = Fixtures {
        jobs: [a.job, b.job],
        numbers: [a.number, b.number],
        tokens: [a.token, b.token],
        picks: [a.pick, b.pick],
        complaints: [a.complaint, b.complaint],
        links: [a.link, b.link],
    };
    World {
        app: router(pool.clone()),
        pool: pool.clone(),
        who,
        fx,
        teams: [a.team, b.team],
    }
}

/// A realm, owner or public row by `who` (SEC-019): the door's answer, and on a homeowner or
/// business route the other side's `{pick}` or `{complaint}` is the one not-found answer.
async fn probe_page(
    w: &World,
    (method, template, door): (&Method, &str, Door),
    who: &Who,
) -> Result<(), String> {
    let side = who.actor.side();
    let link = &w.fx.links[side];
    let get = |path: String| async move {
        send(&w.app, (method, &path), &who.jar, Site::Own, Uuid::new_v4()).await
    };
    let reply = get(fill(template, w.fx.ids(side), link)?).await;
    judge(door, (who.actor, method), &reply, &w.fx)?;
    let foreign = template.contains("{pick}") || template.contains("{complaint}");
    let realm = matches!(door, Door::Business | Door::Homeowner);
    if !realm || !allowed(door, who.actor) || !foreign {
        return Ok(());
    }
    let theirs = get(fill(template, w.fx.ids(1 - side), link)?).await;
    let unknown = get(fill(template, (Uuid::new_v4(), Uuid::new_v4()), link)?).await;
    judge_not_found(&theirs, &unknown)
}

/// Whether `link` is unused and unexpired.
async fn open(pool: &PgPool, link: &Minted) -> bool {
    let mut db = pool.acquire().await.expect("db");
    let key = (link.kind, link.object);
    open_action_link(&mut db, key, &link.token, Timestamp::now())
        .await
        .is_ok()
}

/// An action-link row by `who` (SEC-025, SEC-026): X's fresh link opens, its `GET` changes
/// nothing; another kind, another object, a token minted for Y's member on B's job (refused by
/// its object, since the path names no member) or an unissued one is the one not-found answer,
/// `no-store`.
async fn probe_link(
    w: &World,
    (method, template): (&Method, &str),
    who: &Who,
) -> Result<(), String> {
    let mut db = w.pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let mine = mint(&mut db, w.teams[0], w.fx.jobs[0], now).await;
    let theirs = mint(&mut db, w.teams[1], w.fx.jobs[1], now).await;
    let ids = w.fx.ids(0);
    let get = |link: Minted| async move {
        let path = fill(template, ids, &link)?;
        Ok::<_, String>(send(&w.app, (method, &path), &who.jar, Site::Own, Uuid::new_v4()).await)
    };
    let wrong = [
        Minted {
            kind: ActionKind::Hold,
            ..mine.clone()
        },
        Minted {
            object: w.fx.jobs[1],
            ..mine.clone()
        },
        Minted {
            token: theirs.token,
            ..mine.clone()
        },
    ];
    let not_found = get(Minted {
        token: UNISSUED.to_owned(),
        ..mine.clone()
    })
    .await?;
    if !not_found.no_store {
        return Err("the not-found answer is not no-store (SEC-034)".to_owned());
    }
    for link in wrong {
        judge_not_found(&get(link).await?, &not_found)?;
    }
    let before = open(&w.pool, &mine).await;
    judge_opened(method, &get(mine.clone()).await?)?;
    judge_unchanged(method, before, open(&w.pool, &mine).await)
}

/// SEC-048: `who`'s cross-site and wrong-token `POST`s to `template` are each a 403 with one
/// `csrf_rejected` event naming the template.
async fn probe_csrf(w: &World, events: &Events, template: &str, who: &Who) -> Result<(), String> {
    let side = who.actor.side();
    let path = fill(template, w.fx.ids(side), &w.fx.links[side])?;
    for site in [Site::Cross, Site::WrongToken] {
        let id = Uuid::new_v4();
        let reply = send(&w.app, (&Method::POST, &path), &who.jar, site, id).await;
        judge_csrf(&reply, &events.of(id), template).map_err(|e| format!("{site:?}: {e}"))?;
    }
    Ok(())
}

/// Every actor on one row.
async fn probe_row(
    w: &World,
    events: &Events,
    (method, template, door): &Row,
) -> Result<(), String> {
    for who in &w.who {
        let case = |e: String| format!("{method} {template} as {:?}: {e}", who.actor);
        if *door == Door::ActionLink {
            probe_link(w, (method, template), who).await.map_err(case)?;
        } else {
            probe_page(w, (method, template, *door), who)
                .await
                .map_err(case)?;
        }
        if method == Method::POST {
            probe_csrf(w, events, template, who).await.map_err(case)?;
        }
    }
    Ok(())
}

/// TD-SEC-61 (SEC-019, SEC-034, SEC-048, D-2440): every section 13 row reading `yes` under the
/// homeowner, business, owner, action-link or public door, by all six actors over HTTP through
/// `build_router`; with none served it passes with zero routes, the fixtures still built.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_61_every_served_door_row_over_http(pool: PgPool) {
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let w = world(&pool).await;
    for row in &probed_rows() {
        if let Err(problem) = probe_row(&w, &events, row).await {
            panic!("{problem}");
        }
    }
}

/// TD-SEC-61: the restated door rules give every case of `tests/security/door_cases.txt` its
/// listed door, and every door has a case (the same list `auth_matrix.rs` asserts).
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
        assert_eq!(
            format!("{:?}", door(path, site).expect(line)),
            listed,
            "{line}"
        );
        seen.insert(listed);
    }
    assert_eq!(seen.len(), 9, "every door has a case: {seen:?}");
    assert_eq!(
        door("/elsewhere", false),
        None,
        "no door: auth_matrix.rs fails the run"
    );
}

/// TD-SEC-61: a row with a parameter the fixtures do not fill fails naming it; the five fills work.
#[test]
fn td_sec_61_an_unknown_parameter_fails_naming_it() {
    let fx = planted_fixtures();
    let problem = fill("/b/quote/{job}/auto-confirm", fx.ids(0), &fx.links[0]).expect_err("{job}");
    assert!(
        problem.contains("{job}") && problem.contains("/b/quote/{job}/auto-confirm"),
        "{problem}"
    );
    let path = fill("/b/a/{kind}/{object}/{token}", fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(
        path,
        format!("/b/a/quote/{}/{}", fx.links[1].object, fx.links[1].token)
    );
    let path = fill("/b/{pick}/{complaint}", fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(path, format!("/b/{}/{}", fx.picks[1], fx.complaints[1]));
}

/// Fake fixtures for the planted cases (no database).
fn planted_fixtures() -> Fixtures {
    let link = |n: u128, token: &str| Minted {
        kind: ActionKind::Quote,
        object: Uuid::from_u128(n),
        token: token.repeat(64),
    };
    Fixtures {
        jobs: [Uuid::from_u128(1), Uuid::from_u128(2)],
        numbers: [1001, 1002],
        tokens: ["a".repeat(64), "b".repeat(64)],
        picks: [Uuid::from_u128(11), Uuid::from_u128(12)],
        complaints: [Uuid::from_u128(21), Uuid::from_u128(22)],
        links: [link(1, "c"), link(2, "d")],
    }
}

/// The test-local planted pages: one correct and one leaking page per door, and the action-link
/// state a `GET` may change.
async fn planted(Path(page): Path<String>, State(state): State<Arc<AtomicBool>>) -> Response {
    let kept = [(header::CACHE_CONTROL, "no-store")];
    match page.as_str() {
        "h-good" => (kept, "Your request 1001").into_response(),
        "h-leak" => (kept, "Your request 1001 and request 1002").into_response(),
        "h-cached" => "Your request 1001".into_response(),
        "b-leak-y-pick" => (kept, "Pick of the other business").into_response(),
        "owner-good" => {
            let link = "<a href=\"/owner/sign-in\">Sign in</a>";
            (StatusCode::FORBIDDEN, kept, link).into_response()
        }
        "owner-leak" => (kept, "Owner page").into_response(),
        "a-good" => (kept, "Send your quote").into_response(),
        "a-leak" => {
            state.store(true, Ordering::SeqCst);
            (kept, "Quote sent").into_response()
        }
        "p-good" => "Help for homeowners".into_response(),
        "p-leak" => "Call +19285550190".into_response(),
        _ => (StatusCode::NOT_FOUND, kept).into_response(),
    }
}

/// TD-SEC-61 planted cases: the judging code passes each correct route and fails each leaking
/// one (a homeowner page naming B's job, a business page answering Y's pick, an owner page open to
/// a business, an action link whose `GET` changes state, a public page holding a fixture phone, a
/// realm page without `no_store`).
#[tokio::test]
async fn td_sec_61_planted_routes_pass_and_leaks_fail() {
    let (fx, state) = (planted_fixtures(), Arc::new(AtomicBool::new(false)));
    let app = Router::new()
        .route("/{page}", get(planted))
        .with_state(Arc::clone(&state));
    let jar = ensure_pre_session(CookieJar::new()).expect("jar");
    let get =
        |path: &'static str| send(&app, (&Method::GET, path), &jar, Site::Own, Uuid::new_v4());
    let as_a = (Actor::HomeownerA, &Method::GET);
    for (path, ok) in [("/h-good", true), ("/h-leak", false), ("/h-cached", false)] {
        let judged = judge(Door::Homeowner, as_a, &get(path).await, &fx);
        assert_eq!(judged.is_ok(), ok, "{path}: {judged:?}");
    }
    let not_found = get("/b-unknown").await;
    for (path, ok) in [("/b-good-y-pick", true), ("/b-leak-y-pick", false)] {
        let judged = judge_not_found(&get(path).await, &not_found);
        assert_eq!(judged.is_ok(), ok, "{path}: {judged:?}");
    }
    let as_x = (Actor::BusinessX, &Method::GET);
    for (path, ok) in [("/owner-good", true), ("/owner-leak", false)] {
        let judged = judge(Door::Owner, as_x, &get(path).await, &fx);
        assert_eq!(judged.is_ok(), ok, "{path}: {judged:?}");
    }
    for (path, ok) in [("/a-good", true), ("/a-leak", false)] {
        let before = state.load(Ordering::SeqCst);
        let opened = judge_opened(&Method::GET, &get(path).await);
        let judged = opened.and(judge_unchanged(
            &Method::GET,
            before,
            state.load(Ordering::SeqCst),
        ));
        assert_eq!(judged.is_ok(), ok, "{path}: {judged:?}");
    }
    for (path, ok) in [("/p-good", true), ("/p-leak", false)] {
        let judged = judge(
            Door::Public,
            (Actor::Anonymous, &Method::GET),
            &get(path).await,
            &fx,
        );
        assert_eq!(judged.is_ok(), ok, "{path}: {judged:?}");
    }
}

/// TD-SEC-61, SEC-048: a 403 with exactly one `csrf_rejected` event naming the template passes;
/// no event, two events, another route or another status fails.
#[test]
fn td_sec_61_the_csrf_judge() {
    let reply = |status| Reply {
        status,
        no_store: true,
        text: String::new(),
    };
    let event = |route: &str| ["csrf_rejected".to_owned(), route.to_owned()];
    let t = "/b/notices";
    assert!(judge_csrf(&reply(403), &[event(t)], t).is_ok());
    assert!(judge_csrf(&reply(403), &[], t).is_err());
    assert!(judge_csrf(&reply(403), &[event(t), event(t)], t).is_err());
    assert!(judge_csrf(&reply(403), &[event("unmatched")], t).is_err());
    assert!(judge_csrf(&reply(200), &[event(t)], t).is_err());
}

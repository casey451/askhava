//! The HTTP probe of the authorization matrix's realm, owner, action-link and public doors
//! (TD-SEC-61, TD-SEC-73, TD-SEC-79, T348, T643, T665; SEC-019, SEC-048, SEC-034, SEC-004 to
//! SEC-008, SEC-026, SEC-042, SEC-009, SEC-012; D-2440, D-736, D-4292, D-4349).
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
//! minted kind, object and token (the path names no member, D-3093), and its `GET` changes nothing; a Quote link bound to Y on X's job, whose card never went to Y, is the one not-found answer (D-3706); a public `GET` needs no session and shows no
//! fixture's personal data; every non-public response is `no-store`; every `POST`, cross-site or
//! with a wrong CSRF token (sent as a browser's own post, and without `Sec-Fetch-Site` with
//! `Origin: null`), is a 403 with one `csrf_rejected` security event (its log line, captured here,
//! since `server` may read no module's table, TD-SEC-10) naming the route template; the sign-in
//! and link `POST`s are probed for the `Origin: null` case too (TD-SEC-73, D-4292). Requests go
//! as a browser sends them under `Referrer-Policy: no-referrer` (`Sec-Fetch-Site: same-origin`,
//! `Origin: null`). An admitted `GET` is a 200; an admitted CSRF-only `POST`, and any owner answer
//! (a step-up challenge counts, SEC-043), passes when it is neither the door's refusal nor a 5xx,
//! and an own-site `POST` also when it is no 403 and logged no `csrf_rejected` event.
//! A row with a path parameter other than `{pick}`, `{complaint}`, `{kind}`, `{object}` or
//! `{token}` fails naming it. With no such row served the probe passes with zero routes. Planted
//! cases run the same judging code on a test-local router. Every name, phone and ID is fake.
//!
//! T401 (D-3106, D-3124; SEC-019, SEC-034, PICK-024): each side also has a released pick (a second
//! quoted job of the same business, page-picked and released on the $0 path by
//! [`released_pick`]); the exact path [`RELEASED_PAGE`] is filled with it, every other `{pick}`
//! with the waiting pick, so Confirm and "Can't take it" rows keep a pick that can be confirmed.
//! This file names no item of the fee crate (TD-FEE-07); the seeding lives in the submodule.
//!
//! T404 (D-3112, D-3134; FEE-031, FEE-005, SEC-019): every job the fixture picks first gets its
//! facts row (the card's hvac repair and AC not cooling, from the catalog stored once), and each
//! side has a third quoted job of the same business, page-picked and charged by
//! [`released_pick::charge`]; the exact path [`CHARGES_PAGE`] is filled with that charged pick.
//!
//! T472 (D-3750; PICK-050, SEC-019, SEC-026): an admitted "Can't take it" `POST` by X ends the
//! pick it names, so each side has a fourth quoted job of the same business, page-picked with no
//! backup named (seeded as `quoted_job` and the page pick are) and left waiting; the exact path
//! [`CANT_TAKE_PAGE`] is filled with it, so its end moves no pick onto another row's quote and the
//! Confirm and [`RELEASED_PAGE`] rows keep their own picks.
//!
//! T766 (D-4880; SEC-019, SEC-034, PRIV-028): `/photos` and below is the `PhotoLink` door (the
//! path's token is the key, no session opens it); a served row is probed by all six actors with
//! [`UNISSUED`] as its `{token}`, which must be a `no-store` 404 with no fixture detail and no
//! sign-in link ([`judge_unissued`]); a live token is T674's test. `{job}` is filled like `{j}`
//! with the side's own job number, and the other side's number and [`UNKNOWN_NUMBER`] must be the
//! one not-found answer (on `/h/requests/{job}/photos`).

#[path = r"door_matrix/released_pick.rs"]
mod released_pick;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    AccountStatus, NewComplaint, add_owner, create as create_business, record_complaint,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, IdSecret, Settings, SystemClock, TestControl};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, job_number, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor as AuditActor, AuditRow, export_audit};
use askhava_picks::api::{PagePick, ViewAnswer, page_pick, released_view};
use askhava_portals::api::{
    ActionKind, ActionTarget, CODE_SEND_ROUTE, CookieJar, EMAIL_CHECK_ROUTE, EMAIL_RESEND_ROUTE,
    IDENTITY_CHECK_ROUTE, LETTER_ROUTE, NUMBER_OLD_ROUTE, OwnerAuth, PortalsError, Realm,
    SERVICE_AREA_ROUTE, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SIGNUP_FORM_ROUTE, SIGNUP_ROUTE,
    SecondFactor, SignIn, SignInAddress, SignInMail, bootstrap_owner, csrf_token, current_session,
    ensure_pre_session, mint_action_link, open_action_link, owner_principal, owner_sign_in,
    signin_post, signin_request, start_session,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, Format, KeptCard, OfferTown,
    Place, Price, QuoteForm, QuoteRules, build_card, card_digest, keep_card, mark_card_sent,
    parse_quote, send_cards, submit_quote,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use totp_rs::Secret;
use tower::ServiceExt;

/// The fake host every request names, so a same-origin request passes the cross-site refusal.
const HOST: &str = "askhava.example";
/// The architecture file, whose section 13 is the route table.
const ARCHITECTURE: &str = include_str!("../../../docs/architecture.md");
/// The shared path-to-door cases, asserted by `auth_matrix.rs` too (D-2440).
const DOOR_CASES: &str = include_str!("../../../tests/security/door_cases.txt");
/// The path parameters the probe can fill from its fixtures.
const FILLS: [&str; 7] = [
    "{pick}",
    "{complaint}",
    "{j}",
    "{job}",
    "{kind}",
    "{object}",
    "{token}",
];
/// The job number no fixture's job has: the unknown `{j}` (D-4349).
const UNKNOWN_NUMBER: i64 = 9_999_999;
/// T666's job page, whose `{j}` is the side's own carded job number (D-4349).
const JOB_PAGE: &str = "/b/jobs/{j}";
/// T300's released-job page, whose `{pick}` is the side's released pick (PICK-024, D-3106).
const RELEASED_PAGE: &str = "/b/picks/{pick}";
/// T306's "Why was I charged?" page, whose `{pick}` is the side's charged pick (FEE-031, D-3112).
const CHARGES_PAGE: &str = "/b/charges/{pick}";
/// T472's "Can't take it" page and button, whose `{pick}` is the side's own waiting pick for it,
/// which an admitted `POST` ends (PICK-050, D-3750).
const CANT_TAKE_PAGE: &str = "/b/picks/{pick}/cant-take";
/// A well-formed link token nobody was given.
const UNISSUED: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
/// A base URL nothing listens on, for every vendor client, so no probe reaches a vendor.
const UNREACHABLE: &str = "http://127.0.0.1:9";
/// Homeowners A's and B's fake names, phones and streets.
const NAMES: [&str; 2] = ["Dana Testperson", "Robin Fakename"];
const PHONES: [&str; 2] = ["+19285550190", "+19285550191"];
const STREETS: [&str; 2] = ["100 Example Way", "200 Sample Road"];
/// The one owner's fake bootstrap token (32 characters) and fake pass phrase (D-3155).
const OWNER_TOKEN: &str = "fake-door-probe-token-0123456789";
const OWNER_PASSWORD: &str = "fake door probe pass phrase 2026";

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
    PhotoLink,
}

/// D-3641 (SEC-019, TD-SEC-61): the sign-up step paths open without a session, matched exactly,
/// never as a prefix; `CAPABILITY_ROUTE` and every other `/b/...` path keep the Business door.
const SIGNUP_STEPS: [&str; 9] = [
    SIGNUP_ROUTE,
    SIGNUP_FORM_ROUTE,
    CODE_SEND_ROUTE,
    NUMBER_OLD_ROUTE,
    LETTER_ROUTE,
    IDENTITY_CHECK_ROUTE,
    EMAIL_CHECK_ROUTE,
    EMAIL_RESEND_ROUTE,
    SERVICE_AREA_ROUTE,
];

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
    } else if path.split('/').any(|seg| seg == "sign-in") || SIGNUP_STEPS.contains(&path) {
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
    } else if under("/photos") {
        Door::PhotoLink
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
        Door::Link | Door::Webhook | Door::ActionLink | Door::PhotoLink => false,
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

/// The section 13 rows reading `yes` under the six probed doors; a `yes` row no door names fails
/// naming it (as `auth_matrix.rs` fails the run).
fn probed_rows() -> Vec<Row> {
    let probed = |door: &Door| {
        !matches!(
            door,
            Door::Link | Door::Asset | Door::Webhook | Door::SignIn
        )
    };
    served_rows()
        .into_iter()
        .filter(|(_, _, door)| probed(door))
        .collect()
}

/// D-4292, TD-SEC-73: the served `POST` rows [`probed_rows`] skips under the sign-in and link
/// doors (sixteen today: the four `/q/*` posts, `/f`, `/c`, `/b/sign-in` and its token step, the
/// eight sign-up steps, `/owner/sign-in` and `/owner/sign-in/passkey`), probed by
/// [`probe_skipped_csrf`].
fn skipped_posts() -> Vec<String> {
    let skipped = |(method, _, door): &Row| {
        *method == Method::POST && matches!(door, Door::Link | Door::SignIn)
    };
    let rows = served_rows().into_iter().filter(skipped);
    rows.map(|(_, template, _)| template).collect()
}

/// Every section 13 row reading `yes` with its door; a `yes` row no door names fails naming it.
fn served_rows() -> Vec<Row> {
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
        let method = Method::from_bytes(cells[1].as_bytes()).expect("a method");
        rows.push((method, cells[2].to_owned(), door));
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
    released: [Uuid; 2],
    charged: [Uuid; 2],
    cant_take: [Uuid; 2],
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

    /// The `{pick}`, `{complaint}` and `{j}` of `side`: `{j}` is the number of the side's own
    /// carded job (SEC-019, D-4349).
    fn ids(&self, side: usize) -> Ids {
        (self.picks[side], self.complaints[side], self.numbers[side])
    }

    /// The ids `template` is filled with for `side`: [`RELEASED_PAGE`] takes the released pick,
    /// [`CHARGES_PAGE`] the charged one, [`CANT_TAKE_PAGE`] its own waiting one, every other
    /// template the waiting one (SEC-019, PICK-024, FEE-031, PICK-050, D-3106, D-3112, D-3750).
    fn ids_on(&self, template: &str, side: usize) -> Ids {
        let (pick, complaint, number) = self.ids(side);
        match template {
            RELEASED_PAGE => (self.released[side], complaint, number),
            CHARGES_PAGE => (self.charged[side], complaint, number),
            CANT_TAKE_PAGE => (self.cant_take[side], complaint, number),
            _ => (pick, complaint, number),
        }
    }
}

/// A row's `{pick}`, `{complaint}` and `{j}` or `{job}` (job number) values.
type Ids = (Uuid, Uuid, i64);

/// `template` with `{pick}`, `{complaint}` and `{j}` or `{job}` from `ids` and the action-link
/// parameters from `link`; any other parameter fails naming it (D-2440, D-4349, D-4880).
fn fill(template: &str, (pick, complaint, number): Ids, link: &Minted) -> Result<String, String> {
    let mut path = template.to_owned();
    for seg in template.split('/').filter(|s| s.starts_with('{')) {
        let value = match seg {
            "{pick}" => pick.to_string(),
            "{complaint}" => complaint.to_string(),
            "{j}" | "{job}" => number.to_string(),
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

/// What came back: status, whether `Cache-Control` holds `no-store`, body and `Location`, and
/// whether it was a fail-closed CSRF refusal (a 403, or a `csrf_rejected` event logged under its
/// request ID, [`csrf_logged`]; D-4292).
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    no_store: bool,
    text: String,
    csrf_refused: bool,
}

/// How a request reaches the app (D-4292, TD-SEC-73): `Own` and `WrongToken` as a browser sends
/// its own form post under `Referrer-Policy: no-referrer` (`Sec-Fetch-Site: same-origin`,
/// `Origin: null`) with the jar's CSRF token or a wrong one; `Cross` from another site
/// (`Sec-Fetch-Site: cross-site`); `NullWrongToken` as a browser before Fetch Metadata
/// (`Origin: null`, no `Sec-Fetch-Site`) with a wrong token, which only the token check refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    Own,
    WrongToken,
    Cross,
    NullWrongToken,
}

/// The `Sec-Fetch-Site` (if any) and `Origin` a request from `site` carries.
fn site_headers(site: Site) -> (Option<&'static str>, String) {
    match site {
        Site::Own | Site::WrongToken => (Some("same-origin"), "null".to_owned()),
        Site::Cross => (Some("cross-site"), format!("https://{HOST}")),
        Site::NullWrongToken => (None, "null".to_owned()),
    }
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
        Site::WrongToken | Site::NullWrongToken => "0".repeat(64),
        Site::Own | Site::Cross => csrf_token(jar).unwrap_or_default(),
    };
    let (fetch, origin) = site_headers(site);
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin);
    if let Some(fetch) = fetch {
        builder = builder.header("sec-fetch-site", fetch);
    }
    builder
        .header("x-request-id", id.to_string())
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request")
}

/// Whether the [`Events`] subscriber in force (none in a planted case) logged a `csrf_rejected`
/// event under request ID `id` (SEC-048, D-4292).
fn csrf_logged(id: Option<Uuid>) -> bool {
    let logged = |events: &Events| {
        id.is_some_and(|id| events.of(id).iter().any(|e| e[0] == "csrf_rejected"))
    };
    tracing::dispatcher::get_default(|d| d.downcast_ref::<Events>().is_some_and(logged))
}

/// The answer to `request`, with the `name=value` of every non-empty cookie it sets.
async fn answer(app: &Router, request: Request<Body>) -> (Reply, Vec<String>) {
    let id = request.headers().get("x-request-id");
    let id = id.and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
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
        csrf_refused: parts.status == StatusCode::FORBIDDEN || csrf_logged(id),
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
/// so a step-up challenge passes), an admitted `POST` neither refusal nor 5xx nor a fail-closed
/// CSRF refusal (a public `POST` too, D-4292; a route's own 4xx passes); homeowner A never
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
            Door::Public => status < 500 && !reply.csrf_refused,
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
    let refused = method == Method::POST && reply.csrf_refused;
    let admitted = if strict {
        status == 200
    } else {
        !refusal(door, reply) && status < 500 && !refused
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
/// `POST` anything but the not-found answer, a 5xx or a fail-closed CSRF refusal (D-4292).
fn judge_opened(method: &Method, reply: &Reply) -> Result<(), String> {
    let status = reply.status;
    let opened = if method == Method::GET {
        status == 200
    } else {
        status != 404 && status < 500 && !reply.csrf_refused
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

/// SEC-019, SEC-034, PRIV-028 (D-4880): the answer to a photo link nobody was given is a
/// `no-store` 404 that shows no fixture detail and links to no sign-in page.
fn judge_unissued(reply: &Reply, fx: &Fixtures) -> Result<(), String> {
    let status = reply.status;
    if status != 404 || !reply.no_store {
        return Err(format!(
            "an unissued photo link is not a no-store 404: {status}"
        ));
    }
    if let Some(s) = shows(&reply.text, &fx.personal()) {
        return Err(format!("{status} shows fixture data {s:?}"));
    }
    if reply.text.contains("sign-in") {
        return Err(format!(
            "{status} links to a sign-in page: {:?}",
            reply.text
        ));
    }
    Ok(())
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
    owner: OwnerKey,
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
    let stripe =
        askhava_server::release_sink::stripe_webhook(&settings, pool.clone(), pairs.clone())
            .expect("stripe hooks");
    let all = webhooks.is_some() && voice.is_some() && pick_keys.is_some() && stripe.is_some();
    assert!(all, "every optional part of build_router is configured");
    let signin = askhava_server::business_signin::ServedSignIn::from_settings(
        &settings,
        pool.clone(),
        pairs.clone(),
    )
    .expect("sign-in part");
    let signup = askhava_server::business_signup::ServedSignUp::from_settings(pool.clone(), pairs)
        .expect("sign-up part");
    let parts = askhava_server::RouterParts::from((webhooks, voice, pick_keys, stripe))
        .with_sign_in(signin)
        .with_sign_up(signup);
    askhava_server::build_router(pool, control.router(), parts)
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

/// A job opened, carded to `business` and quoted by it, so it is in Quotes ready. Its card is
/// kept under its own digest and marked sent, as a real job's card is at approval and when it
/// goes out, so `quotes::api::sent_card` returns it and its Quote link opens the form (D-3189,
/// D-3706); the card is made from fake parts and its request ID is a fresh fake one.
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
    let parts = CardParts {
        category: "AC repair".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "The AC runs but the air from the vents stays warm.".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(card.text());
    let facts = CardFacts {
        category: "hvac_repair".to_owned(),
        candidates: vec!["AC not cooling".to_owned()],
        emergency: false,
    };
    let kept = keep_card(db, (Uuid::new_v4(), job), &digest, &card, &facts, now).await;
    assert_eq!(kept.expect("kept"), KeptCard::Stored);
    assert!(mark_card_sent(db, job, &digest, now).await.expect("sent"));
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

/// The vault over the router's own fake keys (`pairs`), so a page the router serves can read
/// what the fixtures store (D-3125).
fn vault() -> Vault {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x41), &key(0x42), &url).expect("vault")
}

/// Homeowner `side`'s vault contact (fake name, phone, street) and her request naming `job`.
async fn her_request(db: &mut PgConnection, side: usize, job: Uuid, now: Timestamp) -> Uuid {
    let vault = vault();
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
    mint_kind(db, team, (ActionKind::Quote, object), now).await
}

/// A `kind` link for `object`, minted for `team`'s owner (SEC-025).
async fn mint_kind(
    db: &mut PgConnection,
    team: Team,
    (kind, object): (ActionKind, Uuid),
    now: Timestamp,
) -> Minted {
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
    released: Uuid,
    charged: Uuid,
    cant_take: Uuid,
    complaint: Uuid,
    link: Minted,
}

/// The page pick of the one quote on `job` (PICK-012), after `job`'s facts row: its card's
/// candidate, hvac repair and AC not cooling, so not fee-free, the default visit tier, one-time
/// (FEE-005, D-2331, D-3112).
async fn picked(pool: &PgPool, job: Uuid, now: Timestamp) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(&mut db, job, "hvac_repair", &types).await;
    facts.expect("job facts");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, AuditActor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    pick
}

/// Side `side`: a business with its owner, her job it quoted, her request and page pick of the
/// quote, the job's quote token and number, a complaint and a Quote action link; and a second
/// job of the business, quoted, page-picked and released to her contact on the $0 path (D-3106);
/// and a third, quoted, page-picked and charged (D-3112); and a fourth, quoted and page-picked
/// with no backup, left waiting for the "Can't take it" rows (D-3750).
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
    let pick = picked(pool, job, now).await;
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
    let second = quoted_job(&mut db, business, now).await;
    let free = picked(pool, second, now).await;
    released_pick::release_free(pool, &vault(), (business, second, free), contact, now).await;
    let third = quoted_job(&mut db, business, now).await;
    let paid = picked(pool, third, now).await;
    released_pick::charge(pool, (business, third, paid), now).await;
    let fourth = quoted_job(&mut db, business, now).await;
    let cant_take = picked(pool, fourth, now).await;
    Side {
        contact,
        team,
        job,
        number,
        token: token.as_str().to_owned(),
        pick,
        released: free,
        charged: paid,
        cant_take,
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

/// The bootstrapped owner's authenticator secret, the probe's clock and its sign-ins so far.
struct OwnerKey {
    secret: Vec<u8>,
    now: Timestamp,
    signins: AtomicI64,
}

/// The one owner, bootstrapped at `now` through `bootstrap_owner` with a fake token and pass
/// phrase; its base32 authenticator secret decoded (SEC-005, SEC-044; D-1917, D-3155).
async fn bootstrapped_owner(pool: &PgPool, now: Timestamp) -> OwnerKey {
    let mut db = pool.acquire().await.expect("db");
    let enrolment = bootstrap_owner(&mut db, Some(OWNER_TOKEN), OWNER_PASSWORD, now).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let secret = secret.as_bytes().to_vec();
    let signins = AtomicI64::new(0);
    OwnerKey {
        secret,
        now,
        signins,
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now` (30-second steps), as
/// `tests/security/tests/owner_auth.rs` computes it.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// A real owner session through `owner_sign_in` (SEC-005, SEC-043; D-3155, D-3744): the k-th
/// sign-in is at the probe's clock plus k times 30 seconds with that step's code, because a
/// TOTP step at or below the last one used is refused.
async fn owner_jar(db: &mut PgConnection, key: &OwnerKey) -> CookieJar {
    let k = key.signins.fetch_add(1, Ordering::SeqCst);
    let at = key.now + SignedDuration::from_secs(30 * k);
    let code = totp_code(&key.secret, at);
    let auth = OwnerAuth::new("askhava.example", "https://askhava.example").expect("party");
    let factor = (OWNER_PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(db, CookieJar::new(), factor, &auth, at).await;
    jar.expect("owner signed in")
}

/// `actor` signed in (anonymous: only the pre-session cookie; homeowners a 12-hour session on
/// their vault contact; the owner through owner sign-in, [`owner_jar`]; a business by its link).
async fn signed_in(pool: &PgPool, actor: Actor, sides: &[Side; 2], owner: &OwnerKey) -> CookieJar {
    let now = owner.now;
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(12);
    let jar = match actor {
        Actor::Anonymous => ensure_pre_session(CookieJar::new()).expect("pre-session"),
        Actor::BusinessX | Actor::BusinessY => business_jar(pool, sides[actor.side()].team).await,
        Actor::Owner => owner_jar(&mut db, owner).await,
        Actor::HomeownerA | Actor::HomeownerB => {
            let login = (Realm::Homeowner, sides[actor.side()].contact);
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
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    drop(db);
    let sides = [side(pool, 0, now).await, side(pool, 1, now).await];
    let owner = bootstrapped_owner(pool, now).await;
    let mut who = Vec::new();
    for actor in ACTORS {
        let jar = signed_in(pool, actor, &sides, &owner).await;
        who.push(Who { actor, jar });
    }
    let [a, b] = sides;
    let fx = Fixtures {
        jobs: [a.job, b.job],
        numbers: [a.number, b.number],
        tokens: [a.token, b.token],
        picks: [a.pick, b.pick],
        released: [a.released, b.released],
        charged: [a.charged, b.charged],
        cant_take: [a.cant_take, b.cant_take],
        complaints: [a.complaint, b.complaint],
        links: [a.link, b.link],
    };
    World {
        app: router(pool.clone()),
        pool: pool.clone(),
        owner,
        who,
        fx,
        teams: [a.team, b.team],
    }
}

/// A realm, owner or public row by `who` (SEC-019): the door's answer, and on a homeowner or
/// business route the other side's `{pick}`, `{complaint}`, `{j}` or `{job}` is the one not-found
/// answer.
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
    let reply = get(fill(template, w.fx.ids_on(template, side), link)?).await;
    judge(door, (who.actor, method), &reply, &w.fx)?;
    let foreign = ["{pick}", "{complaint}", "{j}", "{job}"]
        .iter()
        .any(|p| template.contains(p));
    let realm = matches!(door, Door::Business | Door::Homeowner);
    if !realm || !allowed(door, who.actor) || !foreign {
        return Ok(());
    }
    let theirs = get(fill(template, w.fx.ids_on(template, 1 - side), link)?).await;
    let fresh = (Uuid::new_v4(), Uuid::new_v4(), UNKNOWN_NUMBER);
    let unknown = get(fill(template, fresh, link)?).await;
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
/// `no-store`; a token minted for Y's member on X's kind and object is bound to Y, who holds no
/// card for X's job, so it too is the one not-found answer ([`probe_bound`], D-3706). X's own
/// Quote link opens on its carded job (`quoted_job` keeps and sends the card, D-3189).
async fn probe_link(
    w: &World,
    (method, template): (&Method, &str),
    who: &Who,
) -> Result<(), String> {
    let mut db = w.pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let mine = mint(&mut db, w.teams[0], w.fx.jobs[0], now).await;
    let theirs = mint(&mut db, w.teams[1], w.fx.jobs[1], now).await;
    let ids = w.fx.ids_on(template, 0);
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
    probe_bound(w, (method, template), who, (&mine, &not_found)).await?;
    let before = open(&w.pool, &mine).await;
    judge_opened(method, &get(mine.clone()).await?)?;
    judge_unchanged(method, before, open(&w.pool, &mine).await)
}

/// The business audit rows that name `business` as their actor, read through
/// `ops::api::export_audit` page by page.
async fn acted(pool: &PgPool, business: Uuid) -> usize {
    let mut db = pool.acquire().await.expect("db");
    let (mut after, mut rows) = (0, 0);
    loop {
        let page = export_audit(&mut db, after, i64::MAX).await.expect("audit");
        let Some(last) = page.last() else {
            return rows;
        };
        after = last.id;
        let by = |r: &&AuditRow| r.actor_kind == "business" && r.actor_id == Some(business);
        rows += page.iter().filter(by).count();
    }
}

/// SEC-025, SEC-004, BIZ-038, CARD-023, D-3614, D-3706: a link minted for Y's member on X's kind
/// and object (`mine`, a Quote link, `UseBy::LinkAlone`) is bound to Y, the one person it names,
/// whoever's session is open; X's card never went to Y, so there is no quote form for Y on X's
/// job and the link is the one not-found answer for every session and both methods, neither X's
/// nor Y's business gains an audit row, and X's link and Y's link both stay unused. A server that
/// took the person from the session or from the job's own business would open X's carded form
/// there and fail. A session kind (`UseBy::Session`: Away, on X's business) minted for Y and sent with X's
/// session is the one not-found answer and acts as nobody: no audit row for X or Y, Y's link unused.
async fn probe_bound(
    w: &World,
    (method, template): (&Method, &str),
    who: &Who,
    (mine, not_found): (&Minted, &Reply),
) -> Result<(), String> {
    let [x, y] = w.teams;
    let mut db = w.pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let linked = mint_kind(&mut db, y, (mine.kind, mine.object), now).await;
    let session = mint_kind(&mut db, y, (ActionKind::Away, x.business), now).await;
    drop(db);
    let ids = w.fx.ids_on(template, 0);
    let get = |link: &Minted| {
        let path = fill(template, ids, link);
        async move {
            let path = path?;
            Ok::<_, String>(
                send(&w.app, (method, &path), &who.jar, Site::Own, Uuid::new_v4()).await,
            )
        }
    };
    let state = async || {
        (
            acted(&w.pool, x.business).await,
            acted(&w.pool, y.business).await,
            open(&w.pool, mine).await,
            open(&w.pool, &linked).await,
        )
    };
    let before = state().await;
    judge_not_found(&get(&linked).await?, not_found)?;
    if state().await != before || !before.2 || !before.3 {
        return Err("Y's link on X's object acted or was used (SEC-025)".to_owned());
    }
    if who.actor != Actor::BusinessX {
        return Ok(());
    }
    let both = async || {
        (
            acted(&w.pool, x.business).await,
            acted(&w.pool, y.business).await,
        )
    };
    let before = both().await;
    judge_not_found(&get(&session).await?, not_found)?;
    if both().await != before || !open(&w.pool, &session).await {
        return Err("X's session acted through Y's session-kind link (SEC-025)".to_owned());
    }
    Ok(())
}

/// SEC-048, D-4292: `who`'s cross-site and wrong-token `POST`s to `template`, the wrong token sent
/// as a browser's own post and as one without `Sec-Fetch-Site` and with `Origin: null`, are each
/// a 403 with one `csrf_rejected` event naming the template.
async fn probe_csrf(w: &World, events: &Events, template: &str, who: &Who) -> Result<(), String> {
    let side = who.actor.side();
    let path = fill(template, w.fx.ids_on(template, side), &w.fx.links[side])?;
    for site in [Site::Cross, Site::WrongToken, Site::NullWrongToken] {
        let id = Uuid::new_v4();
        let reply = send(&w.app, (&Method::POST, &path), &who.jar, site, id).await;
        judge_csrf(&reply, &events.of(id), template).map_err(|e| format!("{site:?}: {e}"))?;
    }
    Ok(())
}

/// The quote page forms that need a `quote` field to parse, so their CSRF check is reached.
const QUOTE_FIELD_FORMS: [&str; 3] = ["/q/{token}", "/q/{token}/pick", "/q/{token}/code"];

/// SEC-042, SEC-048, D-4292, TD-SEC-73: an anonymous browser's `POST` to a skipped sign-in or link
/// row ([`skipped_posts`]), filled with A's quote token and a check-in answer, with a wrong token,
/// no `Sec-Fetch-Site` and `Origin: null`, is a 403 with one `csrf_rejected` event naming the
/// template: the token check backs the `Origin: null` fallback on these doors too.
async fn probe_skipped_csrf(w: &World, events: &Events, template: &str) -> Result<(), String> {
    let mut path = template.to_owned();
    for (seg, value) in [("{token}", w.fx.tokens[0].as_str()), ("{answer}", "3-done")] {
        path = path.replacen(seg, value, 1);
    }
    if path.contains('{') {
        return Err(format!("{template}: no fill for its parameter (D-4292)"));
    }
    let extra = if QUOTE_FIELD_FORMS.contains(&template) {
        "&quote=1"
    } else {
        ""
    };
    let anonymous = w.who.iter().find(|who| who.actor == Actor::Anonymous);
    let jar = &anonymous.expect("the anonymous actor").jar;
    let id = Uuid::new_v4();
    let sent = request((&Method::POST, &path, extra), jar, Site::NullWrongToken, id);
    let reply = answer(&w.app, sent).await.0;
    judge_csrf(&reply, &events.of(id), template).map_err(|e| format!("POST {template}: {e}"))
}

/// The owner signed in again before each row it probes, so a sign-out row it probed ends only
/// that session (D-3744).
async fn owner_again(w: &World) -> CookieJar {
    let mut db = w.pool.acquire().await.expect("db");
    owner_jar(&mut db, &w.owner).await
}

/// A photo-link row by `who` (SEC-019, SEC-034, D-4880): [`UNISSUED`] as its `{token}` is
/// [`judge_unissued`]'s answer; a live token is T674's test.
async fn probe_photo(
    w: &World,
    (method, template): (&Method, &str),
    who: &Who,
) -> Result<(), String> {
    let side = who.actor.side();
    let unissued = Minted {
        token: UNISSUED.to_owned(),
        ..w.fx.links[side].clone()
    };
    let path = fill(template, w.fx.ids_on(template, side), &unissued)?;
    let reply = send(&w.app, (method, &path), &who.jar, Site::Own, Uuid::new_v4()).await;
    judge_unissued(&reply, &w.fx)
}

/// Every actor on one row (the owner signed in afresh, [`owner_again`]).
async fn probe_row(
    w: &World,
    events: &Events,
    (method, template, door): &Row,
) -> Result<(), String> {
    for who in &w.who {
        let again = match who.actor {
            Actor::Owner => Some(Who {
                actor: who.actor,
                jar: owner_again(w).await,
            }),
            _ => None,
        };
        let who = again.as_ref().unwrap_or(who);
        let case = |e: String| format!("{method} {template} as {:?}: {e}", who.actor);
        if *door == Door::ActionLink {
            probe_link(w, (method, template), who).await.map_err(case)?;
        } else if *door == Door::PhotoLink {
            probe_photo(w, (method, template), who)
                .await
                .map_err(case)?;
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

/// TD-SEC-61, TD-SEC-73 (SEC-019, SEC-034, SEC-048, SEC-042, D-2440, D-4292): every section 13
/// row reading `yes` under the homeowner, business, owner, action-link or public door, by all six
/// actors over HTTP through `build_router`, and every skipped sign-in or link `POST` by
/// [`probe_skipped_csrf`]; with none served it passes with zero routes, the fixtures still built.
///
/// `admit_security_event` keeps at most `SECURITY_EVENT_CAP_FLOOR` (50) events of one kind per
/// route template and client per minute, and every probe is client `0.0.0.0`: a probed `POST`
/// template gets three refusals from each of the six actors (eighteen) and a skipped one gets one,
/// so none nears 50 and `configure_security_events` is not called.
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
    for template in &skipped_posts() {
        if let Err(problem) = probe_skipped_csrf(&w, &events, template).await {
            panic!("{problem}");
        }
    }
}

/// TD-SEC-61 (SEC-019, PICK-024, D-3106): each side's released pick is shown through
/// `released_view` to its own business's owner and is the one not-found answer to the other
/// side's; [`RELEASED_PAGE`] is filled with it and every other `{pick}` with the waiting pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_61_each_released_pick_opens_to_its_own_business_only(pool: PgPool) {
    let w = world(&pool).await;
    for side in 0..2 {
        for (viewer, team) in w.teams.iter().enumerate() {
            let proof = askhava_server::session_minter().business(team.member, team.business);
            let pick = w.fx.released[side];
            let answer = released_view(&pool, &vault(), &proof, pick).await;
            let answer = answer.expect("view");
            let right = if viewer == side {
                matches!(answer, ViewAnswer::Shown(_))
            } else {
                matches!(answer, ViewAnswer::NotFound)
            };
            assert!(right, "released pick {side} to business {viewer}");
        }
        assert_eq!(w.fx.ids_on(RELEASED_PAGE, side).0, w.fx.released[side]);
        assert_eq!(w.fx.ids_on("/b/picks/{pick}/confirm", side), w.fx.ids(side));
    }
}

/// TD-SEC-61 (SEC-005, SEC-019, D-3155): the fixture's owner session, made through owner sign-in,
/// is an owner principal, and a plain owner-realm session without its owner-session row is not,
/// so the fixture is proved before any owner route is served.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_61_the_owner_actor_is_an_owner_principal(pool: PgPool) {
    let w = world(&pool).await;
    let now = w.owner.now;
    let mut db = pool.acquire().await.expect("db");
    let owner = w
        .who
        .iter()
        .find(|who| who.actor == Actor::Owner)
        .expect("owner");
    let principal = owner_principal(&mut db, &owner.jar, now)
        .await
        .expect("read");
    assert!(
        principal.is_some(),
        "the fixture's owner session is an owner"
    );
    let life = SignedDuration::from_hours(12);
    let login = (Realm::Owner, Uuid::new_v4());
    let plain = start_session(&mut db, CookieJar::new(), login, life, now).await;
    let plain = plain.expect("session");
    let none = owner_principal(&mut db, &plain, now).await.expect("read");
    assert!(none.is_none(), "a plain owner-realm session is no owner");
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
    assert_eq!(seen.len(), 10, "every door has a case: {seen:?}");
    assert_eq!(
        door("/elsewhere", false),
        None,
        "no door: auth_matrix.rs fails the run"
    );
}

/// TD-SEC-61, TD-SEC-79: a row with a parameter the fixtures do not fill fails naming it; the
/// seven fills work (D-4880).
#[test]
fn td_sec_61_an_unknown_parameter_fails_naming_it() {
    let fx = planted_fixtures();
    let problem = fill("/b/made-up/{widget}", fx.ids(0), &fx.links[0]).expect_err("{widget}");
    assert!(
        problem.contains("{widget}") && problem.contains("/b/made-up/{widget}"),
        "{problem}"
    );
    let path = fill("/b/a/{kind}/{object}/{token}", fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(
        path,
        format!("/b/a/quote/{}/{}", fx.links[1].object, fx.links[1].token)
    );
    let path = fill("/b/{pick}/{complaint}", fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(path, format!("/b/{}/{}", fx.picks[1], fx.complaints[1]));
    let path = fill(JOB_PAGE, fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(path, format!("/b/jobs/{}", fx.numbers[1]));
    let path = fill(PHOTOS_PAGE, fx.ids(1), &fx.links[1]).expect("filled");
    assert_eq!(path, format!("/h/requests/{}/photos", fx.numbers[1]));
}

/// T674's homeowner photo page, whose `{job}` is the side's own job number (D-4880).
const PHOTOS_PAGE: &str = "/h/requests/{job}/photos";

/// A planted `/h/requests/{job}/photos` that shows only A's own request, number 1001, and gives
/// every other number the one not-found answer.
async fn planted_photos(Path(number): Path<i64>) -> Response {
    let kept = [(header::CACHE_CONTROL, "no-store")];
    match number {
        1001 => (kept, "Photos for request 1001").into_response(),
        _ => (StatusCode::NOT_FOUND, kept).into_response(),
    }
}

/// TD-SEC-61 (SEC-019, D-4880): on a planted router A's `{job}` fill is admitted, and B's number
/// and an unknown number are the one not-found answer.
#[tokio::test]
async fn td_sec_61_the_job_fill_admits_a_and_hides_b() {
    let fx = planted_fixtures();
    let app = Router::new().route(PHOTOS_PAGE, get(planted_photos));
    let jar = ensure_pre_session(CookieJar::new()).expect("jar");
    let get = |ids: Ids| {
        let path = fill(PHOTOS_PAGE, ids, &fx.links[0]).expect("filled");
        let jar = &jar;
        let app = &app;
        async move { send(app, (&Method::GET, &path), jar, Site::Own, Uuid::new_v4()).await }
    };
    assert_eq!(door(PHOTOS_PAGE, false), Some(Door::Homeowner));
    let as_a = (Actor::HomeownerA, &Method::GET);
    let own = get(fx.ids_on(PHOTOS_PAGE, Actor::HomeownerA.side())).await;
    assert_eq!(judge(Door::Homeowner, as_a, &own, &fx), Ok(()));
    let theirs = get(fx.ids_on(PHOTOS_PAGE, Actor::HomeownerB.side())).await;
    let unknown = get((Uuid::new_v4(), Uuid::new_v4(), UNKNOWN_NUMBER)).await;
    assert_eq!(judge_not_found(&theirs, &unknown), Ok(()));
    assert_eq!(theirs.status, 404);
    let leak = judge_not_found(&own, &unknown);
    assert!(leak.is_err(), "A's own request read as not found: {leak:?}");
}

/// The planted photo links: `good` the right answer to an unissued token, the others leaking one
/// (a fixture phone, a sign-in link, a cacheable 404, a 200).
async fn planted_photo(Path(token): Path<String>) -> Response {
    let kept = [(header::CACHE_CONTROL, "no-store")];
    let missing = StatusCode::NOT_FOUND;
    match token.as_str() {
        "phone" => (missing, kept, "Call +19285550190").into_response(),
        "sign-in" => (missing, kept, "<a href=\"/h/sign-in\">Sign in</a>").into_response(),
        "cached" => missing.into_response(),
        "open" => (kept, "A photo").into_response(),
        _ => (missing, kept).into_response(),
    }
}

/// TD-SEC-61 (SEC-019, SEC-034, D-4880): `/photos/{token}` is the `PhotoLink` door, which admits
/// no session; on a planted router an unissued token is a `no-store` 404 for all six actors, and
/// each leaking answer fails.
#[tokio::test]
async fn td_sec_61_an_unissued_photo_link_is_a_no_store_404() {
    let fx = planted_fixtures();
    let template = "/photos/{token}";
    assert_eq!(door(template, false), Some(Door::PhotoLink));
    let app = Router::new().route(template, get(planted_photo));
    let jar = ensure_pre_session(CookieJar::new()).expect("jar");
    let unissued = Minted {
        token: UNISSUED.to_owned(),
        ..fx.links[0].clone()
    };
    let path = fill(template, fx.ids(0), &unissued).expect("filled");
    for actor in ACTORS {
        assert!(!allowed(Door::PhotoLink, actor), "{actor:?}");
        let reply = send(&app, (&Method::GET, &path), &jar, Site::Own, Uuid::new_v4()).await;
        assert_eq!(judge_unissued(&reply, &fx), Ok(()), "{actor:?}");
    }
    for leak in [
        "/photos/phone",
        "/photos/sign-in",
        "/photos/cached",
        "/photos/open",
    ] {
        let reply = send(&app, (&Method::GET, leak), &jar, Site::Own, Uuid::new_v4()).await;
        assert!(judge_unissued(&reply, &fx).is_err(), "{leak}");
    }
}

/// A planted `/b/jobs/{j}` that shows only X's own job, number 1001, and gives every other number
/// the one not-found answer.
async fn planted_job(Path(number): Path<i64>) -> Response {
    let kept = [(header::CACHE_CONTROL, "no-store")];
    match number {
        1001 => (kept, "Job 1001").into_response(),
        _ => (StatusCode::NOT_FOUND, kept).into_response(),
    }
}

/// TD-SEC-79 (SEC-019, SEC-048, D-4349): on a planted router X's `{j}` fill is admitted, and Y's
/// number and an unknown number are the one not-found answer.
#[tokio::test]
async fn td_sec_79_the_job_number_fill_admits_x_and_hides_y() {
    let fx = planted_fixtures();
    let app = Router::new().route(JOB_PAGE, get(planted_job));
    let jar = ensure_pre_session(CookieJar::new()).expect("jar");
    let get = |ids: Ids| {
        let path = fill(JOB_PAGE, ids, &fx.links[0]).expect("filled");
        let jar = &jar;
        let app = &app;
        async move { send(app, (&Method::GET, &path), jar, Site::Own, Uuid::new_v4()).await }
    };
    let as_x = (Actor::BusinessX, &Method::GET);
    let own = get(fx.ids_on(JOB_PAGE, Actor::BusinessX.side())).await;
    assert_eq!(judge(Door::Business, as_x, &own, &fx), Ok(()));
    let theirs = get(fx.ids_on(JOB_PAGE, Actor::BusinessY.side())).await;
    let unknown = get((Uuid::new_v4(), Uuid::new_v4(), UNKNOWN_NUMBER)).await;
    assert_eq!(judge_not_found(&theirs, &unknown), Ok(()));
    assert_eq!(theirs.status, 404);
    let leak = judge_not_found(&own, &unknown);
    assert!(leak.is_err(), "X's own job read as not found: {leak:?}");
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
        released: [Uuid::from_u128(31), Uuid::from_u128(32)],
        charged: [Uuid::from_u128(41), Uuid::from_u128(42)],
        cant_take: [Uuid::from_u128(51), Uuid::from_u128(52)],
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
        csrf_refused: status == 403,
    };
    let event = |route: &str| ["csrf_rejected".to_owned(), route.to_owned()];
    let t = "/b/notices";
    assert!(judge_csrf(&reply(403), &[event(t)], t).is_ok());
    assert!(judge_csrf(&reply(403), &[], t).is_err());
    assert!(judge_csrf(&reply(403), &[event(t), event(t)], t).is_err());
    assert!(judge_csrf(&reply(403), &[event("unmatched")], t).is_err());
    assert!(judge_csrf(&reply(200), &[event(t)], t).is_err());
}

/// TD-SEC-73 (SEC-042, SEC-048, D-4292, D-3706): an own-site `POST` on the public door, an
/// admitted realm row or an action link fails on a fail-closed CSRF refusal (a 403, or a 200 with
/// a `csrf_rejected` event under its request ID) and passes on a route's own 4xx with no event.
#[test]
fn td_sec_73_an_own_post_fails_on_a_csrf_refusal() {
    let fx = planted_fixtures();
    let reply = |status, csrf_refused| Reply {
        status,
        no_store: true,
        text: String::new(),
        csrf_refused,
    };
    let post = &Method::POST;
    for (got, ok) in [
        (reply(422, false), true),
        (reply(303, false), true),
        (reply(403, true), false),
        (reply(200, true), false),
    ] {
        let public = judge(Door::Public, (Actor::Anonymous, post), &got, &fx);
        let realm = judge(Door::Business, (Actor::BusinessX, post), &got, &fx);
        let opened = judge_opened(post, &got);
        for judged in [public, realm, opened] {
            assert_eq!(judged.is_ok(), ok, "{got:?}: {judged:?}");
        }
    }
}

/// A planted route that answers 200 and logs a `csrf_rejected` security event under the
/// request's own ID, as a fail-closed refusal that does not answer 403 would.
async fn planted_csrf_event(headers: HeaderMap) -> StatusCode {
    let id = headers.get("x-request-id").and_then(|v| v.to_str().ok());
    let id = id.unwrap_or_default();
    tracing::warn!(kind = "csrf_rejected", route = "/planted", request_id = %id, "security event");
    StatusCode::OK
}

/// TD-SEC-73 (SEC-048, D-4292): [`answer`] marks a 200 as a CSRF refusal when the [`Events`]
/// subscriber in force logged a `csrf_rejected` event under its request ID, and not without it,
/// so an own-site `POST` refused that way fails the matrix.
#[tokio::test]
async fn td_sec_73_a_logged_csrf_event_marks_the_reply_refused() {
    let app = Router::new().route("/planted", post(planted_csrf_event));
    let send = |id: Uuid| {
        let request = Request::builder().method(Method::POST).uri("/planted");
        let request = request.header("x-request-id", id.to_string());
        request.body(Body::empty()).expect("request")
    };
    let (bare, _) = answer(&app, send(Uuid::new_v4())).await;
    assert_eq!((bare.status, bare.csrf_refused), (200, false));
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let id = Uuid::new_v4();
    let (logged, _) = answer(&app, send(id)).await;
    assert_eq!(
        events.of(id),
        [["csrf_rejected".to_owned(), "/planted".to_owned()]]
    );
    assert_eq!((logged.status, logged.csrf_refused), (200, true));
    let fx = planted_fixtures();
    let judged = judge(
        Door::Public,
        (Actor::Anonymous, &Method::POST),
        &logged,
        &fx,
    );
    assert!(judged.is_err(), "{judged:?}");
    assert!(judge_opened(&Method::POST, &logged).is_err());
}

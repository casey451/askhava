//! TD-SEC-66: the owner email's Approve link through the assembled router (T399; BIZ-033,
//! BIZ-038, SEC-038, SEC-004, SEC-025, SEC-026, SEC-048; D-1955, D-3081, D-3744, D-3889).
//!
//! `server::build_router` over a real test database. Links are minted with
//! `portals::api::mint_action_link` (D-1955's hash, the backdated `created_at` production
//! writes); the phone is a "Confirm and billing" member's, held out of the sender directory until
//! the owner approves it, and every lookup goes through `messaging::api::sender_business`.
//! Each person signs in through the sign-in link emailed to their own fake mailbox (as
//! `action_links_route.rs` does) and step-ups are recorded with `record_step_up`. Fake
//! numbers (555-01xx) and fake IDs only.

use askhava_businesses::api::{
    Acting, Roles, SenderLink, TeamError, add_member, add_owner, change_phone_at, create,
    link_phone, record_step_up, waiting_phones,
};
use askhava_messaging::api::{OutboxError, sender_add, sender_business};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_core::api::SystemClock;
use askhava_portals::api::{
    ActionKind, ActionTarget, CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SignIn, SignInAddress,
    SignInMail, csrf_token, ensure_pre_session, mint_action_link, open_action_link,
    send_sign_in_link, signin_post,
};
use askhava_privacy::api::Vault;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.example";

/// The sender directory over a fake vault, as the composition links phones.
struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        let added = sender_add(db, &self.0, business_line, number.expose_secret()).await;
        added.map_err(|e| match e {
            OutboxError::SenderTaken => TeamError::PhoneTaken,
            _ => TeamError::Database,
        })
    }
}

/// One business: its owner and a "Confirm and billing" member.
#[derive(Clone, Copy)]
struct Team {
    owner: Acting,
    biller: Acting,
    owner_box: Uuid,
    biller_box: Uuid,
}

/// The router, the pool, the directory and one business.
struct World {
    app: Router,
    pool: PgPool,
    dir: Directory,
    team: Team,
}

/// `minutes` from now (negative: before).
fn ago(minutes: i64) -> Timestamp {
    Timestamp::now() - SignedDuration::from_mins(minutes)
}

/// A business with its owner and a "Confirm and billing" member.
async fn team(db: &mut PgConnection) -> Team {
    let business = create(db, false, ago(60)).await.expect("business");
    let (owner_box, biller_box) = (Uuid::new_v4(), Uuid::new_v4());
    let member = add_owner(db, business, owner_box, ago(60)).await;
    let owner = Acting {
        business,
        member: member.expect("owner"),
    };
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(db, owner, biller_box, roles, ago(60)).await;
    let biller = Acting {
        business,
        member: member.expect("member"),
    };
    Team {
        owner,
        biller,
        owner_box,
        biller_box,
    }
}

async fn world(pool: &PgPool) -> World {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let dir = Directory(Vault::new(&key(0x54), &key(0x64), &url).expect("vault"));
    let mut db = pool.acquire().await.expect("db");
    let team = team(&mut db).await;
    World {
        app: askhava_server::build_router(pool.clone(), Router::new(), (None, None, None, None)),
        pool: pool.clone(),
        dir,
        team,
    }
}

/// `by` links `number`, `minutes` ago: a member's phone waits for the owner.
async fn linked(w: &World, by: Acting, number: &str, minutes: i64) -> Uuid {
    let mut db = w.pool.acquire().await.expect("db");
    let number = SecretString::from(number.to_owned());
    let line = link_phone(&mut db, &w.dir, (by, false), &number, ago(minutes)).await;
    line.expect("linked")
}

/// The business `number` texts for, if any.
async fn found(w: &World, number: &str) -> Option<Uuid> {
    let mut db = w.pool.acquire().await.expect("db");
    sender_business(&mut db, &w.dir.0, number)
        .await
        .expect("lookup")
}

/// Whether `line` still waits for the owner.
async fn waiting(w: &World, line: Uuid) -> bool {
    let mut db = w.pool.acquire().await.expect("db");
    let phones = waiting_phones(&mut db, w.team.owner.business).await;
    phones.expect("waiting").iter().any(|p| p.line == line)
}

/// The path of an Approve link on `line` for the owner, issued `minutes` ago, ending at `ends`.
async fn mint(w: &World, line: Uuid, minutes: i64, ends: Timestamp) -> String {
    let mut db = w.pool.acquire().await.expect("db");
    let target = ActionTarget {
        kind: ActionKind::Approve,
        object: line,
        member: w.team.owner.member,
        business: w.team.owner.business,
    };
    let link = mint_action_link(&mut db, target, None, (ago(minutes), ends)).await;
    let link = link.expect("minted");
    let at = link.expose_secret().find("/b/a/").expect("an action path");
    link.expose_secret()[at..].to_owned()
}

/// A live Approve link on `line`, issued now.
async fn live(w: &World, line: Uuid) -> String {
    mint(w, line, 0, ago(-120)).await
}

/// Whether the link at `path` is still unused and unexpired.
async fn unused(w: &World, path: &str) -> bool {
    let parts: Vec<&str> = path.trim_start_matches("/b/a/").split('/').collect();
    let object = Uuid::parse_str(parts[1]).expect("object");
    let mut db = w.pool.acquire().await.expect("db");
    let open = open_action_link(&mut db, (ActionKind::Approve, object), parts[2], ago(0));
    open.await.is_ok()
}

/// The one sign-in address of one person; the token of the link it "emails" is kept.
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
        let token = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = token;
        Ok(())
    }
}

/// `who` (with sign-in `mailbox`) signed in through their emailed sign-in link (D-1724), with a
/// step-up passed a minute ago when `step_up`: their cookies.
async fn session(w: &World, (who, mailbox): (Acting, Uuid), step_up: bool) -> CookieJar {
    let mut db = w.pool.acquire().await.expect("db");
    if step_up {
        record_step_up(&mut db, who, ago(1)).await.expect("step-up");
    }
    let to = SignInAddress {
        business: who.business,
        mailbox,
    };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let sent = send_sign_in_link(&mut db, &*mail, to, ago(0)).await;
    sent.expect("link");
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pre = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let press = Router::new().route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app = SignIn::new(w.pool.clone(), Arc::new(SystemClock), mail).into_router(press);
    let pressed = app.oneshot(request((Method::POST, &path), &pre)).await;
    let pressed = pressed.expect("pressed");
    let cookies: Vec<&str> = pressed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// `method path` with `jar`'s cookies and CSRF token.
fn request((method, path): (Method, &str), jar: &CookieJar) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let csrf = csrf_token(jar).unwrap_or_default();
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}")))
        .expect("request")
}

/// [`request`] through the router: its status and body.
async fn send(w: &World, asked: (Method, &str), jar: &CookieJar) -> (u16, String) {
    let response = w.app.clone().oneshot(request(asked, jar)).await;
    let response = response.expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The owner presses Approve with a fresh step-up.
async fn approve(w: &World, path: &str) -> (u16, String) {
    let jar = session(w, (w.team.owner, w.team.owner_box), true).await;
    send(w, (Method::POST, path), &jar).await
}

/// TD-SEC-66: opening the link changes nothing however often; the owner's press with a fresh
/// step-up approves once and uses the link; a used link approves no more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_66_the_owner_approves_once_with_a_fresh_step_up(pool: PgPool) {
    let w = world(&pool).await;
    let line = linked(&w, w.team.biller, "928-555-0150", 30).await;
    let path = live(&w, line).await;
    let signed_out = ensure_pre_session(CookieJar::new()).expect("pre-session");
    for _ in 0..2 {
        let (status, _) = send(&w, (Method::GET, &path), &signed_out).await;
        assert_eq!(status, 200, "the GET shows the page (SEC-026)");
        assert!(waiting(&w, line).await && unused(&w, &path).await);
    }
    assert_eq!(found(&w, "928-555-0150").await, None, "held until approved");
    let (status, text) = approve(&w, &path).await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(found(&w, "928-555-0150").await, Some(w.team.owner.business));
    assert!(!unused(&w, &path).await, "the approval used the link");
    let (again, _) = approve(&w, &path).await;
    assert_eq!(again, 404, "a used link approves no more");
}

/// TD-SEC-66: signed out is sent to sign in, the owner without a fresh step-up to the step-up
/// page, a "Confirm and billing" member gets the not-found answer; each leaves the phone waiting
/// and the link unused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_66_no_session_no_step_up_or_a_member_approves_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let line = linked(&w, w.team.biller, "928-555-0151", 30).await;
    let path = live(&w, line).await;
    let out = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let (status, text) = send(&w, (Method::POST, &path), &out).await;
    assert_eq!(status, 401);
    assert!(text.contains("/b/sign-in"), "sends to sign in: {text}");
    let stale = session(&w, (w.team.owner, w.team.owner_box), false).await;
    let (status, text) = send(&w, (Method::POST, &path), &stale).await;
    assert_eq!(status, 401);
    assert!(
        text.contains("/b/step-up"),
        "sends to the step-up page: {text}"
    );
    let biller = session(&w, (w.team.biller, w.team.biller_box), true).await;
    let (status, _) = send(&w, (Method::POST, &path), &biller).await;
    assert_eq!(status, 404, "only the owner approves (BIZ-033)");
    assert!(waiting(&w, line).await, "nothing was approved");
    assert!(unused(&w, &path).await, "the link stays unused");
    assert_eq!(found(&w, "928-555-0151").await, None);
}

/// TD-SEC-66: a phone already approved, or whose number another business took, is refused,
/// approving nothing and leaving the link unused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_66_a_refused_approval_leaves_the_link_unused(pool: PgPool) {
    let w = world(&pool).await;
    let line = linked(&w, w.team.biller, "928-555-0152", 30).await;
    let (first, second) = (live(&w, line).await, live(&w, line).await);
    assert_eq!(approve(&w, &first).await.0, 200);
    let (status, text) = approve(&w, &second).await;
    assert_eq!(status, 409, "already approved");
    assert!(text.contains("could not approve this phone"), "{text}");
    assert!(unused(&w, &second).await);
    let taken = linked(&w, w.team.biller, "928-555-0153", 30).await;
    let path = live(&w, taken).await;
    let mut db = w.pool.acquire().await.expect("db");
    let other = team(&mut db).await;
    linked(&w, other.owner, "928-555-0153", 20).await;
    let (status, _) = approve(&w, &path).await;
    assert_eq!(status, 409, "its number is taken");
    assert!(waiting(&w, taken).await && unused(&w, &path).await);
    assert_eq!(found(&w, "928-555-0153").await, Some(other.owner.business));
}

/// TD-SEC-66: a link issued before the member's later number change approves nothing and stays
/// unused; a link issued after it approves the new number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_66_only_a_link_issued_after_the_number_change_approves(pool: PgPool) {
    let w = world(&pool).await;
    let line = linked(&w, w.team.biller, "928-555-0154", 30).await;
    let old = mint(&w, line, 20, ago(-120)).await;
    let mut db = w.pool.acquire().await.expect("db");
    let number = SecretString::from("928-555-0155".to_owned());
    let changed = change_phone_at(&mut db, &w.dir, (w.team.biller, line), &number, ago(10));
    changed.await.expect("changed");
    let (status, text) = approve(&w, &old).await;
    assert_eq!(status, 409);
    assert!(text.contains("A newer email"), "{text}");
    assert!(waiting(&w, line).await && unused(&w, &old).await);
    let new = live(&w, line).await;
    assert_eq!(approve(&w, &new).await.0, 200);
    assert_eq!(found(&w, "928-555-0155").await, Some(w.team.owner.business));
}

/// TD-SEC-66: an expired link and a link for another business's phone get the not-found answer
/// and approve nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_66_an_expired_or_another_business_link_is_not_found(pool: PgPool) {
    let w = world(&pool).await;
    let line = linked(&w, w.team.biller, "928-555-0156", 30).await;
    let expired = mint(&w, line, 25, ago(5)).await;
    assert_eq!(approve(&w, &expired).await.0, 404);
    assert!(waiting(&w, line).await);
    let mut db = w.pool.acquire().await.expect("db");
    let other = team(&mut db).await;
    let theirs = linked(&w, other.biller, "928-555-0157", 30).await;
    let path = live(&w, theirs).await;
    assert_eq!(approve(&w, &path).await.0, 404, "another business's phone");
    assert!(unused(&w, &path).await);
    assert_eq!(found(&w, "928-555-0157").await, None);
}

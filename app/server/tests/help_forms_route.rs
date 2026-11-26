//! TD-SEC-51: the help forms through the assembled router (T309, D-2128, D-4293; BIZ-047,
//! OPS-036, PRIV-031, SEC-004, SEC-014, SEC-034, SEC-042, SEC-048).
//!
//! `server::build_router` over a real test database with the portal's vault
//! (`RouterParts::with_vault`) and the Get help answer email's fake key and postal line
//! (`RouterParts::with_help_mail`). A business owner signs in through the sign-in link emailed to
//! a fake mailbox (as `business_portal_route.rs` does); the site help form is used with only the
//! pre-session cookie its own `GET /help` sets. Each request carries a fake socket peer (an IPv6
//! address for the limit cases). Security events are counted from their stored rows (SEC-048).
//! Fake numbers (555-01xx), addresses and IDs only.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{add_owner, create};
use askhava_core::api::{MIGRATOR, SystemClock, brand};
use askhava_messaging::api::{MailboxKind, RevokeChannel, suppress};
use askhava_ops::api::open_help_requests;
use askhava_ops::api::{EXPORT_PAGE_MAX, digest_alerts, export_audit, help_request};
use askhava_portals::api::{
    CookieJar, HELP_FORM_ALL, PortalsError, Realm, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn,
    SignInAddress, SignInMail, admit_help_form, csrf_token, ensure_pre_session, signin_post,
    signin_request, start_session,
};
use askhava_privacy::api::{NewContact, Vault, mailbox_for, store_contact};
use askhava_server::help_forms::HelpMail;
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const GET_HELP: &str = "/b/help";
const HELP: &str = "/help";
const ASK: &str = "&topic=emails&job=1042&message=Fake%3A+no+card+emails+since+Monday.";

/// BIZ-047's "Got it" line's start, with the brand file's name.
fn got_it() -> String {
    let name = brand().display_name();
    format!("The {name} owner has your message and usually replies within 2 days")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let mail = HelpMail::new(
        SecretString::from("fake-help-key-for-tests".to_owned()),
        (
            "PO Box 1234, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
    );
    let parts = RouterParts::from(hooks)
        .with_vault(Some(vault()))
        .with_help_mail(Some(mail));
    build_router(pool.clone(), Router::new(), parts)
}

/// How a request is sent: same-origin with the right token, cross-site by `Sec-Fetch-Site`,
/// cross-site by `Origin` alone, or same-origin with a wrong token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    OtherOrigin,
    WrongToken,
}

fn request(
    (method, path): (&Method, &str),
    jar: &CookieJar,
    (site, extra): (Site, &str),
    peer: IpAddr,
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(jar).unwrap_or_default(),
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-request-id", Uuid::new_v4().to_string())
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    builder = match site {
        Site::OtherOrigin => builder.header(header::ORIGIN, "https://elsewhere.example"),
        Site::Cross => builder.header("sec-fetch-site", "cross-site"),
        Site::Own | Site::WrongToken => builder
            .header("sec-fetch-site", "same-origin")
            .header(header::ORIGIN, format!("https://{HOST}")),
    };
    let mut request = builder
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let peer = SocketAddr::new(peer, 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

fn v4() -> IpAddr {
    IpAddr::from([203, 0, 113, 9])
}

/// A fake IPv6 client: host `host` of the /64 `net`.
fn v6(net: u16, host: u16) -> IpAddr {
    format!("2001:db8:{net:x}::{host:x}").parse().expect("ip")
}

/// An answer as the cases judge it.
#[derive(Debug)]
struct Reply {
    status: u16,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cache = parts.headers.get(header::CACHE_CONTROL);
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        no_store: cache.is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("no-store"))),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        cookies,
    }
}

fn jar_of(cookies: &[String]) -> CookieJar {
    let mut headers = HeaderMap::new();
    let joined = cookies.join("; ").parse().expect("cookies");
    headers.insert(header::COOKIE, joined);
    CookieJar::from_headers(&headers)
}

/// The one sign-in address of one person; the link it "emails" is kept.
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

/// A new business's owner at a fake address, signed in through `portals`' emailed link: the
/// business and the owner's cookies.
async fn owner(pool: &PgPool) -> (Uuid, CookieJar) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), "owner@help.example").await;
    let mailbox = mailbox.expect("mailbox");
    let business = create(&mut db, false, now).await.expect("business");
    add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let to = SignInAddress { business, mailbox };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let asked = (Site::Own, "&email=owner%40help.example");
    answer(
        &app,
        request((&Method::POST, SIGNIN_ROUTE), &jar, asked, v4()),
    )
    .await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = request((&Method::POST, &path), &jar, (Site::Own, ""), v4());
    (business, jar_of(&answer(&app, pressed).await.cookies))
}

/// A visitor's jar from the help page's own `GET /help`, which must be `no-store` and carry the
/// form's token as its hidden `csrf` field.
async fn visitor(app: &Router) -> CookieJar {
    let blank = CookieJar::new();
    let shown = answer(
        app,
        request((&Method::GET, HELP), &blank, (Site::Own, ""), v4()),
    )
    .await;
    assert!(
        shown.status == 200 && shown.no_store,
        "GET /help: {shown:?}"
    );
    let jar = jar_of(&shown.cookies);
    let token = csrf_token(&jar).expect("a pre-session cookie");
    let field = format!("name=\"csrf\" value=\"{token}\"");
    assert!(shown.text.contains(&field), "the form carries the token");
    jar
}

/// A site help form post of `message` (and `mobile`) from `peer`.
async fn site_post(app: &Router, jar: &CookieJar, said: (&str, &str), peer: IpAddr) -> Reply {
    let body = format!("&message={}&email=&mobile={}", said.0, said.1);
    let sent = request((&Method::POST, HELP), jar, (Site::Own, &body), peer);
    answer(app, sent).await
}

async fn open_requests(pool: &PgPool) -> Vec<Uuid> {
    let mut db = pool.acquire().await.expect("db");
    let open = open_help_requests(&mut db).await.expect("requests");
    open.into_iter().map(|(id, _)| id).collect()
}

/// Whether `phone` was not yet on the suppression list (listing it now).
async fn newly_listed(pool: &PgPool, phone: &str) -> bool {
    let mut db = pool.acquire().await.expect("db");
    let how = (RevokeChannel::Owner, "word:stop");
    let listed = suppress(&mut db, &vault(), phone, how, Timestamp::now()).await;
    listed.expect("suppress")
}

/// SEC-048: the stored `csrf_rejected` security events on `route`.
async fn refusals(pool: &PgPool, route: &str) -> i64 {
    let sql =
        "SELECT count(*) FROM portals.security_event WHERE kind = 'csrf_rejected' AND route = $1";
    let count = sqlx::query_scalar(sql).bind(route).fetch_one(pool).await;
    count.expect("count")
}

/// TD-SEC-51 (SEC-004, SEC-034): signed out, an owner-portal session and a visitor's pre-session
/// cookie with its right token each get a `no-store` 401 linking `/b/sign-in` for the form and
/// its post, and nothing opens.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_get_help_needs_a_business_session(pool: PgPool) {
    let app = app(&pool);
    let mut db = pool.acquire().await.expect("db");
    let login = (Realm::Owner, Uuid::new_v4());
    let life = SignedDuration::from_hours(12);
    let owner_portal = start_session(&mut db, CookieJar::new(), login, life, Timestamp::now());
    let owner_portal = owner_portal.await.expect("session");
    drop(db);
    let anonymous = ensure_pre_session(CookieJar::new()).expect("pre-session");
    for jar in [CookieJar::new(), owner_portal, anonymous] {
        let shown = request((&Method::GET, GET_HELP), &jar, (Site::Own, ""), v4());
        let sent = request((&Method::POST, GET_HELP), &jar, (Site::Own, ASK), v4());
        for (reply, how) in [
            (answer(&app, shown).await, "GET"),
            (answer(&app, sent).await, "POST"),
        ] {
            let link = reply.text.contains("href=\"/b/sign-in\"");
            let refused = (reply.status == 401 && link) || (how == "POST" && reply.status == 403);
            assert!(refused && reply.no_store, "{how}: {reply:?}");
            assert!(!reply.text.contains("What is it about?") && !reply.text.contains(&got_it()));
        }
    }
    let post = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let sent = request((&Method::POST, GET_HELP), &post, (Site::Own, ASK), v4());
    let reply = answer(&app, sent).await;
    assert_eq!(
        reply.status, 401,
        "a pre-session post with its token reaches the principal"
    );
    assert!(open_requests(&pool).await.is_empty());
}

/// TD-SEC-51 (BIZ-047, OPS-036, A11Y-030): the signed-in owner gets the form; a post with problems
/// is the form again (422, nothing opened); a checked post opens one help request for that
/// business and answers with the topic's answer and the "Got it" line; every answer `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_get_help_opens_one_request_for_the_business(pool: PgPool) {
    let app = app(&pool);
    let (business, jar) = owner(&pool).await;
    let shown = answer(
        &app,
        request((&Method::GET, GET_HELP), &jar, (Site::Own, ""), v4()),
    )
    .await;
    assert!(shown.status == 200 && shown.no_store, "{shown:?}");
    assert!(shown.text.contains("What is it about?"));
    let bad = request(
        (&Method::POST, GET_HELP),
        &jar,
        (Site::Own, "&job=ten"),
        v4(),
    );
    let bad = answer(&app, bad).await;
    assert!(bad.status == 422 && bad.no_store, "{bad:?}");
    assert!(bad.text.contains("There is a problem") && bad.text.contains("value=\"ten\""));
    assert!(open_requests(&pool).await.is_empty());
    let sent = request((&Method::POST, GET_HELP), &jar, (Site::Own, ASK), v4());
    let sent = answer(&app, sent).await;
    assert!(sent.status == 200 && sent.no_store, "{sent:?}");
    assert!(
        sent.text.contains(&got_it()) && sent.text.contains("spam"),
        "{}",
        sent.text
    );
    let open = open_requests(&pool).await;
    assert_eq!(open.len(), 1, "one help request");
    let mut db = pool.acquire().await.expect("db");
    let opened = help_request(&mut db, open[0]).await.expect("read");
    let opened = opened.expect("the request");
    assert_eq!(
        (opened.asker.as_str(), opened.asker_id),
        ("business", Some(business))
    );
}

/// TD-SEC-51 (OPS-036, PRIV-031, SEC-042, D-4293): with only the pre-session cookie its own
/// `GET /help` set, "please delete my data" opens one help request, deletes, copies and audits
/// nothing, and answers with the `/h/sign-in` link, `no-store`; an empty message opens nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_a_help_form_delete_ask_acts_on_nothing(pool: PgPool) {
    let app = app(&pool);
    let jar = visitor(&app).await;
    let empty = site_post(&app, &jar, ("+", ""), v4()).await;
    assert_eq!(empty.status, 303, "{empty:?}");
    assert!(open_requests(&pool).await.is_empty());
    let sent = site_post(&app, &jar, ("Please+delete+my+data.", ""), v4()).await;
    assert!(sent.status == 200 && sent.no_store, "{sent:?}");
    assert!(sent.text.contains("Message sent") && sent.text.contains("href=\"/h/sign-in\""));
    assert_eq!(open_requests(&pool).await.len(), 1, "one help request");
    let mut db = pool.acquire().await.expect("db");
    let audit = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    assert!(
        audit.is_empty(),
        "nothing deleted, copied or read: {audit:?}"
    );
}

/// TD-SEC-51 (CON-035, D-4293): an opt-out with a number on file, an unknown number and an already
/// listed one get the same screen, and each number ends up listed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_the_opt_out_screen_is_the_same_for_every_number(pool: PgPool) {
    let app = app(&pool);
    let (on_file, unknown, listed) = ("+19285550171", "+19285550172", "+19285550173");
    let mut db = pool.acquire().await.expect("db");
    let contact = NewContact {
        phone: Some(SecretString::from(on_file.to_owned())),
        ..NewContact::default()
    };
    store_contact(&mut db, &vault(), &contact)
        .await
        .expect("contact");
    drop(db);
    assert!(newly_listed(&pool, listed).await);
    let jar = visitor(&app).await;
    let mut screens = Vec::new();
    for phone in [on_file, unknown, listed] {
        let mobile = phone.replace('+', "%2B");
        let reply = site_post(&app, &jar, ("No+more+texts+please.", &mobile), v4()).await;
        assert!(reply.status == 200 && reply.no_store, "{reply:?}");
        screens.push(reply.text);
    }
    assert!(screens[0].contains("stopped texting that number"));
    assert!(
        screens.iter().all(|s| *s == screens[0]),
        "one screen for all three"
    );
    for phone in [on_file, unknown, listed] {
        assert!(!newly_listed(&pool, phone).await, "{phone} is listed");
    }
}

/// The help form's refusal screen: the help email address and "reply STOP".
fn too_many(reply: &Reply) -> bool {
    let address = format!("help@{}", brand().domain());
    let named = reply.text.contains(&address) && reply.text.contains("reply STOP to any text");
    reply.status == 429 && reply.no_store && named && !reply.text.contains("Message sent")
}

/// TD-SEC-51 (SEC-014, SEC-040, D-4293): five posts from one IPv6 /64 in an hour are admitted;
/// the sixth from another address of that /64 is refused before anything opens or its number is
/// listed; another /64 is still admitted.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_the_sixth_post_from_one_64_is_refused(pool: PgPool) {
    let app = app(&pool);
    let jar = visitor(&app).await;
    for host in 1..=5 {
        let reply = site_post(&app, &jar, ("Fake+question.", ""), v6(1, host)).await;
        assert_eq!(reply.status, 200, "post {host}");
    }
    let stop = ("No+more+texts+please.", "%2B19285550174");
    let sixth = site_post(&app, &jar, stop, v6(1, 6)).await;
    assert!(too_many(&sixth), "{sixth:?}");
    assert_eq!(
        open_requests(&pool).await.len(),
        5,
        "the sixth opened nothing"
    );
    assert!(
        newly_listed(&pool, "+19285550174").await,
        "nor listed its number"
    );
    let other = site_post(&app, &jar, ("Fake+question.", ""), v6(2, 1)).await;
    assert_eq!(other.status, 200);
}

/// TD-SEC-51 (SEC-014, SEC-015, D-4293): past 200 posts from everyone in an Arizona day a post is
/// refused before anything opens, and the first refusal raises one High alert, once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_the_201st_post_of_the_day_is_refused(pool: PgPool) {
    let app = app(&pool);
    let now = Timestamp::now();
    for net in 1..=200 {
        admit_help_form(&pool, v6(net, 1), now)
            .await
            .expect("admit");
    }
    let jar = visitor(&app).await;
    for net in 201..=202 {
        let reply = site_post(&app, &jar, ("Fake+question.", ""), v6(net, 1)).await;
        assert!(too_many(&reply), "post {net}: {reply:?}");
    }
    assert!(open_requests(&pool).await.is_empty());
    let mut db = pool.acquire().await.expect("db");
    let day = SignedDuration::from_hours(24);
    let alerts = digest_alerts(&mut db, &[HELP_FORM_ALL.scope], now - day, now + day).await;
    assert_eq!(alerts.expect("alerts").len(), 1, "one High alert");
}

/// TD-SEC-51 (SEC-042, SEC-048): a cross-site `POST` (by `Sec-Fetch-Site` or by `Origin`) or one
/// with a wrong token to either form is a 403, opens nothing and is one `csrf_rejected` event.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_51_cross_site_and_wrong_token_posts_are_refused(pool: PgPool) {
    let app = app(&pool);
    let (_, business) = owner(&pool).await;
    let anonymous = visitor(&app).await;
    let forms = [
        (GET_HELP, &business, ASK),
        (HELP, &anonymous, "&message=Fake+question.&mobile="),
    ];
    for (path, jar, body) in forms {
        for site in [Site::Cross, Site::OtherOrigin, Site::WrongToken] {
            let sent = request((&Method::POST, path), jar, (site, body), v6(9, 1));
            let reply = answer(&app, sent).await;
            assert!(
                reply.status == 403 && reply.no_store,
                "{path} {site:?}: {reply:?}"
            );
        }
        assert_eq!(refusals(&pool, path).await, 3, "{path}");
    }
    assert!(open_requests(&pool).await.is_empty(), "nothing opened");
}

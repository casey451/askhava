//! Scanner-safe email links and the "This inbox is shared" switch (TD-SEC-08; SEC-026, SEC-027,
//! SEC-004, BIZ-038, A11Y-023, OPS-037; D-333, D-478, D-493). A router built like the action route
//! will be (`ACTION_ROUTE` is not served yet, D-1958): `GET` only opens the link, the `POST`
//! carries the CSRF token and uses it. A mail scanner's `GET`, twice, changes no link and no
//! expiry; a `POST` without a token is refused and changes nothing; the button's `POST` acts once.
//! Tokens are 256 bits and stored only hashed; pages send `Referrer-Policy: no-referrer`. With the
//! inbox marked shared, Quote and Hold still work and only Confirm asks for a fresh code (the
//! Confirm page's own cases are TD-SEC-32's, T232). Every ID, address and time is fake.

use std::sync::Arc;

use askhava_businesses::api::{Acting, add_owner, create, record_step_up};
use askhava_core::api::ManualClock;
use askhava_messaging::api::{NewNoticeLog, NoticeKind, Sending, record_notice};
use askhava_picks::api::HoldAsk;
use askhava_portals::api::CookieJar;
use askhava_portals::api::{
    ActionKind, ActionLinkError, ActionTarget, BUSINESS_SESSION_LIFE, BusinessPrincipal,
    ButtonForm, CodeTo, CsrfForm, PRE_SESSION_COOKIE, Realm, SharedInboxError, WebSecurity,
    business_principal, csrf_token, fresh_code_needed, inbox_shared, mint_action_link,
    open_action_link, open_quote_form, refuse_cross_site, security_headers, set_inbox_shared,
    start_session, use_action_link,
};
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRef, Path, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::get;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const ROUTE: &str = "/b/a/{kind}/{object}/{token}";
const COOKIE: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";

fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

fn later(minutes: i64) -> Timestamp {
    start()
        .checked_add(SignedDuration::from_mins(minutes))
        .expect("time")
}

#[derive(Clone)]
struct App {
    security: WebSecurity,
    pool: PgPool,
}

impl FromRef<App> for WebSecurity {
    fn from_ref(app: &App) -> Self {
        app.security.clone()
    }
}

type Link = Path<(String, Uuid, String)>;

fn hold() -> Option<HoldAsk> {
    Some(HoldAsk {
        by: askhava_businesses::api::Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    })
}

async fn show(State(app): State<App>, Path((kind, object, token)): Link) -> StatusCode {
    let kind = ActionKind::from_slug(&kind).expect("kind");
    let mut db = app.pool.acquire().await.expect("db");
    match open_action_link(&mut db, (kind, object), &token, later(1)).await {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

async fn press(
    State(app): State<App>,
    Path((kind, object, token)): Link,
    _form: CsrfForm<ButtonForm>,
) -> StatusCode {
    let kind = ActionKind::from_slug(&kind).expect("kind");
    let mut db = app.pool.acquire().await.expect("db");
    match use_action_link(&mut db, (kind, object), &token, hold(), later(2)).await {
        Ok(_grant) => StatusCode::OK,
        Err(_) => StatusCode::GONE,
    }
}

fn router(pool: PgPool) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::new(ManualClock::new(start())));
    let app = App {
        security: security.clone(),
        pool,
    };
    Router::new()
        .route(ROUTE, get(show).post(press))
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(security_headers))
        .with_state(app)
}

fn request(method: &str, path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "askhava.test")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header("origin", "https://askhava.test")
        .body(Body::from(body.to_owned()))
        .expect("request")
}

async fn send(pool: &PgPool, req: Request<Body>) -> (StatusCode, HeaderMap) {
    let response = router(pool.clone()).oneshot(req).await.expect("response");
    (response.status(), response.headers().clone())
}

/// Every link row of `member`: (expiry, used time), as text.
async fn rows(db: &mut PgConnection, member: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT expires_at::text, used_at::text FROM portals.login_link \
         WHERE member_id = $1 ORDER BY id",
    )
    .bind(member)
    .fetch_all(db)
    .await
    .expect("rows")
}

struct Owner {
    by: Acting,
    me: BusinessPrincipal,
}

/// A business owner signed in to a business session (as `step_up.rs` does).
async fn owner(db: &mut PgConnection) -> Owner {
    let business = create(db, false, start()).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(0xc1), start())
        .await
        .expect("owner");
    let login = (Realm::Business, member);
    let jar = start_session(db, CookieJar::new(), login, BUSINESS_SESSION_LIFE, start()).await;
    let jar = jar.expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let me = business_principal(db, &jar, start()).await.expect("read");
    Owner {
        by: Acting { business, member },
        me: me.expect("signed in"),
    }
}

async fn mint(db: &mut PgConnection, o: &Owner, kind: ActionKind, object: Uuid) -> String {
    let target = ActionTarget {
        kind,
        object,
        member: o.by.member,
        business: o.by.business,
    };
    let link = mint_action_link(db, target, hold(), (start(), later(120)))
        .await
        .expect("minted");
    let path = link
        .expose_secret()
        .strip_prefix("https://askhava.com")
        .expect("ours");
    path.to_owned()
}

/// TD-SEC-08 (SEC-026, SEC-009, SEC-012): a scanner's GET, twice, shows the page and changes no
/// link or expiry; the page sends no referrer; a POST without a CSRF token is refused and changes
/// nothing; the button's POST acts, once; the token is 256 bits and only its hash is stored.
/// TD-SEC-09 (SEC-026, D-493): a cross-site POST to the action page is refused even with a good
/// token, and a second press uses nothing more (the served route's answer to it, the same result
/// as the first, waits on D-1958).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_08_a_scanner_get_changes_nothing_and_only_the_csrf_post_acts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let o = owner(&mut db).await;
    let path = mint(&mut db, &o, ActionKind::Hold, Uuid::from_u128(0x1042)).await;
    let token = path.rsplit('/').next().expect("token").to_owned();
    assert_eq!(token.len(), 64, "at least 128 bits: 256 as hex");
    let before = rows(&mut db, o.by.member).await;
    for _ in 0..2 {
        let (status, headers) = send(&pool, request("GET", &path, "")).await;
        assert_eq!(status, StatusCode::OK);
        let policy = headers.get("referrer-policy").map(HeaderValue::to_str);
        assert_eq!(policy.and_then(Result::ok), Some("no-referrer"));
    }
    assert_eq!(
        rows(&mut db, o.by.member).await,
        before,
        "no use, same expiry"
    );
    let (status, _) = send(&pool, request("POST", &path, "")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no CSRF token");
    assert_eq!(rows(&mut db, o.by.member).await, before);
    let mut cookies = HeaderMap::new();
    let cookie = format!("{PRE_SESSION_COOKIE}={COOKIE}");
    cookies.insert(
        header::COOKIE,
        HeaderValue::from_str(&cookie).expect("cookie"),
    );
    let csrf = csrf_token(&CookieJar::from_headers(&cookies)).expect("csrf");
    let body = format!("csrf={csrf}");
    let mut cross = request("POST", &path, &body);
    let site = HeaderValue::from_static("cross-site");
    cross.headers_mut().insert("sec-fetch-site", site);
    let (status, _) = send(&pool, cross).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a cross-site POST");
    assert_eq!(rows(&mut db, o.by.member).await, before);
    let (status, _) = send(&pool, request("POST", &path, &body)).await;
    assert_eq!(status, StatusCode::OK, "the button acts");
    let used = rows(&mut db, o.by.member).await;
    assert!(used.iter().all(|(_, at)| at.is_some()), "used once");
    send(&pool, request("POST", &path, &body)).await;
    assert_eq!(
        rows(&mut db, o.by.member).await,
        used,
        "a second press acts no more"
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portals.login_link WHERE member_id = $1 \
         AND length(token_sha256) = 32 AND encode(token_sha256, 'hex') <> $2",
    )
    .bind(o.by.member)
    .bind(&token)
    .fetch_one(&mut *db)
    .await
    .expect("count");
    assert_eq!(stored, 1, "the hash only, never the token");
}

/// TD-SEC-08 (SEC-026, OPS-037, D-347): the quote form's GET only reads its link and stamps the
/// notice's first opening once; a scanner's second GET changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_08_opening_the_quote_form_stamps_the_first_open_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let o = owner(&mut db).await;
    let job = Uuid::from_u128(0x2042);
    let path = mint(&mut db, &o, ActionKind::Quote, job).await;
    let token = path.rsplit('/').next().expect("token").to_owned();
    let notice = NewNoticeLog {
        business: o.by.business,
        job,
        kind: NoticeKind::Card,
        mailbox: Uuid::from_u128(0x3042),
        email: None,
    };
    let id = record_notice(&mut db, &notice, Sending::Sent(start()))
        .await
        .expect("notice");
    let before = rows(&mut db, o.by.member).await;
    for minute in [5, 9] {
        open_quote_form(&mut db, (job, &token), Some(id), later(minute))
            .await
            .expect("opened");
    }
    let first: Option<String> = sqlx::query_scalar(
        "SELECT (first_opened_at = $2::text::timestamptz)::text FROM messaging.notice_log WHERE id = $1",
    )
    .bind(id)
    .bind(later(5).to_string())
    .fetch_one(&mut *db)
    .await
    .expect("first open");
    assert_eq!(first.as_deref(), Some("true"), "the first opening, kept");
    assert_eq!(
        rows(&mut db, o.by.member).await,
        before,
        "the link is unused"
    );
    let ended = open_quote_form(
        &mut db,
        (Uuid::from_u128(0x2043), &token),
        Some(id),
        later(9),
    );
    assert_eq!(ended.await, Err(ActionLinkError::Ended), "another job");
}

/// TD-SEC-08 (SEC-026, OPS-037, D-2412): the quote form stamps only the link's own business's
/// notice for the link's job; a notice of another job opens the form and stays unstamped.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_08_a_quote_link_never_stamps_another_jobs_notice(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let o = owner(&mut db).await;
    let job = Uuid::from_u128(0x2042);
    let path = mint(&mut db, &o, ActionKind::Quote, job).await;
    let token = path.rsplit('/').next().expect("token").to_owned();
    let other = NewNoticeLog {
        business: o.by.business,
        job: Uuid::from_u128(0x2044),
        kind: NoticeKind::Card,
        mailbox: Uuid::from_u128(0x3042),
        email: None,
    };
    let other = record_notice(&mut db, &other, Sending::Sent(start()))
        .await
        .expect("another job's notice");
    open_quote_form(&mut db, (job, &token), Some(other), later(6))
        .await
        .expect("opened");
    let stamped: Option<String> =
        sqlx::query_scalar("SELECT first_opened_at::text FROM messaging.notice_log WHERE id = $1")
            .bind(other)
            .fetch_one(&mut *db)
            .await
            .expect("other notice");
    assert_eq!(stamped, None, "never another job's notice");
}

/// TD-SEC-08 (SEC-027, SEC-038, D-333, D-478): marking the inbox shared needs a phone that gets
/// texts and a fresh code; while shared, Quote and Hold links still act by the link alone and
/// only Confirm needs a fresh code, even signed in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_08_with_the_inbox_shared_quote_and_hold_still_work(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let o = owner(&mut db).await;
    let phone = CodeTo::Phone(SecretString::from("+19285550143".to_owned()));
    let email = CodeTo::Email(SecretString::from("owner@example.com".to_owned()));
    let refused = set_inbox_shared(&mut db, &o.me, (true, &email), start()).await;
    assert_eq!(refused, Err(SharedInboxError::NeedsTextPhone));
    let refused = set_inbox_shared(&mut db, &o.me, (true, &phone), start()).await;
    assert_eq!(refused, Err(SharedInboxError::NeedsCode));
    assert!(!inbox_shared(&mut db, o.by.member).await.expect("read"));
    record_step_up(&mut db, o.by, start()).await.expect("code");
    set_inbox_shared(&mut db, &o.me, (true, &phone), later(1))
        .await
        .expect("shared");
    assert!(inbox_shared(&mut db, o.by.member).await.expect("read"));
    for kind in [ActionKind::Quote, ActionKind::Hold] {
        let object = Uuid::from_u128(0x4042);
        let path = mint(&mut db, &o, kind, object).await;
        let token = path.rsplit('/').next().expect("token").to_owned();
        let used = use_action_link(&mut db, (kind, object), &token, hold(), later(2)).await;
        assert!(used.is_ok(), "{kind:?} still works");
        assert!(!fresh_code_needed(kind, false, true));
    }
    assert!(
        fresh_code_needed(ActionKind::Confirm, true, true),
        "even signed in"
    );
    assert!(!fresh_code_needed(ActionKind::Confirm, true, false));
    assert!(fresh_code_needed(ActionKind::Confirm, false, false));
    set_inbox_shared(&mut db, &o.me, (false, &email), later(3))
        .await
        .expect("turned off with a fresh code");
    assert!(!inbox_shared(&mut db, o.by.member).await.expect("read"));
}

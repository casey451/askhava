//! TD-SEC-71 (T542, D-3890, D-3892, D-5081; SEC-038, SEC-014, BIZ-040, BIZ-043): the notice and
//! backup address changes served over PostgreSQL. A change or a backup removal without a step-up
//! goes to the step-up page and changes nothing; with one from the last 5 minutes the code is
//! emailed to the new address only, the old notice address keeps every notice until the code is
//! entered and is then told by a queued email (a backup's change tells the main address); a wrong
//! or expired code confirms nothing; a posted mailbox ID is refused (the mailbox is minted on the
//! server); another business's member is the not-found page with an `authz_denied` event; a
//! cross-site or wrong-token `POST` is a 403 and one `csrf_rejected` event; another member's
//! address is refused beside the field with nothing emailed or kept open. People sign in through
//! `portals`' emailed-link sign-in, as `business_portal_route.rs` does. Every value is fake.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    Acting, BACKUP_CHANGED_KIND, CHANGED_KIND, NoticePurpose, Roles, add_member, add_owner, create,
    notice_addresses, record_step_up,
};
use askhava_core::api::{MIGRATOR, SystemClock, WorkerId, claim};
use askhava_messaging::api::{DailyLimit, MailboxCodeSender, MailboxKind};
use askhava_ops::api::{EXPORT_PAGE_MAX, export_audit};
use askhava_portals::api::{
    AddressKind, CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress,
    SignInMail, business_principal, confirm_address_change, csrf_token, ensure_pre_session,
    signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::address_changes::{AddressParts, routes};
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
const SECRET: &str = "fake-id-secret-for-address-tests-0123456789abcdef";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

/// Every email the fake provider was handed: (to, text).
#[derive(Clone, Default)]
struct FakeMail(Arc<Mutex<Vec<(String, String)>>>);

impl FakeMail {
    fn sent(&self) -> Vec<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The 6-digit code in the last email to `to`.
    fn code_to(&self, to: &str) -> String {
        let (_, text) = (self.sent().into_iter().rev())
            .find(|(t, _)| t == to)
            .expect("a code emailed");
        let mut runs = text.split(|c: char| !c.is_ascii_digit());
        runs.find(|r| r.len() == 6).expect("a code").to_owned()
    }
}

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((e.to.to_owned(), e.text.to_owned()));
        Ok(format!("re_fake_{}", sent.len()))
    }

    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn app(pool: &PgPool, mail: &FakeMail) -> Router {
    let postal = ("PO Box 1234, Faketown, AZ 00000", MailboxKind::PoBox);
    let limit = DailyLimit::new(100, 90).expect("limit");
    let secret = || SecretString::from(SECRET.to_owned());
    let sender = MailboxCodeSender::new(secret(), mail.clone(), postal, limit).expect("mailer");
    routes(
        pool.clone(),
        Some(AddressParts::new(vault(), secret(), Some(sender))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    OtherOrigin,
    WrongToken,
}

fn request(path: &str, jar: &CookieJar, (site, body): (Site, &str)) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(jar).unwrap_or_default(),
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let builder = match site {
        Site::OtherOrigin => builder.header(header::ORIGIN, "https://elsewhere.example"),
        Site::Cross => builder.header("sec-fetch-site", "cross-site"),
        Site::Own | Site::WrongToken => builder
            .header("sec-fetch-site", "same-origin")
            .header(header::ORIGIN, format!("https://{HOST}")),
    };
    let mut request = builder
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// An answer as the cases judge it: status, `Location`, `no-store`, body.
#[derive(Debug)]
struct Reply {
    status: u16,
    location: String,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let header = |name| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    let (location, cache) = (
        header(header::LOCATION).to_owned(),
        header(header::CACHE_CONTROL),
    );
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        location,
        no_store: cache.contains("no-store"),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        cookies,
    }
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

/// `to`'s person signed in through `portals`' emailed link (D-1724).
async fn business_jar(pool: &PgPool, to: SignInAddress) -> CookieJar {
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
    answer(
        &app,
        request(SIGNIN_ROUTE, &jar, (Site::Own, "&email=person%40x.example")),
    )
    .await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = answer(&app, request(&path, &jar, (Site::Own, ""))).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        pressed.cookies.join("; ").parse().expect("cookies"),
    );
    CookieJar::from_headers(&headers)
}

/// A person of a business: acting IDs, sign-in mailbox and signed-in cookies.
struct Person {
    by: Acting,
    mailbox: Uuid,
    jar: CookieJar,
}

/// A new business's owner at `address`, or with `owner` a member of that owner's business.
async fn person(pool: &PgPool, owner: Option<Acting>, address: &str) -> Person {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let by = match owner {
        Some(o) => {
            let roles = Roles::new(true, false).expect("roles");
            let member = add_member(&mut db, o, mailbox, roles, now)
                .await
                .expect("member");
            Acting {
                business: o.business,
                member,
            }
        }
        None => {
            let business = create(&mut db, false, now).await.expect("business");
            let member = add_owner(&mut db, business, mailbox, now)
                .await
                .expect("owner");
            Acting { business, member }
        }
    };
    drop(db);
    let jar = business_jar(
        pool,
        SignInAddress {
            business: by.business,
            mailbox,
        },
    )
    .await;
    Person { by, mailbox, jar }
}

/// SEC-038: `by` passed a Verify code `ago` before now.
async fn stepped_up(pool: &PgPool, by: Acting, ago: SignedDuration) {
    let mut db = pool.acquire().await.expect("db");
    let at = Timestamp::now().checked_sub(ago).expect("time");
    record_step_up(&mut db, by, at).await.expect("step-up");
}

/// The form body for `kind`, `member` and `address`.
fn form(kind: &str, member: Uuid, address: &str) -> String {
    format!(
        "&member={member}&kind={kind}&address={}",
        address.replace('@', "%40")
    )
}

async fn post_own(app: &Router, path: &str, p: &Person, body: &str) -> Reply {
    answer(app, request(path, &p.jar, (Site::Own, body))).await
}

/// The job-card notice mailboxes of `business` (backups included, BIZ-043).
async fn card_notices(pool: &PgPool, business: Uuid) -> Vec<Uuid> {
    let mut db = pool.acquire().await.expect("db");
    notice_addresses(&mut db, business, NoticePurpose::Cards)
        .await
        .expect("notices")
}

async fn minted(pool: &PgPool, address: &str) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox")
}

/// Audit actions recorded so far.
async fn actions(pool: &PgPool) -> Vec<String> {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    rows.into_iter().map(|row| row.action).collect()
}

/// The payloads of queued tasks of `kind`.
/// The payloads of tasks of `kind` queued since the last call (each call claims what is due).
async fn queued(pool: &PgPool, kind: &str) -> Vec<serde_json::Value> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let claimed = claim(pool, owner, later, SignedDuration::from_secs(60), 1000);
    let claimed = claimed.await.expect("claim").into_iter();
    claimed
        .filter(|t| t.kind == kind)
        .map(|t| t.payload)
        .collect()
}

/// The security events every test thread logs, by thread and kind (SEC-048). The binary's one
/// global subscriber, not a thread's default: a callsite another test's thread registers first
/// would otherwise stay cached as "never" for this thread (as in `home_request_box.rs`).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(ThreadId, String)>>>);

impl Events {
    /// The global subscriber, installed on first use.
    fn captured() -> Self {
        static EVENTS: OnceLock<Events> = OnceLock::new();
        EVENTS
            .get_or_init(|| {
                let events = Events::default();
                tracing::subscriber::set_global_default(events.clone())
                    .expect("one global subscriber");
                events
            })
            .clone()
    }

    /// How many `kind` events this test's thread has logged; each `sqlx::test` runs on its own
    /// thread, so a test running alongside never adds to it.
    fn count(&self, kind: &str) -> usize {
        let me = std::thread::current().id();
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().filter(|(t, k)| *t == me && k == kind).count()
    }
}

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
            fields
                .0
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        if get("message").as_deref() == Some("security event") {
            let kind = get("kind").unwrap_or_default();
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((std::thread::current().id(), kind));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// TD-SEC-71 (SEC-038): with no step-up, or one over 5 minutes old, a notice or backup change and
/// a backup removal each go to the step-up page; nothing is emailed, requested or removed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_no_fresh_step_up_goes_to_the_step_up_page(pool: PgPool) {
    let (mail, x) = (
        FakeMail::default(),
        person(&pool, None, "owner@x.example").await,
    );
    let app = app(&pool, &mail);
    let member = x.by.member;
    let before = (
        actions(&pool).await,
        card_notices(&pool, x.by.business).await,
    );
    for ago in [None, Some(SignedDuration::from_secs(301))] {
        if let Some(ago) = ago {
            stepped_up(&pool, x.by, ago).await;
        }
        for (path, body) in [
            (
                "/b/addresses/request",
                form("notice", member, "new@x.example"),
            ),
            (
                "/b/addresses/request",
                form("backup", member, "spare@x.example"),
            ),
            ("/b/addresses/remove-backup", format!("&member={member}")),
        ] {
            let r = post_own(&app, path, &x, &body).await;
            assert_eq!(
                (r.status, r.location.as_str()),
                (303, "/b/step-up?return=%2Fb%2Faddresses")
            );
            assert!(r.no_store, "{path}");
        }
    }
    assert!(mail.sent().is_empty(), "nothing emailed");
    let refused = actions(&pool).await;
    assert!(
        !refused
            .iter()
            .any(|a| a.ends_with("_requested") || a.ends_with("_removed"))
    );
    assert_eq!(refused.len(), before.0.len(), "no change logged");
    assert_eq!(card_notices(&pool, x.by.business).await, before.1);
}

/// TD-SEC-71 (SEC-038, BIZ-040): within 5 minutes of a step-up the code goes to the new address
/// only; the old address keeps every notice through a wrong and an expired code; the right code
/// moves the notices and queues the email telling the old address. A posted mailbox is refused.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_a_notice_change_mails_the_new_address_and_tells_the_old(pool: PgPool) {
    let (mail, x) = (
        FakeMail::default(),
        person(&pool, None, "owner@x.example").await,
    );
    let app = app(&pool, &mail);
    stepped_up(&pool, x.by, SignedDuration::from_mins(4)).await;
    let new = minted(&pool, "new@x.example").await;
    let posted = format!(
        "{}&mailbox={new}",
        form("notice", x.by.member, "new@x.example")
    );
    let r = post_own(&app, "/b/addresses/request", &x, &posted).await;
    assert_eq!(r.status, 422, "a posted mailbox is refused");
    let body = form("notice", x.by.member, "new@x.example");
    let r = post_own(&app, "/b/addresses/request", &x, &body).await;
    assert!(
        r.status == 200 && r.text.contains("We emailed a 6-digit code"),
        "{}",
        r.status
    );
    let to: Vec<String> = mail.sent().into_iter().map(|(t, _)| t).collect();
    assert_eq!(to, ["new@x.example"], "the new address only");
    let code = mail.code_to("new@x.example");
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let r = post_own(
        &app,
        "/b/addresses/confirm",
        &x,
        &format!("{body}&code={wrong}"),
    )
    .await;
    assert!(r.status == 200 && r.text.contains("That code didn't work"));
    let later = Timestamp::now()
        .checked_add(SignedDuration::from_mins(31))
        .expect("time");
    let mut db = pool.acquire().await.expect("db");
    let who = business_principal(&mut db, &x.jar, Timestamp::now())
        .await
        .expect("who")
        .expect("in");
    let keys = (&vault(), &SecretString::from(SECRET.to_owned()));
    let address = SecretString::from("new@x.example".to_owned());
    let change = (AddressKind::Notice, x.by.member, &address);
    let late = confirm_address_change(&mut db, keys, &who, change, (&code, later)).await;
    assert_eq!(late, Ok(false), "an expired code confirms nothing");
    drop(db);
    assert_eq!(
        card_notices(&pool, x.by.business).await,
        [x.mailbox],
        "the old one keeps notices"
    );
    assert!(queued(&pool, CHANGED_KIND).await.is_empty());
    let r = post_own(
        &app,
        "/b/addresses/confirm",
        &x,
        &format!("{body}&code={code}"),
    )
    .await;
    assert!(
        r.status == 200 && r.text.contains("Done. Your notices now go"),
        "{}",
        r.status
    );
    assert_eq!(card_notices(&pool, x.by.business).await, [new]);
    let told = queued(&pool, CHANGED_KIND).await;
    assert_eq!(told.len(), 1, "the old address is told");
    assert_eq!(told[0]["old"], serde_json::json!(x.mailbox));
}

/// TD-SEC-71 (BIZ-043, SEC-038): a backup gets its code alone and, once confirmed, every card
/// notice, the main address told; removing it needs the same step-up and tells the main address.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_a_backup_change_and_its_removal(pool: PgPool) {
    let (mail, x) = (
        FakeMail::default(),
        person(&pool, None, "owner@x.example").await,
    );
    let app = app(&pool, &mail);
    stepped_up(&pool, x.by, SignedDuration::from_mins(1)).await;
    let body = form("backup", x.by.member, "spare@x.example");
    let r = post_own(&app, "/b/addresses/request", &x, &body).await;
    assert_eq!(r.status, 200);
    let to: Vec<String> = mail.sent().into_iter().map(|(t, _)| t).collect();
    assert_eq!(to, ["spare@x.example"], "the backup address only");
    assert_eq!(
        card_notices(&pool, x.by.business).await,
        [x.mailbox],
        "not before its code"
    );
    let code = mail.code_to("spare@x.example");
    let r = post_own(
        &app,
        "/b/addresses/confirm",
        &x,
        &format!("{body}&code={code}"),
    )
    .await;
    assert!(r.status == 200 && r.text.contains("Your backup address now gets"));
    let spare = minted(&pool, "spare@x.example").await;
    assert_eq!(card_notices(&pool, x.by.business).await, [x.mailbox, spare]);
    assert_eq!(
        queued(&pool, BACKUP_CHANGED_KIND).await.len(),
        1,
        "the main address is told"
    );
    stepped_up(&pool, x.by, SignedDuration::from_mins(6)).await;
    let remove = format!("&member={}", x.by.member);
    let r = post_own(&app, "/b/addresses/remove-backup", &x, &remove).await;
    assert_eq!(r.status, 303, "a removal needs a fresh step-up");
    assert_eq!(card_notices(&pool, x.by.business).await, [x.mailbox, spare]);
    stepped_up(&pool, x.by, SignedDuration::from_mins(2)).await;
    let r = post_own(&app, "/b/addresses/remove-backup", &x, &remove).await;
    assert!(r.status == 200 && r.text.contains("removed your backup address"));
    assert_eq!(card_notices(&pool, x.by.business).await, [x.mailbox]);
    assert_eq!(queued(&pool, BACKUP_CHANGED_KIND).await.len(), 1);
    let r = post_own(&app, "/b/addresses/remove-backup", &x, &remove).await;
    assert!(
        r.status == 200 && r.text.contains("You don&#39;t have a backup address now."),
        "no backup left: nothing said removed or emailed"
    );
}

/// TD-SEC-71 (SEC-019, SEC-048): a member of another business, or another member for one who is
/// not the owner, is the one not-found page and an `authz_denied` event; nothing is emailed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_another_business_member_is_not_found(pool: PgPool) {
    let mail = FakeMail::default();
    let x = person(&pool, None, "owner@x.example").await;
    let y = person(&pool, None, "owner@y.example").await;
    let helper = person(&pool, Some(x.by), "helper@x.example").await;
    let app = app(&pool, &mail);
    stepped_up(&pool, x.by, SignedDuration::from_mins(1)).await;
    stepped_up(&pool, helper.by, SignedDuration::from_mins(1)).await;
    let events = Events::captured();
    let before = events.count("authz_denied");
    let cases = [
        (
            &x,
            "/b/addresses/request",
            form("notice", y.by.member, "new@y.example"),
        ),
        (
            &x,
            "/b/addresses/request",
            form("backup", y.by.member, "new@y.example"),
        ),
        (
            &x,
            "/b/addresses/remove-backup",
            format!("&member={}", y.by.member),
        ),
        (
            &helper,
            "/b/addresses/request",
            form("notice", x.by.member, "new@x.example"),
        ),
    ];
    for (n, (who, path, body)) in cases.iter().enumerate() {
        let r = post_own(&app, path, who, body).await;
        assert!(
            r.status == 404 && r.text.contains("We can't find that page"),
            "{path}"
        );
        assert_eq!(events.count("authz_denied"), before + n + 1, "{path}");
    }
    assert!(mail.sent().is_empty(), "nothing emailed");
    assert_eq!(card_notices(&pool, y.by.business).await, [y.mailbox]);
}

/// TD-SEC-71 (SEC-042, SEC-048): a cross-site (`Sec-Fetch-Site` or `Origin`) or wrong-token
/// `POST` to each form is a 403, one `csrf_rejected` event, and changes and emails nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_cross_site_and_wrong_token_posts_are_refused_and_logged(pool: PgPool) {
    let (mail, x) = (
        FakeMail::default(),
        person(&pool, None, "owner@x.example").await,
    );
    let app = app(&pool, &mail);
    stepped_up(&pool, x.by, SignedDuration::from_mins(1)).await;
    let body = form("notice", x.by.member, "new@x.example");
    let forms = [
        ("/b/addresses/request", body.clone()),
        ("/b/addresses/confirm", format!("{body}&code=123456")),
        (
            "/b/addresses/remove-backup",
            format!("&member={}", x.by.member),
        ),
    ];
    let audit = actions(&pool).await;
    let events = Events::captured();
    let before = events.count("csrf_rejected");
    let mut n = 0;
    for site in [Site::Cross, Site::OtherOrigin, Site::WrongToken] {
        for (path, body) in &forms {
            let r = answer(&app, request(path, &x.jar, (site, body))).await;
            n += 1;
            assert_eq!(r.status, 403, "{site:?} {path}");
            assert_eq!(events.count("csrf_rejected"), before + n, "{site:?} {path}");
        }
    }
    assert!(mail.sent().is_empty(), "nothing emailed");
    assert_eq!(actions(&pool).await, audit, "nothing changed");
}

/// TD-SEC-71 (BIZ-040, A11Y-030, D-5081): another member's sign-in address is refused beside the
/// field, nothing emailed, no request logged and no check left open; an address a member takes
/// after the request is refused at its code and the check closes.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_71_another_members_address_is_refused_with_the_message(pool: PgPool) {
    let (mail, x) = (
        FakeMail::default(),
        person(&pool, None, "owner@x.example").await,
    );
    person(&pool, Some(x.by), "helper@x.example").await;
    let app = app(&pool, &mail);
    stepped_up(&pool, x.by, SignedDuration::from_mins(1)).await;
    let taken = "Someone on your team already signs in with this address.";
    for kind in ["notice", "backup"] {
        let body = form(kind, x.by.member, "helper@x.example");
        let r = post_own(&app, "/b/addresses/request", &x, &body).await;
        assert!(r.status == 200 && r.text.contains(taken), "{kind}");
        assert!(r.text.contains("<title>Error: ") && r.text.contains("value=\"helper@x.example\""));
    }
    assert!(mail.sent().is_empty(), "nothing emailed");
    let logged = actions(&pool).await;
    assert!(
        !logged
            .iter()
            .any(|a| a == "businesses.notice_change_requested")
    );
    assert!(
        !logged
            .iter()
            .any(|a| a == "businesses.backup_change_requested")
    );
    let body = form("notice", x.by.member, "late@x.example");
    assert_eq!(
        post_own(&app, "/b/addresses/request", &x, &body)
            .await
            .status,
        200
    );
    let code = mail.code_to("late@x.example");
    person(&pool, Some(x.by), "late@x.example").await;
    let r = post_own(
        &app,
        "/b/addresses/confirm",
        &x,
        &format!("{body}&code={code}"),
    )
    .await;
    assert!(r.status == 200 && r.text.contains(taken), "{}", r.status);
    let again = post_own(
        &app,
        "/b/addresses/confirm",
        &x,
        &format!("{body}&code={code}"),
    )
    .await;
    assert!(
        again.text.contains("That code didn't work"),
        "the clash closed the check"
    );
    assert_eq!(
        card_notices(&pool, x.by.business).await.first(),
        Some(&x.mailbox)
    );
}

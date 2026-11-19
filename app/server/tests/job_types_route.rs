//! TD-BIZ-51 (T375, D-2730; BIZ-004, BIZ-049, BIZ-050, SEC-004): the business's Job types page
//! served by the assembled router over PostgreSQL. Signed out, the page answers a 4xx linking to
//! `/b/sign-in`; a "Cards and quotes" member gets the one not-found answer, and its `POST` stores
//! nothing; the owner sees the offer from its license (licensed rows ticked as stored) and saves a
//! tick; a tick outside the offer changes nothing; a business paused for `no_covered_job_type` that
//! saves a covered tick is active again. The axe-core case is the page's `PAGE_STATES` entry in
//! `tests/a11y/page_states.ts`. People sign in through `portals`' emailed-link sign-in, as
//! `business_portal_route.rs` does. Every number, address and ID is fake.

use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    Acting, Capability, CapabilityAnswers, Checks, Event, JobTypeChoice, LargestJob, Roles,
    add_member, add_owner, apply, apply_lists, business, checklist_offer, create, job_types,
    pre_ticked, save_capability,
};
use askhava_catalog::api::{Catalog, load_embedded, parse};
use askhava_core::api::{MIGRATOR, SystemClock};
use askhava_licensing::api::{ClassMap, check_license, import_roc_list};
use askhava_ops::api::Actor;
use askhava_portals::api::{
    CookieJar, JOB_TYPES_ROUTE, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn,
    SignInAddress, SignInMail, csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::Date;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
/// The owner's fake license number.
const NUMBER: &str = "ROC900100";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

/// A same-site request carrying `jar`'s cookies and its CSRF token before `extra`.
fn request(method: &Method, path: &str, jar: &CookieJar, extra: &str) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request")
}

/// The status, the `Set-Cookie` pairs and the body text.
async fn answer(app: &Router, request: Request<Body>) -> (u16, Vec<String>, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (parts.status.as_u16(), cookies, text)
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

/// `to`'s person signed in through `portals`' emailed link.
async fn signed_in(pool: &PgPool, to: SignInAddress) -> CookieJar {
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
    let asked = request(
        &Method::POST,
        SIGNIN_ROUTE,
        &jar,
        "&email=person%40x.example",
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let (_, cookies, _) = answer(&app, request(&Method::POST, &path, &jar, "")).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// A license list holding `classes` for [`NUMBER`] on day `d`, plus filler numbers (D-905).
async fn record(db: &mut PgConnection, d: i8, classes: &[&str]) {
    let mut text = "license_number,class,status,issued_on\n".to_owned();
    for n in 0..10 {
        text.push_str(&format!("ROC9990{n:02},R-11,Active,2015-01-01\n"));
    }
    for class in classes {
        text.push_str(&format!("{NUMBER},{class},Active,2015-01-01\n"));
    }
    let day = Date::new(2026, 11, d).expect("fake date");
    import_roc_list(db, day, &text).await.expect("list");
}

/// The pre-ticked job types `classes` give in `category`.
fn covered(classes: &[&str], category: &str) -> Vec<JobTypeChoice> {
    let offer = checklist_offer(map(), &catalog(), classes);
    let ticks = pre_ticked(&offer).into_iter();
    ticks.filter(|c| c.category == category).collect()
}

/// An active business holding R-11 and R-37 with `ticked` stored, and its owner signed in.
async fn owner(pool: &PgPool, ticked: Vec<JobTypeChoice>) -> (Acting, CookieJar) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    load_embedded(&mut db).await.expect("catalog stored");
    record(&mut db, 1, &["R-11", "R-37"]).await;
    let mailbox = mailbox_for(&mut db, &vault(), "owner@x.example")
        .await
        .expect("mailbox");
    let id = create(&mut db, false, now).await.expect("business");
    let member = add_owner(&mut db, id, mailbox, now).await.expect("owner");
    check_license(&mut db, id, NUMBER).await.expect("checked");
    let offer = checklist_offer(map(), &catalog(), &["R-11", "R-37"]);
    let answers = CapabilityAnswers {
        ticked,
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    let by = (Actor::System, now);
    save_capability(&mut db, id, &offer, &answers, by)
        .await
        .expect("saved");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(&mut db, id, Event::Checked(ok), Actor::System, now)
        .await
        .expect("active");
    drop(db);
    let jar = signed_in(
        pool,
        SignInAddress {
            business: id,
            mailbox,
        },
    )
    .await;
    (
        Acting {
            business: id,
            member,
        },
        jar,
    )
}

fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let parts = RouterParts::from(hooks).with_vault(Some(vault()));
    build_router(pool.clone(), Router::new(), parts)
}

/// The form fields ticking `ticks`, each value percent-encoded.
fn ticks(ticks: &[JobTypeChoice]) -> String {
    let encode = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    let value = |c: &JobTypeChoice| encode(&format!("{}:{}", c.category, c.job_type));
    ticks
        .iter()
        .map(|c| format!("&job_type={}", value(c)))
        .collect()
}

async fn status(pool: &PgPool, id: Uuid) -> String {
    let mut db = pool.acquire().await.expect("db");
    business(&mut db, id)
        .await
        .expect("read")
        .status
        .code()
        .to_owned()
}

/// `v` in the stored order (by category, then job type).
fn sorted(mut v: Vec<JobTypeChoice>) -> Vec<JobTypeChoice> {
    v.sort();
    v
}

async fn stored(pool: &PgPool, id: Uuid) -> Vec<JobTypeChoice> {
    let mut db = pool.acquire().await.expect("db");
    job_types(&mut db, id).await.expect("read")
}

/// TD-BIZ-51 (SEC-004, SEC-019): signed out the page sends to sign-in; a "Cards and quotes"
/// member gets the not-found page and its post stores nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_51_only_the_owner_or_billing_reaches_the_page(pool: PgPool) {
    let (by, _) = owner(&pool, covered(&["R-37"], "plumbing_repair")).await;
    let app = app(&pool);
    let out = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let (code, _, text) = answer(&app, request(&Method::GET, JOB_TYPES_ROUTE, &out, "")).await;
    assert!(
        (400..500).contains(&code) && text.contains("/b/sign-in"),
        "{code}"
    );
    let mut db = pool.acquire().await.expect("db");
    let mailbox = mailbox_for(&mut db, &vault(), "cards@x.example")
        .await
        .expect("mailbox");
    let cards = Roles::new(true, false).expect("roles");
    add_member(&mut db, by, mailbox, cards, Timestamp::now())
        .await
        .expect("member");
    drop(db);
    let to = SignInAddress {
        business: by.business,
        mailbox,
    };
    let jar = signed_in(&pool, to).await;
    let (code, _, text) = answer(&app, request(&Method::GET, JOB_TYPES_ROUTE, &jar, "")).await;
    assert!(
        code == 404 && text.contains("We can't find that page"),
        "{code}"
    );
    let all = ticks(&covered(&["R-11"], "electrical"));
    let (code, _, _) = answer(&app, request(&Method::POST, JOB_TYPES_ROUTE, &jar, &all)).await;
    assert_eq!(code, 404);
    assert_eq!(
        stored(&pool, by.business).await,
        sorted(covered(&["R-37"], "plumbing_repair"))
    );
}

/// TD-BIZ-51 (BIZ-004, BIZ-049, LIC-043): the owner sees the offer with the stored ticks and saves
/// a tick; a tick outside the offer changes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_51_the_owner_saves_a_tick_and_one_outside_changes_nothing(pool: PgPool) {
    let plumbing = covered(&["R-37"], "plumbing_repair");
    let (by, jar) = owner(&pool, plumbing.clone()).await;
    let app = app(&pool);
    let (code, _, page) = answer(&app, request(&Method::GET, JOB_TYPES_ROUTE, &jar, "")).await;
    assert_eq!(code, 200);
    let first = &plumbing[0].job_type;
    assert!(page.contains("Which jobs do you want?") && page.contains(first.as_str()));
    assert!(page.contains(" checked>"), "the stored ticks are checked");
    let mut both = plumbing.clone();
    both.push(covered(&["R-11"], "electrical")[0].clone());
    let form = ticks(&both);
    let (code, _, page) = answer(&app, request(&Method::POST, JOB_TYPES_ROUTE, &jar, &form)).await;
    assert!(code == 200 && page.contains("Saved."), "{code}");
    let want = sorted(both);
    assert_eq!(stored(&pool, by.business).await, want);
    let outside = ticks(&[JobTypeChoice::new("plumbing_repair", "Not a job type")]);
    let (code, _, page) = answer(
        &app,
        request(&Method::POST, JOB_TYPES_ROUTE, &jar, &outside),
    )
    .await;
    assert!(code == 200 && page.contains("Nothing was saved."), "{code}");
    assert_eq!(stored(&pool, by.business).await, want);
}

/// TD-BIZ-51 (BIZ-050, D-2730): a business paused for `no_covered_job_type` stays paused on an
/// uncovered tick and is active again once it saves a covered one.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_51_a_covered_tick_resumes_a_no_cover_pause(pool: PgPool) {
    let plumbing = covered(&["R-37"], "plumbing_repair");
    let (by, jar) = owner(&pool, plumbing.clone()).await;
    let mut db = pool.acquire().await.expect("db");
    record(&mut db, 2, &["R-11"]).await;
    apply_lists(&mut db, (map(), &catalog()), Timestamp::now())
        .await
        .expect("applied");
    drop(db);
    assert_eq!(status(&pool, by.business).await, "paused");
    let app = app(&pool);
    let (_, _, page) = answer(&app, request(&Method::GET, JOB_TYPES_ROUTE, &jar, "")).await;
    assert!(
        !page.contains(plumbing[0].job_type.as_str()),
        "a removed job type is not offered"
    );
    let old = ticks(&plumbing);
    let (_, _, page) = answer(&app, request(&Method::POST, JOB_TYPES_ROUTE, &jar, &old)).await;
    assert!(page.contains("Nothing was saved."));
    assert_eq!(status(&pool, by.business).await, "paused");
    let form = ticks(&covered(&["R-11"], "electrical")[..1]);
    let (code, _, page) = answer(&app, request(&Method::POST, JOB_TYPES_ROUTE, &jar, &form)).await;
    assert!(code == 200 && page.contains("back on"), "{code}");
    assert_eq!(status(&pool, by.business).await, "active");
}

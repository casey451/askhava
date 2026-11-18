//! PICK-050, SEC-019, SEC-026 (T472, D-3750): the "Can't take it" page and button served by the
//! assembled router over PostgreSQL. The owner's and a "Confirm and billing" member's `GET` shows
//! the page and changes nothing; the owner's `POST` ends the pick as `cant_take` with no fee and
//! moves it to the backup she named with PICK-047's event queued, or with no backup queues
//! PICK-046's; a "Cards and quotes"-only member, the other business and a second `POST` get the one
//! not-found answer and change nothing; a wrong CSRF token is a 403. People sign in through
//! `portals`' emailed-link sign-in, as `business_portal_route.rs` does, and every record is built
//! and read through module APIs. Every ID, address and key is fake.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{AccountStatus, Acting, Roles, add_member, add_owner, create};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{MIGRATOR, SystemClock, unfinished};
use askhava_jobs::api::{
    JobStatus, TEXT_KIND, create as create_job, job_number, move_to, record_job_facts,
};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor as AuditActor, EXPORT_PAGE_MAX, export_audit};
use askhava_payments::api::business_statement;
use askhava_picks::api::{
    PICK_LAPSED_TEMPLATE, PICK_MOVED_TEMPLATE, PagePick, bind_backup, cant_take_job, choose_backup,
    ended_businesses, page_pick, waiting_picks_of,
};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const BROWSER: [u8; 32] = [7; 32];

/// An answer: status, `Location`, whether it is `no-store`, and its body and cookies.
struct Reply {
    status: u16,
    location: Option<String>,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

/// A same-origin request as a browser sends it, with the jar's CSRF token or a wrong one.
fn request(
    method: &Method,
    path: &str,
    jar: &CookieJar,
    (good, extra): (bool, &str),
) -> Request<Body> {
    let csrf = if good {
        csrf_token(jar).unwrap_or_default()
    } else {
        "0".repeat(64)
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let header_text = |name| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        location: header_text(header::LOCATION),
        no_store: header_text(header::CACHE_CONTROL).is_some_and(|v| v.contains("no-store")),
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
        (true, "&email=person%40x.example"),
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = answer(&app, request(&Method::POST, &path, &jar, (true, ""))).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        pressed.cookies.join("; ").parse().expect("cookies"),
    );
    CookieJar::from_headers(&headers)
}

/// A new active business and its owner, signed in.
async fn owner(pool: &PgPool) -> (Acting, CookieJar) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let business = create(&mut db, false, now).await.expect("business");
    let mailbox = Uuid::new_v4();
    let member = add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    (Acting { business, member }, jar)
}

/// A member of `owner`'s business with `roles`, signed in.
async fn member(pool: &PgPool, owner: Acting, roles: Roles) -> CookieJar {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = Uuid::new_v4();
    add_member(&mut db, owner, mailbox, roles, now)
        .await
        .expect("member");
    drop(db);
    signed_in(
        pool,
        SignInAddress {
            business: owner.business,
            mailbox,
        },
    )
    .await
}

/// `business` able to take an AC repair job in Lake Havasu City.
fn able(business: Uuid) -> CardBusiness {
    CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// The AC repair job as matching reads it.
fn card() -> CardJob {
    let candidate = Job {
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
        candidates: vec![candidate],
        text: String::new(),
    }
}

/// A firm $325 quote.
fn firm() -> (QuoteForm, QuoteRules) {
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
    (form, rules)
}

/// A job carded to and quoted by `businesses` in order, then her page pick of quote 1 with
/// quote 2 as her bound backup when `backup`; returns (job, pick).
async fn picked(pool: &PgPool, businesses: &[Uuid], backup: bool) -> (Uuid, Uuid) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let job = create_job(&mut db, now, false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, now, AuditActor::System).await;
    assert!(moved.expect("open").applied().is_some(), "opened");
    let to: Vec<CardBusiness> = businesses.iter().copied().map(able).collect();
    let map = ClassMap::shipped().expect("the shipped license map");
    send_cards(&mut db, map, job, &card(), &to, now)
        .await
        .expect("cards");
    for (i, business) in businesses.iter().enumerate() {
        let (form, rules) = firm();
        let at = now + SignedDuration::from_secs(i64::try_from(i).expect("small"));
        let quote = parse_quote(form, &rules).expect("quote");
        submit_quote(&mut db, (job, *business), &quote, at)
            .await
            .expect("quoted");
    }
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let made = page_pick(&mut db, (job, false), 1, AuditActor::System, now).await;
    let Ok(PagePick::Picked(pick)) = made else {
        panic!("a page pick")
    };
    if backup {
        choose_backup(&mut db, (job, 1), (Some(2), &BROWSER), now)
            .await
            .expect("asked");
        assert_eq!(
            bind_backup(&mut db, pick, &BROWSER, now)
                .await
                .expect("bound"),
            Some(2)
        );
    }
    (job, pick)
}

fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    build_router(pool.clone(), Router::new(), RouterParts::from(hooks))
}

/// What a refused request must leave as it was: the audit rows, whether the pick still waits for
/// `business`, and the business's waiting picks.
async fn state(pool: &PgPool, pick: Uuid, business: Uuid) -> (usize, bool, Vec<(Uuid, Uuid)>) {
    let mut db = pool.acquire().await.expect("db");
    let audit = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit")
        .len();
    let waits = cant_take_job(&mut db, pick, business)
        .await
        .expect("read")
        .is_some();
    (
        audit,
        waits,
        waiting_picks_of(&mut db, business).await.expect("waiting"),
    )
}

/// Whether `pick`'s `template` text event is queued (PICK-046, PICK-047).
async fn queued(pool: &PgPool, pick: Uuid, template: &str) -> bool {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("{TEXT_KIND}.{pick}.{template}");
    unfinished(&mut db, TEXT_KIND, &key).await.expect("queue")
}

fn path(pick: Uuid) -> String {
    format!("/b/picks/{pick}/cant-take")
}

async fn catalog(pool: &PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
}

/// `method` on `pick`'s "Can't take it" path by `jar`'s person, with the right token when `good`.
async fn send(pool: &PgPool, method: &Method, pick: Uuid, jar: &CookieJar, good: bool) -> Reply {
    answer(&app(pool), request(method, &path(pick), jar, (good, ""))).await
}

/// PICK-050, SEC-026: `jar`'s `GET` shows the page for job `number` with the session's CSRF
/// token and the forms stylesheet, `no-store`.
async fn shows_page(pool: &PgPool, pick: Uuid, jar: &CookieJar, number: i64) {
    let page = send(pool, &Method::GET, pick, jar, true).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.no_store, "no-store");
    assert!(
        page.text.contains(&format!("job {number}")),
        "the job's number"
    );
    assert!(page.text.contains("/static/portals/forms.css"));
    assert!(page.text.contains(&csrf_token(jar).expect("csrf")));
}

/// PICK-050, D-446: `business`'s pick on `job` ended as `cant_take` (or another end that never
/// counts against it) and no introduction fee is on its ledger.
async fn ended_with_no_fee(pool: &PgPool, job: Uuid, business: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let ended = ended_businesses(&mut db, job).await.expect("ended");
    assert_eq!(ended, vec![business]);
    drop(db);
    let now = Timestamp::now();
    let month = (now - SignedDuration::from_hours(48), now);
    let fees = business_statement(pool, business, month).await;
    assert!(
        fees.expect("statement").lines.is_empty(),
        "no introduction fee"
    );
}

/// PICK-050, SEC-026: the owner's and a billing member's `GET` show the page with its button and
/// the job's number and change nothing; the owner's `POST` ends the pick with no fee, moves it to
/// her backup with PICK-047's event queued, and answers 303 to the portal home; a second `POST`
/// is the one not-found answer and changes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn pick_050_the_owner_ends_the_pick_and_it_moves_to_her_backup(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let billing = member(&pool, x, Roles::new(false, true).expect("roles")).await;
    let (backup, _) = owner(&pool).await;
    let (job, pick) = picked(&pool, &[x.business, backup.business], true).await;
    let mut db = pool.acquire().await.expect("db");
    let number = job_number(&mut db, job).await.expect("number");
    drop(db);
    let before = state(&pool, pick, x.business).await;
    shows_page(&pool, pick, &jar, number).await;
    shows_page(&pool, pick, &billing, number).await;
    let after_get = state(&pool, pick, x.business).await;
    assert_eq!(after_get, before, "a GET changes nothing");
    let pressed = send(&pool, &Method::POST, pick, &jar, true).await;
    let at = (pressed.status, pressed.location.as_deref());
    assert_eq!(at, (303, Some("/b")));
    ended_with_no_fee(&pool, job, x.business).await;
    let moved = state(&pool, pick, backup.business).await.2;
    assert_eq!(moved.iter().map(|m| m.1).collect::<Vec<_>>(), vec![job]);
    assert!(queued(&pool, pick, PICK_MOVED_TEMPLATE).await);
    assert!(!queued(&pool, pick, PICK_LAPSED_TEMPLATE).await);
    let after = state(&pool, pick, x.business).await;
    let again = send(&pool, &Method::POST, pick, &jar, true).await;
    assert_eq!(again.status, 404, "a second press");
    assert_eq!(
        state(&pool, pick, x.business).await,
        after,
        "changes nothing"
    );
    let still = state(&pool, pick, backup.business).await.2;
    assert_eq!(still, moved, "a second press moves nothing");
}

/// PICK-046: with no backup the `POST` queues PICK-046's event and no PICK-047 one.
#[sqlx::test(migrator = "MIGRATOR")]
async fn pick_050_with_no_backup_queues_pick_046s_event(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (job, pick) = picked(&pool, &[x.business], false).await;
    let pressed = send(&pool, &Method::POST, pick, &jar, true).await;
    assert_eq!(pressed.status, 303);
    ended_with_no_fee(&pool, job, x.business).await;
    assert!(queued(&pool, pick, PICK_LAPSED_TEMPLATE).await);
    assert!(!queued(&pool, pick, PICK_MOVED_TEMPLATE).await);
}

/// SEC-019, SEC-026, SEC-009: a cards-only member, the other business and an unknown ID get the
/// one not-found answer on both methods and change nothing; a wrong CSRF token is a 403.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_019_everyone_else_gets_the_one_not_found_answer(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let cards = member(&pool, x, Roles::new(true, false).expect("roles")).await;
    let (_, other) = owner(&pool).await;
    let (_, pick) = picked(&pool, &[x.business], false).await;
    let before = state(&pool, pick, x.business).await;
    let unknown = path(Uuid::new_v4());
    let mut texts = BTreeSet::new();
    for (who, to) in [(&cards, path(pick)), (&other, path(pick)), (&jar, unknown)] {
        for method in [Method::GET, Method::POST] {
            let reply = answer(&app(&pool), request(&method, &to, who, (true, ""))).await;
            assert_eq!(reply.status, 404, "{method} {to}");
            assert!(reply.no_store);
            texts.insert(reply.text);
        }
    }
    assert_eq!(texts.len(), 1, "one not-found answer");
    let wrong = send(&pool, &Method::POST, pick, &jar, false).await;
    assert_eq!(wrong.status, 403, "a wrong CSRF token");
    let after = state(&pool, pick, x.business).await;
    assert_eq!(after, before, "refusals change nothing");
}

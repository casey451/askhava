//! TD-BIZ-62: leaving and the pause question through the assembled router (T734, D-4727,
//! D-5670; BIZ-051, BIZ-054, SEC-038, PICK-047, PICK-046, PICK-051, A11Y-030).
//!
//! `server::build_router` over a real test database with a fake `CardDetach`
//! (`business_leave_routes::ServedDetach::from_fn`), and once with the Stripe adapter
//! (`adapters::api::StripeCardDetach`) against a local fake HTTP server. A signed-in owner's
//! business has a waiting pick on job A (her backup is quote 3), a waiting pick on job B (no
//! backup) and two saved cards. People sign in through `portals`' emailed-link sign-in and jobs are
//! carded, quoted and picked through module APIs, as `cant_take_route.rs` does (`server` reads no
//! module's table, TD-SEC-10); step-ups are recorded with `record_session_step_up` in the session
//! that then acts (D-5314). The ledger rows a leave keeps are TD-BIZ-32's
//! (`tests/integration/tests/business_leaving.rs`). The stand-in Stripe half of
//! TD-BIZ-62 is `tests/e2e/business_leave.spec.ts`. Fake IDs, card IDs and wording only.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter, StripeCardDetach,
};
use askhava_businesses::api::{
    AccountStatus, Acting, Checks, Event, PAUSE_ASK_AFTER_DAYS, Roles, add_member, add_owner,
    apply, create, pause_ask_due, pause_self, reason_asked, record_session_step_up,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{MIGRATOR, SystemClock, unfinished};
use askhava_jobs::api::{JobStatus, TEXT_KIND, create as create_job, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{BusinessId, SavedCard, ensure_customer, save_card, saved_card_of};
use askhava_picks::api::{
    PICK_LAPSED_TEMPLATE, PICK_MOVED_TEMPLATE, PagePick, bind_backup, choose_backup, page_pick,
    waiting_picks_of,
};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, current_session, ensure_pre_session, signin_post, signin_request,
};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, page_quotes,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::business_leave_routes::ServedDetach;
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
const CARDS: [&str; 2] = ["pm_FakeT734a", "pm_FakeT734b"];
const LEAVE: &str = "/b/leave";
const PAUSE: &str = "/b/pause-reason";
const FAKE_KEY: &str = "sk_test_fake_t734_route_key";

/// An answer: status, `Location`, `no-store`, body and every `Set-Cookie` line.
struct Reply {
    status: u16,
    location: Option<String>,
    no_store: bool,
    text: String,
    set_cookies: Vec<String>,
}

fn ago(minutes: i64) -> Timestamp {
    Timestamp::now() - SignedDuration::from_mins(minutes)
}

/// A same-origin form post or page load as a browser sends it, with the jar's CSRF token.
fn request(method: &Method, path: &str, jar: &CookieJar, extra: &str) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
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
    let text_of = |name| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let set_cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        location: text_of(header::LOCATION),
        no_store: text_of(header::CACHE_CONTROL).is_some_and(|v| v.contains("no-store")),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        set_cookies,
    }
}

/// The fake detach: records each card and answers `ok` (or Stripe's 500).
fn fake_detach(ok: bool) -> (ServedDetach, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let detach = ServedDetach::from_fn(move |pm: String| {
        log.lock().unwrap_or_else(PoisonError::into_inner).push(pm);
        let answer = if ok {
            Ok(())
        } else {
            Err(AdapterError::Vendor {
                status: 500,
                code: None,
            })
        };
        std::future::ready(answer)
    });
    (detach, seen)
}

fn app(pool: &PgPool, detach: ServedDetach) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let parts = RouterParts::from(hooks).with_card_detach(detach);
    build_router(pool.clone(), Router::new(), parts)
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

/// `to`'s person signed in through `portals`' emailed link (D-1724): the jar and the session.
async fn signed_in(pool: &PgPool, to: SignInAddress) -> (CookieJar, Uuid) {
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
    let pressed = answer(&app, request(&Method::POST, &path, &jar, "")).await;
    let pairs: Vec<&str> = (pressed.set_cookies.iter())
        .filter_map(|c| c.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, pairs.join("; ").parse().expect("cookies"));
    let jar = CookieJar::from_headers(&headers);
    let mut db = pool.acquire().await.expect("db");
    let session = current_session(&mut db, &jar, Timestamp::now()).await;
    (jar, session.expect("read").expect("signed in").id())
}

/// A new active business and its owner, signed in: who, the jar and the session.
async fn owner(pool: &PgPool) -> (Acting, CookieJar, Uuid) {
    let (mut db, now) = (pool.acquire().await.expect("db"), ago(60));
    let business = create(&mut db, false, now).await.expect("business");
    let checks = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let moved = apply(
        &mut db,
        business,
        Event::Checked(checks),
        Actor::System,
        now,
    )
    .await;
    moved.expect("checked: active");
    let mailbox = Uuid::new_v4();
    let member = add_owner(&mut db, business, mailbox, now).await;
    drop(db);
    let (jar, session) = signed_in(pool, SignInAddress { business, mailbox }).await;
    let who = Acting {
        business,
        member: member.expect("owner"),
    };
    (who, jar, session)
}

/// A member of `owner`'s business with `roles`, signed in: who, the jar and the session.
async fn member(pool: &PgPool, owner: Acting, roles: Roles) -> (Acting, CookieJar, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let mailbox = Uuid::new_v4();
    let member = add_member(&mut db, owner, mailbox, roles, ago(60)).await;
    drop(db);
    let to = SignInAddress {
        business: owner.business,
        mailbox,
    };
    let (jar, session) = signed_in(pool, to).await;
    let who = Acting {
        business: owner.business,
        member: member.expect("member"),
    };
    (who, jar, session)
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
async fn picked(db: &mut PgConnection, businesses: &[Uuid], backup: bool) -> (Uuid, Uuid) {
    let now = ago(30);
    let job = create_job(db, now, false, Actor::System).await;
    let job = job.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, Actor::System).await;
    assert!(moved.expect("open").applied().is_some(), "opened");
    let to: Vec<CardBusiness> = businesses.iter().copied().map(able).collect();
    let map = ClassMap::shipped().expect("the shipped license map");
    send_cards(db, map, job, &card(), &to, now)
        .await
        .expect("cards");
    for (i, business) in businesses.iter().enumerate() {
        let (form, rules) = firm();
        let at = now + SignedDuration::from_secs(i64::try_from(i).expect("small"));
        let quote = parse_quote(form, &rules).expect("quote");
        submit_quote(db, (job, *business), &quote, at)
            .await
            .expect("quoted");
    }
    let types = ["AC not cooling".to_owned()];
    record_job_facts(db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let Ok(PagePick::Picked(pick)) = page_pick(db, (job, false), 1, Actor::System, now).await
    else {
        panic!("a page pick")
    };
    if backup {
        choose_backup(db, (job, 1), (Some(2), &BROWSER), now)
            .await
            .expect("asked");
        let bound = bind_backup(db, pick, &BROWSER, now).await.expect("bound");
        assert_eq!(bound, Some(2));
    }
    (job, pick)
}

/// Stripe's customer create, answering a fake `cus_…`; nothing else is called.
struct FakeCustomer;

impl PaymentsAdapter for FakeCustomer {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_FakeT734".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// `who`'s Stripe customer and its two saved cards.
async fn cards(pool: &PgPool, who: Acting) {
    let secret = SecretString::from("fake-t734-id-secret".to_owned());
    let business = BusinessId(who.business);
    ensure_customer(pool, &FakeCustomer, &secret, business)
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    for (n, pm) in (20..).zip(CARDS) {
        let card = SavedCard::new(pm, "card", "visa", "4242").expect("card");
        save_card(&mut db, business, &card, ago(n))
            .await
            .expect("saved");
    }
}

/// Job A picked with her backup on quote 2, job B picked with none; (job A, pick A, pick B, A's
/// backup business). `who` is quote 1 on both.
async fn waiting(pool: &PgPool, who: Acting) -> (Uuid, Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let backup = Uuid::new_v4();
    let (a, pick_a) = picked(&mut db, &[who.business, backup], true).await;
    let (_, pick_b) = picked(&mut db, &[who.business, Uuid::new_v4()], false).await;
    (a, pick_a, pick_b, backup)
}

/// Whether `pick`'s `template` text event is queued (PICK-046, PICK-047).
async fn queued(db: &mut PgConnection, pick: Uuid, template: &str) -> bool {
    let key = format!("{TEXT_KIND}.{pick}.{template}");
    unfinished(db, TEXT_KIND, &key).await.expect("queue")
}

/// Whether `business` still has a saved card.
async fn has_card(db: &mut PgConnection, business: Uuid) -> bool {
    let saved = saved_card_of(db, BusinessId(business)).await;
    saved.expect("read").is_some()
}

/// TD-BIZ-62 (SEC-038, BIZ-054): without a fresh code the leave answers the step-up form and
/// changes nothing; the leave screen asks the question while never asked, `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_a_stale_code_answers_the_step_up_form(pool: PgPool) {
    let (who, jar, session) = owner(&pool).await;
    cards(&pool, who).await;
    let mut db = pool.acquire().await.expect("db");
    let (detach, seen) = fake_detach(true);
    let app = app(&pool, detach);
    let page = answer(&app, request(&Method::GET, LEAVE, &jar, "")).await;
    assert_eq!((page.status, page.no_store), (200, true));
    assert!(page.text.contains("Can you tell us why?"), "{}", page.text);
    assert!(page.text.contains(&csrf_token(&jar).expect("csrf")));
    let stale = record_session_step_up(&mut db, who, session, ago(6));
    stale.await.expect("old code");
    let press = answer(&app, request(&Method::POST, LEAVE, &jar, "&reason=fee")).await;
    assert_eq!(press.status, 303);
    let step_up = Some("/b/step-up?return=%2Fb%2Fleave");
    assert_eq!(press.location.as_deref(), step_up);
    assert!(press.no_store);
    assert!(!reason_asked(&mut db, who.business).await.expect("read"));
    assert!(has_card(&mut db, who.business).await, "nothing removed");
    let detached = seen.lock().unwrap_or_else(PoisonError::into_inner).len();
    assert_eq!(detached, 0);
}

/// What a completed leave must show: `job`'s quote by `who` withdrawn, no saved card left, and
/// the session gone (its `GET` is the signed-out answer).
async fn has_left(
    app: &Router,
    (db, jar): (&mut PgConnection, &CookieJar),
    who: Acting,
    job: Uuid,
) {
    let quotes = page_quotes(db, job).await.expect("quotes");
    let own = quotes.iter().find(|q| q.business == who.business);
    assert!(own.expect("own quote").withdrawn, "quote withdrawn");
    assert!(!has_card(db, who.business).await, "cards marked removed");
    let after = answer(app, request(&Method::GET, LEAVE, jar, "")).await;
    assert_eq!(after.status, 401, "every session ended");
}

/// TD-BIZ-62 (BIZ-051, PICK-047, PICK-046, PICK-051, SEC-038): the owner with a fresh code
/// leaves with a reason: quotes withdrawn, pick A moved to her backup with PICK-047's text, pick B
/// with PICK-046's, both cards detached and marked removed, every session ended and the cookie
/// cleared.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_the_owner_leaves_with_a_reason(pool: PgPool) {
    let (who, jar, session) = owner(&pool).await;
    let (a, pick_a, pick_b, backup) = waiting(&pool, who).await;
    cards(&pool, who).await;
    let mut db = pool.acquire().await.expect("db");
    let (detach, seen) = fake_detach(true);
    let app = app(&pool, detach);
    let code = record_session_step_up(&mut db, who, session, ago(1));
    code.await.expect("code");
    let form = "&reason=few_jobs&other=";
    let left = answer(&app, request(&Method::POST, LEAVE, &jar, form)).await;
    assert_eq!((left.status, left.no_store), (200, true), "{}", left.text);
    assert!(left.text.contains("You've left"), "{}", left.text);
    let cleared = left.set_cookies.iter().any(|c| {
        c.starts_with("__Host-askhava-session=;") && c.contains("Max-Age=0") && c.contains("Secure")
    });
    assert!(cleared, "{:?}", left.set_cookies);
    let detached = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(detached, CARDS.map(str::to_owned).to_vec());
    has_left(&app, (&mut db, &jar), who, a).await;
    assert!(
        queued(&mut db, pick_a, PICK_MOVED_TEMPLATE).await,
        "PICK-047"
    );
    assert!(!queued(&mut db, pick_a, PICK_LAPSED_TEMPLATE).await);
    assert!(
        queued(&mut db, pick_b, PICK_LAPSED_TEMPLATE).await,
        "PICK-046"
    );
    let moved = waiting_picks_of(&mut db, backup).await.expect("waiting");
    assert_eq!(moved.len(), 1, "the backup has the pick");
    assert!(reason_asked(&mut db, who.business).await.expect("read"));
}

/// TD-BIZ-62 (BIZ-051, D-5670): a member who is not the owner, even with a fresh code, gets a 303
/// to the home on `GET` and a bare `no-store` 403 on `POST`; nothing is stored or detached.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_a_member_who_is_not_the_owner_is_refused(pool: PgPool) {
    let (boss, _, _) = owner(&pool).await;
    cards(&pool, boss).await;
    let roles = Roles::new(true, true).expect("roles");
    let (who, jar, session) = member(&pool, boss, roles).await;
    let mut db = pool.acquire().await.expect("db");
    record_session_step_up(&mut db, who, session, ago(1))
        .await
        .expect("code");
    let (detach, seen) = fake_detach(true);
    let app = app(&pool, detach);
    let page = answer(&app, request(&Method::GET, LEAVE, &jar, "")).await;
    assert_eq!((page.status, page.location.as_deref()), (303, Some("/b")));
    assert!(page.no_store);
    let press = answer(&app, request(&Method::POST, LEAVE, &jar, "&reason=fee")).await;
    assert_eq!((press.status, press.no_store), (403, true));
    assert!(press.text.is_empty(), "a bare 403: {}", press.text);
    assert!(!reason_asked(&mut db, who.business).await.expect("read"));
    assert!(has_card(&mut db, who.business).await, "nothing removed");
    let detached = seen.lock().unwrap_or_else(PoisonError::into_inner).len();
    assert_eq!(detached, 0);
}

/// TD-BIZ-62 (BIZ-051): every detach failing is counted, and the business has still left with
/// its cards marked removed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_a_detach_failure_still_leaves(pool: PgPool) {
    let (who, jar, session) = owner(&pool).await;
    let (a, _, _, _) = waiting(&pool, who).await;
    cards(&pool, who).await;
    let mut db = pool.acquire().await.expect("db");
    let (detach, seen) = fake_detach(false);
    let app = app(&pool, detach);
    record_session_step_up(&mut db, who, session, ago(1))
        .await
        .expect("code");
    let left = answer(&app, request(&Method::POST, LEAVE, &jar, "")).await;
    assert_eq!(left.status, 200, "{}", left.text);
    let tried = seen.lock().unwrap_or_else(PoisonError::into_inner).len();
    assert_eq!(tried, 2, "both detaches tried, both failed");
    has_left(&app, (&mut db, &jar), who, a).await;
}

/// TD-BIZ-62 (BIZ-054, D-463): no pause, no question (303 home); after the business's own pause
/// past 14 days the owner sees it once, its answer is stored and changes nothing else, and a
/// "Cards and quotes"-only member is never shown it.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_the_pause_question_shows_once(pool: PgPool) {
    let (who, jar, _) = owner(&pool).await;
    let roles = Roles::new(true, false).expect("roles");
    let (_, cards_jar, _) = member(&pool, who, roles).await;
    let mut db = pool.acquire().await.expect("db");
    let (detach, _) = fake_detach(true);
    let app = app(&pool, detach);
    let get = |jar| request(&Method::GET, PAUSE, jar, "");
    let early = answer(&app, get(&jar)).await;
    assert_eq!((early.status, early.location.as_deref()), (303, Some("/b")));
    pause_self(&mut db, who.business, ago(14 * 24 * 60 + 5))
        .await
        .expect("paused");
    let refused = answer(&app, get(&cards_jar)).await;
    assert_eq!(refused.status, 303, "a cards-only member is not asked");
    let page = answer(&app, get(&jar)).await;
    assert_eq!((page.status, page.no_store), (200, true));
    assert!(page.text.contains("over 2 weeks"), "{}", page.text);
    let post = request(&Method::POST, PAUSE, &jar, "&reason=busy&other=");
    let sent = answer(&app, post).await;
    assert_eq!((sent.status, sent.location.as_deref()), (303, Some("/b")));
    assert!(reason_asked(&mut db, who.business).await.expect("read"));
    let due = pause_ask_due(
        &mut db,
        who.business,
        PAUSE_ASK_AFTER_DAYS,
        Timestamp::now(),
    )
    .await;
    assert!(!due.expect("read"), "asked once, still paused");
    let again = answer(&app, get(&jar)).await;
    assert_eq!(again.status, 303, "asked once");
    let home = answer(&app, request(&Method::GET, "/b", &jar, "")).await;
    assert_eq!(home.status, 200, "still signed in, nothing else changed");
}

/// One request the fake Stripe server received: path, `Authorization` and `Idempotency-Key`.
type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

/// A local fake Stripe answering every detach with `status`; its base URL and what it saw.
async fn fake_stripe(status: u16) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    let handler = move |request: Request<Body>| {
        let log = Arc::clone(&log);
        async move {
            let header_of = |name: &str| {
                let value = request.headers().get(name).and_then(|v| v.to_str().ok());
                value.unwrap_or_default().to_owned()
            };
            let entry = (
                request.uri().path().to_owned(),
                header_of("authorization"),
                header_of("idempotency-key"),
            );
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(entry);
            let code = axum::http::StatusCode::from_u16(status).expect("status");
            (code, format!("{{\"echo\":\"{FAKE_KEY}\"}}"))
        }
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}/stripe", listener.local_addr().expect("addr"));
    let app = Router::new().fallback(handler);
    tokio::spawn(async move { axum::serve(listener, app).await });
    (base, seen)
}

/// TD-BIZ-62 (BIZ-051, SEC-018, SEC-032, R6): the adapter sends `POST
/// /v1/payment_methods/{pm}/detach` with the key as Bearer and a derived idempotency key that
/// names neither card nor key, through the leave; a refusal's error holds no key.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_62_the_adapter_detaches_at_a_local_fake_server(pool: PgPool) {
    let (who, jar, session) = owner(&pool).await;
    cards(&pool, who).await;
    let mut db = pool.acquire().await.expect("db");
    let (base, seen) = fake_stripe(200).await;
    let key = || SecretString::from(FAKE_KEY.to_owned());
    let client = StripeCardDetach::new(&base, key()).expect("client");
    assert!(!format!("{client:?}").contains(FAKE_KEY));
    let app = app(&pool, ServedDetach::stripe(client));
    record_session_step_up(&mut db, who, session, ago(1))
        .await
        .expect("code");
    let left = answer(&app, request(&Method::POST, LEAVE, &jar, "")).await;
    assert_eq!(left.status, 200, "{}", left.text);
    let got = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let paths: Vec<&str> = got.iter().map(|(p, _, _)| p.as_str()).collect();
    let want: Vec<String> = CARDS
        .iter()
        .map(|pm| format!("/stripe/v1/payment_methods/{pm}/detach"))
        .collect();
    assert_eq!(paths, want);
    for ((_, auth, idem), pm) in got.iter().zip(CARDS) {
        assert_eq!(auth, &format!("Bearer {FAKE_KEY}"));
        assert_eq!(idem.len(), 64, "a derived key");
        assert!(!idem.contains(pm) && !idem.contains(FAKE_KEY));
    }
    let (refusing, _) = fake_stripe(500).await;
    let client = StripeCardDetach::new(&refusing, key()).expect("client");
    let refused = client.detach_card(CARDS[0]).await;
    let error = refused.expect_err("refused");
    assert_eq!(
        error,
        AdapterError::Vendor {
            status: 500,
            code: None
        }
    );
    assert!(!format!("{error:?} {error}").contains(FAKE_KEY));
}

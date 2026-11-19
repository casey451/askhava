//! TD-SEC-49 (T306, D-2128, D-2711, D-3112, D-4551; BIZ-007, BIZ-029, BIZ-045, PICK-057, SEC-004,
//! SEC-034, SEC-042, SEC-048): the complaints page and its reply, the emergency-email switch with
//! its after-hours step, the job's notice log and "Not a fit for us", served by the assembled
//! router over PostgreSQL behind the business session. The complaints page shows each
//! complaint's kind and date with no homeowner detail and takes a reply only within its 5 days;
//! the switch changes only the caller's own person, and its after-hours step is the owner's or a
//! "Confirm and billing" member's; another business's pick, complaint or job and a missing role
//! get the one not-found answer and read nothing from the vault; no session gets a 4xx linking to
//! `/b/sign-in`; every answer is `no-store`; a cross-site or wrong-token `POST` is a 403 that
//! changes nothing and is one `csrf_rejected` event. People sign in through `portals`' emailed
//! link, as `business_portal_route.rs` does. Every value is fake.
//!
//! T562 (FEE-031, SEC-019, SEC-034, SEC-048; D-3112, D-3890, D-4603): `/b/charges/{pick}` shows
//! "Why was I charged?" for the business's own charge, paid through `payments::api::confirm_and_pay`
//! with a test-local fake Stripe as the door probe's `released_pick::charge` does: with the basis
//! line when the basis derived from the accepted quote gives the fee fixed at the pick, without it
//! when a flat fee mode fixed another; another business's pick and a missing role get the same
//! not-found answer and read nothing from the vault; no session gets a 4xx linking to `/b/sign-in`.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_businesses::api::{
    AccountStatus, Acting, EMERGENCY_YES_LIFE, NewComplaint, Roles, add_member, add_owner,
    after_hours_box, complaints, create, record_complaint, switch_state,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, MIGRATOR, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create as create_job, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor as AuditActor, EXPORT_PAGE_MAX, export_audit};
use askhava_payments::api::{
    BusinessId, ChargeOutcome, FeeMode, IntentStripe, PickNow, Press, SavedCard,
    accept_authorization, confirm_and_pay, ensure_customer, fee_terms_in_force, pick_fee,
    save_card, switch_fee_mode,
};
use askhava_picks::api::{PagePick, page_pick};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, reasons, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, mailbox_for, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, Format, OfferTown, Place,
    Price, QuoteForm, QuoteRules, build_card, card_digest, keep_card, mark_card_sent, parse_quote,
    send_cards, submit_quote,
};
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
/// The homeowner's fake name, which no business page may show.
const HER_NAME: &str = "Dana Testperson";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

/// Same-origin with the right token, cross-site by `Sec-Fetch-Site` or by `Origin`, or a wrong token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    OtherOrigin,
    WrongToken,
}

fn request(
    method: &Method,
    path: &str,
    jar: &CookieJar,
    (site, extra): (Site, &str),
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(jar).unwrap_or_default(),
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let builder = Request::builder()
        .method(method.clone())
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
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// An answer as the cases judge it.
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let (parts, body) = app
        .clone()
        .oneshot(request)
        .await
        .expect("answer")
        .into_parts();
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
    let asked = request(
        &Method::POST,
        SIGNIN_ROUTE,
        &jar,
        (Site::Own, "&email=p%40x.example"),
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = answer(&app, request(&Method::POST, &path, &jar, (Site::Own, ""))).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        pressed.cookies.join("; ").parse().expect("cookies"),
    );
    CookieJar::from_headers(&headers)
}

/// A person of a business: their acting IDs and signed-in cookies.
struct Person {
    by: Acting,
    jar: CookieJar,
}

/// A new business's owner at `address`, or with `of` a member of that business with `roles`.
async fn person(pool: &PgPool, address: &str, of: Option<(Acting, Roles)>) -> Person {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let by = match of {
        Some((owner, roles)) => Acting {
            business: owner.business,
            member: add_member(&mut db, owner, mailbox, roles, now)
                .await
                .expect("member"),
        },
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
    Person { by, jar }
}

/// A job opened, carded to `business` with an AC repair card and quoted by it at `cents`.
async fn quoted_job(db: &mut PgConnection, (business, cents): (Uuid, u64), now: Timestamp) -> Uuid {
    let job = create_job(db, now, false, AuditActor::System)
        .await
        .expect("job");
    let moved = move_to(
        db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        now,
        AuditActor::System,
    );
    assert_eq!(moved.await.expect("open"), Guarded::Applied(()));
    let (card, to) = card_for(business);
    let map = ClassMap::shipped().expect("the shipped license map");
    let cards = send_cards(db, map, job, &card, &[to], now).await;
    assert_eq!(cards.expect("card").len(), 1);
    sent(db, job, now).await;
    let (form, rules) = firm(cents);
    let quote = parse_quote(form, &rules).expect("quote");
    submit_quote(db, (job, business), &quote, now)
        .await
        .expect("quoted");
    job
}

/// An AC repair job in Lake Havasu City as matching reads it, and `business` able to take it.
fn card_for(business: Uuid) -> (CardJob, CardBusiness) {
    let kind = ("hvac_repair".to_owned(), "AC not cooling".to_owned());
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([kind.clone()]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let candidate = Job {
        category: kind.0,
        job_type: kind.1,
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    let card = CardJob {
        towns: vec![town],
        candidates: vec![candidate],
        text: String::new(),
    };
    (card, to)
}

/// Keeps and marks sent `job`'s card, made from fake parts (D-3189), so `job_trade` reads it.
async fn sent(db: &mut PgConnection, job: Uuid, now: Timestamp) {
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
    let shown = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(shown.text());
    let facts = CardFacts {
        category: "hvac_repair".to_owned(),
        candidates: vec!["AC not cooling".to_owned()],
        emergency: false,
    };
    let kept = keep_card(db, (Uuid::new_v4(), job), &digest, &shown, &facts, now).await;
    kept.expect("kept");
    assert!(mark_card_sent(db, job, &digest, now).await.expect("sent"));
}

/// A firm quote form at `cents` and its rules.
fn firm(cents: u64) -> (QuoteForm, QuoteRules) {
    let form = QuoteForm {
        price: Price::Firm(cents),
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

/// `business`'s page pick on a new job it quoted at $325 that her fake text request names:
/// (pick, job).
async fn picked(pool: &PgPool, business: Uuid) -> (Uuid, Uuid) {
    picked_at(pool, (business, 32_500), "+19285550193").await
}

/// `business`'s page pick on a new job it quoted at `cents` that her fake text request from
/// `phone` names.
async fn picked_at(pool: &PgPool, (business, cents): (Uuid, u64), phone: &str) -> (Uuid, Uuid) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    load_embedded(&mut db).await.expect("catalog");
    let job = quoted_job(&mut db, (business, cents), now).await;
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(&mut db, job, "hvac_repair", &types).await;
    facts.expect("job facts");
    her_request(&mut db, (job, phone), now).await;
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let pick = page_pick(&mut tx, (job, false), 1, AuditActor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = pick.expect("pick") else {
        panic!("no new pick")
    };
    (pick, job)
}

/// Her fake vault contact with `phone` and her text request naming `job`.
async fn her_request(db: &mut PgConnection, (job, phone): (Uuid, &str), now: Timestamp) {
    let new = NewContact {
        name: Some(SecretString::from(HER_NAME)),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: Some(SecretString::from("301 Fake Lane")),
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my AC is warm",
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
    let named = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(db, &named).await.expect("job named");
}

/// `business`'s complaint of kind `no_show` on `job`, received `ago` before now: its ID.
async fn complaint(pool: &PgPool, (business, job): (Uuid, Uuid), ago: SignedDuration) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let words = format!("{HER_NAME} says nobody came; call +19285550193");
    let new = NewComplaint {
        business,
        job,
        kind: "no_show",
        words: &words,
    };
    let at = Timestamp::now() - ago;
    record_complaint(&mut db, &new, at)
        .await
        .expect("complaint")
        .id
}

/// `business`'s complaint replies, newest complaint first.
async fn replies(pool: &PgPool, business: Uuid) -> Vec<Option<String>> {
    let mut db = pool.acquire().await.expect("db");
    let list = complaints(&mut db, business).await.expect("complaints");
    list.into_iter().map(|c| c.reply).collect()
}

/// The assembled router, with the vault the notice log reads.
fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    build_router(
        pool.clone(),
        Router::new(),
        RouterParts::from(hooks).with_vault(Some(vault())),
    )
}

/// Audit actions so far (each vault read is one).
async fn actions(pool: &PgPool) -> Vec<String> {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    rows.into_iter().map(|row| row.action).collect()
}

/// The security events logged while it is the default subscriber, by kind (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    fn count(&self, kind: &str) -> usize {
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().filter(|k| *k == kind).count()
    }
}

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
                .push(kind);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

async fn send(
    app: &Router,
    (method, path): (&Method, &str),
    jar: &CookieJar,
    extra: &str,
) -> Reply {
    answer(app, request(method, path, jar, (Site::Own, extra))).await
}

/// TD-SEC-49 (BIZ-029, SEC-034): the owner sees each complaint's kind and date with no homeowner
/// detail, replies within 5 days (stored, then no box), and a reply to a 6-day-old complaint is a
/// 422 that stores nothing; every answer is `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_complaints_show_no_homeowner_detail_and_reply_within_5_days(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let app = app(&pool);
    let (_, job) = picked(&pool, x.by.business).await;
    let old = complaint(&pool, (x.by.business, job), SignedDuration::from_hours(144)).await;
    let new = complaint(&pool, (x.by.business, job), SignedDuration::from_hours(1)).await;
    let page = send(&app, (&Method::GET, "/b/complaints"), &x.jar, "").await;
    assert!(page.status == 200 && page.no_store, "{page:?}");
    assert_eq!(
        page.text.matches("No show").count(),
        2,
        "each complaint's kind"
    );
    assert!(
        !page.text.contains("Dana") && !page.text.contains("5550193"),
        "no homeowner detail"
    );
    assert_eq!(
        page.text.matches("<textarea").count(),
        1,
        "only the new complaint's box is open"
    );
    let late = send(
        &app,
        (&Method::POST, &format!("/b/complaints/{old}/reply")),
        &x.jar,
        "&reply=Sorry",
    )
    .await;
    assert!(late.status == 422 && late.no_store, "{late:?}");
    let sent = send(
        &app,
        (&Method::POST, &format!("/b/complaints/{new}/reply")),
        &x.jar,
        "&reply=We+will+call",
    )
    .await;
    assert_eq!(sent.status, 303);
    assert_eq!(
        replies(&pool, x.by.business).await,
        [Some("We will call".to_owned()), None]
    );
    let again = send(
        &app,
        (&Method::POST, &format!("/b/complaints/{new}/reply")),
        &x.jar,
        "&reply=Twice",
    )
    .await;
    assert_eq!(again.status, 422, "one reply");
    assert_eq!(
        replies(&pool, x.by.business).await,
        [Some("We will call".to_owned()), None]
    );
}

/// TD-SEC-49 (BIZ-007, BIZ-042, D-2711): the switch changes only the caller's own person; the
/// after-hours step by a "Cards and quotes" member is the not-found answer, and by the owner it
/// calls `set_after_hours` (refused with no cover, ticked once the owner's switch has its Yes).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_the_switch_changes_only_the_callers_own_person(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let cards = Roles::new(true, false).expect("roles");
    let quoter = person(&pool, "quotes@x.example", Some((x.by, cards))).await;
    let app = app(&pool);
    let page = send(&app, (&Method::GET, "/b/emergency-emails"), &quoter.jar, "").await;
    assert!(page.status == 200 && page.no_store, "{page:?}");
    let on = send(
        &app,
        (&Method::POST, "/b/emergency-emails"),
        &quoter.jar,
        "&step=on",
    )
    .await;
    assert_eq!(on.status, 200);
    let state = |by: Acting| {
        let pool = pool.clone();
        async move {
            let mut db = pool.acquire().await.expect("db");
            let now = (Timestamp::now(), EMERGENCY_YES_LIFE);
            let ticked = after_hours_box(&mut db, x.by)
                .await
                .expect("box")
                .expect("owner")
                .ticked;
            (
                switch_state(&mut db, by, now).await.expect("state").on,
                ticked,
            )
        }
    };
    assert_eq!(
        (state(quoter.by).await, state(x.by).await),
        ((true, false), (false, false))
    );
    let tick = send(
        &app,
        (&Method::POST, "/b/emergency-emails"),
        &quoter.jar,
        "&step=after_hours_on",
    )
    .await;
    assert!(tick.status == 404 && tick.no_store, "{tick:?}");
    let refused = send(
        &app,
        (&Method::POST, "/b/emergency-emails"),
        &x.jar,
        "&step=after_hours_on",
    )
    .await;
    assert_eq!(
        (refused.status, state(x.by).await),
        (200, (false, false)),
        "no cover"
    );
    for step in ["on", "send_test", "yes", "after_hours_on"] {
        let r = send(
            &app,
            (&Method::POST, "/b/emergency-emails"),
            &x.jar,
            &format!("&step={step}"),
        )
        .await;
        assert_eq!(r.status, 200, "{step}");
    }
    assert_eq!(
        state(x.by).await,
        (true, true),
        "the owner's own switch and the tick"
    );
    let odd = send(
        &app,
        (&Method::POST, "/b/emergency-emails"),
        &x.jar,
        "&step=everyone",
    )
    .await;
    assert_eq!(odd.status, 422);
}

/// TD-SEC-49 (BIZ-045, PICK-057, SEC-019, SEC-048): the owner sees the notice log of the own
/// pick's job and answers "Not a fit for us" once; another business's pick, complaint or job, a
/// "Confirm and billing"-only member's answer and a "Cards and quotes" member on complaints all
/// get the one not-found answer, read nothing from the vault and are `authz_denied` events.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_notice_log_not_a_fit_and_refusals(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let y = person(&pool, "owner@y.example", None).await;
    let billing = Roles::new(false, true).expect("roles");
    let payer = person(&pool, "billing@x.example", Some((x.by, billing))).await;
    let cards = Roles::new(true, false).expect("roles");
    let quoter = person(&pool, "quotes@x.example", Some((x.by, cards))).await;
    let app = app(&pool);
    let (pick, job) = picked(&pool, x.by.business).await;
    let theirs = complaint(&pool, (x.by.business, job), SignedDuration::from_hours(1)).await;
    let log = send(
        &app,
        (&Method::GET, &format!("/b/picks/{pick}/notices")),
        &x.jar,
        "",
    )
    .await;
    assert!(
        log.status == 200 && log.no_store && log.text.contains("What we sent you about job"),
        "{log:?}"
    );
    assert!(!log.text.contains("Dana"), "no homeowner detail");
    let fit = format!("/b/not-a-fit/{job}/t0ken");
    let reason = format!("&reason={}", reasons()[0].0);
    let thanks = send(&app, (&Method::POST, &fit), &quoter.jar, &reason).await;
    assert!(
        thanks.status == 200 && thanks.text.contains("Thanks, got it"),
        "{thanks:?}"
    );
    let twice = send(&app, (&Method::POST, &fit), &x.jar, &reason).await;
    assert!(
        twice.text.contains("You already told us"),
        "one answer per business and job"
    );
    let none = send(&app, (&Method::POST, &fit), &x.jar, "").await;
    assert_eq!(none.status, 422, "no reason shows the reasons again");
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let reads = actions(&pool).await;
    let cases = [
        (
            &y.jar,
            Method::GET,
            format!("/b/picks/{pick}/notices"),
            String::new(),
        ),
        (
            &x.jar,
            Method::GET,
            format!("/b/picks/{}/notices", Uuid::new_v4()),
            String::new(),
        ),
        (
            &x.jar,
            Method::GET,
            "/b/picks/not-an-id/notices".to_owned(),
            String::new(),
        ),
        (
            &y.jar,
            Method::POST,
            format!("/b/complaints/{theirs}/reply"),
            "&reply=Hi".to_owned(),
        ),
        (&y.jar, Method::POST, fit.clone(), reason.clone()),
        (&payer.jar, Method::POST, fit.clone(), reason.clone()),
        (
            &quoter.jar,
            Method::GET,
            "/b/complaints".to_owned(),
            String::new(),
        ),
    ];
    let first = send(&app, (&cases[0].1, &cases[0].2), cases[0].0, &cases[0].3).await;
    assert!(first.status == 404 && first.no_store, "{first:?}");
    for (jar, method, path, extra) in &cases {
        assert_eq!(
            send(&app, (method, path), jar, extra).await,
            first,
            "{method} {path}"
        );
    }
    assert_eq!(actions(&pool).await, reads, "nothing read from the vault");
    assert_eq!(events.count("authz_denied"), cases.len() + 1);
    assert_eq!(
        replies(&pool, x.by.business).await,
        [None],
        "nothing stored"
    );
}

/// TD-SEC-49 (SEC-004, SEC-034, SEC-042, SEC-048): signed out, every page and form is a `no-store`
/// 4xx linking to `/b/sign-in`; a cross-site (`Sec-Fetch-Site` or `Origin`) or wrong-token `POST`
/// to each form is a 403 that changes nothing and is one `csrf_rejected` event.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_signed_out_cross_site_and_wrong_token_change_nothing(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let app = app(&pool);
    let (pick, job) = picked(&pool, x.by.business).await;
    let mine = complaint(&pool, (x.by.business, job), SignedDuration::from_hours(1)).await;
    let anon = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let gets = [
        "/b/complaints".to_owned(),
        "/b/emergency-emails".to_owned(),
        format!("/b/picks/{pick}/notices"),
    ];
    let reply = format!("/b/complaints/{mine}/reply");
    let fit = format!("/b/not-a-fit/{job}/t0ken");
    let posts = [
        (reply.as_str(), "&reply=Hi".to_owned()),
        ("/b/emergency-emails", "&step=on".to_owned()),
        (fit.as_str(), format!("&reason={}", reasons()[0].0)),
    ];
    let paths = gets.iter().map(|p| (Method::GET, p.as_str(), ""));
    let all = paths.chain(posts.iter().map(|(p, e)| (Method::POST, *p, e.as_str())));
    for (method, path, extra) in all {
        let r = send(&app, (&method, path), &anon, extra).await;
        let refused = (400..500).contains(&r.status) && r.text.contains("/b/sign-in");
        assert!(refused && r.no_store, "{method} {path}: {r:?}");
    }
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let sites = [Site::Cross, Site::OtherOrigin, Site::WrongToken];
    for (path, extra) in &posts {
        for site in sites {
            let r = answer(&app, request(&Method::POST, path, &x.jar, (site, extra))).await;
            assert!(r.status == 403 && r.no_store, "{path} {site:?}: {r:?}");
        }
    }
    assert_eq!(events.count("csrf_rejected"), posts.len() * sites.len());
    assert_eq!(replies(&pool, x.by.business).await, [None]);
    let mut db = pool.acquire().await.expect("db");
    let state = switch_state(&mut db, x.by, (Timestamp::now(), EMERGENCY_YES_LIFE)).await;
    assert!(!state.expect("state").on, "the switch is unchanged");
}

/// A fake Stripe answering `succeeded` for one pick of one business (as `door_matrix`'s
/// `released_pick` does); no vendor is reached.
struct FakeStripe {
    business: Uuid,
    pick: Uuid,
    cents: u64,
}

impl FakeStripe {
    fn customer(&self) -> String {
        format!("cus_fake{}", self.business.simple())
    }

    fn intent(&self) -> String {
        format!("pi_fake{}", self.pick.simple())
    }

    fn succeeded(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let body = serde_json::json!({
            "id": id,
            "status": "succeeded",
            "amount": self.cents,
            "currency": "usd",
            "customer": self.customer(),
            "metadata": {"pick_id": self.pick.to_string()},
            "livemode": false,
            "latest_charge": format!("ch_fake{}", self.pick.simple()),
        });
        Ok(serde_json::from_value(body).expect("a fake intent"))
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok(self.customer())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(id)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        Ok(self.intent())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(intent)
    }
}

/// `business`'s fake Stripe customer and saved card, the fee terms in force accepted (FEE-014,
/// FEE-015): the card's ID.
async fn card_on_file(pool: &PgPool, stripe: &FakeStripe, business: Uuid) -> String {
    let (secret, id) = (secret(), BusinessId(business));
    ensure_customer(pool, stripe, &secret, id)
        .await
        .expect("customer");
    let card = format!("pm_fake{}", business.simple());
    let saved = SavedCard::new(&card, "card", "visa", "4242").expect("card");
    let accepted = Timestamp::now() - SignedDuration::from_mins(1);
    let mut db = pool.acquire().await.expect("db");
    save_card(&mut db, id, &saved, accepted)
        .await
        .expect("saved");
    let terms = fee_terms_in_force(&mut db).await;
    let text = "Fake authorization text for a test.";
    accept_authorization(&mut db, id, terms.expect("terms"), text, accepted)
        .await
        .expect("accepted");
    card
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// Confirms and pays `pick` of `business` on `job` at the fee fixed at the pick (FEE-005).
async fn charge(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) {
    let mut db = pool.acquire().await.expect("db");
    let fixed = pick_fee(&mut db, pick).await.expect("read").expect("fixed");
    drop(db);
    let stripe = FakeStripe {
        business,
        pick,
        cents: fixed.cents,
    };
    let card = card_on_file(pool, &stripe, business).await;
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: u32::try_from(fixed.cents).expect("cents"),
        payment_method: &card,
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let paid = confirm_and_pay(pool, &stripe, &secret(), &press, Timestamp::now()).await;
    assert_eq!(
        paid.expect("charge"),
        ChargeOutcome::Succeeded(stripe.intent())
    );
}

/// TD-SEC-49 (FEE-031, SEC-019, SEC-034, SEC-048, D-4603): `/b/charges/{pick}` shows "Why was I
/// charged?" for the business's own charge to its owner and its "Confirm and billing" member,
/// with the basis line from the accepted quote, and without it when a flat fee mode fixed another
/// tier; another business's pick, a "Cards and quotes"-only member, an uncharged and an unknown
/// pick get the one not-found answer, read nothing from the vault and are `authz_denied` events;
/// no session gets a `no-store` 4xx linking to `/b/sign-in`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_why_charged_shows_the_own_charge_and_refuses_the_rest(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let y = person(&pool, "owner@y.example", None).await;
    let payer = Roles::new(false, true).expect("roles");
    let payer = person(&pool, "billing@x.example", Some((x.by, payer))).await;
    let quoter = Roles::new(true, false).expect("roles");
    let quoter = person(&pool, "quotes@x.example", Some((x.by, quoter))).await;
    let app = app(&pool);
    let (pick, job) = picked(&pool, x.by.business).await;
    charge(&pool, (x.by.business, job, pick)).await;
    let path = format!("/b/charges/{pick}");
    for who in [&x, &payer] {
        let page = send(&app, (&Method::GET, &path), &who.jar, "").await;
        assert!(page.status == 200 && page.no_store, "{page:?}");
        assert!(page.text.contains("Why was I charged for job"), "{page:?}");
        assert!(page.text.contains("$325, firm price"), "the accepted quote");
        assert!(page.text.contains("id=\"basis-line\""), "the derived basis");
        assert!(
            page.text
                .contains("The fee basis from your quote is $325.00")
        );
        assert!(!page.text.contains("Dana"), "no homeowner detail");
    }
    let (uncharged, _) = picked_at(&pool, (y.by.business, 32_500), "+19285550195").await;
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let reads = actions(&pool).await;
    let cases = [
        (&y.jar, path.clone()),
        (&quoter.jar, path.clone()),
        (&y.jar, format!("/b/charges/{uncharged}")),
        (&x.jar, format!("/b/charges/{}", Uuid::new_v4())),
        (&x.jar, "/b/charges/not-an-id".to_owned()),
    ];
    let first = send(&app, (&Method::GET, &cases[0].1), cases[0].0, "").await;
    assert!(first.status == 404 && first.no_store, "{first:?}");
    for (jar, path) in &cases {
        assert_eq!(
            send(&app, (&Method::GET, path), jar, "").await,
            first,
            "{path}"
        );
    }
    assert_eq!(actions(&pool).await, reads, "nothing read from the vault");
    assert_eq!(events.count("authz_denied"), cases.len() + 1);
    let anon = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let out = send(&app, (&Method::GET, &path), &anon, "").await;
    let refused = (400..500).contains(&out.status) && out.text.contains("/b/sign-in");
    assert!(refused && out.no_store, "{out:?}");
}

/// TD-SEC-49 (FEE-031, FEE-007, D-3890, D-4603): under the flat fee mode a $2,000 quote's charge
/// is fixed at the category's set tier, not the large tier its basis gives, so the page shows
/// every other line and no basis line rather than a guessed one.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_why_charged_shows_no_basis_line_when_the_tier_differs(pool: PgPool) {
    let x = person(&pool, "owner@x.example", None).await;
    let app = app(&pool);
    let mut db = pool.acquire().await.expect("db");
    let flat = switch_fee_mode(&mut db, Uuid::new_v4(), FeeMode::Flat).await;
    assert!(flat.expect("switched").is_some());
    drop(db);
    let (pick, job) = picked_at(&pool, (x.by.business, 200_000), "+19285550194").await;
    charge(&pool, (x.by.business, job, pick)).await;
    let page = send(
        &app,
        (&Method::GET, &format!("/b/charges/{pick}")),
        &x.jar,
        "",
    )
    .await;
    assert!(page.status == 200 && page.no_store, "{page:?}");
    assert!(
        page.text.contains("$2,000, firm price"),
        "the accepted quote"
    );
    assert!(page.text.contains("id=\"tier-line\""), "{page:?}");
    assert!(!page.text.contains("id=\"basis-line\""), "no guessed basis");
    assert!(!page.text.contains("large job"), "{page:?}");
}

/// TD-SEC-49 (FEE-031, FEE-007, D-3890, D-4603): under the flat fee mode a $325 quote's charge on
/// an AC repair job is the job type's small tier, the same $25 its basis gives by chance; the fee
/// was still set by job type, so the page shows no basis line.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_49_why_charged_shows_no_basis_line_under_flat_fees_when_the_tiers_match(
    pool: PgPool,
) {
    let x = person(&pool, "owner@x.example", None).await;
    let app = app(&pool);
    let mut db = pool.acquire().await.expect("db");
    let flat = switch_fee_mode(&mut db, Uuid::new_v4(), FeeMode::Flat).await;
    assert!(flat.expect("switched").is_some());
    drop(db);
    let (pick, job) = picked(&pool, x.by.business).await;
    charge(&pool, (x.by.business, job, pick)).await;
    let mut db = pool.acquire().await.expect("db");
    let fixed = pick_fee(&mut db, pick).await.expect("read").expect("fee");
    assert!(
        fixed.mode == FeeMode::Flat && fixed.cents == 2_500,
        "{fixed:?}"
    );
    drop(db);
    let path = format!("/b/charges/{pick}");
    let page = send(&app, (&Method::GET, &path), &x.jar, "").await;
    assert!(page.status == 200 && page.no_store, "{page:?}");
    assert!(page.text.contains("$325, firm price"), "the accepted quote");
    assert!(page.text.contains("id=\"tier-line\""), "{page:?}");
    assert!(!page.text.contains("id=\"basis-line\""), "no basis line");
    assert!(!page.text.contains("fee basis from your quote"), "{page:?}");
}

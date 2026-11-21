//! T754 (FEE-010, FEE-013, FEE-016, FEE-021, PICK-022, SEC-049; D-4763, D-5158, D-5171): a $0
//! Confirm after the page read opened a PaymentIntent, against PostgreSQL and a fake Stripe, fake
//! data only. The page is read while no free first win is due (so the read opens the intent) and
//! pressed after the business's category was switched on again (the waiver the read did not
//! see). The press ends the intent with no Stripe call and releases once; a charge in flight
//! leaves no $0 row; a later `payment_intent.succeeded` on the ended intent never releases again;
//! and a verified success on the pick's own intent that `paid_release` refuses raises one High
//! alert per pick and reason.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, PaymentIntent,
    PaymentsAdapter,
};
use askhava_businesses::api::{Checks, Event, SignUpForm, add_owner, apply, sign_up};
use askhava_core::api::{ClientIpPolicy, Guarded, MIGRATOR, ManualClock, SystemClock};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{
    ClassMap, DailyCheck, Holder, Job, LicenseKind, arizona_day, check_license, import_roc_list,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, IntentStripe, Releasable, ReleaseError, ReleaseSink,
    STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook, accept_authorization, ensure_customer,
    fee_terms_in_force, held_release, no_fee_release, stripe_webhook_routes,
};
use askhava_picks::api::{PagePick, page_pick};
use askhava_portals::api::{
    ApprovalSecret, ConfirmRoute, ConfirmSettings, CookieJar, PortalsError, SIGNIN_LINK_ROUTE,
    SIGNIN_ROUTE, SecretAnswer, SignIn, SignInAddress, SignInMail, confirm_routes, csrf_token,
    ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::ReleaseBasis;
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "laftly.test";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t754";
const WEBHOOK_SECRET: &str = "whsec_fake_t754";
const ROC_LIST: &str = "license_number,class,status,issued_on\nROC900754,R-39,Active,2016-01-01\n";

fn now() -> Timestamp {
    Timestamp::now()
}

/// The picks the sink was asked to release.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Uuid>>>);

impl ReleaseSink for Sink {
    async fn release(&self, cleared: Releasable) -> Result<(), ReleaseError> {
        self.0.lock().expect("lock").push(cleared.pick());
        Ok(())
    }
}

/// What the fake Stripe answers a PaymentIntent read with, and every intent call it got.
struct Told {
    status: &'static str,
    amount: i64,
    pick: Uuid,
    calls: Vec<String>,
}

#[derive(Clone)]
struct FakeStripe(Arc<Mutex<Told>>);

impl FakeStripe {
    fn told(&self) -> std::sync::MutexGuard<'_, Told> {
        self.0.lock().expect("lock")
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok(format!("cus_fake{}", Uuid::new_v4().simple()))
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let mut told = self.told();
        told.calls.push(format!("fetch {id}"));
        Ok(PaymentIntent {
            id: id.to_owned(),
            status: told.status.to_owned(),
            amount: told.amount,
            currency: "usd".to_owned(),
            customer: "cus_fake754".to_owned(),
            metadata: IntentMetadata {
                pick_id: told.pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: Some("ch_fake754".to_owned()),
        })
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
        pick: &str,
    ) -> Result<String, AdapterError> {
        let id = format!("pi_fake{}", pick.replace('-', ""));
        self.told().calls.push(format!("create {id}"));
        Ok(id)
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        id: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.told().calls.push(format!("confirm {id}"));
        Err(AdapterError::NotSent)
    }
}

/// No press in this file may confirm a charge.
struct NoConfirm;

impl ApprovalSecret for NoConfirm {
    fn confirm_keeping_secret<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        _: &'a str,
        _: &'a str,
    ) -> SecretAnswer<'a> {
        panic!("no charge is confirmed here")
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, Sink>>;

/// The fake Stripe, the sink and the webhook hooks over them; Stripe reads start as a $25 success
/// for `pick`.
struct World {
    stripe: FakeStripe,
    sink: Sink,
    hooks: Hooks,
}

fn world(pool: &PgPool, pick: Uuid) -> World {
    let told = Told {
        status: "succeeded",
        amount: 2500,
        pick,
        calls: Vec::new(),
    };
    let (stripe, sink) = (FakeStripe(Arc::new(Mutex::new(told))), Sink::default());
    let none: [(String, String); 0] = [];
    let hooks = Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: stripe.clone(),
        secret: SecretString::from(WEBHOOK_SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake754".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(now())),
        sink: sink.clone(),
    });
    World {
        stripe,
        sink,
        hooks,
    }
}

impl World {
    /// The Confirm route, launched 400 days ago: a business that joins today has a free first
    /// win only through a category switched on in its last 90 days (FEE-010).
    fn route(&self) -> Router {
        confirm_routes(ConfirmRoute {
            hooks: self.hooks.clone(),
            settings: Some(ConfirmSettings {
                publishable_key: "pk_test_fake754".to_owned(),
                launch: now() - SignedDuration::from_hours(24 * 400),
                secret: SecretString::from(ID_SECRET),
            }),
            approval: Arc::new(NoConfirm),
        })
    }

    /// The page read that opens the pick's PaymentIntent: the category was switched on 400 days
    /// ago, so no waiver is due yet.
    async fn read(&self, p: &Picked) {
        switched_on(&self.hooks.pool, 400).await;
        let (status, _) = call(&self.route(), &Method::GET, p, "").await;
        assert_eq!(status, 200, "the page");
        assert_eq!(
            intent(&self.hooks.pool, p.pick).await.1,
            Some(false),
            "open"
        );
    }

    /// The press after the category was switched on again, so a free first win the read did
    /// not see is now due: its JSON answer.
    async fn press(&self, p: &Picked) -> String {
        switched_on(&self.hooks.pool, 0).await;
        let body = format!("&press={}&payment_method=pm_card_fake754", Uuid::new_v4());
        call(&self.route(), &Method::POST, p, &body).await.1
    }

    /// A signed `payment_intent.succeeded` naming `intent` and `pick`: the route's status.
    async fn succeeded(&self, intent: &str, pick: Uuid) -> u16 {
        let object = json!({"id": intent, "metadata": {"pick_id": pick.to_string()}});
        let id = format!("evt_fake{}", Uuid::new_v4().simple());
        let body = json!({"id": id, "type": "payment_intent.succeeded", "livemode": false,
            "data": {"object": object}})
        .to_string();
        let at = now().as_second();
        let mut mac =
            <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
        mac.update(format!("{at}.{body}").as_bytes());
        let sig: String = (mac.finalize().into_bytes().iter())
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut request = Request::post(STRIPE_WEBHOOK_PATH)
            .header("stripe-signature", format!("t={at},v1={sig}"))
            .body(Body::from(body))
            .expect("request");
        let peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 443));
        request.extensions_mut().insert(ConnectInfo(peer));
        let reply = stripe_webhook_routes(self.hooks.clone()).oneshot(request);
        reply.await.expect("answer").status().as_u16()
    }
}

/// Sets the hvac category's latest switch-on to `days` ago (FEE-010, D-195).
async fn switched_on(pool: &PgPool, days: i32) {
    let sql = "UPDATE catalog.category SET switched_on_at = now() - make_interval(days => $1)
       WHERE code = 'hvac_repair'";
    let run = sqlx::query(sql).bind(days);
    assert_eq!(run.execute(pool).await.expect("switch").rows_affected(), 1);
}

/// A same-site request by `jar`'s person; `body` follows the CSRF token.
fn request((method, path): (&Method, &str), jar: &CookieJar, body: &str) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let csrf = csrf_token(jar).unwrap_or_default();
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 41], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status and body of `p`'s Confirm page read or press.
async fn call(app: &Router, method: &Method, p: &Picked, body: &str) -> (u16, String) {
    let path = format!("/b/picks/{}/confirm", p.pick);
    let mut request = request((method, &path), &p.jar, body);
    if *method == Method::POST {
        let json = "application/json".parse().expect("header");
        request.headers_mut().insert(header::ACCEPT, json);
    }
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The owner's one sign-in address; the link it "emails" is kept.
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
        (&Method::POST, SIGNIN_ROUTE),
        &jar,
        "&email=person%40x.example",
    );
    app.clone().oneshot(asked).await.expect("asked");
    let token = mail.sent.lock().expect("lock").clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let response = app
        .oneshot(request((&Method::POST, &path), &jar, ""))
        .await
        .expect("signed in");
    let cookies: Vec<&str> = (response.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// The hvac card for `business` in Lake Havasu City.
fn card(business: Uuid) -> (CardJob, CardBusiness) {
    let to = CardBusiness {
        id: business,
        status: askhava_businesses::api::AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let to_job = CardJob {
        towns: vec![OfferTown {
            code: "lake_havasu_city".to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![Job {
            category: "hvac_repair".to_owned(),
            job_type: "AC not cooling".to_owned(),
            license: LicenseKind::Roc,
            handyman_allowed: false,
            listed: true,
        }],
        text: String::new(),
    };
    (to_job, to)
}

/// An open hvac job with a $325 firm quote by `business`, so the fee fixed at the pick is $25.
async fn quoted(db: &mut PgConnection, business: Uuid) -> Uuid {
    let job = create(db, now(), false, Actor::System).await.expect("job");
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(db, job, "hvac_repair", &types).await;
    facts.expect("facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now(), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let (to_job, to) = card(business);
    let map = ClassMap::shipped().expect("map");
    let sent = send_cards(db, map, job, &to_job, &[to], now()).await;
    sent.expect("cards");
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor".to_owned(),
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
    let saved = submit_quote(db, (job, business), &quote, now()).await;
    assert_eq!(saved.expect("saved"), Submitted::Saved);
    job
}

/// A business signed up for hvac repair today, its owner signed in and a pick of its quote.
struct Picked {
    business: Uuid,
    job: Uuid,
    pick: Uuid,
    jar: CookieJar,
}

async fn picked(pool: &PgPool) -> (Picked, World) {
    let (secret, mailbox) = (SecretString::from(ID_SECRET), Uuid::new_v4());
    let setup = world(pool, Uuid::nil()).stripe;
    let form = SignUpForm {
        categories: vec!["hvac_repair".to_owned()],
        mailbox_id: Some(mailbox),
        heard_from: None,
        g_code: None,
    };
    let map = ClassMap::shipped().expect("map");
    let up = sign_up(pool, (&setup, &secret), map, &form, now()).await;
    let business = up.expect("signed up").business;
    let mut db = pool.acquire().await.expect("db");
    let owner = add_owner(&mut db, business, mailbox, now()).await;
    owner.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let to = apply(&mut db, business, Event::Checked(ok), Actor::System, now()).await;
    to.expect("active");
    let listed = import_roc_list(&mut db, arizona_day(now()), ROC_LIST).await;
    listed.expect("ROC list");
    let check = check_license(&mut db, business, "ROC900754").await;
    assert_eq!(check.expect("license check"), DailyCheck::Active);
    let job = quoted(&mut db, business).await;
    let mut tx = pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    let id = BusinessId(business);
    let customer = ensure_customer(pool, &setup, &secret, id).await;
    customer.expect("customer");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let accepted = accept_authorization(&mut db, id, terms, AUTHORIZATION_TEXT, now());
    accepted.await.expect("authorization");
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    let p = Picked {
        business,
        job,
        pick,
        jar,
    };
    (p, world(pool, pick))
}

/// The pick's stored PaymentIntent ID and whether it has ended (`None`: no intent row).
async fn intent(pool: &PgPool, pick: Uuid) -> (String, Option<bool>) {
    let sql = "SELECT stripe_intent_id, ended_at IS NOT NULL FROM payments.pick_intent
       WHERE pick_id = $1";
    let row: Option<(String, bool)> = sqlx::query_as(sql)
        .bind(pick)
        .fetch_optional(pool)
        .await
        .expect("intent");
    row.map_or((String::new(), None), |(id, ended)| (id, Some(ended)))
}

/// The count `sql` reads for `id`.
async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    let read = sqlx::query_scalar(sql).bind(id);
    read.fetch_one(pool).await.expect("count")
}

const ZERO_ROWS: &str =
    "SELECT count(*) FROM payments.ledger_entry WHERE business_id = $1 AND amount_cents = 0";

/// The kinds of the alerts about `pick`, every one High.
async fn alerts(pool: &PgPool, pick: Uuid) -> Vec<String> {
    let sql = "SELECT kind, severity::text FROM ops.alert WHERE subject_id = $1 ORDER BY kind";
    let rows: Vec<(String, String)> = sqlx::query_as(sql)
        .bind(pick)
        .fetch_all(pool)
        .await
        .expect("alerts");
    assert!(
        rows.iter().all(|(_, severity)| severity == "high"),
        "{rows:?}"
    );
    rows.into_iter().map(|(kind, _)| kind).collect()
}

/// One attempt of `pick` with `status`, as a press left it.
async fn attempt(pool: &PgPool, pick: Uuid, status: &str) {
    let sql = "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
       VALUES ($1, 1, gen_random_uuid(), $2, now())";
    let run = sqlx::query(sql).bind(pick).bind(status);
    run.execute(pool).await.expect("attempt");
}

/// FEE-010, FEE-016, PICK-022, SEC-049 (D-4763 M1, D-5158): the read opens the intent; the press
/// that decides $0 ends it with no Stripe call for it, carries no `client_secret` and releases
/// once; the release held in safe mode clears through the no-fee path; a success on the ended
/// intent with no attempt raises m3's High alert once and releases nothing; and a pick with a
/// succeeded attempt is still refused as free.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_zero_press_after_an_opened_intent_ends_it_and_releases_once(pool: PgPool) {
    let (p, w) = picked(&pool).await;
    w.read(&p).await;
    let before = w.stripe.told().calls.clone();
    assert_eq!(before.len(), 1, "one intent opened: {before:?}");
    let reply = w.press(&p).await;
    assert!(reply.contains("\"confirmed\""), "{reply}");
    assert!(!reply.contains("client_secret"), "{reply}");
    let (stored, ended) = intent(&pool, p.pick).await;
    assert_eq!(ended, Some(true), "the intent is ended");
    assert_eq!(w.stripe.told().calls, before, "no Stripe call for it");
    assert_eq!(count(&pool, ZERO_ROWS, p.business).await, 1);
    assert_eq!(*w.sink.0.lock().expect("lock"), vec![p.pick]);
    let account = &w.hooks.account;
    let held = held_release(&pool, &w.stripe, account, p.pick, now()).await;
    let held = held.expect("the held release clears");
    assert_eq!(
        (held.pick(), held.basis()),
        (p.pick, ReleaseBasis::NoFeeDue)
    );
    assert_eq!(w.stripe.told().calls, before, "and asks Stripe nothing");
    for _ in 0..2 {
        assert_eq!(w.succeeded(&stored, p.pick).await, 200);
    }
    let wrong_intent = "payments.success_refused_wrong_intent".to_owned();
    assert_eq!(alerts(&pool, p.pick).await, vec![wrong_intent]);
    assert_eq!(w.sink.0.lock().expect("lock").len(), 1, "no second release");
    attempt(&pool, p.pick, "succeeded").await;
    let mut db = pool.acquire().await.expect("db");
    let free = no_fee_release(&mut db, p.pick, (p.business, p.job)).await;
    assert_eq!(free.err(), Some(ReleaseError::NoFeeDecision));
}

/// FEE-016 (D-4763 M1): a press that would decide $0 while a charge of the pick is in flight
/// takes no $0 path: no $0 ledger row, the intent not ended, nothing released.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_press_with_a_charge_in_flight_is_not_waived(pool: PgPool) {
    let (p, w) = picked(&pool).await;
    w.read(&p).await;
    attempt(&pool, p.pick, "sent").await;
    let reply = w.press(&p).await;
    assert!(!reply.contains("\"confirmed\""), "{reply}");
    assert_eq!(count(&pool, ZERO_ROWS, p.business).await, 0);
    assert_eq!(intent(&pool, p.pick).await.1, Some(false));
    assert!(w.sink.0.lock().expect("lock").is_empty());
    let mut db = pool.acquire().await.expect("db");
    let free = no_fee_release(&mut db, p.pick, (p.business, p.job)).await;
    assert_eq!(free.err(), Some(ReleaseError::NoFeeDecision));
}

/// FEE-021, PICK-022 (D-5158 (a)): after a declined try and then the $0 press, a success on the
/// ended intent queues one late-success refund task and one High alert, and releases nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_late_success_after_a_declined_try_is_refunded_not_released(pool: PgPool) {
    let (p, w) = picked(&pool).await;
    w.read(&p).await;
    attempt(&pool, p.pick, "declined").await;
    let reply = w.press(&p).await;
    assert!(reply.contains("\"confirmed\""), "{reply}");
    let stored = intent(&pool, p.pick).await.0;
    for _ in 0..2 {
        assert_eq!(w.succeeded(&stored, p.pick).await, 200);
    }
    let tasks = "SELECT count(*) FROM core.task WHERE kind = 'payments.late_success_refund'
       AND payload ->> 'pick' = $1::text";
    assert_eq!(count(&pool, tasks, p.pick).await, 1);
    let late = "payments.late_success".to_owned();
    assert_eq!(alerts(&pool, p.pick).await, vec![late]);
    assert_eq!(*w.sink.0.lock().expect("lock"), vec![p.pick]);
}

/// SEC-049 (D-4763 m3, D-5171): each verified success on the pick's own stored intent refused as
/// `WrongAmount`, `WrongPick` or `NotPaid` raises one High alert and its replay none; a success
/// naming another intent raises none; nothing is released.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_refused_success_on_the_picks_own_intent_alerts_once(pool: PgPool) {
    let (p, w) = picked(&pool).await;
    w.read(&p).await;
    let stored = intent(&pool, p.pick).await.0;
    assert_eq!(w.succeeded("pi_fakeother754", p.pick).await, 200);
    assert!(alerts(&pool, p.pick).await.is_empty(), "another intent");
    let cases = [
        ("requires_payment_method", 2500, p.pick, "not_paid"),
        ("succeeded", 7500, p.pick, "wrong_amount"),
        ("succeeded", 2500, Uuid::from_u128(0x754), "wrong_pick"),
    ];
    let mut expected = Vec::new();
    for (status, amount, pick, reason) in cases {
        {
            let mut told = w.stripe.told();
            (told.status, told.amount, told.pick) = (status, amount, pick);
        }
        for _ in 0..2 {
            assert_eq!(w.succeeded(&stored, p.pick).await, 200, "{reason}");
        }
        expected.push(format!("payments.success_refused_{reason}"));
        assert_eq!(alerts(&pool, p.pick).await, expected, "{reason}");
    }
    assert!(w.sink.0.lock().expect("lock").is_empty());
}

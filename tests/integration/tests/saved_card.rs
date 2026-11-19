//! TD-SEC-70 (SEC-038, FEE-014, FEE-015, FEE-048; T541, D-3890, D-3892): a card saved or
//! changed from the Confirm page's "Save this card for next time" box.
//!
//! The served `portals::api::confirm_routes` against PostgreSQL with a Rust fake Stripe whose
//! `ApprovalSecret` records each saving confirm, card read and $0 SetupIntent: a ticked box whose
//! card will be stored confirms with `setup_future_usage=off_session` (a $0 Confirm's SetupIntent
//! with `usage=off_session`) and stores the `pm_…` Stripe's answer names with Stripe's own type,
//! brand and last four and the FEE-015 authorization; a posted brand or last four is refused and
//! a posted `pm_…` is never stored; a business with a saved card changes it only with a step-up
//! code from the last 5 minutes, else the payment goes through, nothing is saved and the reply
//! links the step-up page under its own label; a bank approval, a $0 `requires_action`, a non-card
//! method and a failed read store nothing; no reply holds the stored `pm_…`. Fake data only.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, CardFacts, Charge, ConfirmedIntent, IdempotencyKey, IntentMetadata,
    PaymentIntent, PaymentsAdapter, SetUpIntent,
};
use askhava_businesses::api::{Acting, SignUpForm, add_owner, record_step_up, sign_up};
use askhava_core::api::{ClientIpPolicy, Guarded, MIGRATOR, ManualClock, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer as TextAnswer, TextIn, receive_text, record_answer};
use askhava_integration::log_capture;
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, IntentStripe, Releasable, ReleaseError,
    ReleaseSink, SavedCard, StripeAccount, StripeWebhook, accept_authorization, ensure_customer,
    fee_terms_in_force, save_card,
};
use askhava_picks::api::{PagePick, page_pick};
use askhava_portals::api::{
    Answer, ApprovalSecret, ConfirmRoute, ConfirmSettings, CookieJar, PortalsError,
    SAVE_CODE_LABEL, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SecretAnswer, SignIn, SignInAddress,
    SignInMail, confirm_routes, csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "laftly.test";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t541";
const POSTED: &str = "pm_card_posted541";
const ANSWERED: &str = "pm_fakeanswered541";
const SET_UP: &str = "pm_fakesetup541";

fn now() -> Timestamp {
    Timestamp::now()
}

/// A sink that releases nothing; the save comes after the payment, never from a release.
struct Sink;

impl ReleaseSink for Sink {
    async fn release(&self, _: Releasable) -> Result<(), ReleaseError> {
        Ok(())
    }
}

/// The Stripe the payments code reaches: customers and the pick's one PaymentIntent.
#[derive(Default)]
struct FakeStripe;

fn intent(id: &str, state: &str) -> PaymentIntent {
    PaymentIntent {
        id: id.to_owned(),
        status: state.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_fake541".to_owned(),
        metadata: IntentMetadata {
            pick_id: String::new(),
        },
        livemode: Some(false),
        latest_charge: (state == "succeeded").then(|| "ch_fake541".to_owned()),
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
        Ok(intent(id, "succeeded"))
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
        Ok(format!("pi_fake{}", pick.replace('-', "")))
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        id: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        Ok(intent(id, "succeeded"))
    }
}

/// How the fake Stripe answers the saved-card calls.
#[derive(Clone)]
struct Script {
    /// The confirm answers `requires_action` (3-D Secure).
    approve: bool,
    /// The card read's answer; `None` is a failed read.
    facts: Option<(&'static str, &'static str, &'static str)>,
    /// The $0 SetupIntent's status.
    set_up: &'static str,
}

const CARD: Script = Script {
    approve: false,
    facts: Some(("card", "mastercard", "4444")),
    set_up: "succeeded",
};

/// One $0 SetupIntent call: its form fields and the card posted with it.
type SetUpCall = (Vec<(String, String)>, String);

/// The fake Stripe's saved-card calls, each recorded.
struct FakeApproval {
    script: Script,
    plain: AtomicUsize,
    saving: AtomicUsize,
    reads: Mutex<Vec<String>>,
    set_ups: Mutex<Vec<SetUpCall>>,
}

impl FakeApproval {
    fn new(script: Script) -> Self {
        Self {
            script,
            plain: AtomicUsize::new(0),
            saving: AtomicUsize::new(0),
            reads: Mutex::default(),
            set_ups: Mutex::default(),
        }
    }

    fn state(&self) -> &'static str {
        if self.script.approve {
            "requires_action"
        } else {
            "succeeded"
        }
    }
}

impl ApprovalSecret for FakeApproval {
    fn confirm_keeping_secret<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        id: &'a str,
        _: &'a str,
    ) -> SecretAnswer<'a> {
        self.plain.fetch_add(1, Ordering::SeqCst);
        let made = intent(id, self.state());
        Box::pin(async move { Ok((made, None)) })
    }

    fn confirm_saving<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        id: &'a str,
        _: &'a str,
    ) -> Answer<'a, ConfirmedIntent> {
        self.saving.fetch_add(1, Ordering::SeqCst);
        let made = ConfirmedIntent {
            intent: intent(id, self.state()),
            client_secret: None,
            payment_method: Some(ANSWERED.to_owned()),
        };
        Box::pin(async move { Ok(made) })
    }

    fn read_card<'a>(&'a self, id: &'a str) -> Answer<'a, CardFacts> {
        self.reads.lock().expect("lock").push(id.to_owned());
        let facts = self.script.facts.map(|(kind, brand, last4)| CardFacts {
            method_type: kind.to_owned(),
            brand: brand.to_owned(),
            last4: last4.to_owned(),
        });
        Box::pin(async move { facts.ok_or(AdapterError::OutcomeUnknown) })
    }

    fn set_up_card<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        form: &'a [(&'a str, &'a str)],
        card: &'a str,
    ) -> Answer<'a, SetUpIntent> {
        let fields = form
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let mut set_ups = self.set_ups.lock().expect("lock");
        set_ups.push((fields, card.to_owned()));
        let answer = SetUpIntent {
            status: self.script.set_up.to_owned(),
            payment_method: Some(SET_UP.to_owned()),
        };
        Box::pin(async move { Ok(answer) })
    }
}

/// A Confirm route whose saved-card calls the test reads.
struct Served {
    app: Router,
    approval: Arc<FakeApproval>,
}

fn served(pool: &PgPool, launch: Timestamp, script: Script) -> Served {
    let approval = Arc::new(FakeApproval::new(script));
    let app = route(pool, launch, approval.clone());
    Served { app, approval }
}

/// The Confirm route over the fake Stripe with `approval`'s calls.
fn route(pool: &PgPool, launch: Timestamp, approval: Arc<dyn ApprovalSecret>) -> Router {
    let none: [(String, String); 0] = [];
    let hooks = Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe,
        secret: SecretString::from("whsec_fake_t541"),
        account: StripeAccount {
            live: false,
            account: "acct_fake541".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(now())),
        sink: Sink,
    });
    confirm_routes(ConfirmRoute {
        hooks,
        settings: Some(ConfirmSettings {
            publishable_key: "pk_test_fake541".to_owned(),
            launch,
            secret: SecretString::from(ID_SECRET),
        }),
        approval,
    })
}

/// A same-site form post (or `GET` with an empty `body`) by `jar`'s person.
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
        .header(header::ACCEPT, "application/json")
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = std::net::SocketAddr::from(([203, 0, 113, 41], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status and body of `p`'s press with `body` after the CSRF token.
async fn press(app: &Router, p: &Picked, body: &str) -> (u16, String) {
    let path = format!("/b/picks/{}/confirm", p.pick);
    let request = request((&Method::POST, &path), &p.jar, body);
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn ticked(card: &str) -> String {
    format!(
        "&press={}&payment_method={card}&save_card=yes",
        Uuid::new_v4()
    )
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
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
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

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x41), &key(0x42), &url).expect("vault")
}

/// Her contact and request naming `job` (fake data).
async fn her_request(pool: &PgPool, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(format!(
            "+1928555{:04}",
            Uuid::new_v4().as_u128() % 10_000
        ))),
        email: None,
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let inbound = Inbound {
        words: "my ac is out",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from(ID_SECRET);
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = TextAnswer {
        conversation,
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
}

/// The hvac card for `business` in Lake Havasu City.
fn card(business: Uuid) -> (CardJob, CardBusiness) {
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
    let to = CardBusiness {
        id: business,
        status: askhava_businesses::api::AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let job = CardJob {
        towns: vec![town],
        candidates: vec![job],
        text: String::new(),
    };
    (job, to)
}

/// A $325 firm quote by `business`, so the fee fixed at the pick is $25 (FEE-005).
async fn quoted(pool: &PgPool, business: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        now(),
        Actor::System,
    );
    assert_eq!(moved.await.expect("open"), Guarded::Applied(()));
    let (to_job, to) = card(business);
    let map = ClassMap::shipped().expect("map");
    send_cards(&mut db, map, job, &to_job, &[to], now())
        .await
        .expect("cards");
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
    let saved = submit_quote(&mut db, (job, business), &quote, now()).await;
    assert_eq!(saved.expect("saved"), Submitted::Saved);
    job
}

/// A business, its owner signed in and her pick of its quote, the authorization accepted.
struct Picked {
    business: Uuid,
    owner: Uuid,
    pick: Uuid,
    jar: CookieJar,
}

/// [`Picked`]; with `signup`, signed up for hvac repair first, so a free first win is due.
async fn picked(pool: &PgPool, signup: bool) -> Picked {
    let (secret, mailbox) = (SecretString::from(ID_SECRET), Uuid::new_v4());
    let business = if signup {
        let form = SignUpForm {
            categories: vec!["hvac_repair".to_owned()],
            mailbox_id: Some(mailbox),
            heard_from: None,
            g_code: None,
        };
        let map = ClassMap::shipped().expect("map");
        let up = sign_up(pool, (&FakeStripe, &secret), map, &form, now()).await;
        up.expect("signed up").business
    } else {
        let mut db = pool.acquire().await.expect("db");
        let made = askhava_businesses::api::create(&mut db, false, now()).await;
        made.expect("business")
    };
    let mut db = pool.acquire().await.expect("db");
    let owner = add_owner(&mut db, business, mailbox, now())
        .await
        .expect("owner");
    drop(db);
    let job = quoted(pool, business).await;
    her_request(pool, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    let id = BusinessId(business);
    ensure_customer(pool, &FakeStripe, &secret, id)
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let accepted = accept_authorization(&mut db, id, terms, AUTHORIZATION_TEXT, now());
    accepted.await.expect("authorization");
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    Picked {
        business,
        owner,
        pick,
        jar,
    }
}

/// Each stored card of `business`: its `pm_…`, brand, last four and authorization version.
async fn cards(pool: &PgPool, business: Uuid) -> Vec<(String, String, String, i32)> {
    sqlx::query_as(
        "SELECT m.stripe_payment_method_id, m.brand, m.last4, a.version
         FROM payments.payment_method m
         JOIN payments.card_authorization a ON a.id = m.authorization_id
         WHERE m.business_id = $1 ORDER BY m.saved_at",
    )
    .bind(business)
    .fetch_all(pool)
    .await
    .expect("cards")
}

/// A card `business` saved earlier, so a new one is a change (SEC-038).
async fn saved_before(pool: &PgPool, business: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let card = SavedCard::new("pm_fakeearlier541", "card", "visa", "4242").expect("card");
    save_card(&mut db, BusinessId(business), &card, now())
        .await
        .expect("saved");
}

fn paid_launch() -> Timestamp {
    now() - SignedDuration::from_hours(24 * 400)
}

/// TD-SEC-70 (FEE-014, FEE-015, FEE-048): a ticked box with no saved card confirms with the
/// saving confirm and stores the `pm_…` Stripe's answer names with the brand and last four of
/// Stripe's read of that charge and the FEE-015 version; a posted brand or last four is a 422.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_a_ticked_box_stores_stripes_own_report(pool: PgPool) {
    let p = picked(&pool, false).await;
    let s = served(&pool, paid_launch(), CARD);
    let forged = format!("{}&brand=amex&last4=0005", ticked(POSTED));
    assert_eq!(press(&s.app, &p, &forged).await.0, 422);
    let logs = log_capture::start();
    let (status, reply) = press(&s.app, &p, &ticked(POSTED)).await;
    no_method_logged(&logs);
    assert_eq!(status, 200);
    assert!(
        reply.contains("Your card is saved for next time"),
        "{reply}"
    );
    assert!(
        !reply.contains(ANSWERED) && !reply.contains(POSTED),
        "{reply}"
    );
    let a = &s.approval;
    assert_eq!(
        (
            a.saving.load(Ordering::SeqCst),
            a.plain.load(Ordering::SeqCst)
        ),
        (1, 0)
    );
    assert_eq!(
        *a.reads.lock().expect("lock"),
        vec!["ch_fake541".to_owned()]
    );
    let stored = cards(&pool, p.business).await;
    let want = (ANSWERED, "mastercard", "4444", AUTHORIZATION_VERSION);
    assert_eq!(stored.len(), 1);
    let got = &stored[0];
    assert_eq!(
        (got.0.as_str(), got.1.as_str(), got.2.as_str(), got.3),
        want
    );
}

/// TD-SEC-70 (FEE-048): an unticked box sends nothing for saving and stores nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_off_session_use_is_asked_only_when_the_card_is_stored(pool: PgPool) {
    let p = picked(&pool, false).await;
    let s = served(&pool, paid_launch(), CARD);
    let body = format!("&press={}&payment_method={POSTED}", Uuid::new_v4());
    let (_, reply) = press(&s.app, &p, &body).await;
    assert!(
        reply.contains("\"waiting\"") && !reply.contains("saved"),
        "{reply}"
    );
    let a = &s.approval;
    assert_eq!(
        (
            a.saving.load(Ordering::SeqCst),
            a.plain.load(Ordering::SeqCst)
        ),
        (0, 1)
    );
    assert!(a.reads.lock().expect("lock").is_empty());
    assert!(cards(&pool, p.business).await.is_empty());
}

/// TD-SEC-70 (SEC-038): with a saved card and no code in the last 5 minutes the payment goes
/// through, nothing is sent for saving or stored, and the reply links the step-up page under
/// its own label; with a fresh code the new card is saved.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_a_saved_card_changes_only_with_a_fresh_code(pool: PgPool) {
    let p = picked(&pool, false).await;
    saved_before(&pool, p.business).await;
    let by = Acting {
        business: p.business,
        member: p.owner,
    };
    let mut db = pool.acquire().await.expect("db");
    let stale = now() - SignedDuration::from_secs(301);
    record_step_up(&mut db, by, stale)
        .await
        .expect("old step-up");
    let s = served(&pool, paid_launch(), CARD);
    let (_, reply) = press(&s.app, &p, &ticked(POSTED)).await;
    assert!(
        reply.contains("\"waiting\"") && reply.contains(SAVE_CODE_LABEL),
        "{reply}"
    );
    assert!(reply.contains("\"/b/step-up\""), "{reply}");
    assert_eq!(s.approval.saving.load(Ordering::SeqCst), 0);
    assert_eq!(cards(&pool, p.business).await.len(), 1);
    let q = picked(&pool, false).await;
    saved_before_other(&pool, q.business).await;
    let by = Acting {
        business: q.business,
        member: q.owner,
    };
    record_step_up(&mut db, by, now() - SignedDuration::from_secs(120))
        .await
        .expect("step-up");
    let logs = log_capture::start();
    let (_, reply) = press(&s.app, &q, &ticked(POSTED)).await;
    no_method_logged(&logs);
    assert!(reply.contains("Your card is saved") && !reply.contains(SAVE_CODE_LABEL));
    let stored = cards(&pool, q.business).await;
    assert!(stored.iter().any(|c| c.0 == ANSWERED), "{stored:?}");
}

/// TD-SEC-70: no log line of the press holds a `pm_…` ID, posted or answered by Stripe.
fn no_method_logged(logs: &log_capture::Capture) {
    let all = logs.all();
    for id in [ANSWERED, POSTED, SET_UP] {
        assert!(!all.contains(id), "a log line holds {id}: {all}");
    }
}

/// A second business's earlier card (each `pm_…` belongs to one business).
async fn saved_before_other(pool: &PgPool, business: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let card = SavedCard::new("pm_fakeearlier541b", "card", "visa", "4242").expect("card");
    save_card(&mut db, BusinessId(business), &card, now())
        .await
        .expect("saved");
}

/// TD-SEC-70 (FEE-014, D-073): a bank approval, a non-card method and a failed read store
/// nothing, and the reply says the card was not saved.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_approval_non_card_and_failed_read_store_nothing(pool: PgPool) {
    let cases = [
        (
            Script {
                approve: true,
                ..CARD
            },
            "needs a bank check",
        ),
        (
            Script {
                facts: Some(("us_bank_account", "", "")),
                ..CARD
            },
            "couldn't save",
        ),
        (
            Script {
                facts: None,
                ..CARD
            },
            "couldn't save",
        ),
    ];
    for (script, said) in cases {
        let p = picked(&pool, false).await;
        let s = served(&pool, paid_launch(), script);
        let (_, reply) = press(&s.app, &p, &ticked(POSTED)).await;
        assert!(reply.contains(said) && !reply.contains(ANSWERED), "{reply}");
        assert!(cards(&pool, p.business).await.is_empty(), "{said}");
    }
}

/// TD-SEC-70 (FEE-048): a $0 Confirm's ticked box makes an off-session SetupIntent with the
/// posted card and stores the `pm_…` the SetupIntent names, read from Stripe; a
/// `requires_action` SetupIntent stores nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_a_zero_confirm_saves_through_an_off_session_setup_intent(pool: PgPool) {
    let launch = now() - SignedDuration::from_hours(24);
    let p = picked(&pool, true).await;
    let s = served(&pool, launch, CARD);
    let logs = log_capture::start();
    let (_, reply) = press(&s.app, &p, &ticked(POSTED)).await;
    no_method_logged(&logs);
    assert!(
        reply.contains("\"confirmed\"") && reply.contains("saved for next time"),
        "{reply}"
    );
    let set_ups = s.approval.set_ups.lock().expect("lock").clone();
    assert_eq!(set_ups.len(), 1);
    let (form, card) = &set_ups[0];
    assert_eq!(card, POSTED);
    assert!(
        form.contains(&("usage".to_owned(), "off_session".to_owned())),
        "{form:?}"
    );
    assert_eq!(
        *s.approval.reads.lock().expect("lock"),
        vec![SET_UP.to_owned()]
    );
    let stored = cards(&pool, p.business).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].0, SET_UP);
    let q = picked(&pool, true).await;
    let held = served(
        &pool,
        launch,
        Script {
            set_up: "requires_action",
            ..CARD
        },
    );
    let (_, reply) = press(&held.app, &q, &ticked(POSTED)).await;
    assert!(
        reply.contains("\"confirmed\"") && reply.contains("couldn't save"),
        "{reply}"
    );
    assert!(held.approval.reads.lock().expect("lock").is_empty());
    assert!(cards(&pool, q.business).await.is_empty());
}

/// D-3892: an `ApprovalSecret` with only its required confirm (the default saved-card calls)
/// charges as before and stores nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_70_the_default_calls_save_nothing(pool: PgPool) {
    struct Plain;
    impl ApprovalSecret for Plain {
        fn confirm_keeping_secret<'a>(
            &'a self,
            _: &'a IdempotencyKey,
            id: &'a str,
            _: &'a str,
        ) -> SecretAnswer<'a> {
            let made = intent(id, "succeeded");
            Box::pin(async move { Ok((made, None)) })
        }
    }
    let p = picked(&pool, false).await;
    let app = route(&pool, paid_launch(), Arc::new(Plain));
    let (_, reply) = press(&app, &p, &ticked(POSTED)).await;
    assert!(
        reply.contains("\"waiting\"") && reply.contains("couldn't save"),
        "{reply}"
    );
    assert!(cards(&pool, p.business).await.is_empty());
}

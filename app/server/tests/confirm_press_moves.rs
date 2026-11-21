//! TD-JOB-19: the served Confirm press re-checks the license and account and moves the job to
//! Payment pending only when a charge will be attempted (T664; JOB-013, JOB-014, PICK-019,
//! PICK-020, PICK-044, FEE-020, OPS-024; D-056, D-4349, D-4897, D-5017).
//!
//! `portals::api::confirm_routes` under the real `security_headers` layer, as `confirm_route.rs`
//! serves it, with a fake Stripe that counts its confirms, can decline, can refuse to make a
//! customer, and notes at each confirm the status of a watched job. The stale, not Active, no-row
//! and non-ROC license cases, the account status, the window's end and the charging pause are
//! stored rows written through each module's own `api` (a server test names no other module's
//! schema); a stale check is the route's clock set 25 hours on. Every name, phone, key and ID
//! is fake.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_businesses::api::{
    AccountStatus, Checks, Event, PauseReason, SignUpForm, add_owner, apply, sign_up,
};
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, SystemClock, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, TEXT_KIND, create, current, move_to, record_job_facts};
use askhava_licensing::api::{
    CityListStanding, ClassMap, DailyCheck, Holder, Job, LicenseKind, arizona_day, check_license,
    import_roc_list,
};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, FeeJob, FeeMode, FeeQuote, IntentStripe, PeriodJob,
    RecurringCap, Service, StripeAccount, StripeWebhook, VisitTier, accept_authorization,
    ensure_customer, fee_at_pick, fee_terms_in_force,
};
use askhava_picks::api::{
    ConfirmSettings as Window, PagePick, cant_take_job, notice_sent, page_pick, pick_numbers,
};
use askhava_portals::api::{
    ApprovalSecret, ConfirmFee, ConfirmRoute, ConfirmSettings, ConfirmedPick, CookieJar,
    PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SecretAnswer, SignIn, SignInAddress, SignInMail,
    confirm_routes, csrf_token, decide_at_confirm, ensure_pre_session, security_headers,
    signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::release_sink::PickReleaser;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::middleware::from_fn;
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t664";

fn now() -> Timestamp {
    Timestamp::now()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x61), &key(0x62), &url).expect("vault")
}

fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    config.pool().clone()
}

/// A Stripe that counts PaymentIntent creates and confirms; `approve` answers `requires_action`.
/// A Stripe that counts confirms and notes, at each, the status of the `watch`ed job;
/// `decline` answers `requires_payment_method`, `no_customer` makes no customer.
struct FakeStripe {
    pool: PgPool,
    confirms: AtomicUsize,
    watch: Mutex<Uuid>,
    seen: Mutex<Vec<&'static str>>,
    decline: AtomicBool,
    no_customer: AtomicBool,
}

fn intent(id: &str, state: &str) -> PaymentIntent {
    PaymentIntent {
        id: id.to_owned(),
        status: state.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_fake664".to_owned(),
        metadata: IntentMetadata {
            pick_id: String::new(),
        },
        livemode: Some(false),
        latest_charge: (state == "succeeded").then(|| "ch_fake664".to_owned()),
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        if self.no_customer.load(Ordering::SeqCst) {
            return Err(AdapterError::NotSent);
        }
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
        self.confirms.fetch_add(1, Ordering::SeqCst);
        let job = *self.watch.lock().unwrap_or_else(PoisonError::into_inner);
        let mut db = self
            .pool
            .acquire()
            .await
            .map_err(|_| AdapterError::NotSent)?;
        if let Ok(row) = current(&mut db, job).await {
            let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            seen.push(row.status.code());
        }
        let decline = self.decline.load(Ordering::SeqCst);
        let state = if decline {
            "requires_payment_method"
        } else {
            "succeeded"
        };
        Ok(intent(id, state))
    }
}

/// The fake confirm through the counting Stripe; no bank approval here.
struct FakeApproval(Arc<StripeWebhook<FakeStripe, PickReleaser>>);

impl ApprovalSecret for FakeApproval {
    fn confirm_keeping_secret<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        intent: &'a str,
        card: &'a str,
    ) -> SecretAnswer<'a> {
        Box::pin(async move {
            let made = self.0.stripe.confirm_intent(key, intent, card).await?;
            Ok((made, None))
        })
    }
}

/// The Confirm route over a fresh fake Stripe, its clock `ahead` of now, launched `launched` ago.
struct Counted {
    app: Router,
    hooks: Arc<StripeWebhook<FakeStripe, PickReleaser>>,
}

fn counted(pool: &PgPool, launched: SignedDuration, ahead: SignedDuration) -> Counted {
    let none: [(String, String); 0] = [];
    let clock = Arc::new(ManualClock::new(now() + ahead));
    let stripe = FakeStripe {
        pool: pool.clone(),
        confirms: AtomicUsize::new(0),
        watch: Mutex::default(),
        seen: Mutex::default(),
        decline: AtomicBool::new(false),
        no_customer: AtomicBool::new(false),
    };
    let hooks = Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe,
        secret: SecretString::from("whsec_fake_t664"),
        account: StripeAccount {
            live: false,
            account: "acct_fake664".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    });
    let settings = ConfirmSettings {
        publishable_key: "pk_test_fake664".to_owned(),
        launch: now() - launched,
        secret: SecretString::from(ID_SECRET),
    };
    let route = ConfirmRoute {
        hooks: Arc::clone(&hooks),
        settings: Some(settings),
        approval: Arc::new(FakeApproval(Arc::clone(&hooks))),
    };
    let app = confirm_routes(route).layer(from_fn(security_headers));
    Counted { app, hooks }
}

impl Counted {
    fn confirms(&self) -> usize {
        self.hooks.stripe.confirms.load(Ordering::SeqCst)
    }

    /// `GET`s the page (opening the pick's PaymentIntent) or, with `card`, presses with it.
    async fn send(&self, p: &Picked, card: Option<&str>) -> String {
        let path = format!("/b/picks/{}/confirm", p.pick);
        let (method, body) = match card {
            Some(card) => (
                Method::POST,
                format!("&press={}&payment_method={card}", Uuid::new_v4()),
            ),
            None => (Method::GET, String::new()),
        };
        let request = request((&method, &path), &p.jar, &body);
        let response = self.app.clone().oneshot(request).await.expect("answer");
        let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// A same-site request by `jar`'s person; `body` is the form's fields after the CSRF token.
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
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
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
    let pressed = request((&Method::POST, &path), &jar, "");
    let response = app.oneshot(pressed).await.expect("signed in");
    let cookies: Vec<&str> = (response.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    let joined = cookies.join("; ").parse().expect("cookies");
    headers.insert(header::COOKIE, joined);
    CookieJar::from_headers(&headers)
}

/// Her contact and request naming `job` (fake data); returns her contact.
async fn her_request(pool: &PgPool, job: Uuid) -> Uuid {
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
    let answer = Answer {
        conversation,
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
    contact
}

/// The category, job type and license kind of the test job: a handyman job needs no ROC class.
const fn trade(handyman: bool) -> (&'static str, &'static str, LicenseKind) {
    if handyman {
        ("handyman", "door repair", LicenseKind::None)
    } else {
        ("hvac_repair", "AC not cooling", LicenseKind::Roc)
    }
}

fn card(business: Uuid, handyman: bool) -> (CardJob, CardBusiness) {
    let (category, job_type, license) = trade(handyman);
    let job = Job {
        category: category.to_owned(),
        job_type: job_type.to_owned(),
        license,
        handyman_allowed: handyman,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    let job = CardJob {
        towns: vec![town],
        candidates: vec![job],
        text: String::new(),
    };
    let licenses = Holder {
        handyman,
        city_list: if handyman {
            CityListStanding::Matched
        } else {
            CityListStanding::default()
        },
        ..Holder::default()
    };
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([(category.to_owned(), job_type.to_owned())]),
        classes: if handyman {
            Vec::new()
        } else {
            vec!["R-39".to_owned()]
        },
        licenses,
    };
    (job, to)
}

/// A $325 firm quote, so the fee fixed at the pick is $25 (FEE-005).
async fn quoted(pool: &PgPool, business: Uuid, handyman: bool) -> Uuid {
    let (category, job_type, license) = trade(handyman);
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = [job_type.to_owned()];
    record_job_facts(&mut db, job, category, &types)
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
    let (to_job, to) = card(business, handyman);
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
        license,
        handyman,
        certified: handyman,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(&mut db, (job, business), &quote, now()).await;
    assert_eq!(saved.expect("saved"), Submitted::Saved);
    job
}

/// A business, its owner signed in, a quoted job and her page pick, with the card authorization
/// A business, its owner signed in, a quoted job and her page pick, with the card authorization
/// accepted.
struct Picked {
    business: Uuid,
    job: Uuid,
    pick: Uuid,
    jar: CookieJar,
}

/// A business; with `signup`, signed up for hvac repair by `mailbox` (its join date, FEE-010).
async fn new_business(pool: &PgPool, signup: bool, mailbox: Uuid) -> Uuid {
    if signup {
        let form = SignUpForm {
            categories: vec!["hvac_repair".to_owned()],
            mailbox_id: Some(mailbox),
            heard_from: None,
            g_code: None,
        };
        let map = ClassMap::shipped().expect("map");
        let (fake, secret) = (counted(pool, DAY, NONE), SecretString::from(ID_SECRET));
        let up = sign_up(pool, (&fake.hooks.stripe, &secret), map, &form, now()).await;
        up.expect("signed up").business
    } else {
        let mut db = pool.acquire().await.expect("db");
        let made = askhava_businesses::api::create(&mut db, false, now()).await;
        made.expect("business")
    }
}

/// A fake ROC license number, and today's fake ROC list with it Active in class R-39.
const ROC_NUMBER: &str = "ROC900039";
const ROC_LIST: &str = "license_number,class,status,issued_on\nROC900039,R-39,Active,2016-01-01\n";

const DAY: SignedDuration = SignedDuration::from_hours(24);
const OLD: SignedDuration = SignedDuration::from_hours(24 * 400);
const NONE: SignedDuration = SignedDuration::ZERO;
const CARD: Option<&str> = Some("pm_card_visa");

/// How [`picked`] leaves the pressing business and its job.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seed {
    /// An Active account and a fresh Active check row; hvac repair (an ROC class).
    Eligible,
    /// [`Seed::Eligible`], signed up first: its join date, so a free first win (FEE-010).
    SignedUp,
    /// A Pending account: its sign-up checks never passed.
    PendingAccount,
    /// An Active account with no stored check row.
    NoCheck,
    /// An Active account whose latest stored check, of a later list, is not Active.
    Inactive,
    /// [`Seed::NoCheck`] on a handyman job, which needs no ROC class.
    Handyman,
}

/// The stored rows `seed` asks for (PICK-020, D-056, D-4897).
async fn seeded(db: &mut PgConnection, business: Uuid, seed: Seed) {
    if seed != Seed::PendingAccount {
        let ok = Checks {
            code_done: true,
            match_passed: true,
            flagged: false,
        };
        let to = apply(db, business, Event::Checked(ok), Actor::System, now()).await;
        to.expect("active");
    }
    if seed == Seed::NoCheck || seed == Seed::Handyman {
        return;
    }
    let today = arizona_day(now());
    import_roc_list(db, today, ROC_LIST)
        .await
        .expect("ROC list");
    let check = check_license(db, business, ROC_NUMBER).await;
    assert_eq!(check.expect("license check"), DailyCheck::Active);
    if seed == Seed::Inactive {
        let later = today.tomorrow().expect("day");
        let list = ROC_LIST.replace(",Active,", ",Suspended,");
        import_roc_list(db, later, &list).await.expect("later list");
        let check = check_license(db, business, ROC_NUMBER).await;
        assert_ne!(check.expect("license check"), DailyCheck::Active);
    }
}

/// [`Picked`] as `seed` says, with the card authorization accepted.
async fn picked(pool: &PgPool, seed: Seed) -> Picked {
    let (secret, mailbox) = (SecretString::from(ID_SECRET), Uuid::new_v4());
    let business = new_business(pool, seed == Seed::SignedUp, mailbox).await;
    let mut db = pool.acquire().await.expect("db");
    let owner = add_owner(&mut db, business, mailbox, now()).await;
    owner.expect("owner");
    seeded(&mut db, business, seed).await;
    drop(db);
    let job = quoted(pool, business, seed == Seed::Handyman).await;
    her_request(pool, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    let fake = counted(pool, DAY, NONE);
    ensure_customer(pool, &fake.hooks.stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let by = BusinessId(business);
    let accepted = accept_authorization(&mut db, by, terms, AUTHORIZATION_TEXT, now());
    accepted.await.expect("authorization");
    drop(db);
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    Picked {
        business,
        job,
        pick,
        jar,
    }
}

/// The job's status code and version, whether the pick still waits for its Confirm, and how
/// many of the business's picks lapsed (ended `expired`).
async fn left(pool: &PgPool, p: &Picked) -> (&'static str, i32, bool, u32) {
    let mut tx = pool.begin().await.expect("tx");
    let job = current(&mut tx, p.job).await.expect("job");
    let waits = cant_take_job(&mut tx, p.pick, p.business).await;
    let months = (now() - OLD, now() + OLD);
    let numbers = pick_numbers(&mut tx, p.business, months).await;
    tx.rollback().await.expect("rollback");
    let lapsed = numbers.expect("numbers").lapsed;
    let waits = waits.expect("pick").is_some();
    (job.status.code(), job.version, waits, lapsed)
}

/// The job's status code, whether the pick still waits and the lapsed picks, as one line.
async fn state(pool: &PgPool, p: &Picked) -> String {
    let (status, _, waits, lapsed) = left(pool, p).await;
    format!("{status} waits={waits} lapsed={lapsed}")
}

/// Her queued PICK-046 texts for `p`'s job, claiming every queued task.
async fn her_texts(pool: &PgPool, p: &Picked) -> usize {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = now() + SignedDuration::from_hours(24 * 30);
    let claimed = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    let job = p.job.to_string();
    let hers = |t: &&Claimed| {
        let payload = t.payload.to_string();
        t.kind == TEXT_KIND && payload.contains(&job) && payload.contains("pick_lapsed")
    };
    claimed.expect("claim").iter().filter(hers).count()
}

/// Starts `p`'s Confirm window now, `minutes` business minutes long; returns how far ahead of
/// now a clock must be to stand one minute past its end (at most a night and the window).
async fn window_over(pool: &PgPool, p: &Picked, minutes: u32) -> SignedDuration {
    let mut db = pool.acquire().await.expect("db");
    let settings = Window {
        window_minutes: minutes,
    };
    let due = notice_sent(&mut db, p.pick, now(), settings).await;
    let due = due.expect("window").expect("started");
    due.duration_since(now()) + SignedDuration::from_mins(1)
}

/// Whether `business` still has its free first win (FEE-010): the decision for a pick of a new
/// job, in a transaction rolled back.
async fn win_unused(pool: &PgPool, business: Uuid) -> bool {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(32_500), None, job, FeeMode::Tiers, None);
    let fee = fee.expect("fee");
    let confirmed = ConfirmedPick {
        pick: Uuid::new_v4(),
        business: BusinessId(business),
        job: Uuid::new_v4(),
        picked: &fee,
        same_customer_jobs: &[],
    };
    let mut tx = pool.begin().await.expect("tx");
    let secret = SecretString::from(ID_SECRET);
    let decided = decide_at_confirm(&mut tx, &secret, confirmed, Some(now() - DAY)).await;
    tx.rollback().await.expect("rollback");
    matches!(decided, Ok(ConfirmFee::NoFee(_)))
}

/// TD-JOB-19 (PICK-020, JOB-013, D-056, D-4897): a first press whose stored check is stale, not
/// Active or missing, whose account is not Active or whose window ended is refused before any
/// charge: no attempt, the pick ended (`expired` only for the window), the job back in Quotes
/// ready with her PICK-046 text, and an answer naming no license detail.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_19_an_ineligible_first_press_is_refused_before_any_charge(pool: PgPool) {
    let stale = SignedDuration::from_hours(25);
    let cases = [
        (Seed::Eligible, stale, false),
        (Seed::NoCheck, NONE, false),
        (Seed::PendingAccount, NONE, false),
        (Seed::Eligible, NONE, true),
        (Seed::Inactive, NONE, false),
    ];
    for (n, (seed, ahead, over)) in cases.into_iter().enumerate() {
        let p = picked(&pool, seed).await;
        let ahead = if over {
            window_over(&pool, &p, 240).await
        } else {
            ahead
        };
        let c = counted(&pool, OLD, ahead);
        c.send(&p, None).await;
        let reply = c.send(&p, CARD).await;
        assert!(reply.contains("\"ended\""), "{n}: {reply}");
        assert!(!reply.to_lowercase().contains("license"), "{reply}");
        let lapsed = u32::from(over);
        let ended = format!("quotes_ready waits=false lapsed={lapsed}");
        assert_eq!(state(&pool, &p).await, ended, "case {n}");
        assert_eq!((c.confirms(), her_texts(&pool, &p).await), (0, 1), "{n}");
        let again = c.send(&p, CARD).await;
        assert!(again.contains("\"ended\"") && c.confirms() == 0, "{again}");
    }
}

/// TD-JOB-19 (PICK-020, D-4897): work that needs no ROC class skips the ROC freshness check, so
/// a business with no stored check confirms, while its account status still refuses.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_19_a_category_with_no_roc_class_needs_no_stored_check(pool: PgPool) {
    let p = picked(&pool, Seed::Handyman).await;
    let c = counted(&pool, OLD, NONE);
    c.send(&p, None).await;
    let reply = c.send(&p, CARD).await;
    assert!(reply.contains("\"waiting\""), "{reply}");
    assert_eq!(c.confirms(), 1);
    let q = picked(&pool, Seed::Handyman).await;
    let c = counted(&pool, OLD, NONE);
    let mut db = pool.acquire().await.expect("db");
    let pause = Event::Pause(PauseReason::Owner);
    let paused = apply(&mut db, q.business, pause, Actor::System, now()).await;
    paused.expect("paused");
    drop(db);
    let reply = c.send(&q, CARD).await;
    assert!(reply.contains("\"ended\""), "{reply}");
    let ended = "quotes_ready waits=false lapsed=0";
    assert_eq!((state(&pool, &q).await.as_str(), c.confirms()), (ended, 0));
}

/// TD-JOB-19 (JOB-014, PICK-019, PICK-044, FEE-020): loading the page moves nothing; an eligible
/// press with a card moves the job to Payment pending before the charge; a declined card leaves
/// it there; a retry after the window's deadline is not refused as expired and moves nothing
/// more; a retry after the license check went stale ends the pick through `end_pending`, back
/// to Quotes ready with her text.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_19_a_press_with_a_card_moves_the_job_and_a_retry_is_never_expired(pool: PgPool) {
    let p = picked(&pool, Seed::Eligible).await;
    let over = window_over(&pool, &p, 1).await;
    let before = counted(&pool, OLD, NONE);
    before.send(&p, None).await;
    let waits = left(&pool, &p).await;
    assert_eq!(
        (waits.0, waits.2),
        ("awaiting_confirm", true),
        "a load moves nothing"
    );
    before.hooks.stripe.decline.store(true, Ordering::SeqCst);
    *before.hooks.stripe.watch.lock().expect("watch") = p.job;
    let reply = before.send(&p, CARD).await;
    assert!(reply.contains("\"declined\""), "{reply}");
    let seen = before.hooks.stripe.seen.lock().expect("seen").clone();
    assert_eq!(seen, ["payment_pending"], "moved before the charge attempt");
    let pending = left(&pool, &p).await;
    assert_eq!(
        (pending.0, pending.1, pending.2),
        ("payment_pending", waits.1 + 1, true)
    );
    let after = counted(&pool, OLD, over);
    after.hooks.stripe.decline.store(true, Ordering::SeqCst);
    let reply = after.send(&p, CARD).await;
    assert!(reply.contains("\"declined\""), "never expired: {reply}");
    assert_eq!(left(&pool, &p).await, pending, "the retry moved nothing");
    assert_eq!(after.confirms(), 1);
    let lapsed = counted(&pool, OLD, over + SignedDuration::from_hours(25));
    let reply = lapsed.send(&p, CARD).await;
    assert!(reply.contains("\"ended\""), "{reply}");
    assert_eq!(state(&pool, &p).await, "quotes_ready waits=false lapsed=0");
    assert_eq!((lapsed.confirms(), her_texts(&pool, &p).await), (0, 1));
}

/// TD-JOB-19 (PICK-019, OPS-024): a press with no card, a press while charges are paused (the
/// charging switch's stored row) answer as before, charge nothing and leave the job in Awaiting
/// confirm with its window, so a later press with a card still charges. (A press whose Stripe
/// customer cannot be made is not reached here: every pick fixture needs the card authorization,
/// which needs the customer row.)
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_19_a_press_that_will_not_charge_keeps_the_confirm_window(pool: PgPool) {
    let p = picked(&pool, Seed::Eligible).await;
    let c = counted(&pool, OLD, NONE);
    c.send(&p, None).await;
    let no_card = c.send(&p, Some("")).await;
    assert!(no_card.contains("Enter a card"), "{no_card}");
    let mut db = pool.acquire().await.expect("db");
    let on = set_switch(&mut db, Switch::Charging, true, Actor::System).await;
    assert!(on.expect("paused"));
    let paused = c.send(&p, CARD).await;
    let off = set_switch(&mut db, Switch::Charging, false, Actor::System).await;
    assert!(off.expect("resumed"));
    drop(db);
    assert!(paused.contains("\"try_later\""), "{paused}");
    let kept = state(&pool, &p).await;
    assert_eq!(kept, "awaiting_confirm waits=true lapsed=0");
    assert_eq!(c.confirms(), 0);
    let paid = c.send(&p, CARD).await;
    assert!(paid.contains("\"waiting\"") && c.confirms() == 1, "{paid}");
}

/// TD-JOB-19 (JOB-014, D-4349): a $0 press moves the job before the release (two moves, ending
/// Connected); a press after the job was cancelled ends the pick and charges nothing, and on
/// the $0 path its free first win stays unused.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_19_a_zero_press_moves_first_and_a_cancelled_job_ends_the_pick(pool: PgPool) {
    let free = picked(&pool, Seed::SignedUp).await;
    let c = counted(&pool, DAY, NONE);
    let before = left(&pool, &free).await.1;
    let reply = c.send(&free, Some("")).await;
    assert!(reply.contains("\"confirmed\""), "{reply}");
    let after = left(&pool, &free).await;
    assert_eq!((after.0, after.1), ("connected", before + 2));
    assert!(!win_unused(&pool, free.business).await);
    let gone_free = picked(&pool, Seed::SignedUp).await;
    let gone_paid = picked(&pool, Seed::Eligible).await;
    let c = counted(&pool, DAY, NONE);
    for (p, card) in [(&gone_free, Some("")), (&gone_paid, CARD)] {
        let mut db = pool.acquire().await.expect("db");
        let arrow = (JobStatus::AwaitingConfirm, JobStatus::Cancelled);
        let moved = move_to(&mut db, p.job, arrow, now(), Actor::System).await;
        assert_eq!(moved.expect("cancelled"), Guarded::Applied(()));
        drop(db);
        let reply = c.send(p, card).await;
        assert!(reply.contains("\"ended\""), "{reply}");
        assert_eq!(state(&pool, p).await, "cancelled waits=false lapsed=0");
    }
    assert!(win_unused(&pool, gone_free.business).await);
    assert_eq!(c.confirms(), 0);
}

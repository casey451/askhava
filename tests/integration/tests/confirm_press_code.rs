//! TD-SEC-82 (SEC-027, SEC-014, SEC-020; T810, D-5313, D-5443): while "This inbox is shared" is
//! on, each Confirm press asks for its own fresh Verify code.
//!
//! The served `portals::api::confirm_routes_with` against PostgreSQL, with a counting fake Stripe,
//! a counting fake Verify that ends a code once it answered it right (as Twilio Verify does) and a
//! code address that knows only the owner's own fake phone. The inbox is marked shared by a direct
//! `portals.shared_inbox` row, as `confirm_terms.rs` does. With it on, the signed-in Confirm page
//! shows the code step; "Text me a code" texts one code to the owner's own phone and changes
//! nothing else; a press with no code, a wrong code or a code already checked right once charges
//! nothing, opens no PaymentIntent, stores no terms row and answers the code's error; a press with
//! the right code charges once. With the inbox not shared a press needs no code and "Text me a
//! code" sends none. The owner's Quote and Hold links still act by the link alone. Fake data only.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, CodeChannel, IdempotencyKey, IntentMetadata, PaymentIntent,
    PaymentsAdapter, VerifyAdapter,
};
use askhava_businesses::api::{Checks, Event, add_owner, apply};
use askhava_core::api::{ClientIpPolicy, Guarded, MIGRATOR, ManualClock, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer as TextAnswer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{
    ClassMap, DailyCheck, Holder, Job, LicenseKind, arizona_day, check_license, import_roc_list,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, IntentStripe, Releasable, ReleaseError, ReleaseSink,
    StripeAccount, StripeWebhook, accept_authorization, ensure_customer, fee_terms_in_force,
};
use askhava_picks::api::{HoldAsk, PagePick, page_pick};
use askhava_portals::api::{
    ActionKind, ActionTarget, ApprovalSecret, CodeAddress, CodeCheck, CodeFuture, CodeSent, CodeTo,
    ConfirmCodes, ConfirmRoute, ConfirmSettings, CookieJar, PortalsError, PressCodes,
    SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SecretAnswer, SignIn, SignInAddress, SignInMail,
    confirm_routes_with, csrf_token, ensure_pre_session, fresh_code_needed, mint_action_link,
    signin_post, signin_request, use_action_link,
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
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t810";
/// The owner's own fake texting phone, the only one the code address knows.
const OWN_PHONE: &str = "+19285550182";
/// The one code the fake Verify sends.
const CODE: &str = "428190";
const WRONG: &str = "111111";
const CODE_STEP: &str = "Confirming needs a code sent to your phone";

fn now() -> Timestamp {
    Timestamp::now()
}

/// A sink that releases nothing.
struct Sink;

impl ReleaseSink for Sink {
    async fn release(&self, _: Releasable) -> Result<(), ReleaseError> {
        Ok(())
    }
}

/// What the fake Stripe was asked: PaymentIntents opened and charges confirmed.
#[derive(Default)]
struct Counts {
    intents: AtomicUsize,
    confirms: AtomicUsize,
}

/// The Stripe the payments code reaches, counting each PaymentIntent it opens.
#[derive(Default)]
struct FakeStripe(Arc<Counts>);

fn intent(id: &str, state: &str) -> PaymentIntent {
    PaymentIntent {
        id: id.to_owned(),
        status: state.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_fake810".to_owned(),
        metadata: IntentMetadata {
            pick_id: String::new(),
        },
        livemode: Some(false),
        latest_charge: (state == "succeeded").then(|| "ch_fake810".to_owned()),
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
        self.0.intents.fetch_add(1, Ordering::SeqCst);
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

/// The press's confirm, counted; it always succeeds.
struct Approval(Arc<Counts>);

impl ApprovalSecret for Approval {
    fn confirm_keeping_secret<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        id: &'a str,
        _: &'a str,
    ) -> SecretAnswer<'a> {
        self.0.confirms.fetch_add(1, Ordering::SeqCst);
        let made = intent(id, "succeeded");
        Box::pin(async move { Ok((made, None)) })
    }
}

/// The fake Verify's state: each phone texted and whether the last code is still live.
#[derive(Default)]
struct Texted {
    sent: Mutex<Vec<String>>,
    live: Mutex<bool>,
}

/// A Verify that sends [`CODE`] and ends it once it answered it right, or when it answered a
/// check on an ended code with Twilio's 404.
#[derive(Clone, Default)]
struct FakeVerify(Arc<Texted>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.sent.lock().expect("lock").push(to.to_owned());
        *self.0.live.lock().expect("lock") = true;
        Ok("VEfake810".to_owned())
    }

    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        let mut live = self.0.live.lock().expect("lock");
        if !*live {
            return Err(AdapterError::Vendor {
                status: 404,
                code: Some(20_404),
            });
        }
        let right = code == CODE;
        *live = !right;
        Ok(right)
    }
}

/// The owner's own fake phone; anyone else has none.
struct OwnPhone(Uuid);

impl CodeAddress for OwnPhone {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = if member == self.0 {
            CodeTo::Phone(SecretString::from(OWN_PHONE))
        } else {
            CodeTo::Email(SecretString::default())
        };
        Box::pin(async move { Ok(to) })
    }
}

/// The served Confirm route with codes, and what its fakes were asked.
struct World {
    app: Router,
    counts: Arc<Counts>,
    verify: FakeVerify,
}

impl World {
    fn intents(&self) -> usize {
        self.counts.intents.load(Ordering::SeqCst)
    }

    fn confirms(&self) -> usize {
        self.counts.confirms.load(Ordering::SeqCst)
    }

    fn sent(&self) -> Vec<String> {
        self.verify.0.sent.lock().expect("lock").clone()
    }
}

/// The Confirm route over the fakes, with `owner`'s codes (D-5443: built as `saved_card.rs`).
fn world(pool: &PgPool, owner: Uuid) -> World {
    let none: [(String, String); 0] = [];
    let counts = Arc::new(Counts::default());
    let hooks = Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe(Arc::clone(&counts)),
        secret: SecretString::from("whsec_fake_t810"),
        account: StripeAccount {
            live: false,
            account: "acct_fake810".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(now())),
        sink: Sink,
    });
    let route = ConfirmRoute {
        hooks,
        settings: Some(ConfirmSettings {
            publishable_key: "pk_test_fake810".to_owned(),
            launch: now() - SignedDuration::from_hours(24 * 400),
            secret: SecretString::from(ID_SECRET),
        }),
        approval: Arc::new(Approval(Arc::clone(&counts))),
    };
    let verify = FakeVerify::default();
    let key = SecretString::from(ID_SECRET);
    let codes = ConfirmCodes::new(verify.clone(), OwnPhone(owner), key);
    let app = confirm_routes_with(route, Some(PressCodes::new(codes)));
    World {
        app,
        counts,
        verify,
    }
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
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = std::net::SocketAddr::from(([203, 0, 113, 82], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status and body of `method` on `p`'s Confirm path plus `tail`, with `body`.
async fn call(
    app: &Router,
    (method, tail): (&Method, &str),
    p: &Picked,
    body: &str,
) -> (u16, String) {
    let path = format!("/b/picks/{}/confirm{tail}", p.pick);
    let response = app
        .clone()
        .oneshot(request((method, &path), &p.jar, body))
        .await
        .expect("answer");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The Confirm page.
async fn page(w: &World, p: &Picked) -> (u16, String) {
    call(&w.app, (&Method::GET, ""), p, "").await
}

/// "Text me a code".
async fn text_me(w: &World, p: &Picked) -> (u16, String) {
    call(&w.app, (&Method::POST, "/code"), p, "").await
}

/// One press with a fake card and `code`, if any.
async fn press(w: &World, p: &Picked, code: Option<&str>) -> String {
    let code = code.map_or_else(String::new, |c| format!("&code={c}"));
    let body = format!(
        "&press={}&payment_method=pm_card_visa{code}",
        Uuid::new_v4()
    );
    call(&w.app, (&Method::POST, ""), p, &body).await.1
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

/// A business, its owner signed in and her pick of its quote on `job`, the authorization
/// accepted.
struct Picked {
    business: Uuid,
    owner: Uuid,
    job: Uuid,
    pick: Uuid,
    jar: CookieJar,
    /// [`stored`] right after the pick.
    before: (String, i64, i64),
}

/// A fake ROC license number, and today's fake ROC list with it Active in class R-39.
const ROC_NUMBER: &str = "ROC900810";
const ROC_LIST: &str = "license_number,class,status,issued_on\nROC900810,R-39,Active,2016-01-01\n";

/// PICK-020: makes `business` one that may Confirm: its account Active through its sign-up checks,
/// and one fresh Active ROC check row of a fake license number.
async fn eligible(db: &mut PgConnection, business: Uuid, now: Timestamp) {
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let to = apply(db, business, Event::Checked(ok), Actor::System, now).await;
    to.expect("active");
    let listed = import_roc_list(db, arizona_day(now), ROC_LIST).await;
    listed.expect("ROC list");
    let check = check_license(db, business, ROC_NUMBER).await;
    assert_eq!(check.expect("license check"), DailyCheck::Active);
}

/// [`Picked`]: a $25 charged pick, no free first win due.
async fn picked(pool: &PgPool) -> Picked {
    let (secret, mailbox) = (SecretString::from(ID_SECRET), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now()).await;
    let business = business.expect("business");
    let owner = add_owner(&mut db, business, mailbox, now())
        .await
        .expect("owner");
    eligible(&mut db, business, now()).await;
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
    ensure_customer(pool, &FakeStripe::default(), &secret, id)
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let accepted = accept_authorization(&mut db, id, terms, AUTHORIZATION_TEXT, now());
    accepted.await.expect("authorization");
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    let mut p = Picked {
        business,
        owner,
        job,
        pick,
        jar,
        before: (String::new(), 0, 0),
    };
    p.before = stored(pool, &p).await;
    p
}

/// "This inbox is shared" on for `p`'s owner, by a direct row (D-5319, D-5443).
async fn shared(pool: &PgPool, p: &Picked) {
    sqlx::query(
        "INSERT INTO portals.shared_inbox (member_id, business_id, shared, changed_at) \
         VALUES ($1, $2, true, now())",
    )
    .bind(p.owner)
    .bind(p.business)
    .execute(pool)
    .await
    .expect("shared");
}

/// `p`'s pick status, its stored PaymentIntents and its business's terms rows.
async fn stored(pool: &PgPool, p: &Picked) -> (String, i64, i64) {
    let status = sqlx::query_scalar("SELECT status::text FROM picks.pick WHERE id = $1")
        .bind(p.pick)
        .fetch_one(pool)
        .await
        .expect("pick");
    let intents =
        sqlx::query_scalar("SELECT count(*) FROM payments.pick_intent WHERE pick_id = $1")
            .bind(p.pick)
            .fetch_one(pool)
            .await
            .expect("intents");
    let terms = sqlx::query_scalar(
        "SELECT count(*) FROM businesses.terms_acceptance WHERE business_id = $1",
    )
    .bind(p.business)
    .fetch_one(pool)
    .await
    .expect("terms");
    (status, intents, terms)
}

/// A press `w` refused with `check`'s error: nothing charged, opened or stored since the pick.
async fn refused(pool: &PgPool, (w, p): (&World, &Picked), reply: &str, check: CodeCheck) {
    let read: serde_json::Value = serde_json::from_str(reply).expect("a JSON reply");
    assert_eq!(read["message"], check.error().expect("an error"), "{reply}");
    assert_eq!(read["state"], "declined", "{reply}");
    assert_eq!((w.confirms(), w.intents()), (0, 0));
    assert_eq!(stored(pool, p).await, p.before, "nothing stored");
}

/// TD-SEC-82 (SEC-027): with the inbox shared the signed-in page shows the code step, and
/// "Text me a code" texts one code to the owner's own phone and changes nothing else.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_82_text_me_a_code_sends_one_code_and_changes_nothing(pool: PgPool) {
    let p = picked(&pool).await;
    shared(&pool, &p).await;
    let w = world(&pool, p.owner);
    let (status, sent) = text_me(&w, &p).await;
    assert_eq!(status, 200, "{sent}");
    assert!(sent.contains(CODE_STEP) && sent.contains(CodeSent::Sent.line()));
    assert_eq!(w.sent(), vec![OWN_PHONE.to_owned()], "the own phone, once");
    assert_eq!((w.confirms(), w.intents()), (0, 0));
    assert_eq!(stored(&pool, &p).await, p.before, "nothing else changed");
    let (status, shown) = page(&w, &p).await;
    assert_eq!(status, 200, "{shown}");
    assert!(shown.contains(CODE_STEP) && shown.contains("id=\"code-form\""));
    assert!(
        !shown.contains(CodeSent::Sent.line()),
        "a fresh page asks again"
    );
    assert_eq!(w.sent().len(), 1, "a page load sends nothing");
}

/// TD-SEC-82 (SEC-027, SEC-020): a press with no code or a wrong code charges nothing, opens no
/// PaymentIntent, stores no terms row and answers the code's error.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_82_no_code_or_a_wrong_code_charges_nothing(pool: PgPool) {
    let p = picked(&pool).await;
    shared(&pool, &p).await;
    let w = world(&pool, p.owner);
    let bare = press(&w, &p, None).await;
    refused(&pool, (&w, &p), &bare, CodeCheck::Missing).await;
    text_me(&w, &p).await;
    let wrong = press(&w, &p, Some(WRONG)).await;
    refused(&pool, (&w, &p), &wrong, CodeCheck::Wrong).await;
}

/// TD-SEC-82 (SEC-027, D-5313): a code already checked right once (as by the Confirm link) is
/// ended, so a press with it charges nothing; a fresh code charges once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_82_each_press_needs_its_own_fresh_code(pool: PgPool) {
    let p = picked(&pool).await;
    shared(&pool, &p).await;
    let w = world(&pool, p.owner);
    text_me(&w, &p).await;
    let used = w.verify.check_code(OWN_PHONE, CODE).await;
    assert!(used.expect("checked"), "the link's check used it");
    let reused = press(&w, &p, Some(CODE)).await;
    refused(&pool, (&w, &p), &reused, CodeCheck::Ended).await;
    text_me(&w, &p).await;
    let right = press(&w, &p, Some(CODE)).await;
    assert!(!right.contains("\"declined\""), "{right}");
    assert_eq!(w.confirms(), 1, "charged once");
    let (_, _, terms) = stored(&pool, &p).await;
    assert_eq!(terms, p.before.2 + 1, "{right}");
}

/// TD-SEC-82 (SEC-027, D-5320): with the inbox not shared "Text me a code" sends none and shows
/// no code step, and a press needs no code.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_82_an_unshared_inbox_needs_no_code(pool: PgPool) {
    let p = picked(&pool).await;
    let w = world(&pool, p.owner);
    let (status, shown) = text_me(&w, &p).await;
    assert_eq!(status, 200, "{shown}");
    assert!(!shown.contains(CODE_STEP));
    assert!(w.sent().is_empty(), "no code sent");
    let pressed = press(&w, &p, None).await;
    assert!(!pressed.contains("\"declined\""), "{pressed}");
    assert_eq!(w.confirms(), 1, "charged with no code");
}

/// TD-SEC-82 (SEC-027, BIZ-038): with the inbox shared the owner's Quote and Hold links still act
/// by the link alone; only Confirm asks for a code.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_82_quote_and_hold_links_act_by_the_link_alone(pool: PgPool) {
    let p = picked(&pool).await;
    shared(&pool, &p).await;
    let hold = Some(HoldAsk {
        by: askhava_businesses::api::Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    });
    let mut db = pool.acquire().await.expect("db");
    for (kind, object) in [(ActionKind::Quote, p.job), (ActionKind::Hold, p.pick)] {
        let target = ActionTarget {
            kind,
            object,
            member: p.owner,
            business: p.business,
        };
        let ends = now() + SignedDuration::from_hours(2);
        let link = mint_action_link(&mut db, target, hold, (now(), ends)).await;
        let link = link.expect("minted");
        let token = link.expose_secret().rsplit('/').next().expect("token");
        let used = use_action_link(&mut db, (kind, object), token, hold, now()).await;
        assert!(used.is_ok(), "{kind:?} still acts");
        assert!(!fresh_code_needed(kind, false, true));
    }
    assert!(fresh_code_needed(ActionKind::Confirm, true, true));
}

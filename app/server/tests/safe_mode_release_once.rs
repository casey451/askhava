//! TD-OPS-49 (OPS-001, OPS-024, PICK-022, FEE-021; T707, D-2290, D-3144, D-4326, D-4419; moved
//! from TD-OPS-40): T408's held release, driven through the handler `register_held_release`
//! registers and the Stripe webhook route, over the real Stripe client talking to a fake Stripe
//! on this host. A held paid pick whose job was cancelled while safe mode lasted releases nothing
//! and its charge gets one late-success refund; a released held pick releases nothing more on a
//! second verified event or a rerun; a pick released before safe mode began is untouched and
//! queues no held-release task. Safe mode moves only through a critical alert and
//! `ops::api::check_safe_mode`. Every row comes through a module API; fake data only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, StripePayments, TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, Lease, MIGRATOR, ManualClock, Outcome, Registry, Settings,
    WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, current, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{
    Actor, NewAlert, Severity, acknowledge_alert, check_safe_mode, raise_alert, record_push,
};
use askhava_payments::api::{
    BusinessId, ChargeOutcome, IntentStripe, LATE_KIND, PickNow, Press, STRIPE_WEBHOOK_PATH,
    StripeAccount, StripeWebhook, confirm_and_pay, ensure_customer, stripe_webhook_routes,
};
use askhava_picks::api::{BUSINESS_NOTICE_KIND, PagePick, is_released, page_pick};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::release_hold::{HELD_RELEASE_KIND, register_held_release};
use askhava_server::release_sink::{PickReleaser, StripeHooks};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::{Json, Router, routing::get};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const SECRET: &str = "whsec_fake_t707_signing_secret";
const INTENT: &str = "pi_fake707paid";
/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x58), &key(0x68), &url).expect("vault")
}

/// A masked pool of one fake number.
fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = "APP_ENV=dev DATABASE_URL=postgres://fake@localhost/fake \
        TWILIO_ACCOUNT_SID=AC00000000000000000000000000000000 TWILIO_AUTH_TOKEN=fake-auth-token \
        TWILIO_VERIFY_SERVICE_SID=VA00000000000000000000000000000001 MASKED_POOL_SIZE=1 \
        MASKED_NUMBERS=+15005550100"
        .split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (format!("ASKHAVA_{k}"), v.to_owned()))
        .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let twilio = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    twilio.pool().clone()
}

/// Her fake contact in the vault and her request naming `job`.
async fn her_request(db: &mut PgConnection, job: Uuid) {
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from("+19285550191")),
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        ..NewContact::default()
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my AC is out",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::from_u128(0x707e51),
        contact,
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &text, IntakeSettings::default()).await;
    let code = job.simple().to_string();
    let answer = Answer {
        conversation: handled.expect("request").conversation.expect("open"),
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x707e52), now()),
    };
    record_answer(db, &answer).await.expect("job named");
}

/// A business with its owner, a quoted job (Quotes ready), her request and her page pick:
/// `(business, job, pick)`.
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now()).await;
    let business = business.expect("business");
    let owner = add_owner(&mut db, business, Uuid::new_v4(), now()).await;
    owner.expect("owner");
    let job = create(&mut db, now(), false, Actor::System).await;
    let job = job.expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, now(), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(
        &mut db,
        map,
        job,
        &card_job(),
        &[card_business(business)],
        now(),
    )
    .await;
    assert_eq!(sent.expect("cards").len(), 1);
    let saved = submit_quote(&mut db, (job, business), &quote(), now()).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    her_request(&mut db, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    (business, job, pick)
}

fn card_job() -> CardJob {
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
    CardJob {
        towns: vec![town],
        candidates: vec![job],
        text: String::new(),
    }
}

fn card_business(id: Uuid) -> CardBusiness {
    CardBusiness {
        id,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

fn quote() -> Quote {
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
    parse_quote(form, &rules).expect("quote")
}

/// The intent as the fake Stripe reports it: succeeded, $25, test mode, for `pick`.
fn intent(pick: Uuid, id: &str) -> PaymentIntent {
    PaymentIntent {
        id: id.to_owned(),
        status: "succeeded".to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_fake707".to_owned(),
        metadata: IntentMetadata {
            pick_id: pick.to_string(),
        },
        livemode: Some(false),
        latest_charge: Some("ch_fake707paid".to_owned()),
    }
}

/// The press's fake Stripe (FEE-016): the customer, the intent and its confirm.
struct PressStripe(Uuid);

impl PaymentsAdapter for PressStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake707".to_owned())
    }
    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }
    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(intent(self.0, id))
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl IntentStripe for PressStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        Ok(INTENT.to_owned())
    }
    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        id: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        Ok(intent(self.0, id))
    }
}

/// A paid pick: the business, job and page pick, then the $25 press.
async fn paid(pool: &PgPool) -> (Uuid, Uuid) {
    let (business, job, pick) = picked(pool).await;
    let stripe = PressStripe(pick);
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake707",
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let charged = confirm_and_pay(pool, &stripe, &secret, &press, now()).await;
    assert!(matches!(charged, Ok(ChargeOutcome::Succeeded(_))));
    (job, pick)
}

/// The webhook state over the real Stripe client, pointed at a fake Stripe on this host that
/// reports the intent succeeded for `pick`.
async fn hooks(pool: &PgPool, pick: Uuid) -> StripeHooks {
    let fetched = move || async move { Json(json!(intent_value(pick))) };
    let app = Router::new().route("/stripe/v1/payment_intents/{id}", get(fetched));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}/stripe", listener.local_addr().expect("addr"));
    tokio::spawn(async move { axum::serve(listener, app).await });
    let key = SecretString::from("sk_test_fake707".to_owned());
    let none: [(String, String); 0] = [];
    let clock = Arc::new(ManualClock::new(now()));
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: StripePayments::new(&base, key).expect("client"),
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake707".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    })
}

fn intent_value(pick: Uuid) -> Value {
    let found = intent(pick, INTENT);
    json!({
        "id": found.id, "status": found.status, "amount": found.amount,
        "currency": found.currency, "customer": found.customer, "livemode": false,
        "metadata": {"pick_id": pick}, "latest_charge": found.latest_charge,
    })
}

/// One signed `payment_intent.succeeded` delivery `event` for `pick` from the stand-in's host.
async fn deliver(hooks: StripeHooks, pick: Uuid, event: &str) -> StatusCode {
    let body = format!(
        r#"{{"id":"{event}","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{INTENT}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
    );
    let at = now().as_second();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(SECRET.as_bytes()).expect("key");
    mac.update(format!("{at}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut request = Request::post(STRIPE_WEBHOOK_PATH)
        .header("stripe-signature", format!("t={at},v1={sig}"))
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::from(([127, 0, 0, 1], 443));
    request.extensions_mut().insert(ConnectInfo(peer));
    let reply = stripe_webhook_routes(hooks).oneshot(request).await;
    reply.expect("answer").status()
}

/// Claims every due task: the owner and, for `pick`, its held-release runs, business notices and
/// late-success refunds.
async fn drained(pool: &PgPool, pick: Uuid) -> (WorkerId, Vec<Claimed>, usize, usize) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let all = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    let (all, name) = (all.expect("claim"), pick.to_string());
    let of = |kind: &str| -> Vec<Claimed> {
        let named = |t: &&Claimed| t.kind == kind && t.payload["pick"] == name;
        all.iter().filter(named).cloned().collect()
    };
    let (notices, late) = (of(BUSINESS_NOTICE_KIND).len(), of(LATE_KIND).len());
    (owner, of(HELD_RELEASE_KIND), notices, late)
}

/// Runs `task` through the registered held-release handler as `owner`.
async fn run(
    pool: &PgPool,
    hooks: &StripeHooks,
    owner: WorkerId,
    task: &Claimed,
) -> Option<Outcome> {
    let mut registry = Registry::new();
    register_held_release(&mut registry, Some(hooks.clone()));
    let clock = ManualClock::new(now());
    let done = registry.run(pool, &clock, owner, task.clone(), Lease::WORKER);
    done.await.ok()
}

async fn released(pool: &PgPool, pick: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    is_released(&mut db, pick).await.expect("release row")
}

/// Safe mode on through a critical alert pushed 25 hours ago; returns the alert.
async fn safe_mode_starts(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let alert = NewAlert {
        kind: "ops.t707_fake_critical",
        severity: Severity::Critical,
        subject: None,
    };
    let alert = raise_alert(&mut db, &alert).await.expect("alert");
    let pushed = now() - SignedDuration::from_hours(25);
    record_push(&mut db, alert, pushed).await.expect("push");
    assert!(check_safe_mode(&mut db, now()).await.expect("on"));
    alert
}

async fn safe_mode_ends(pool: &PgPool, alert: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let at = now();
    assert!(acknowledge_alert(&mut db, alert, at).await.expect("ack"));
    assert!(!check_safe_mode(&mut db, at).await.expect("off"));
}

/// TD-OPS-49 (OPS-024, FEE-017, FEE-021, D-2290, D-4789): a held paid pick whose job was
/// cancelled while safe mode lasted releases nothing when it ends; its charge gets exactly one
/// late-success refund, however often the run is tried, and a second verified event before that
/// refund runs releases nothing either (D-4791).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_49_a_held_pick_whose_job_was_cancelled_is_refunded_once(pool: PgPool) {
    let (job, pick) = paid(&pool).await;
    let alert = safe_mode_starts(&pool).await;
    let hooks = hooks(&pool, pick).await;
    assert_eq!(
        deliver(hooks.clone(), pick, "evt_fake707a").await,
        StatusCode::OK
    );
    let mut db = pool.acquire().await.expect("db");
    let status = current(&mut db, job).await.expect("job").status;
    let cancel = (status, JobStatus::Cancelled);
    let moved = move_to(&mut db, job, cancel, now(), Actor::System).await;
    assert!(moved.expect("cancel").applied().is_some(), "cancelled");
    drop(db);
    safe_mode_ends(&pool, alert).await;
    let (owner, held, _, _) = drained(&pool, pick).await;
    assert_eq!(held.len(), 1);
    assert_eq!(
        run(&pool, &hooks, owner, &held[0]).await,
        Some(Outcome::Done)
    );
    run(&pool, &hooks, owner, &held[0]).await;
    assert!(!released(&pool, pick).await);
    let again = deliver(hooks.clone(), pick, "evt_fake707f").await;
    assert_eq!(again, StatusCode::OK);
    assert!(!released(&pool, pick).await, "redelivery released nothing");
    let (_, more, notices, late) = drained(&pool, pick).await;
    assert_eq!(
        (more.len(), notices, late),
        (0, 0, 1),
        "one late-success refund"
    );
}

/// TD-OPS-49 (PICK-022, OPS-024): a held pick released once safe mode ends releases nothing
/// more on a second verified event or a rerun of the run that released it.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_49_a_released_held_pick_releases_once(pool: PgPool) {
    let (_, pick) = paid(&pool).await;
    let alert = safe_mode_starts(&pool).await;
    let hooks = hooks(&pool, pick).await;
    assert_eq!(
        deliver(hooks.clone(), pick, "evt_fake707b").await,
        StatusCode::OK
    );
    safe_mode_ends(&pool, alert).await;
    let (owner, held, _, _) = drained(&pool, pick).await;
    assert_eq!(held.len(), 1);
    assert_eq!(
        run(&pool, &hooks, owner, &held[0]).await,
        Some(Outcome::Done)
    );
    assert!(released(&pool, pick).await);
    assert_eq!(
        deliver(hooks.clone(), pick, "evt_fake707c").await,
        StatusCode::OK
    );
    run(&pool, &hooks, owner, &held[0]).await;
    let (_, more, notices, late) = drained(&pool, pick).await;
    assert_eq!((more.len(), notices, late), (0, 1, 0), "one notice");
}

/// TD-OPS-49 (OPS-001, PICK-022): a pick released before safe mode began is untouched by the
/// gate: a later verified event in safe mode queues no held-release task and releases no more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_49_a_pick_released_before_safe_mode_is_untouched(pool: PgPool) {
    let (_, pick) = paid(&pool).await;
    let hooks = hooks(&pool, pick).await;
    assert_eq!(
        deliver(hooks.clone(), pick, "evt_fake707d").await,
        StatusCode::OK
    );
    assert!(released(&pool, pick).await);
    safe_mode_starts(&pool).await;
    assert_eq!(
        deliver(hooks.clone(), pick, "evt_fake707e").await,
        StatusCode::OK
    );
    let (_, held, notices, late) = drained(&pool, pick).await;
    assert_eq!(
        (held.len(), notices, late),
        (0, 1, 0),
        "no held task, one notice"
    );
}

/// TD-OPS-49 (PICK-022, FEE-017, FEE-021, D-4792): a pick released before its job was cancelled
/// (`Settled::Was` with its release row) gets no late-success refund and no second release when
/// another verified event arrives, as a retry after a failed release would.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_49_a_released_pick_of_a_cancelled_job_is_not_refunded(pool: PgPool) {
    let (job, pick) = paid(&pool).await;
    let hooks = hooks(&pool, pick).await;
    let first = deliver(hooks.clone(), pick, "evt_fake707g").await;
    assert_eq!(first, StatusCode::OK);
    assert!(released(&pool, pick).await);
    let mut db = pool.acquire().await.expect("db");
    let status = current(&mut db, job).await.expect("job").status;
    let moved = move_to(
        &mut db,
        job,
        (status, JobStatus::Cancelled),
        now(),
        Actor::System,
    )
    .await;
    assert!(moved.expect("cancel").applied().is_some(), "cancelled");
    drop(db);
    let again = deliver(hooks.clone(), pick, "evt_fake707h").await;
    assert_eq!(again, StatusCode::OK);
    let (_, held, notices, late) = drained(&pool, pick).await;
    assert_eq!(
        (held.len(), notices, late),
        (0, 1, 0),
        "no refund, one notice"
    );
}

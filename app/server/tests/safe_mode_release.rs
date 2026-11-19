//! TD-OPS-40 (OPS-001, OPS-024; T408, D-3144, D-4026, D-4326, D-4419): with safe mode on, a
//! verified success settles and releases nothing and queues one held-release task (a second
//! delivery none); a failed safe-mode read answers 500 and queues nothing; while safe mode lasts
//! the task enqueues its next run 15 minutes on (a rerun nothing, D-3263); once it ends the task
//! releases once; an intent Stripe does not report succeeded is never released; a $0 pick the
//! same way. Safe mode is set only through the stored `ops.safe_mode` row (D-4326); every other
//! row comes through a module API, as in `release_recovery.rs`. Fake data only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_calls::api::job_had_line;
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, EntryKind, IntentStripe, LedgerEntry, NewEntry, PickNow, Press,
    STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook, confirm_and_pay, ensure_customer,
    ledger_append, no_fee_release, stripe_webhook_routes,
};
use askhava_picks::api::{BUSINESS_NOTICE_KIND, PagePick, Released, is_released, page_pick};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::release_hold::{HELD_RELEASE_KIND, HOLD_EVERY, run_held_release};
use askhava_server::release_sink::PickReleaser;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const SECRET: &str = "whsec_fake_t408_signing_secret";
const INTENT: &str = "pi_fake408paid";
/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x59), &key(0x69), &url).expect("vault")
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
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// Turns the stored safe-mode flag on or off (D-4326).
async fn safe_mode(pool: &PgPool, on: bool) {
    sqlx::query(r#"UPDATE ops.safe_mode SET "on" = $1, since = CASE WHEN $1 THEN now() END"#)
        .bind(on)
        .execute(pool)
        .await
        .expect("flag");
}

/// The homeowner's fake contact in the vault and her request naming `job`.
async fn her_request(pool: &PgPool, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from("+19285550190")),
        email: None,
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let inbound = Inbound {
        words: "my sink leaks",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::from_u128(0x408e01),
        contact,
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default())
        .await
        .expect("request");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation: handled.conversation.expect("open request"),
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x408e10), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
}

/// An active, verified R-39 business serving Lake Havasu City that ticked the job type.
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

fn card_job() -> CardJob {
    let job = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    CardJob {
        towns: vec![OfferTown {
            code: "lake_havasu_city".to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![job],
        text: String::new(),
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

/// A business and its owner, a job quoted by it (Quotes ready), her request naming it and her page pick:
/// `(business, job, pick)`.
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("business");
    let owner = add_owner(&mut db, business, Uuid::new_v4(), now()).await;
    owner.expect("owner");
    let job = create(&mut db, now(), false, Actor::System)
        .await
        .expect("job");
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
    let pool_of = [card_business(business)];
    let sent = send_cards(&mut db, map, job, &card_job(), &pool_of, now()).await;
    assert_eq!(sent.expect("cards").len(), 1);
    let saved = submit_quote(&mut db, (job, business), &quote(), now()).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    drop(db);
    her_request(pool, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    match picked.expect("pick") {
        PagePick::Picked(pick) => (business, job, pick),
        PagePick::Same(_) | PagePick::Taken => panic!("no new pick"),
    }
}

/// The $25 Confirm-and-pay press through `payments` with the fake Stripe (FEE-016).
async fn paid(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) {
    let status = "succeeded";
    let stripe = FakeStripe { pick, status };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake408",
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let charged = confirm_and_pay(pool, &stripe, &secret, &press, now()).await;
    assert!(matches!(
        charged.expect("charge"),
        ChargeOutcome::Succeeded(_)
    ));
}

/// Every task queued since the last call, claimed once.
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    claim(pool, owner, later, SignedDuration::from_secs(60), 1000)
        .await
        .expect("claim")
}

/// How many held-release and business-notice tasks for `pick` were queued since the last claim.
async fn counts(pool: &PgPool, pick: Uuid) -> (usize, usize) {
    let queued = tasks(pool).await;
    let held = of(&queued, HELD_RELEASE_KIND, pick).len();
    (held, of(&queued, BUSINESS_NOTICE_KIND, pick).len())
}

/// The tasks of `kind` for `pick`.
fn of(tasks: &[Claimed], kind: &str, pick: Uuid) -> Vec<Claimed> {
    let pick = pick.to_string();
    let named = |t: &&Claimed| t.kind == kind && t.payload["pick"].as_str() == Some(pick.as_str());
    tasks.iter().filter(named).cloned().collect()
}

/// Whether `pick` has its release row, and whether its job had a masked line.
async fn released(pool: &PgPool, (job, pick): (Uuid, Uuid)) -> (bool, bool) {
    let mut db = pool.acquire().await.expect("db");
    let row = is_released(&mut db, pick).await.expect("release row");
    (row, job_had_line(&mut db, job).await.expect("line"))
}

struct FakeStripe {
    pick: Uuid,
    status: &'static str,
}

impl FakeStripe {
    fn intent(&self, id: &str, status: &str) -> PaymentIntent {
        PaymentIntent {
            id: id.to_owned(),
            status: status.to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_fake408".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: Some(id.replacen("pi_", "ch_", 1)),
        }
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake408".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(self.intent(id, self.status))
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
        Ok(INTENT.to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        Ok(self.intent(intent, "succeeded"))
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, PickReleaser>>;

/// The webhook state over the real sink, with Stripe reporting the intent as `status`.
fn hooks(pool: &PgPool, pick: Uuid, status: &'static str) -> Hooks {
    let none: [(String, String); 0] = [];
    let clock = Arc::new(ManualClock::new(now()));
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe { pick, status },
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake408".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    })
}

/// One signed `payment_intent.succeeded` delivery `event` for `pick` from the stand-in's host.
async fn deliver(hooks: Hooks, pick: Uuid, event: &str) -> StatusCode {
    let body = format!(
        r#"{{"id":"{event}","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{INTENT}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
    );
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(SECRET.as_bytes()).expect("key");
    mac.update(format!("{NOW}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut request = Request::post(STRIPE_WEBHOOK_PATH)
        .header("stripe-signature", format!("t={NOW},v1={sig}"))
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::from(([127, 0, 0, 1], 443));
    request.extensions_mut().insert(ConnectInfo(peer));
    let reply = stripe_webhook_routes(hooks)
        .oneshot(request)
        .await
        .expect("answer");
    reply.status()
}

/// The one held-release task queued since the last claim for `pick`, checked due at `due`.
async fn one_held(pool: &PgPool, pick: Uuid, due: Timestamp) -> Claimed {
    let queued = tasks(pool).await;
    assert!(
        of(&queued, BUSINESS_NOTICE_KIND, pick).is_empty(),
        "no notice"
    );
    let mut held = of(&queued, HELD_RELEASE_KIND, pick);
    assert_eq!(held.len(), 1, "one held-release task");
    let task = held.remove(0);
    assert_eq!(
        task.payload["run_at"].as_str(),
        Some(due.to_string().as_str())
    );
    task
}

/// Safe mode off, the held task runs: one release row and one business notice.
async fn ends_and_releases_once(
    pool: &PgPool,
    hooks: &Hooks,
    (job, pick): (Uuid, Uuid),
    due: Timestamp,
) {
    let held = one_held(pool, pick, due).await;
    safe_mode(pool, false).await;
    run_held_release(pool, hooks, &held.payload)
        .await
        .expect("released");
    assert!(
        released(pool, (job, pick)).await.0,
        "released once safe mode ends"
    );
    assert_eq!(
        counts(pool, pick).await,
        (0, 1),
        "one notice, no further run"
    );
}

/// TD-OPS-40 (OPS-001, OPS-024): with safe mode on a verified success releases nothing and
/// queues one held task, a second delivery none; each run while safe mode lasts enqueues the next
/// 15 minutes after its due time, a rerun of the same run nothing; once it ends, one release.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_40_a_success_in_safe_mode_is_held_then_released_once_it_ends(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    paid(&pool, (business, job, pick)).await;
    safe_mode(&pool, true).await;
    let hooks = hooks(&pool, pick, "succeeded");
    for event in ["evt_fake408a", "evt_fake408b"] {
        assert_eq!(deliver(hooks.clone(), pick, event).await, StatusCode::OK);
    }
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    let first = one_held(&pool, pick, now() + HOLD_EVERY).await;
    run_held_release(&pool, &hooks, &first.payload)
        .await
        .expect("held again");
    run_held_release(&pool, &hooks, &first.payload)
        .await
        .expect("rerun");
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    let second = one_held(&pool, pick, now() + HOLD_EVERY + HOLD_EVERY).await;
    run_held_release(&pool, &hooks, &second.payload)
        .await
        .expect("held again");
    let due = now() + HOLD_EVERY + HOLD_EVERY + HOLD_EVERY;
    ends_and_releases_once(&pool, &hooks, (job, pick), due).await;
    assert!(
        released(&pool, (job, pick)).await.1,
        "the masked line opens"
    );
}

/// TD-OPS-40 (OPS-001, SEC-030): a failed safe-mode read fails closed: the route answers 500,
/// nothing is released and nothing is queued.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_40_a_failed_safe_mode_read_releases_and_queues_nothing(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    paid(&pool, (business, job, pick)).await;
    sqlx::query("ALTER TABLE ops.safe_mode RENAME TO safe_mode_unreadable")
        .execute(&pool)
        .await
        .expect("rename");
    let hooks = hooks(&pool, pick, "succeeded");
    let answer = deliver(hooks, pick, "evt_fake408c").await;
    assert_eq!(answer, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    assert_eq!(counts(&pool, pick).await, (0, 0));
}

/// TD-OPS-40 (OPS-024, SEC-030, PICK-022): a held paid pick whose intent Stripe no longer reports
/// succeeded is never released by the task.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_40_a_held_pick_stripe_does_not_report_paid_is_not_released(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    paid(&pool, (business, job, pick)).await;
    safe_mode(&pool, true).await;
    let answer = deliver(hooks(&pool, pick, "succeeded"), pick, "evt_fake408d").await;
    assert_eq!(answer, StatusCode::OK);
    let held = one_held(&pool, pick, now() + HOLD_EVERY).await;
    safe_mode(&pool, false).await;
    let unpaid = hooks(&pool, pick, "requires_payment_method");
    run_held_release(&pool, &unpaid, &held.payload)
        .await
        .expect("refused, finished");
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    assert_eq!(counts(&pool, pick).await, (0, 0));
}

/// TD-OPS-40 (OPS-024, FEE-010): a $0 pick through the same sink is held while safe mode is on
/// and released once by the task when it ends.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_40_a_zero_fee_pick_is_held_and_released_the_same_way(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t408.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    drop(db);
    safe_mode(&pool, true).await;
    let hooks = hooks(&pool, pick, "succeeded");
    let held = hooks
        .sink
        .settle_and_release(cleared.expect("cleared"))
        .await;
    assert!(matches!(held.expect("held"), Released::MovedOn));
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    ends_and_releases_once(&pool, &hooks, (job, pick), now() + HOLD_EVERY).await;
}

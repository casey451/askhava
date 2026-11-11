//! TD-PICK-49 and TD-PICK-50 (PICK-022, PICK-023, PICK-024, PRIV-005, BIZ-014, CALL-001; D-1688,
//! D-1739): the production release through `server`'s `PickReleaser` with real proofs (only
//! `server` may mint, `tests/integration/tests/architecture.rs`): Stripe's webhook route with a
//! signed fake event and a fake Stripe client, `payments::api::no_fee_release` with a $0 decision
//! written through `payments::api::ledger_append`, a vault that cannot read and then can, a new
//! pool, vault and sink for each restart, and `picks.line_retry` once the masked pool recovers.
//!
//! `server` code names no module's schema (TD-SEC-10), so every row comes through a module API
//! (D-2013): the business and its owner (`businesses`), the job and its Open move (`jobs`), the
//! card and the quote (`quotes::api::send_cards`, `submit_quote`, which moves it to Quotes ready),
//! the page pick (`picks::api::page_pick`), her vault contact and request (`privacy`, `intake`),
//! the Stripe customer, PaymentIntent and charge (`payments::api::ensure_customer`,
//! `confirm_and_pay`, with a fake Stripe) and the $0 decisions (`ledger_append`). Fake data only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_calls::api::{CloseReason, Connected, assign_number, close_line, job_had_line};
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, job_number, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, EntryKind, IntentStripe, LedgerEntry, NewEntry, PickNow, Press,
    Releasable, STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook, confirm_and_pay,
    ensure_customer, ledger_append, money_view, no_fee_release, pick_fee, receipt_for,
    stripe_webhook_routes,
};
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, LINE_RETRY_KIND, PagePick, Released, ViewAnswer, page_pick,
    released_view, retry_line, settle,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::release_sink::PickReleaser;
use askhava_server::session_minter;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tower::ServiceExt;

const SECRET: &str = "whsec_fake_t254_signing_secret";
const INTENT: &str = "pi_fake254paid";
const NAME: &str = "Dana Testperson";
const STREET: &str = "100 Example Way, Lake Havasu City";
/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn vault_with(seal: u8) -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(seal), &key(0x69), &url).expect("vault")
}

/// The vault the contact was sealed under, and one whose sealing key opens nothing of it.
fn vault() -> Vault {
    vault_with(0x59)
}

fn broken_vault() -> Vault {
    vault_with(0x5a)
}

/// A masked pool of one fake number.
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
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn sink(pool: &PgPool, vault: Vault) -> PickReleaser {
    PickReleaser::new(
        pool.clone(),
        vault,
        masked(),
        Arc::new(ManualClock::new(now())),
    )
}

/// A new pool over the same database: the process after a restart.
async fn restarted(pool: &PgPool) -> PgPool {
    let options = pool.connect_options().as_ref().clone();
    PgPoolOptions::new()
        .connect_with(options)
        .await
        .expect("pool")
}

/// The homeowner's contact in the vault and her request naming each of `jobs`, through
/// `privacy` and `intake`; returns the contact.
async fn her_request(pool: &PgPool, jobs: &[Uuid]) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from(NAME)),
        phone: Some(SecretString::from("+19285550190")),
        email: None,
        address: Some(SecretString::from(STREET)),
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
        inbox_id: Uuid::from_u128(0x254e01),
        contact,
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default())
        .await
        .expect("request");
    let conversation = handled.conversation.expect("open request");
    for (n, job) in jobs.iter().enumerate() {
        let code = job.simple().to_string();
        let question = format!("job.plumbing.p{n}");
        let answer = Answer {
            conversation,
            question: &question,
            options: Some(&[(code.as_str(), "fakejob")]),
            words: "fakejob",
            reply: (Uuid::from_u128(0x254e10 + n as u128), now()),
        };
        record_answer(&mut db, &answer).await.expect("job named");
    }
    contact
}

/// A business through `businesses` (pending; matching reads the in-memory record below).
async fn new_business(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("business")
}

/// The business's owner through `businesses`; returns the owner's member ID.
async fn owner(pool: &PgPool, business: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let mailbox = Uuid::new_v4();
    add_owner(&mut db, business, mailbox, now())
        .await
        .expect("owner")
}

/// An AC repair job in Lake Havasu City, as matching reads it.
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

/// A job, opened, carded to `quoters` and quoted by each in turn, so it is in Quotes ready.
async fn quoted_job(pool: &PgPool, quoters: &[Uuid]) -> Uuid {
    quoted_job_of(pool, quoters, false).await
}

/// [`quoted_job`], an emergency job when `emergency` (INT-029).
async fn quoted_job_of(pool: &PgPool, quoters: &[Uuid], emergency: bool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), emergency, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, now(), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let pool_of: Vec<CardBusiness> = quoters.iter().map(|b| card_business(*b)).collect();
    let sent = send_cards(&mut db, map(), job, &card_job(), &pool_of, now()).await;
    assert_eq!(sent.expect("cards").len(), quoters.len());
    for business in quoters {
        let saved = submit_quote(&mut db, (job, *business), &quote(), now()).await;
        assert_eq!(saved.expect("quote"), Submitted::Saved);
    }
    job
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("the shipped license map")
}

/// The homeowner's page pick of quote `n` (1-based) on `job`; returns the waiting pick.
async fn page_picked(pool: &PgPool, job: Uuid, n: usize) -> Uuid {
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), n, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    match picked.expect("pick") {
        PagePick::Picked(id) => id,
        PagePick::Same(_) | PagePick::Taken => panic!("no new pick"),
    }
}

/// One business, its owner, a quoted job, her request naming it and her page pick:
/// `(business, owner, job, pick)`.
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let business = new_business(pool).await;
    let person = owner(pool, business).await;
    let job = quoted_job(pool, &[business]).await;
    her_request(pool, &[job]).await;
    let pick = page_picked(pool, job, 1).await;
    (business, person, job, pick)
}

/// The Confirm-and-pay press of $25 (the fee `picks` fixed at the page pick of the $325 firm
/// quote, D-1871) through `payments`, with the fake Stripe, so the pick has its
/// PaymentIntent and one charge attempt (FEE-016, FEE-047).
async fn paid(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) {
    let stripe = FakeStripe { pick };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake254",
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

/// The $0 free-first-win decision for `business` on `job` in the ledger (FEE-010, FEE-012).
async fn zero_fee(pool: &PgPool, (business, job): (Uuid, Uuid)) {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t254.free.{job}.{business}");
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
}

async fn cleared_free(pool: &PgPool, pick: Uuid, (business, job): (Uuid, Uuid)) -> Releasable {
    let mut db = pool.acquire().await.expect("db");
    no_fee_release(&mut db, pick, (business, job))
        .await
        .expect("cleared")
}

/// What the owner sees now through the released view; the first name and street, or `None`.
async fn seen(
    pool: &PgPool,
    (person, business): (Uuid, Uuid),
    pick: Uuid,
) -> Option<(String, String)> {
    let proof = session_minter().business(person, business);
    match released_view(pool, &vault(), &proof, pick)
        .await
        .expect("view")
    {
        ViewAnswer::Shown(view) => {
            let first = view.first_name().expect("name").expose_secret().to_owned();
            let street = view.address().expect("address").expose_secret().to_owned();
            Some((first, street))
        }
        ViewAnswer::Removed | ViewAnswer::NotFound => None,
    }
}

/// Every unclaimed task due by 2030, claimed once (a later call gets only tasks queued since).
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    claim(pool, owner, later, SignedDuration::from_secs(60), 1000)
        .await
        .expect("claim")
}

fn count(tasks: &[Claimed], kind: &str, pick: Uuid) -> usize {
    let pick = pick.to_string();
    let named = |t: &&Claimed| t.kind == kind && t.payload["pick"].as_str() == Some(pick.as_str());
    tasks.iter().filter(named).count()
}

/// No queued task carries a homeowner detail in plain text (PRIV-002).
fn no_plaintext(tasks: &[Claimed]) {
    for task in tasks {
        let text = task.payload.to_string();
        assert!(
            !text.contains("Dana") && !text.contains("Example Way"),
            "{}",
            task.kind
        );
    }
}

async fn had_line(pool: &PgPool, job: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    job_had_line(&mut db, job).await.expect("line")
}

/// The ledger holds no charge: the release writes no fee (FEE-032).
async fn no_fee_written(pool: &PgPool) {
    let span = (
        Timestamp::UNIX_EPOCH,
        Timestamp::from_second(1_900_000_000).expect("t"),
    );
    let view = money_view(pool, span).await.expect("money");
    assert_eq!(view.charges, 0);
}

struct FakeStripe {
    pick: Uuid,
}

impl FakeStripe {
    fn succeeded(&self, id: &str) -> PaymentIntent {
        PaymentIntent {
            id: id.to_owned(),
            status: "succeeded".to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_fake254".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick.to_string(),
            },
            livemode: Some(false),
        }
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake254".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(self.succeeded(id))
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
        Ok(self.succeeded(intent))
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, PickReleaser>>;

fn hooks(pool: &PgPool, vault: Vault, pick: Uuid) -> Hooks {
    let none: [(String, String); 0] = [];
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe { pick },
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake254".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(now())),
        sink: sink(pool, vault),
    })
}

/// One signed `payment_intent.succeeded` delivery for `pick` from the stand-in's host.
async fn deliver(hooks: Hooks, pick: Uuid) -> StatusCode {
    let body = format!(
        r#"{{"id":"evt_fake254","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{INTENT}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
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

async fn number(pool: &PgPool, job: Uuid) -> i64 {
    let mut db = pool.acquire().await.expect("db");
    job_number(&mut db, job).await.expect("number")
}

/// TD-PICK-50 (PICK-022, CALL-001): a verified success moves the awaiting-confirm pick to paid,
/// mints for her contact and releases; a duplicate delivery and two simultaneous ones give one
/// release, one notice, one masked line and no fee.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_50_a_verified_success_settles_paid_and_releases_once(pool: PgPool) {
    let (business, person, job, pick) = picked(&pool).await;
    paid(&pool, (business, job, pick)).await;
    let hooks = hooks(&pool, vault(), pick);
    assert_eq!(deliver(Arc::clone(&hooks), pick).await, StatusCode::OK);
    assert_eq!(deliver(Arc::clone(&hooks), pick).await, StatusCode::OK);
    let (a, b) = tokio::join!(
        deliver(Arc::clone(&hooks), pick),
        deliver(Arc::clone(&hooks), pick)
    );
    assert_eq!((a, b), (StatusCode::OK, StatusCode::OK));
    let shown = seen(&pool, (person, business), pick).await;
    assert_eq!(shown, Some(("Dana".to_owned(), STREET.to_owned())));
    assert!(had_line(&pool, job).await);
    let n = number(&pool, job).await;
    let mut db = pool.acquire().await.expect("db");
    let closed = close_line(&mut db, (job, n), CloseReason::Expired, now()).await;
    assert!(closed.expect("close").is_some(), "one open line");
    let again = close_line(&mut db, (job, n), CloseReason::Expired, now()).await;
    assert!(again.expect("close").is_none(), "and only one");
    no_fee_written(&pool).await;
    let all = tasks(&pool).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, pick), 1);
    assert_eq!(count(&all, LINE_RETRY_KIND, pick), 0);
    no_plaintext(&all);
}

/// TD-PICK-50 (PICK-022, FEE-010): a recorded $0 decision moves the pick to free and releases it
/// through the same sink; a pick that left awaiting confirm (replaced, so cancelled) is not moved
/// and nothing is released.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_50_a_zero_fee_releases_free_and_a_cancelled_pick_nothing(pool: PgPool) {
    let (business, one, two) = (
        new_business(&pool).await,
        new_business(&pool).await,
        new_business(&pool).await,
    );
    let person = owner(&pool, business).await;
    let job = quoted_job(&pool, &[business]).await;
    let replaced_job = quoted_job(&pool, &[one, two]).await;
    her_request(&pool, &[job, replaced_job]).await;
    let pick = page_picked(&pool, job, 1).await;
    zero_fee(&pool, (business, job)).await;
    let gone = page_picked(&pool, replaced_job, 1).await;
    let mut db = pool.acquire().await.expect("db");
    let stored = pick_fee(&mut db, gone).await.expect("read").expect("fixed");
    drop(db);
    assert_eq!(stored.job, replaced_job);
    let first = stored.business.0;
    let first_person = owner(&pool, first).await;
    page_picked(&pool, replaced_job, 2).await;
    zero_fee(&pool, (first, replaced_job)).await;
    let sink = sink(&pool, vault());
    let free = sink.settle_and_release(cleared_free(&pool, pick, (business, job)).await);
    assert!(matches!(free.await.expect("release"), Released::Shown(_)));
    let moved = sink.settle_and_release(cleared_free(&pool, gone, (first, replaced_job)).await);
    assert!(matches!(moved.await.expect("release"), Released::MovedOn));
    assert!(seen(&pool, (person, business), pick).await.is_some());
    assert_eq!(seen(&pool, (first_person, first), gone).await, None);
    assert!(!had_line(&pool, replaced_job).await);
    let all = tasks(&pool).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, gone), 0);
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, pick), 1);
}

/// TD-PICK-50 (D-1739): a restart between the settle's commit and the claim leaves the pick free,
/// and the retried clearing, on a new pool, vault and sink, releases exactly once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_50_a_restart_after_the_settle_releases_once_on_the_retry(pool: PgPool) {
    let (business, person, job, pick) = picked(&pool).await;
    zero_fee(&pool, (business, job)).await;
    {
        // The settle commits, then the process stops before the claim: the clearing is lost.
        let cleared = cleared_free(&pool, pick, (business, job)).await;
        let mut tx = pool.begin().await.expect("tx");
        settle(&mut tx, &cleared).await.expect("settled");
        tx.commit().await.expect("commit");
    }
    let after = restarted(&pool).await;
    let sink = sink(&after, vault());
    let first = sink.settle_and_release(cleared_free(&after, pick, (business, job)).await);
    assert!(matches!(first.await.expect("release"), Released::Shown(_)));
    let second = sink.settle_and_release(cleared_free(&after, pick, (business, job)).await);
    assert!(matches!(second.await.expect("release"), Released::Already));
    assert!(seen(&after, (person, business), pick).await.is_some());
    let all = tasks(&after).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, pick), 1);
}

/// TD-PICK-50 (CALL-001, D-1688): a line that fails to open (the pool is held) queues one retry,
/// which opens it once the pool recovers, opens nothing for a job that already has a line, and
/// never reopens a line closed after 30 days.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_50_a_failed_line_opens_on_the_retry_and_never_reopens(pool: PgPool) {
    let (business, _, job, pick) = picked(&pool).await;
    zero_fee(&pool, (business, job)).await;
    let (holder, impatient) = held_masked(&pool).await;
    let cleared = cleared_free(&impatient, pick, (business, job)).await;
    let shown = sink(&impatient, vault()).settle_and_release(cleared).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
    assert!(!had_line(&pool, job).await);
    holder.rollback().await.expect("pool recovers");
    let all = tasks(&pool).await;
    assert_eq!(count(&all, LINE_RETRY_KIND, pick), 1);
    let retry = all
        .into_iter()
        .find(|t| t.kind == LINE_RETRY_KIND)
        .expect("retry");
    retry_line(pool.clone(), masked(), retry.clone(), now())
        .await
        .expect("opened");
    assert!(had_line(&pool, job).await);
    retry_line(pool.clone(), masked(), retry.clone(), now())
        .await
        .expect("nothing");
    let n = number(&pool, job).await;
    let mut db = pool.acquire().await.expect("db");
    let closed = close_line(&mut db, (job, n), CloseReason::Expired, now()).await;
    assert!(closed.expect("close").is_some(), "one line, opened once");
    let later = now() + SignedDuration::from_hours(24 * 31);
    retry_line(pool.clone(), masked(), retry, later)
        .await
        .expect("nothing again");
    let reopened = close_line(&mut db, (job, n), CloseReason::Expired, later).await;
    assert!(
        reopened.expect("close").is_none(),
        "a closed line is never reopened"
    );
}

/// A transaction holding the one masked number, and a pool whose lock waits give up after
/// 300 ms, so a line opened through it fails while the holder lives.
async fn held_masked(pool: &PgPool) -> (sqlx::Transaction<'static, sqlx::Postgres>, PgPool) {
    let options = pool.connect_options().as_ref().clone();
    let impatient = PgPoolOptions::new()
        .connect_with(options.options([("lock_timeout", "300ms")]))
        .await
        .expect("pool");
    let mut holder = pool.begin().await.expect("tx");
    let other = Connected {
        job: Uuid::new_v4(),
        homeowner: Uuid::new_v4(),
        business: Uuid::new_v4(),
    };
    assign_number(&mut holder, &masked(), other, now())
        .await
        .expect("held");
    (holder, impatient)
}

/// TD-PICK-50 (CALL-001, D-1688, D-2017): an emergency release that fails before its claim (the
/// vault cannot read) queues no line retry, so the one per-pick retry is not spent; the release
/// that follows, whose line fails to open, still queues it and the retry opens the line.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_50_an_error_before_the_claim_leaves_the_line_retry_unspent(pool: PgPool) {
    let business = new_business(&pool).await;
    let job = quoted_job_of(&pool, &[business], true).await;
    her_request(&pool, &[job]).await;
    let pick = page_picked(&pool, job, 1).await;
    zero_fee(&pool, (business, job)).await;
    let cleared = cleared_free(&pool, pick, (business, job)).await;
    let failed = sink(&pool, broken_vault())
        .settle_and_release(cleared)
        .await;
    assert!(failed.is_err(), "the vault read before the claim fails");
    assert_eq!(count(&tasks(&pool).await, LINE_RETRY_KIND, pick), 0);
    let (holder, impatient) = held_masked(&pool).await;
    let cleared = cleared_free(&impatient, pick, (business, job)).await;
    let shown = sink(&impatient, vault()).settle_and_release(cleared).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
    assert!(!had_line(&pool, job).await);
    holder.rollback().await.expect("pool recovers");
    let retry = tasks(&pool)
        .await
        .into_iter()
        .find(|t| t.kind == LINE_RETRY_KIND)
        .expect("the retry is queued");
    retry_line(pool.clone(), masked(), retry, now())
        .await
        .expect("opened");
    assert!(had_line(&pool, job).await);
}

/// TD-PICK-49 (PICK-022, PICK-024, PRIV-005, BIZ-014, D-1565): a regular job's release claims its
/// row and the vault read then fails, so Stripe gets the error; after a restart (a new pool,
/// vault and sink) the resent event finds the release made, and the signed-in owner sees the
/// details through the released view, again on a reload; one release notice, one charge, no
/// fee, no plaintext outside the vault, and the line still opens through its retry.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_49_a_failed_vault_read_recovers_and_shows_once(pool: PgPool) {
    let (business, person, job, pick) = picked(&pool).await;
    paid(&pool, (business, job, pick)).await;
    let failing = hooks(&pool, broken_vault(), pick);
    assert_eq!(
        deliver(failing, pick).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let after = restarted(&pool).await;
    assert_eq!(
        deliver(hooks(&after, vault(), pick), pick).await,
        StatusCode::OK
    );
    let expected = Some(("Dana".to_owned(), STREET.to_owned()));
    assert_eq!(seen(&after, (person, business), pick).await, expected);
    assert_eq!(
        seen(&after, (person, business), pick).await,
        expected,
        "a reload"
    );
    let mut db = after.acquire().await.expect("db");
    let charge = receipt_for(&mut db, pick, business).await;
    assert!(charge.expect("receipt").is_some(), "one charge");
    drop(db);
    no_fee_written(&after).await;
    let all = tasks(&after).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, pick), 1);
    no_plaintext(&all);
    let retry = all
        .into_iter()
        .find(|t| t.kind == LINE_RETRY_KIND)
        .expect("retry");
    retry_line(after.clone(), masked(), retry, now())
        .await
        .expect("opened");
    assert!(had_line(&after, job).await);
}

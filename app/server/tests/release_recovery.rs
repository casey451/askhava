//! TD-PICK-49 and TD-PICK-50 (PICK-022, PICK-023, PICK-024, PRIV-005, BIZ-014, CALL-001; D-1688,
//! D-1739): the production release through `server`'s `PickReleaser` with real proofs (only
//! `server` may mint, `tests/integration/tests/architecture.rs`): Stripe's webhook route with a
//! signed fake event and a fake Stripe client, `payments::api::no_fee_release` with a $0 decision
//! written through `payments::api::ledger_append`, a vault that cannot read and then can, a new
//! pool, vault and sink for each restart, and `picks.line_retry` once the masked pool recovers.
//!
//! `server` code names no module's schema, so the rows no module API can make without a whole
//! quote flow (the business, the jobs, the picks and the PaymentIntent) come from the sqlx
//! fixture `fixtures/release_recovery.sql` (D-2013); the homeowner's contact, her request, the
//! owner and the $0 decisions are made through the module APIs. Fake data only.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_calls::api::{CloseReason, Connected, assign_number, close_line, job_had_line};
use askhava_core::api::{
    Claimed, ClientIpPolicy, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_payments::api::{
    EntryKind, LedgerEntry, NewEntry, Releasable, STRIPE_WEBHOOK_PATH, StripeAccount,
    StripeWebhook, ledger_append, money_view, no_fee_release, stripe_webhook_routes,
};
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, LINE_RETRY_KIND, Released, ViewAnswer, released_view, retry_line, settle,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
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

const BUSINESS: Uuid = Uuid::from_u128(0x254b01);
const PAID_JOB: Uuid = Uuid::from_u128(0x254a01);
const FREE_JOB: Uuid = Uuid::from_u128(0x254a02);
const LINE_JOB: Uuid = Uuid::from_u128(0x254a03);
const GONE_JOB: Uuid = Uuid::from_u128(0x254a04);
const PAID_PICK: Uuid = Uuid::from_u128(0x254c01);
const FREE_PICK: Uuid = Uuid::from_u128(0x254c02);
const LINE_PICK: Uuid = Uuid::from_u128(0x254c03);
const GONE_PICK: Uuid = Uuid::from_u128(0x254c04);
const SECRET: &str = "whsec_fake_t254_signing_secret";
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
async fn homeowner(pool: &PgPool, jobs: &[Uuid]) -> Uuid {
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

/// The business's owner through `businesses`; returns the owner's member ID.
async fn owner(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    askhava_businesses::api::add_owner(&mut db, BUSINESS, Uuid::from_u128(0x254f01), now())
        .await
        .expect("owner")
}

/// The $0 free-first-win decision for `job` in the ledger (FEE-010, FEE-012).
async fn zero_fee(pool: &PgPool, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t254.free.{job}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(BUSINESS),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
}

async fn cleared_free(pool: &PgPool, pick: Uuid, job: Uuid) -> Releasable {
    let mut db = pool.acquire().await.expect("db");
    no_fee_release(&mut db, pick, (BUSINESS, job))
        .await
        .expect("cleared")
}

/// What the owner sees now through the released view; the first name and street, or `None`.
async fn seen(pool: &PgPool, person: Uuid, pick: Uuid) -> Option<(String, String)> {
    let proof = session_minter().business(person, BUSINESS);
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

/// Every task due by 2030, claimed once (so call it once per test, at the end).
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

struct FakeStripe;

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(PaymentIntent {
            id: id.to_owned(),
            status: "succeeded".to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_fake254".to_owned(),
            metadata: IntentMetadata {
                pick_id: PAID_PICK.to_string(),
            },
            livemode: Some(false),
        })
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

fn hooks(pool: &PgPool, vault: Vault) -> Arc<StripeWebhook<FakeStripe, PickReleaser>> {
    let none: [(String, String); 0] = [];
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe,
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

/// One signed `payment_intent.succeeded` delivery from the stand-in's host.
async fn deliver(hooks: Arc<StripeWebhook<FakeStripe, PickReleaser>>) -> StatusCode {
    let body = format!(
        r#"{{"id":"evt_fake254","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"pi_fake254paid","metadata":{{"pick_id":"{PAID_PICK}"}}}}}}}}"#
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

/// TD-PICK-50 (PICK-022, CALL-001): a verified success moves the awaiting-confirm pick to paid,
/// mints for her contact and releases; a duplicate delivery and two simultaneous ones give one
/// release, one notice, one masked line and no fee.
#[sqlx::test(migrator = "MIGRATOR", fixtures("release_recovery"))]
async fn td_pick_50_a_verified_success_settles_paid_and_releases_once(pool: PgPool) {
    let person = owner(&pool).await;
    homeowner(&pool, &[PAID_JOB]).await;
    let hooks = hooks(&pool, vault());
    assert_eq!(deliver(Arc::clone(&hooks)).await, StatusCode::OK);
    assert_eq!(deliver(Arc::clone(&hooks)).await, StatusCode::OK);
    let (a, b) = tokio::join!(deliver(Arc::clone(&hooks)), deliver(Arc::clone(&hooks)));
    assert_eq!((a, b), (StatusCode::OK, StatusCode::OK));
    let shown = seen(&pool, person, PAID_PICK).await;
    assert_eq!(shown, Some(("Dana".to_owned(), STREET.to_owned())));
    assert!(had_line(&pool, PAID_JOB).await);
    let mut db = pool.acquire().await.expect("db");
    let closed = close_line(&mut db, (PAID_JOB, 2541), CloseReason::Expired, now()).await;
    assert!(closed.expect("close").is_some(), "one open line");
    let again = close_line(&mut db, (PAID_JOB, 2541), CloseReason::Expired, now()).await;
    assert!(again.expect("close").is_none(), "and only one");
    no_fee_written(&pool).await;
    let all = tasks(&pool).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, PAID_PICK), 1);
    assert_eq!(count(&all, LINE_RETRY_KIND, PAID_PICK), 0);
    no_plaintext(&all);
}

/// TD-PICK-50 (PICK-022, FEE-010): a recorded $0 decision moves the pick to free and releases it
/// through the same sink; a pick that left awaiting confirm is not moved and nothing is released.
#[sqlx::test(migrator = "MIGRATOR", fixtures("release_recovery"))]
async fn td_pick_50_a_zero_fee_releases_free_and_a_cancelled_pick_nothing(pool: PgPool) {
    let person = owner(&pool).await;
    homeowner(&pool, &[FREE_JOB, GONE_JOB]).await;
    zero_fee(&pool, FREE_JOB).await;
    zero_fee(&pool, GONE_JOB).await;
    let sink = sink(&pool, vault());
    let free = sink
        .settle_and_release(cleared_free(&pool, FREE_PICK, FREE_JOB).await)
        .await;
    assert!(matches!(free.expect("release"), Released::Shown(_)));
    let gone = sink
        .settle_and_release(cleared_free(&pool, GONE_PICK, GONE_JOB).await)
        .await;
    assert!(matches!(gone.expect("release"), Released::MovedOn));
    assert!(seen(&pool, person, FREE_PICK).await.is_some());
    assert_eq!(seen(&pool, person, GONE_PICK).await, None);
    assert!(!had_line(&pool, GONE_JOB).await);
    let all = tasks(&pool).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, GONE_PICK), 0);
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, FREE_PICK), 1);
}

/// TD-PICK-50 (D-1739): a restart between the settle's commit and the claim leaves the pick free,
/// and the retried clearing, on a new pool, vault and sink, releases exactly once.
#[sqlx::test(migrator = "MIGRATOR", fixtures("release_recovery"))]
async fn td_pick_50_a_restart_after_the_settle_releases_once_on_the_retry(pool: PgPool) {
    let person = owner(&pool).await;
    homeowner(&pool, &[FREE_JOB]).await;
    zero_fee(&pool, FREE_JOB).await;
    {
        // The settle commits, then the process stops before the claim: the clearing is lost.
        let cleared = cleared_free(&pool, FREE_PICK, FREE_JOB).await;
        let mut tx = pool.begin().await.expect("tx");
        settle(&mut tx, &cleared).await.expect("settled");
        tx.commit().await.expect("commit");
    }
    let after = restarted(&pool).await;
    let sink = sink(&after, vault());
    let first = sink
        .settle_and_release(cleared_free(&after, FREE_PICK, FREE_JOB).await)
        .await;
    assert!(matches!(first.expect("release"), Released::Shown(_)));
    let second = sink
        .settle_and_release(cleared_free(&after, FREE_PICK, FREE_JOB).await)
        .await;
    assert!(matches!(second.expect("release"), Released::Already));
    assert!(seen(&after, person, FREE_PICK).await.is_some());
    let all = tasks(&after).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, FREE_PICK), 1);
}

/// TD-PICK-50 (CALL-001, D-1688): a line that fails to open (the pool is held) queues one retry,
/// which opens it once the pool recovers, opens nothing for a job that already has a line, and
/// never reopens a line closed after 30 days.
#[sqlx::test(migrator = "MIGRATOR", fixtures("release_recovery"))]
async fn td_pick_50_a_failed_line_opens_on_the_retry_and_never_reopens(pool: PgPool) {
    owner(&pool).await;
    homeowner(&pool, &[LINE_JOB]).await;
    zero_fee(&pool, LINE_JOB).await;
    let options = pool.connect_options().as_ref().clone();
    let impatient = PgPoolOptions::new()
        .connect_with(options.options([("lock_timeout", "300ms")]))
        .await
        .expect("pool");
    let mut holder = pool.begin().await.expect("tx");
    let other = Connected {
        job: Uuid::from_u128(0x254a99),
        homeowner: Uuid::from_u128(0x254a98),
        business: Uuid::from_u128(0x254a97),
    };
    assign_number(&mut holder, &masked(), other, now())
        .await
        .expect("held");
    let cleared = cleared_free(&impatient, LINE_PICK, LINE_JOB).await;
    let shown = sink(&impatient, vault()).settle_and_release(cleared).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
    assert!(!had_line(&pool, LINE_JOB).await);
    holder.rollback().await.expect("pool recovers");
    let retries: Vec<Claimed> = tasks(&pool)
        .await
        .into_iter()
        .filter(|t| t.kind == LINE_RETRY_KIND)
        .collect();
    assert_eq!(retries.len(), 1);
    let retry = retries.into_iter().next().expect("retry");
    retry_line(pool.clone(), masked(), retry.clone(), now())
        .await
        .expect("opened");
    assert!(had_line(&pool, LINE_JOB).await);
    retry_line(pool.clone(), masked(), retry.clone(), now())
        .await
        .expect("nothing");
    let mut db = pool.acquire().await.expect("db");
    let closed = close_line(&mut db, (LINE_JOB, 2543), CloseReason::Expired, now()).await;
    assert!(closed.expect("close").is_some(), "one line, opened once");
    let later = now() + SignedDuration::from_hours(24 * 31);
    retry_line(pool.clone(), masked(), retry, later)
        .await
        .expect("nothing again");
    let reopened = close_line(&mut db, (LINE_JOB, 2543), CloseReason::Expired, later).await;
    assert!(
        reopened.expect("close").is_none(),
        "a closed line is never reopened"
    );
}

/// TD-PICK-49 (PICK-022, PICK-024, PRIV-005, BIZ-014, D-1565): a regular job's release claims its
/// row and the vault read then fails, so Stripe gets the error; after a restart (a new pool,
/// vault and sink) the resent event finds the release made, and the signed-in owner sees the
/// details through the released view, again on a reload; one release notice, no fee, no
/// plaintext outside the vault, and the line still opens through its retry.
#[sqlx::test(migrator = "MIGRATOR", fixtures("release_recovery"))]
async fn td_pick_49_a_failed_vault_read_recovers_and_shows_once(pool: PgPool) {
    let person = owner(&pool).await;
    homeowner(&pool, &[PAID_JOB]).await;
    let failing = hooks(&pool, broken_vault());
    assert_eq!(deliver(failing).await, StatusCode::INTERNAL_SERVER_ERROR);
    let after = restarted(&pool).await;
    assert_eq!(deliver(hooks(&after, vault())).await, StatusCode::OK);
    let expected = Some(("Dana".to_owned(), STREET.to_owned()));
    assert_eq!(seen(&after, person, PAID_PICK).await, expected);
    assert_eq!(seen(&after, person, PAID_PICK).await, expected, "a reload");
    no_fee_written(&after).await;
    let all = tasks(&after).await;
    assert_eq!(count(&all, BUSINESS_NOTICE_KIND, PAID_PICK), 1);
    no_plaintext(&all);
    let retry = all
        .into_iter()
        .find(|t| t.kind == LINE_RETRY_KIND)
        .expect("retry");
    retry_line(after.clone(), masked(), retry, now())
        .await
        .expect("opened");
    assert!(had_line(&after, PAID_JOB).await);
}

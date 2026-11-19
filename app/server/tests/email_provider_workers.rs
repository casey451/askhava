//! T490 (SEC-025, BIZ-040, CON-020, OPS-038; D-3750, D-3805, D-3882, D-4293): the email provider
//! passed to the worker's account mailers and the sign-up code sender, against PostgreSQL and a
//! local recording server shaped as the Resend stand-in (`POST <base>/emails`, 127.0.0.1 only).
//! The welcome sends one email to the owner's address read through `VaultAccountDirectory` (a
//! mailbox with no row reads `None`, `last4` is always `None`); a returned-address notice sends one
//! email per recipient through `PortalNoticeParts`, naming the gone address read before the
//! transaction; a sign-up code request sends one code email per admitted request and none past
//! the per-business cap; with no key in CI no provider is built and no welcome is marked sent.
//! Fake data only (`example` addresses, fake keys).

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_businesses::api::{
    AccountDirectory, Acting, Returned, Roles, SignUpForm, WELCOME_KIND, add_member, add_owner,
    create, mark_returned, may_get_cards, sign_up,
};
use askhava_core::api::{
    Clock, Lease, MIGRATOR, NewTask, Settings, SystemClock, WorkerId, claim, enqueue,
};
use askhava_licensing::api::ClassMap;
use askhava_picks::api::ConfirmSettings;
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::account_directory::VaultAccountDirectory;
use askhava_server::business_signup::{
    SIGNUP_EMAIL_CODES_PER_BUSINESS, provider_mail, send_signup_code,
};
use askhava_server::email_provider::email_provider;
use askhava_server::{offdb_stream::OffDbStream, worker_registry};
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const API_KEY: &str = "re_stand_in_fake_key_0001";
const DB_URL: &str = "postgres://askhava_app@db.invalid/askhava";
const POSTAL: &str = "PO Box 1234, Faketown, AZ 00000";
const OWNER: &str = "owner@x.example";
const MEMBER: &str = "member@x.example";

/// Each email the recording server took: its recipient and plain text.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from(DB_URL.to_owned());
    Vault::new(&key(0x53).into(), &key(0x63).into(), &url).expect("vault")
}

fn sent(seen: &Seen) -> Vec<(String, String)> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A recording server shaped as the Resend stand-in on 127.0.0.1; returns its base URL.
async fn recorder(seen: Seen) -> String {
    async fn emails(State(seen): State<Seen>, body: String) -> String {
        let email: serde_json::Value = serde_json::from_str(&body).expect("json");
        let field = |name: &str| email[name].as_str().unwrap_or_default().to_owned();
        let to = email["to"][0]
            .as_str()
            .map_or_else(|| field("to"), str::to_owned);
        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.push((to, field("text")));
        format!("{{\"id\":\"re_fake_{}\"}}", seen.len())
    }
    let app = Router::new()
        .route("/resend/emails", post(emails))
        .with_state(seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let at = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{at}/resend")
}

/// Fake CI pairs with the recorder as the Resend base and, when `keyed`, a fake API key.
fn pairs(base: &str, keyed: bool) -> Vec<(String, String)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t490-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let storage = format!("file://{}", dir.display());
    let (seal, index) = (key(0x53), key(0x63));
    let mut all = vec![
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", DB_URL),
        ("ASKHAVA_STORAGE_BASE_URL", storage.as_str()),
        ("ASKHAVA_RESEND_DOMAIN", "testfake.example.com"),
        ("ASKHAVA_RESEND_BASE_URL", base),
        ("ASKHAVA_ID_SECRET", SECRET),
        ("ASKHAVA_POSTAL_ADDRESS", POSTAL),
        ("VAULT_KEY", seal.as_str()),
        ("VAULT_INDEX_KEY", index.as_str()),
    ];
    if keyed {
        all.push(("ASKHAVA_RESEND_API_KEY", API_KEY));
    }
    all.iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// The worker's registry as `worker.rs` builds it over `pairs`.
fn registry(pairs: Vec<(String, String)>) -> askhava_core::api::Registry {
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let stream = OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let parts = (None, ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// Claims and runs every due task once.
async fn run_due(pool: &PgPool, registry: &askhava_core::api::Registry) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, Timestamp::now(), Lease::WORKER.length, 10).await;
    for task in tasks.expect("claim") {
        let ran = registry
            .run(pool, &SystemClock, owner, task, Lease::WORKER)
            .await;
        ran.expect("ran");
    }
}

/// A business with a verified owner at [`OWNER`] and member at [`MEMBER`], its welcome requested
/// and queued; returns the business and the member's mailbox.
async fn business(pool: &PgPool) -> (Uuid, Uuid) {
    let now = Timestamp::now();
    let mut db = pool.acquire().await.expect("db");
    let owner_box = mailbox_for(&mut db, &vault(), OWNER).await.expect("box");
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, owner_box, now).await;
    let member = owner.expect("owner");
    let acting = Acting { business, member };
    let member_box = mailbox_for(&mut db, &vault(), MEMBER).await.expect("box");
    let added = add_member(&mut db, acting, member_box, Roles::BOTH, now).await;
    for (mailbox, who) in [(owner_box, member), (member_box, added.expect("member"))] {
        sqlx::query(
            "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, \
             sent_at, window_at, verified_at) VALUES ($1, $2, $3, false, now(), now(), now())",
        )
        .bind(business)
        .bind(who)
        .bind(mailbox)
        .execute(&mut *db)
        .await
        .expect("verified");
    }
    sqlx::query("INSERT INTO businesses.welcome (business_id, requested_at) VALUES ($1, now())")
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("welcome row");
    let task = NewTask::new(WELCOME_KIND, json!({"business": business}), now);
    enqueue(&mut db, &task).await.expect("welcome task");
    (business, member_box)
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_welcome_goes_to_the_owner_address_read_through_the_vault(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let registry = registry(pairs(&base, true));
    let (business, _) = business(&pool).await;
    run_due(&pool, &registry).await;
    let emails = sent(&seen);
    assert_eq!(emails.len(), 1, "one welcome email");
    assert_eq!(emails[0].0, OWNER, "to the owner's own address");
    let mut db = pool.acquire().await.expect("db");
    assert!(
        may_get_cards(&mut db, business).await.expect("read"),
        "marked sent"
    );
    let directory = VaultAccountDirectory::new(Arc::new(vault()));
    let none = directory.address(&mut db, Uuid::from_u128(0x7490)).await;
    assert!(none.expect("read").is_none(), "no mailbox row, no address");
    let last4 = directory
        .last4(&mut db, (business, Uuid::from_u128(0x7491)))
        .await;
    assert!(
        last4.expect("read").is_none(),
        "no phone digits are readable"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_returned_notice_names_the_gone_address_once_per_recipient(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let registry = registry(pairs(&base, true));
    let (business, member_box) = business(&pool).await;
    run_due(&pool, &registry).await;
    let mut db = pool.acquire().await.expect("db");
    let at = Timestamp::now();
    let marked = mark_returned(&mut db, (business, member_box), Returned::Hard, at).await;
    assert!(marked.expect("marked"), "returned");
    run_due(&pool, &registry).await;
    let emails = sent(&seen);
    assert_eq!(
        emails.len(),
        2,
        "the welcome, then one returned-address email"
    );
    assert_eq!(
        emails[1].0, OWNER,
        "to the owner, never the returned address"
    );
    assert!(emails[1].1.contains(MEMBER), "it names the gone address");
}

/// A fake Stripe that makes customers only.
struct FakeStripe;

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_Fake1".to_owned())
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

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_sign_up_code_email_per_admitted_request_and_none_past_the_cap(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let mail = provider_mail(&pool, &pairs(&base, true)).expect("mail");
    let mut db = pool.acquire().await.expect("db");
    let mailbox = mailbox_for(&mut db, &vault(), OWNER).await.expect("box");
    let cats = "categories:\n  - {id: plumbing_repair, enabled: true, launch_open: true, \
                job_types: [leak repair]}\n";
    let map = ClassMap::load("commercial_only: []\nclasses: []\n", cats).expect("catalog");
    let form = SignUpForm {
        categories: vec!["plumbing_repair".to_owned()],
        mailbox_id: Some(mailbox),
        heard_from: None,
        g_code: None,
    };
    let secret = SecretString::from(SECRET.to_owned());
    let up = sign_up(&pool, (&FakeStripe, &secret), &map, &form, Timestamp::now()).await;
    let business = up.expect("signed up").business;
    let cap = usize::try_from(SIGNUP_EMAIL_CODES_PER_BUSINESS.max).expect("cap");
    for n in 1..=cap + 1 {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, u8::try_from(n).expect("n")));
        let parts = (&pool, &secret, &mail);
        let done = send_signup_code(parts, &mut db, business, (ip, Timestamp::now())).await;
        done.expect("sent or capped");
        assert_eq!(
            sent(&seen).len(),
            n.min(cap),
            "one email per admitted request"
        );
    }
    assert!(
        sent(&seen).iter().all(|(to, _)| to == OWNER),
        "only to the sign-up mailbox"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn no_key_in_ci_builds_no_provider_and_marks_no_welcome_sent(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let pairs = pairs(&base, false);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let built = email_provider(&settings, pairs.clone()).expect("no error in CI");
    assert!(built.is_none(), "no key: no provider");
    let registry = registry(pairs);
    let (business, _) = business(&pool).await;
    run_due(&pool, &registry).await;
    assert!(sent(&seen).is_empty(), "no email");
    let mut db = pool.acquire().await.expect("db");
    assert!(
        !may_get_cards(&mut db, business).await.expect("read"),
        "not marked sent"
    );
}

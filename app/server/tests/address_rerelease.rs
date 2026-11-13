//! TD-INT-39 (T255; INT-027, INT-029, PICK-022, PRIV-007; D-1563, D-1739, D-3358, D-3384,
//! D-3431): an emergency release that came before the street address asks for it again, once per
//! pick, and releases once it arrives, through the real mint, the re-clear and the sink.
//!
//! Every row comes through module APIs (business, job, card, quote, page pick, contact and
//! request, the charge and the $0 ledger entry), fake data only; this crate seeds no module's
//! schema (TD-SEC-10). The homeowner's contact holds only her fake phone until she answers, so
//! the first release waits; her answer is written onto that same contact.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, MaskedPool, PaymentIntent, PaymentsAdapter,
    TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_calls::api::job_had_line;
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    ADDRESS_IN_KIND, Answer, AskedAgain, Taken, TextIn, ZipStep, answer_zip, ask_again,
    receive_text, record_answer, run_address_needed, take_name_address,
};
use askhava_jobs::api::{JobStatus, create, job_number, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor, export_audit};
use askhava_payments::api::{
    BusinessId, ChargeOutcome, Contact, EmailCheck, EntryKind, IntentStripe, LedgerEntry, NewEntry,
    PickNow, Press, RefundTrigger, STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook,
    confirm_and_pay, ensure_customer, ledger_append, no_fee_release, request_refund,
    stripe_webhook_routes,
};
use askhava_picks::api::{
    ADDRESS_NEEDED_KIND, BUSINESS_NOTICE_KIND, PagePick, Released, ReleasedPick, ViewAnswer,
    no_call_at_72_hours, page_pick, released_view,
};
use askhava_privacy::api::{NewContact, Vault, purge_contact, set_name_address, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::release_sink::{PickReleaser, address_in};
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
use sqlx::types::Uuid;
use tower::ServiceExt;

const SECRET: &str = "whsec_fake_t255_signing_secret";
const INTENT: &str = "pi_fake255paid";
const ANSWER: &str = "Dana 100 Example Way";
/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn id_secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x55), &key(0x65), &url).expect("vault")
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

fn sink(pool: &PgPool) -> PickReleaser {
    PickReleaser::new(
        pool.clone(),
        vault(),
        masked(),
        Arc::new(ManualClock::new(now())),
    )
}

/// Her request: a vault contact holding only her fake phone, the request naming each of `jobs`
/// and her ZIP (the property the question needs); `n` tells fake homeowners apart; returns
/// `(contact, request)`.
async fn her_request(pool: &PgPool, jobs: &[Uuid], n: u128) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(format!("+1928555{:04}", 250 + n))),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let text = |words, m: u128| TextIn {
        inbox_id: Uuid::from_u128(0x255e00 + 16 * n + m),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now(),
    };
    let first = text("my AC is out", 0);
    let handled = receive_text(&mut db, &id_secret(), &first, IntakeSettings::default())
        .await
        .expect("request");
    let conversation = handled.conversation.expect("open request");
    for (i, job) in jobs.iter().enumerate() {
        let code = job.simple().to_string();
        let question = format!("job.hvac.p{i}");
        let answer = Answer {
            conversation,
            question: &question,
            options: Some(&[(code.as_str(), "fakejob")]),
            words: "fakejob",
            reply: (Uuid::from_u128(0x255f00 + 16 * n + i as u128), now()),
        };
        record_answer(&mut db, &answer).await.expect("job named");
    }
    let zip = text("86403", 1);
    let (secret, settings) = (id_secret(), IntakeSettings::default());
    let step = answer_zip(&mut db, &secret, (&zip, conversation), settings).await;
    assert!(matches!(step.expect("zip"), ZipStep::Stored(_)));
    (contact, conversation)
}

async fn new_business(pool: &PgPool) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("business");
    let person = add_owner(&mut db, business, Uuid::new_v4(), now())
        .await
        .expect("owner");
    (business, person)
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

/// An emergency job, opened, carded to `business` and quoted, so it is in Quotes ready.
async fn emergency_job(pool: &PgPool, business: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), true, Actor::System)
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
    job
}

/// One business and owner, its quoted emergency job, her request and page pick:
/// `(business, owner, job, pick, contact, request)`.
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid) {
    let (business, person) = new_business(pool).await;
    let job = emergency_job(pool, business).await;
    let (contact, conversation) = her_request(pool, &[job], 0).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    (business, person, job, pick, contact, conversation)
}

/// The fake Stripe: every intent succeeds for the $25 fee; a refund is never sent.
struct FakeStripe {
    pick: Uuid,
}

impl FakeStripe {
    fn succeeded(&self, id: &str) -> PaymentIntent {
        serde_json::from_value(serde_json::json!({
            "id": id, "status": "succeeded", "amount": 2500, "currency": "usd",
            "customer": "cus_fake255", "metadata": {"pick_id": self.pick.to_string()},
            "livemode": false, "latest_charge": "ch_fake255",
        }))
        .expect("intent")
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake255".to_owned())
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

/// The Confirm-and-pay press of the $25 fee through `payments`, so the pick has a stored success
/// and its FEE-042 evidence row.
async fn charged(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) {
    let stripe = FakeStripe { pick };
    ensure_customer(pool, &stripe, &id_secret(), BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake255",
        press: Uuid::new_v4(),
        emergency: true,
        now: PickNow::Payable,
    };
    let paid = confirm_and_pay(pool, &stripe, &id_secret(), &press, now()).await;
    assert!(matches!(paid.expect("charge"), ChargeOutcome::Succeeded(_)));
}

/// One signed `payment_intent.succeeded` delivery for `pick` to the route with the real sink.
async fn deliver(pool: &PgPool, pick: Uuid) -> StatusCode {
    let none: [(String, String); 0] = [];
    let hooks = Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe: FakeStripe { pick },
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake255".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(now())),
        sink: sink(pool),
    });
    let body = format!(
        r#"{{"id":"evt_fake255","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{INTENT}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
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
    let reply = stripe_webhook_routes(hooks).oneshot(request).await;
    reply.expect("answer").status()
}

/// The $0 free-first-win decision for `business` on `job`, cleared, settled and released.
async fn released_free(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) -> Released {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t255.free.{job}.{business}");
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
    let sink = sink(pool);
    let released = sink.settle_and_release(cleared.expect("cleared")).await;
    released.expect("release")
}

/// What the owner sees now through the released view: first name and street, or `None`.
async fn seen(pool: &PgPool, (person, business): (Uuid, Uuid), pick: Uuid) -> Option<String> {
    let proof = session_minter().business(person, business);
    match released_view(pool, &vault(), &proof, pick)
        .await
        .expect("view")
    {
        ViewAnswer::Shown(view) => {
            let first = view.first_name().expect("name").expose_secret().to_owned();
            let street = view.address().expect("address").expose_secret().to_owned();
            Some(format!("{first}|{street}"))
        }
        ViewAnswer::Removed | ViewAnswer::NotFound => None,
    }
}

/// Every unclaimed task due by 2030, claimed once.
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    claim(pool, owner, later, SignedDuration::from_secs(60), 1000)
        .await
        .expect("claim")
}

fn of_kind(tasks: &[Claimed], kind: &str) -> Vec<Claimed> {
    tasks.iter().filter(|t| t.kind == kind).cloned().collect()
}

async fn take(pool: &PgPool, conversation: Uuid, at: Timestamp) -> Taken {
    let mut db = pool.acquire().await.expect("db");
    let raw = SecretString::from(ANSWER);
    let inbox = (conversation, Uuid::new_v4());
    let taken = take_name_address(&mut db, &vault(), inbox, &raw, at).await;
    taken.expect("taken")
}

async fn had_line(pool: &PgPool, job: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    job_had_line(&mut db, job).await.expect("line")
}

async fn fields_set(pool: &PgPool) -> usize {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, 1000).await.expect("export");
    rows.iter()
        .filter(|r| r.action == askhava_privacy::api::CONTACT_FIELDS_SET)
        .count()
}

/// TD-INT-39 (a), (b): a paid emergency pick released before the address waits and asks again
/// once; her answer goes onto her own contact, the pick is re-cleared from its stored success,
/// released once with its masked line, and the owner sees her first name and street, never her
/// phone; a second answer queues and releases nothing and sends the address no second time.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_39_the_address_arrives_and_the_paid_pick_releases_once(pool: PgPool) {
    let (business, person, job, pick, contact, conversation) = picked(&pool).await;
    charged(&pool, (business, job, pick)).await;
    assert_eq!(deliver(&pool, pick).await, StatusCode::OK);
    assert_eq!(seen(&pool, (person, business), pick).await, None);
    assert!(!had_line(&pool, job).await, "no line before the address");
    let needed = of_kind(&tasks(&pool).await, ADDRESS_NEEDED_KIND);
    assert_eq!(needed.len(), 1);
    let task = needed.into_iter().next().expect("task");
    run_address_needed(pool.clone(), id_secret(), task)
        .await
        .expect("asked");
    let mut db = pool.acquire().await.expect("db");
    let again = ask_again(&mut db, &id_secret(), (job, pick), now()).await;
    assert_eq!(again, Ok(AskedAgain::Already), "once per pick");
    drop(db);
    assert_eq!(
        take(&pool, conversation, now()).await,
        Taken::Stored(contact)
    );
    let arrived = of_kind(&tasks(&pool).await, ADDRESS_IN_KIND);
    assert_eq!(arrived.len(), 1);
    assert_eq!(arrived[0].payload, serde_json::json!({"pick": pick}));
    let shown = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    let Released::Shown(view) = shown.expect("re-release") else {
        panic!("not shown");
    };
    let street = view.address().expect("address").expose_secret().to_owned();
    assert_eq!(street, "100 Example Way");
    let expected = Some("Dana|100 Example Way".to_owned());
    assert_eq!(seen(&pool, (person, business), pick).await, expected);
    assert!(
        !format!("{expected:?}").contains("5550255"),
        "never the phone"
    );
    assert!(had_line(&pool, job).await, "the masked line opens");
    assert_eq!(
        take(&pool, conversation, now()).await,
        Taken::Stored(contact)
    );
    let later = tasks(&pool).await;
    assert!(
        of_kind(&later, ADDRESS_IN_KIND).is_empty(),
        "nothing queued again"
    );
    let twice = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    assert!(matches!(twice.expect("again"), Released::Already));
    assert_eq!(
        of_kind(&later, BUSINESS_NOTICE_KIND).len(),
        1,
        "one release notice"
    );
}

/// TD-INT-39: the $0 path re-clears from the recorded decision, but a free pick that left free
/// (a no-show) before the address arrived releases nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_39_a_pick_that_left_free_releases_nothing(pool: PgPool) {
    let (business, person, job, pick, _, conversation) = picked(&pool).await;
    let first = released_free(&pool, (business, job, pick)).await;
    assert!(matches!(first, Released::WaitingForAddress));
    let mut db = pool.acquire().await.expect("db");
    for arrow in [
        (JobStatus::AwaitingConfirm, JobStatus::PaymentPending),
        (JobStatus::PaymentPending, JobStatus::Connected),
    ] {
        let moved = move_to(&mut db, job, arrow, now(), Actor::System).await;
        assert_eq!(moved.expect("move"), Guarded::Applied(()));
    }
    let number = job_number(&mut db, job).await.expect("number");
    let case = ReleasedPick {
        pick,
        job,
        job_number: number,
        business,
        business_name: "Fake Cooling",
        homeowner: Uuid::from_u128(0x255),
        language: (askhava_messaging::api::Language::En, false),
    };
    let facts = (Contact::Call, EmailCheck::NoReply, now(), &[][..]);
    let ended = no_call_at_72_hours(
        &mut db,
        &case,
        facts,
        now() + SignedDuration::from_hours(80),
    );
    assert!(ended.await.expect("no-show"), "the pick left free");
    drop(db);
    take(&pool, conversation, Timestamp::now()).await;
    let shown = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    assert!(matches!(shown.expect("nothing"), Released::MovedOn));
    assert_eq!(seen(&pool, (person, business), pick).await, None);
    assert!(!had_line(&pool, job).await);
}

/// TD-INT-39: a refund begun on a paid pick before the address arrived re-clears nothing; and a
/// paid pick with a stored success and no `picks.address_wait` row releases nothing on an
/// `intake.address_in` event and calls no re-clear (`MovedOn`, not the release's `Already`).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_39_a_refunded_or_never_waiting_pick_releases_nothing(pool: PgPool) {
    let (business, person, job, pick, _, conversation) = picked(&pool).await;
    charged(&pool, (business, job, pick)).await;
    assert_eq!(deliver(&pool, pick).await, StatusCode::OK);
    let stripe = FakeStripe { pick };
    let refund = (pick, RefundTrigger::FraudWarning);
    let refunded = request_refund(&pool, &stripe, &id_secret(), refund, now()).await;
    assert!(refunded.is_ok(), "a refund row, its send failed");
    take(&pool, conversation, Timestamp::now()).await;
    let shown = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    assert!(matches!(shown.expect("nothing"), Released::MovedOn));
    assert_eq!(seen(&pool, (person, business), pick).await, None);
    let other = Uuid::new_v4();
    let unknown = address_in(&pool, (&vault(), &masked()), other, now()).await;
    assert!(matches!(unknown.expect("nothing"), Released::MovedOn));
}

/// Homeowner `n`'s request whose one job became final at `final_at`, her contact purged when
/// `purge`; `(contact, request)`.
async fn ended_request(
    pool: &PgPool,
    final_at: Timestamp,
    (n, purge): (u128, bool),
) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, final_at, false, Actor::System)
        .await
        .expect("job");
    let cancel = (JobStatus::Intake, JobStatus::Cancelled);
    let moved = move_to(&mut db, job, cancel, final_at, Actor::System).await;
    assert_eq!(moved.expect("cancel"), Guarded::Applied(()));
    drop(db);
    let (contact, conversation) = her_request(pool, &[job], n).await;
    if purge {
        let mut db = pool.acquire().await.expect("db");
        let (name, street) = (SecretString::from("Dana"), SecretString::from("1 Fake St"));
        let earlier = set_name_address(&mut db, &vault(), contact, &name, &street).await;
        earlier.expect("her earlier answer");
        assert!(purge_contact(&mut db, contact).await.expect("purge"));
    }
    (contact, conversation)
}

/// TD-INT-39 (c), (c'), PRIV-007: a late answer for a request the purge treats as ended (its job
/// final 180 days or more) is `Ended` and writes nothing; so is one received inside the window
/// but processed after it closed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_39_a_request_the_purge_ended_is_never_refilled(pool: PgPool) {
    let final_at = Timestamp::now() - SignedDuration::from_hours(181 * 24);
    let (_, conversation) = ended_request(&pool, final_at, (1, true)).await;
    assert_eq!(
        take(&pool, conversation, Timestamp::now()).await,
        Taken::Ended
    );
    let inside = final_at + SignedDuration::from_hours(179 * 24);
    assert_eq!(take(&pool, conversation, inside).await, Taken::Ended);
    assert_eq!(
        fields_set(&pool).await,
        2,
        "only her earlier answer; nothing written now"
    );
    let recent = Timestamp::now() - SignedDuration::from_hours(24);
    let (contact, open) = ended_request(&pool, recent, (2, false)).await;
    assert_eq!(
        take(&pool, open, Timestamp::now()).await,
        Taken::Stored(contact)
    );
    assert_eq!(fields_set(&pool).await, 4);
}

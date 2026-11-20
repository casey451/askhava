//! TD-JOB-18 (JOB-002, JOB-004, JOB-005, JOB-006, JOB-014, JOB-015, PICK-022, PRIV-005, FEE-018,
//! FEE-020, FEE-021, FEE-038, OPS-024, PICK-048; T663, D-4349): a confirmed pick moves its job to
//! Payment pending and Connected, and the Payment pending wait defers to `payments`' hold.
//! PostgreSQL through each module's API, with the real release sink and webhook route over a fake
//! Stripe; a few states the code reaches only by timing are planted with one SQL statement, each
//! named where it is. Fake data only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{
    CHECK_IN_KIND, JobStatus, PAYMENT_RETURNED_KIND, TEXT_KIND, TIMER_KIND, create, current,
    move_to, record_job_facts, run_timer,
};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{
    Actor, NewAlert, Severity, Switch, acknowledge_alert, check_safe_mode, raise_alert,
    record_push, set_switch,
};
use askhava_payments::api::{
    BusinessId, EntryKind, IntentStripe, LATE_KIND, LedgerEntry, NewEntry, PickNow, Press,
    STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook, confirm_and_pay, ensure_customer,
    ledger_append, no_fee_release, run_charge_check, stripe_webhook_routes,
};
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, Backup, ConfirmMove, HELD_LONG_ALERT, HELD_SUCCESS_KIND, PagePick,
    PendingEnd, Released, Settled, confirm_move, end_pending, is_released, page_pick,
    payment_returned, run_payment_returned, settle,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the release sink and the held release are called here"
)]
mod server;
use server::release_hold::{HELD_RELEASE_KIND, run_held_release};
use server::release_sink::PickReleaser;
#[allow(
    unused_imports,
    reason = "the server modules name each other through the crate root"
)]
use server::*;

const SECRET: &str = "whsec_fake_t663_signing_secret";
const INTENT: &str = "pi_fake663paid";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key";
/// 10:00 Arizona on a Monday.
const NOW: i64 = 1_793_638_800;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn hours(n: i64) -> Timestamp {
    now() + SignedDuration::from_hours(n)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x63), &key(0x66), &url).expect("vault")
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

/// Safe mode on through `ops::api`: a critical alert pushed 25 hours ago; returns the alert.
async fn safe_mode_starts(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let alert = NewAlert {
        kind: "ops.t663_fake_critical",
        severity: Severity::Critical,
        subject: None,
    };
    let alert = raise_alert(&mut db, &alert).await.expect("alert");
    record_push(&mut db, alert, hours(-25)).await.expect("push");
    assert!(check_safe_mode(&mut db, now()).await.expect("on"));
    alert
}

/// Safe mode off: `alert` acknowledged and `check_safe_mode` run again.
async fn safe_mode_ends(pool: &PgPool, alert: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    assert!(acknowledge_alert(&mut db, alert, now()).await.expect("ack"));
    assert!(!check_safe_mode(&mut db, now()).await.expect("off"));
}

/// The homeowner's fake contact in the vault and her request naming `job`.
async fn her_request(pool: &PgPool, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from("+19285550163")),
        email: None,
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new).await;
    let inbound = Inbound {
        words: "my sink leaks",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::from_u128(0x663e01),
        contact: contact.expect("contact"),
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default()).await;
    let code = job.simple().to_string();
    let answer = Answer {
        conversation: handled.expect("request").conversation.expect("open"),
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x663e10), now()),
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

/// A business and its owner, a job it quoted, her request naming it and her page pick, the job
/// in Awaiting confirm: `(business, job, pick)`.
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now()).await;
    let business = business.expect("business");
    add_owner(&mut db, business, Uuid::new_v4(), now())
        .await
        .expect("owner");
    let job = create(&mut db, now(), false, Actor::System).await;
    let job = job.expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    moved(pool, job, (JobStatus::Intake, JobStatus::Open)).await;
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

/// Moves `job` along `arrow` by hand at the start; it must apply.
async fn moved(pool: &PgPool, job: Uuid, arrow: (JobStatus, JobStatus)) {
    let mut db = pool.acquire().await.expect("db");
    let applied = move_to(&mut db, job, arrow, now(), Actor::System).await;
    assert_eq!(applied.expect("move"), Guarded::Applied(()));
}

/// `confirm_move` for `pick` of `job` at the start, committed.
async fn confirmed(pool: &PgPool, (pick, job): (Uuid, Uuid)) -> ConfirmMove {
    let mut tx = pool.begin().await.expect("tx");
    let answer = confirm_move(&mut tx, (pick, job), now()).await;
    tx.commit().await.expect("commit");
    answer.expect("confirm move")
}

/// The $25 Confirm-and-pay press with Stripe confirming the intent as `confirms`.
async fn press(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid), confirms: &'static str) {
    let stripe = FakeStripe {
        pick,
        reports: "succeeded",
        confirms,
    };
    let secret = SecretString::from(ID_SECRET.to_owned());
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake663",
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    confirm_and_pay(pool, &stripe, &secret, &press, now())
        .await
        .expect("pressed");
}

/// The free first win for `business` and `job` in the ledger (FEE-010).
async fn free_win(pool: &PgPool, (business, job): (Uuid, Uuid)) {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t663.free.{job}.{business}");
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

/// Every task queued since the last call, claimed once.
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    claim(pool, owner, later, SignedDuration::from_secs(60), 1000)
        .await
        .expect("claim")
}

/// The tasks of `kind` whose payload's `key` is `id`.
fn of(tasks: &[Claimed], kind: &str, (key, id): (&str, Uuid)) -> Vec<Claimed> {
    let named = |t: &&Claimed| t.kind == kind && t.payload[key] == id.to_string();
    tasks.iter().filter(named).cloned().collect()
}

/// Runs a counting query with `$1` bound to `id`.
async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Job `job`'s status code now.
async fn status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("db");
    current(&mut db, job).await.expect("job").status.code()
}

/// Job `job`'s history arrows, oldest first, as `from>to` codes.
async fn history(pool: &PgPool, job: Uuid) -> Vec<String> {
    let sql = "SELECT from_status || '>' || to_status FROM jobs.status_history \
               WHERE job_id = $1 ORDER BY version";
    sqlx::query_scalar(sql)
        .bind(job)
        .fetch_all(pool)
        .await
        .expect("history")
}

/// Pick `pick`'s status code and whether its PaymentIntent (if any) has ended.
async fn ends(pool: &PgPool, pick: Uuid) -> (String, bool) {
    let sql = "SELECT status FROM picks.pick WHERE id = $1";
    let code: String = sqlx::query_scalar(sql)
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect(sql);
    let ended = "SELECT count(*) FROM payments.pick_intent WHERE pick_id = $1 \
                 AND ended_at IS NOT NULL";
    (code, count(pool, ended, pick).await == 1)
}

/// How many late-success refunds were queued for `pick` (FEE-021).
async fn late_refunds(pool: &PgPool, pick: Uuid) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'pick' = $1::text";
    sqlx::query_scalar(sql)
        .bind(pick)
        .bind(LATE_KIND)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Her PICK-046 or PICK-047 text events for `job`.
async fn her_texts(pool: &PgPool, job: Uuid) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'job' = $1::text \
               AND payload->>'template' IN ('pick_lapsed', 'pick_moved')";
    sqlx::query_scalar(sql)
        .bind(job)
        .bind(TEXT_KIND)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Whether `pick` has its release row.
async fn released(pool: &PgPool, pick: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    is_released(&mut db, pick).await.expect("release row")
}

/// `payment_returned` for `job` at `at`, committed.
async fn returned(pool: &PgPool, job: Uuid, at: Timestamp) {
    let mut tx = pool.begin().await.expect("tx");
    payment_returned(&mut tx, job, at)
        .await
        .expect("payment returned");
    tx.commit().await.expect("commit");
}

struct FakeStripe {
    pick: Uuid,
    reports: &'static str,
    confirms: &'static str,
}

impl FakeStripe {
    fn intent(&self, id: &str, state: &str) -> PaymentIntent {
        PaymentIntent {
            id: id.to_owned(),
            status: state.to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_fake663".to_owned(),
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
        Ok("cus_fake663".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(self.intent(id, self.reports))
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
        if self.confirms == "unknown" {
            return Err(AdapterError::OutcomeUnknown);
        }
        Ok(self.intent(intent, self.confirms))
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, PickReleaser>>;

/// The webhook state over the real sink, with Stripe reporting the intent as succeeded.
fn hooks(pool: &PgPool, pick: Uuid) -> Hooks {
    let none: [(String, String); 0] = [];
    let clock = Arc::new(ManualClock::new(now()));
    let stripe = FakeStripe {
        pick,
        reports: "succeeded",
        confirms: "succeeded",
    };
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe,
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_fake663".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    })
}

/// One signed `payment_intent.succeeded` delivery for `pick` from the stand-in's host.
async fn deliver(pool: &PgPool, pick: Uuid, event: &str) {
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
    let reply = stripe_webhook_routes(hooks(pool, pick))
        .oneshot(request)
        .await;
    assert_eq!(reply.expect("answer").status(), StatusCode::OK);
}

/// TD-JOB-18 (JOB-002, JOB-004, JOB-005): the press's move writes one history and one audit row,
/// a second call moves nothing, and a cancelled job answers `Lost`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_confirm_move_once_then_already_then_lost(pool: PgPool) {
    let (_, job, pick) = picked(&pool).await;
    let audits = "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 \
                  AND action = 'status_changed'";
    let (rows, audited) = (
        history(&pool, job).await.len(),
        count(&pool, audits, job).await,
    );
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert_eq!(history(&pool, job).await.len(), rows + 1);
    assert_eq!(count(&pool, audits, job).await, audited + 1);
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Already);
    assert_eq!(
        history(&pool, job).await.len(),
        rows + 1,
        "nothing more moved"
    );
    moved(
        &pool,
        job,
        (JobStatus::PaymentPending, JobStatus::Cancelled),
    )
    .await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Lost);
}

/// TD-JOB-18 (PICK-022, JOB-015, D-2930): a paid clearing settles the job to Connected in the
/// settle's transaction, so the release's claim queues the day-3 and day-14 check-ins.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_paid_clearing_connects_and_queues_the_check_ins(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "succeeded").await;
    tasks(&pool).await;
    deliver(&pool, pick, "evt_fake663a").await;
    deliver(&pool, pick, "evt_fake663g").await;
    assert_eq!(status(&pool, job).await, "connected");
    assert!(released(&pool, pick).await);
    let queued = tasks(&pool).await;
    assert_eq!(
        of(&queued, CHECK_IN_KIND, ("job", job)).len(),
        2,
        "day 3 and day 14"
    );
    let last = history(&pool, job).await.pop();
    assert_eq!(last.as_deref(), Some("payment_pending>connected"));
    returned(&pool, job, hours(2)).await;
    assert_eq!(
        status(&pool, job).await,
        "connected",
        "a late wait's end does nothing"
    );
}

/// TD-JOB-18 (PICK-022, FEE-010): the $0 path settles the job to Connected the same way.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_the_zero_fee_path_connects(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    free_win(&pool, (business, job)).await;
    let mut db = pool.acquire().await.expect("db");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    drop(db);
    let sink = hooks(&pool, pick);
    let done = sink
        .sink
        .settle_and_release(cleared.expect("cleared"))
        .await;
    assert!(!matches!(done.expect("released"), Released::MovedOn));
    assert_eq!(status(&pool, job).await, "connected");
    assert!(released(&pool, pick).await);
}

/// TD-JOB-18 (PICK-022, PRIV-005): a clearing for a pick already ended (planted by hand) is
/// `Left`: the job stays in Payment pending and nothing is released.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_clearing_for_an_ended_pick_is_left(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    free_win(&pool, (business, job)).await;
    let mut db = pool.acquire().await.expect("db");
    let cleared = no_fee_release(&mut db, pick, (business, job))
        .await
        .expect("cleared");
    let planted = "UPDATE picks.pick SET ended_at = now() WHERE id = $1";
    sqlx::query(planted)
        .bind(pick)
        .execute(&mut *db)
        .await
        .expect("ended");
    let mut tx = pool.begin().await.expect("tx");
    let done = settle(&mut tx, &cleared).await.expect("settled");
    tx.commit().await.expect("commit");
    assert_eq!(done, Settled::Left);
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert!(!released(&pool, pick).await);
}

/// TD-JOB-18 (PICK-048): an auto-confirmed pick, its job still in Awaiting confirm, gets both
/// moves in order at the settle (two history rows).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_an_auto_confirmed_pick_gets_both_moves(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    let before = history(&pool, job).await.len();
    press(&pool, (business, job, pick), "succeeded").await;
    deliver(&pool, pick, "evt_fake663b").await;
    let rows = history(&pool, job).await;
    assert_eq!(
        rows[before..],
        [
            "awaiting_confirm>payment_pending",
            "payment_pending>connected"
        ]
    );
    assert!(released(&pool, pick).await);
}

/// TD-JOB-18 (PRIV-005, FEE-021): a clearing for a job back in Quotes ready (planted by hand) is
/// `Left`: nothing released, one refund queued, the pick `payment_failed`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_clearing_for_a_quotes_ready_job_is_left_and_refunded(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    moved(
        &pool,
        job,
        (JobStatus::PaymentPending, JobStatus::QuotesReady),
    )
    .await;
    press(&pool, (business, job, pick), "succeeded").await;
    deliver(&pool, pick, "evt_fake663c").await;
    assert!(!released(&pool, pick).await);
    assert_eq!(late_refunds(&pool, pick).await, 1);
    assert_eq!(ends(&pool, pick).await, ("payment_failed".to_owned(), true));
}

/// TD-JOB-18 (PRIV-005, FEE-021): a clearing for a cancelled job is `Left`: nothing released,
/// one refund, the pick `payment_failed`, and the cancelled job's homeowner gets no text.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_clearing_for_a_cancelled_job_is_left_with_no_text(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    moved(
        &pool,
        job,
        (JobStatus::AwaitingConfirm, JobStatus::Cancelled),
    )
    .await;
    press(&pool, (business, job, pick), "succeeded").await;
    deliver(&pool, pick, "evt_fake663d").await;
    deliver(&pool, pick, "evt_fake663e").await;
    assert!(!released(&pool, pick).await);
    assert_eq!(late_refunds(&pool, pick).await, 1);
    assert_eq!(ends(&pool, pick).await, ("payment_failed".to_owned(), true));
    assert_eq!(status(&pool, job).await, "cancelled");
    assert_eq!(her_texts(&pool, job).await, 0);
}

/// TD-JOB-18 (JOB-006, JOB-014, FEE-020, PICK-046): the Payment pending timer moves nothing and
/// queues one `picks.payment_returned` task; with a declined attempt and no pay timer left
/// (the card-fix timer ended by hand) its run ends the pick and its intent, returns the job to
/// Quotes ready and queues her PICK-046 text.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_the_timer_hands_a_declined_pick_back(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "requires_payment_method").await;
    let planted = "UPDATE payments.pay_timer SET ended_at = now() WHERE pick_id = $1";
    sqlx::query(planted)
        .bind(pick)
        .execute(&pool)
        .await
        .expect("timers ended");
    let queued = tasks(&pool).await;
    let live = |t: &&Claimed| t.payload["status"] == "payment_pending";
    let timer = of(&queued, TIMER_KIND, ("job", job));
    let timer = timer.iter().find(live).expect("the live timer").clone();
    run_timer(pool.clone(), timer).await.expect("timer");
    assert_eq!(
        status(&pool, job).await,
        "payment_pending",
        "the timer moves nothing"
    );
    let mut asked = of(&tasks(&pool).await, PAYMENT_RETURNED_KIND, ("job", job));
    assert_eq!(asked.len(), 1);
    let task = asked.remove(0);
    run_payment_returned(pool.clone(), task, hours(2))
        .await
        .expect("returned");
    assert_eq!(ends(&pool, pick).await, ("payment_failed".to_owned(), true));
    assert_eq!(status(&pool, job).await, "quotes_ready");
    assert_eq!(her_texts(&pool, job).await, 1);
}

/// What holds a pick in one held case.
#[derive(Clone, Copy)]
enum Hold {
    CardFix,
    StripeDown,
    NeedsAction,
    Processing,
    Free,
    Paused,
}

/// Plants `hold` on the confirmed pick of `ids`.
async fn plant(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid), hold: Hold) {
    let ids = (business, job, pick);
    match hold {
        Hold::CardFix => press(pool, ids, "requires_payment_method").await,
        Hold::StripeDown => press(pool, ids, "unknown").await,
        Hold::NeedsAction => {
            press(pool, ids, "requires_action").await;
            let planted = "UPDATE payments.pay_timer SET ended_at = now() WHERE pick_id = $1";
            sqlx::query(planted)
                .bind(pick)
                .execute(pool)
                .await
                .expect("timers");
        }
        Hold::Processing => press(pool, ids, "processing").await,
        Hold::Free => free_win(pool, (business, job)).await,
        Hold::Paused => {
            let mut db = pool.acquire().await.expect("db");
            let owner = Actor::Owner(Uuid::from_u128(0x0e));
            set_switch(&mut db, Switch::Charging, true, owner)
                .await
                .expect("paused");
        }
    }
}

/// TD-JOB-18 (JOB-014, FEE-018, FEE-020, FEE-038): a held pick restarts the wait from the run
/// and the job stays in Payment pending; only a $0 decision queues a held success, whose run
/// releases the pick and connects the job.
async fn stays_held(pool: PgPool, hold: Hold) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    plant(&pool, (business, job, pick), hold).await;
    tasks(&pool).await;
    returned(&pool, job, hours(2)).await;
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert_eq!(ends(&pool, pick).await.0, "awaiting_confirm");
    let mut db = pool.acquire().await.expect("db");
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.timer_from, hours(2), "the wait restarted");
    returned(&pool, job, hours(1)).await;
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(
        row.timer_from,
        hours(2),
        "an earlier run never shortens the wait"
    );
    let held = of(&tasks(&pool).await, HELD_SUCCESS_KIND, ("pick", pick));
    assert_eq!(held.len(), usize::from(matches!(hold, Hold::Free)));
    if let Some(task) = held.first() {
        run_held_release(&pool, &hooks(&pool, pick), &task.payload)
            .await
            .expect("held success");
        assert_eq!(status(&pool, job).await, "connected", "the $0 hold settles");
        assert!(released(&pool, pick).await);
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_card_fix_timer_holds(pool: PgPool) {
    stays_held(pool, Hold::CardFix).await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_stripe_down_timer_holds(pool: PgPool) {
    stays_held(pool, Hold::StripeDown).await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_needs_action_attempt_holds(pool: PgPool) {
    stays_held(pool, Hold::NeedsAction).await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_processing_attempt_holds(pool: PgPool) {
    stays_held(pool, Hold::Processing).await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_zero_fee_decision_holds(pool: PgPool) {
    stays_held(pool, Hold::Free).await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_paused_charges_hold(pool: PgPool) {
    stays_held(pool, Hold::Paused).await;
}

/// TD-JOB-18 (PICK-022): a `succeeded` attempt whose webhook never came queues one held success,
/// whose run releases the pick once and connects the job; a second run releases nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_held_success_releases_once_and_connects(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "succeeded").await;
    tasks(&pool).await;
    returned(&pool, job, hours(2)).await;
    assert_eq!(status(&pool, job).await, "payment_pending");
    let mut held = of(&tasks(&pool).await, HELD_SUCCESS_KIND, ("pick", pick));
    assert_eq!(held.len(), 1);
    let task = held.remove(0);
    let hooks = hooks(&pool, pick);
    for _ in 0..2 {
        run_held_release(&pool, &hooks, &task.payload)
            .await
            .expect("held success");
    }
    assert_eq!(status(&pool, job).await, "connected");
    assert!(released(&pool, pick).await);
    let notices = of(&tasks(&pool).await, BUSINESS_NOTICE_KIND, ("pick", pick));
    assert_eq!(notices.len(), 1, "released once");
}

/// TD-JOB-18 (D-4349): a pick held over 24 hours raises one High alert; a later run, even after
/// it is acknowledged (resolved by hand: `acknowledge_alert` takes critical alerts only), raises
/// no second.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_long_hold_alerts_once_ever(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    plant(&pool, (business, job, pick), Hold::CardFix).await;
    let alerts = "SELECT count(*) FROM ops.alert WHERE subject_id = $1 \
                  AND kind = 'picks.payment_held_long' AND severity = 'high'";
    returned(&pool, job, hours(23)).await;
    assert_eq!(count(&pool, alerts, pick).await, 0, "not yet");
    returned(&pool, job, hours(25)).await;
    assert_eq!(count(&pool, alerts, pick).await, 1);
    let planted = "UPDATE ops.alert SET resolved_at = $3 WHERE subject_id = $1 AND kind = $2";
    let acked = sqlx::query(planted)
        .bind(pick)
        .bind(HELD_LONG_ALERT)
        .bind(hours(25).to_sqlx())
        .execute(&pool)
        .await
        .expect("acknowledged by hand");
    assert_eq!(acked.rows_affected(), 1);
    returned(&pool, job, hours(26)).await;
    assert_eq!(count(&pool, alerts, pick).await, 1, "never a second");
    assert_eq!(status(&pool, job).await, "payment_pending");
}

/// `end_pending` for a payment failure on `pick` of `job` at `at`, committed.
async fn ended(pool: &PgPool, (pick, job): (Uuid, Uuid), at: Timestamp) -> Backup {
    let mut tx = pool.begin().await.expect("tx");
    let by = (Actor::System, at);
    let backup = end_pending(&mut tx, (pick, job), PendingEnd::PaymentFailed, by).await;
    tx.commit().await.expect("commit");
    backup.expect("end pending")
}

/// TD-JOB-18 (JOB-004): `end_pending` on a pick no longer waiting (planted as expired) changes
/// nothing and ends no intent.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_end_pending_on_a_pick_no_longer_waiting_changes_nothing(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "processing").await;
    let planted = "UPDATE picks.pick SET status = 'expired', ended_at = $2 WHERE id = $1";
    sqlx::query(planted)
        .bind(pick)
        .bind(now().to_sqlx())
        .execute(&pool)
        .await
        .expect("expired");
    let rows = history(&pool, job).await.len();
    assert_eq!(ended(&pool, (pick, job), hours(1)).await, Backup::Done);
    assert_eq!(ends(&pool, pick).await, ("expired".to_owned(), false));
    assert_eq!(history(&pool, job).await.len(), rows);
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Lost);
    returned(&pool, job, hours(2)).await;
    assert_eq!(
        status(&pool, job).await,
        "quotes_ready",
        "no waiting pick: back at once"
    );
}

/// TD-JOB-18 (PICK-033, FEE-021): `end_pending` for "Can't take it" or ineligibility cancels the
/// pick with that reason, ends its intent and returns the job to Quotes ready.
async fn ends_for(pool: PgPool, reason: PendingEnd, code: &str) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "requires_payment_method").await;
    let mut tx = pool.begin().await.expect("tx");
    let ended = end_pending(&mut tx, (pick, job), reason, (Actor::System, hours(1))).await;
    tx.commit().await.expect("commit");
    assert_eq!(ended.expect("ended"), Backup::NoBackup);
    assert_eq!(ends(&pool, pick).await, ("cancelled".to_owned(), true));
    let sql = "SELECT end_reason FROM picks.pick WHERE id = $1";
    let why: String = sqlx::query_scalar(sql)
        .bind(pick)
        .fetch_one(&pool)
        .await
        .expect(sql);
    assert_eq!(why, code);
    assert_eq!(status(&pool, job).await, "quotes_ready");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_end_pending_for_cant_take(pool: PgPool) {
    ends_for(pool, PendingEnd::CantTake, "cant_take").await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_end_pending_for_ineligible(pool: PgPool) {
    ends_for(pool, PendingEnd::Ineligible, "ineligible").await;
}

/// TD-JOB-18 (FEE-019, FEE-021, PICK-046): `end_pending` on a job in Payment pending ends the pick
/// and its intent through `payments`' own pick end and returns the job to Quotes ready with her
/// text; a success recorded after it by the charge check, with no webhook, is refunded once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_end_pending_then_a_late_success_is_refunded_once(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "processing").await;
    assert_eq!(ended(&pool, (pick, job), hours(1)).await, Backup::NoBackup);
    assert_eq!(ends(&pool, pick).await, ("payment_failed".to_owned(), true));
    assert_eq!(status(&pool, job).await, "quotes_ready");
    assert_eq!(her_texts(&pool, job).await, 1);
    assert_eq!(late_refunds(&pool, pick).await, 0, "nothing charged yet");
    let stripe = FakeStripe {
        pick,
        reports: "succeeded",
        confirms: "succeeded",
    };
    let payload = json!({"pick": pick, "try": 1, "round": 0});
    for _ in 0..2 {
        run_charge_check(&pool, &stripe, &payload, hours(2))
            .await
            .expect("check");
    }
    assert_eq!(late_refunds(&pool, pick).await, 1, "refunded once");
    assert!(!released(&pool, pick).await);
}

/// TD-JOB-18 (OPS-024, FEE-038, T408): with safe mode on a success is not settled and the job
/// stays in Payment pending past the hour; when safe mode ends the held release settles it to
/// Connected and releases once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_safe_mode_holds_then_the_held_release_connects(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    press(&pool, (business, job, pick), "succeeded").await;
    let alert = safe_mode_starts(&pool).await;
    tasks(&pool).await;
    deliver(&pool, pick, "evt_fake663f").await;
    let mut parked = of(&tasks(&pool).await, HELD_RELEASE_KIND, ("pick", pick));
    assert_eq!(parked.len(), 1, "one held release");
    returned(&pool, job, hours(2)).await;
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert!(!released(&pool, pick).await);
    let mut held = of(&tasks(&pool).await, HELD_SUCCESS_KIND, ("pick", pick));
    assert_eq!(held.len(), 1);
    safe_mode_ends(&pool, alert).await;
    let hooks = hooks(&pool, pick);
    for task in [parked.remove(0), held.remove(0)] {
        run_held_release(&pool, &hooks, &task.payload)
            .await
            .expect("released");
    }
    assert_eq!(status(&pool, job).await, "connected");
    assert!(released(&pool, pick).await);
    let notices = of(&tasks(&pool).await, BUSINESS_NOTICE_KIND, ("pick", pick));
    assert_eq!(notices.len(), 1, "released once");
}

/// Clears `pick` on the $0 path for `business` and `job` and settles it through the sink.
async fn free_settle(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) -> Released {
    let mut db = pool.acquire().await.expect("db");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    drop(db);
    let sink = hooks(pool, pick);
    let done = sink
        .sink
        .settle_and_release(cleared.expect("cleared"))
        .await;
    done.expect("settled")
}

/// TD-JOB-18 (PICK-022, PRIV-005, JOB-004): a clearing for a pick in another status (planted as
/// `charging`), or for a pick `picks` has no row for (its fixed $0 fee planted for an unknown
/// pick), settles and moves nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_18_a_clearing_for_an_unknown_or_other_status_pick_moves_nothing(pool: PgPool) {
    let (business, job, pick) = picked(&pool).await;
    assert_eq!(confirmed(&pool, (pick, job)).await, ConfirmMove::Moved);
    free_win(&pool, (business, job)).await;
    let planted = "UPDATE picks.pick SET status = 'charging' WHERE id = $1";
    sqlx::query(planted)
        .bind(pick)
        .execute(&pool)
        .await
        .expect("charging");
    let left = free_settle(&pool, (business, job, pick)).await;
    assert!(matches!(left, Released::MovedOn));
    assert_eq!(status(&pool, job).await, "payment_pending");
    let unknown = Uuid::from_u128(0x663_0bad);
    let planted = "INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, \
                   amount_cents, terms_version, fixed_at) \
                   VALUES ($1, $2, $3, 'tiers', 'fee_free', 0, 1, $4)";
    sqlx::query(planted)
        .bind(unknown)
        .bind(business)
        .bind(job)
        .bind(now().to_sqlx())
        .execute(&pool)
        .await
        .expect("fee of an unknown pick");
    let left = free_settle(&pool, (business, job, unknown)).await;
    assert!(matches!(left, Released::MovedOn));
    assert_eq!(status(&pool, job).await, "payment_pending");
    assert!(!released(&pool, unknown).await);
}

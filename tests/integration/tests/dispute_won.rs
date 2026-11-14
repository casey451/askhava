//! TD-FEE-65 (FEE-027, FEE-028, FEE-029, FEE-030, FEE-050; T353, D-2625, D-3134): on a fresh
//! database, after a submitted $75 dispute, a `charge.dispute.closed` with `status=won` appends
//! one `countering-fee-return` and one `dispute-reversal`, leaving the money view's dispute fees at
//! Stripe's $15 dispute fee and the net disputed amount 0; the same event again, or a second `won`
//! event, appends nothing; `lost` changes no ledger row; a won dispute with no countering fee
//! appends no return; the nightly reconcile over a won dispute finds no difference.
//!
//! Stripe is a fake: the webhook is signed with a fake secret and the reconcile lists one fake
//! charge. All IDs are fake.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use askhava_adapters::api::PaymentsAdapter;
use askhava_adapters::api::{AdapterError, AdapterMode, Charge, IdempotencyKey, PaymentIntent};
use askhava_core::api::{ClientIpPolicy, ManualClock};
use askhava_payments::api::{
    DISPUTE_CLOSED_ACTION, DISPUTE_FEE_CENTS, DISPUTE_REVERSAL_CODE, DisputeOpened, EntryKind,
    MISMATCH_ALERT, MoneyView, ReconcileStripe, Releasable, ReleaseError, ReleaseSink,
    STRIPE_WEBHOOK_PATH, StripeAccount, StripeCharge, StripeWebhook, money_view, reconcile,
    record_dispute, record_owner_submission, stripe_webhook_routes,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const BUSINESS: Uuid = Uuid::from_u128(0xb353_0001);
const PICK: Uuid = Uuid::from_u128(0x353_0042);
const JOB: Uuid = Uuid::from_u128(0x353_0142);
const INTENT: &str = "pi_FakeT353";
const DISPUTE: &str = "dp_FakeT353";
const AMOUNT: i64 = 7500;
const NOW: i64 = 1_794_100_000;
const DAY: i64 = 86_400;
const SECRET: &str = "whsec_fake_secret_t353";
const STAND_IN: [u8; 4] = [127, 0, 0, 1];

/// Nothing is charged, refunded or read here; the reconcile lists one disputed $75 charge.
struct FakeStripe;

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }
    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("nothing is charged here")
    }
    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("no intent is read here")
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("nothing is refunded here")
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        Ok(vec![StripeCharge {
            id: "ch_FakeT353".to_owned(),
            payment_intent: INTENT.to_owned(),
            pick: Some(PICK),
            amount_cents: AMOUNT,
            refunded_cents: 0,
            dispute: Some((DISPUTE.to_owned(), AMOUNT)),
            balance_transaction: "txn_FakeT353".to_owned(),
            fee_cents: 248,
        }])
    }
}

/// No release happens here.
#[derive(Clone)]
struct Sink;

impl ReleaseSink for Sink {
    async fn release(&self, _: Releasable) -> Result<(), ReleaseError> {
        panic!("nothing is released here")
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, Sink>>;

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn hooks(pool: PgPool) -> Hooks {
    let none: [(String, String); 0] = [];
    Arc::new(StripeWebhook {
        pool,
        stripe: FakeStripe,
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_FakeT353".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::from(STAND_IN),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(at(0))),
        sink: Sink,
    })
}

/// Sends `charge.dispute.closed` event `id` for the dispute with `status`, signed.
async fn closed(hooks: &Hooks, id: &str, status: &str) -> StatusCode {
    let object = json!({"id": DISPUTE, "status": status, "amount": AMOUNT});
    let kind = "charge.dispute.closed";
    let body = json!({"id": id, "type": kind, "livemode": false, "data": {"object": object}});
    let body = body.to_string();
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
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((STAND_IN, 443))));
    let reply = stripe_webhook_routes(hooks.clone()).oneshot(request).await;
    reply.expect("answer").status()
}

/// One paid $75 pick with its FEE-042 evidence record, disputed in full; its evidence submitted
/// by Casey (and so its countering fee recorded) when `submitted`.
async fn disputed(pool: &PgPool, submitted: bool) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT353', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(INTENT)
            .bind(NOW - DAY)
            .execute(pool)
            .await
            .expect("seed");
    }
    let opened = DisputeOpened {
        dispute: DISPUTE,
        payment_intent: INTENT,
        amount_cents: AMOUNT,
        due_at: at(10 * DAY),
    };
    assert!(record_dispute(pool, &opened, at(0)).await.expect("dispute"));
    if submitted {
        let mut conn = pool.acquire().await.expect("conn");
        let fee = record_owner_submission(&mut conn, DISPUTE, at(DAY)).await;
        assert!(fee.expect("submission"));
    }
}

async fn ledger(pool: &PgPool) -> Vec<(String, i64, Option<String>)> {
    sqlx::query_as("SELECT kind, amount_cents, stripe_id FROM payments.ledger_entry ORDER BY seq")
        .fetch_all(pool)
        .await
        .expect("ledger")
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn view(pool: &PgPool) -> MoneyView {
    let now = Timestamp::now();
    let window = (
        now - SignedDuration::from_hours(24),
        now + SignedDuration::from_hours(24),
    );
    money_view(pool, window).await.expect("view")
}

fn entry(kind: &str, cents: i64) -> (String, i64, Option<String>) {
    (kind.to_owned(), cents, Some(DISPUTE.to_owned()))
}

/// TD-FEE-65 (FEE-028, FEE-030, FEE-050): a won, submitted dispute gets back its countering fee
/// and its amount, once however often Stripe sends the event or another `won` one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_65_a_won_dispute_returns_its_countering_fee_and_amount_once(pool: PgPool) {
    disputed(&pool, true).await;
    let before = view(&pool).await;
    assert_eq!(before.disputes, AMOUNT);
    assert_eq!(before.dispute_fees, 2 * DISPUTE_FEE_CENTS);
    let hooks = hooks(pool.clone());
    assert_eq!(closed(&hooks, "evt_FakeT353a", "won").await, StatusCode::OK);
    let rows = ledger(&pool).await;
    let expected = vec![
        entry("dispute", AMOUNT + DISPUTE_FEE_CENTS),
        entry("countering-fee", DISPUTE_FEE_CENTS),
        entry("countering-fee-return", DISPUTE_FEE_CENTS),
        entry(DISPUTE_REVERSAL_CODE, AMOUNT),
    ];
    assert_eq!(rows, expected);
    let after = view(&pool).await;
    assert_eq!(
        after.dispute_fees, DISPUTE_FEE_CENTS,
        "Stripe keeps its dispute fee"
    );
    assert_eq!(after.disputes, 0);
    assert_eq!(after.net(), -DISPUTE_FEE_CENTS);
    assert_eq!(closed(&hooks, "evt_FakeT353a", "won").await, StatusCode::OK);
    assert_eq!(closed(&hooks, "evt_FakeT353b", "won").await, StatusCode::OK);
    assert_eq!(
        ledger(&pool).await,
        expected,
        "the same dispute appends once"
    );
    let audits = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(count(&pool, audits, DISPUTE_CLOSED_ACTION).await, 1);
}

/// TD-FEE-65 (FEE-027, FEE-050): a lost dispute keeps its amount and fees; only the audit row and
/// the alert are written, as before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_65_a_lost_dispute_changes_no_ledger_row(pool: PgPool) {
    disputed(&pool, true).await;
    let before = ledger(&pool).await;
    let hooks = hooks(pool.clone());
    assert_eq!(
        closed(&hooks, "evt_FakeT353c", "lost").await,
        StatusCode::OK
    );
    assert_eq!(ledger(&pool).await, before);
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    assert_eq!(count(&pool, alerts, DISPUTE_CLOSED_ACTION).await, 1);
    assert_eq!(view(&pool).await.disputes, AMOUNT);
}

/// TD-FEE-65 (FEE-050): a dispute won with no evidence submitted had no countering fee, so none
/// is returned; its amount still comes back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_65_a_won_dispute_without_a_countering_fee_returns_none(pool: PgPool) {
    disputed(&pool, false).await;
    let hooks = hooks(pool.clone());
    assert_eq!(closed(&hooks, "evt_FakeT353d", "won").await, StatusCode::OK);
    let expected = vec![
        entry("dispute", AMOUNT + DISPUTE_FEE_CENTS),
        entry(DISPUTE_REVERSAL_CODE, AMOUNT),
    ];
    assert_eq!(ledger(&pool).await, expected);
    let after = view(&pool).await;
    assert_eq!(after.dispute_fees, DISPUTE_FEE_CENTS);
    assert_eq!(after.disputes, 0);
}

/// TD-FEE-65 (FEE-029, FEE-030, D-3134): the reconcile leaves a dispute the ledger shows won out
/// of both sides, so Stripe's listed dispute raises no difference, with no Stripe status read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_65_the_reconcile_over_a_won_dispute_finds_no_difference(pool: PgPool) {
    disputed(&pool, true).await;
    let hooks = hooks(pool.clone());
    assert_eq!(closed(&hooks, "evt_FakeT353e", "won").await, StatusCode::OK);
    let secret = SecretString::from("fake-id-secret-t353".to_owned());
    let report = reconcile(&pool, &FakeStripe, &secret, at(2 * DAY))
        .await
        .expect("run");
    assert!(report.matched(), "{report:?}");
    assert_eq!(report.stripe.disputed, 0);
    assert_eq!(report.ledger.disputed, 0);
    assert_eq!(report.ledger.charged, AMOUNT);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM ops.alert WHERE kind = $1",
            MISMATCH_ALERT
        )
        .await,
        0
    );
    let rows = ledger(&pool).await;
    let kinds: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert!(kinds.contains(&EntryKind::Charge.code()), "{kinds:?}");
}

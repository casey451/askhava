//! TD-FEE-43 (SEC-016, SEC-017, FEE-026, FEE-027, FEE-052; D-1754, D-2071): signed fake Stripe
//! events through the real webhook route on a fresh database. `refund.updated` succeeded, failed
//! and canceled, `charge.refund.updated`, `refund.failed`, `charge.dispute.created`,
//! `charge.dispute.closed`, `radar.early_fraud_warning.created` and `payment_intent.succeeded`
//! each reach their handler once, and the same event ID sent 100 times changes nothing after the
//! first; a failed-then-succeeded pair arriving in the other order leaves the state Stripe reports;
//! a database error, or a Stripe that cannot be asked, answers 500 and the resend is handled; an
//! event for an unknown object answers 200 and changes nothing. All IDs are fake.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, PaymentIntent,
    PaymentsAdapter, StripeRefund,
};
use askhava_core::api::{ClientIpPolicy, ManualClock};
use askhava_payments::api::{
    DISPUTE_CLOSED_ACTION, FRAUD_WARNING_KIND, Releasable, ReleaseError, ReleaseSink,
    STRIPE_WEBHOOK_PATH, StripeAccount, StripeWebhook, stripe_webhook_routes,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const NOW: i64 = 1_794_000_000;
const SECRET: &str = "whsec_fake_secret_t259";
const STAND_IN: [u8; 4] = [127, 0, 0, 1];

/// Refunds Stripe reports now, by `re_…`; an absent one is Stripe's 404, `re_FakeDown` unreachable.
/// PaymentIntent `pi_Fake<n>` has succeeded for pick `n`; `pi_Fake9` cannot be read.
struct FakeStripe(Mutex<HashMap<String, &'static str>>);

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
    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let pick = id.strip_prefix("pi_Fake").filter(|n| *n != "9");
        let pick = pick.ok_or(AdapterError::OutcomeUnknown)?;
        Ok(PaymentIntent {
            id: id.to_owned(),
            status: "succeeded".to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT259".to_owned(),
            metadata: IntentMetadata {
                pick_id: pick_of(pick.parse().expect("n")).to_string(),
            },
            livemode: Some(false),
        })
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("nothing is refunded here")
    }
    async fn read_refund(&self, id: &str) -> Result<StripeRefund, AdapterError> {
        if id == "re_FakeDown" {
            return Err(AdapterError::OutcomeUnknown);
        }
        let status = self.0.lock().expect("lock").get(id).copied();
        let status = status.ok_or(AdapterError::Vendor {
            status: 404,
            code: None,
        })?;
        Ok(StripeRefund {
            id: id.to_owned(),
            status: status.to_owned(),
            amount: Some(7500),
            currency: Some("usd".to_owned()),
            payment_intent: Some("pi_Fake1".to_owned()),
        })
    }
}

/// Every pick the route released.
#[derive(Default, Clone)]
struct Sink(Arc<Mutex<Vec<Uuid>>>);

impl ReleaseSink for Sink {
    async fn release(&self, cleared: Releasable) -> Result<(), ReleaseError> {
        self.0.lock().expect("lock").push(cleared.pick());
        Ok(())
    }
}

type Hooks = Arc<StripeWebhook<FakeStripe, Sink>>;

fn pick_of(n: u128) -> Uuid {
    Uuid::from_u128(0x259_0000 + n)
}

fn hooks(pool: PgPool, refunds: &[(&str, &'static str)], sink: Sink) -> Hooks {
    let reads = refunds
        .iter()
        .map(|(id, s)| ((*id).to_owned(), *s))
        .collect();
    let none: [(String, String); 0] = [];
    Arc::new(StripeWebhook {
        pool,
        stripe: FakeStripe(Mutex::new(reads)),
        secret: SecretString::from(SECRET),
        account: StripeAccount {
            live: false,
            account: "acct_FakeT259".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::from(STAND_IN),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: Arc::new(ManualClock::new(Timestamp::from_second(NOW).expect("t"))),
        sink,
    })
}

/// Sends event `id` of `kind` about `object`, signed with the endpoint's secret.
async fn send(hooks: &Hooks, id: &str, kind: &str, object: Value) -> StatusCode {
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
    let peer = SocketAddr::from((STAND_IN, 443));
    request.extensions_mut().insert(ConnectInfo(peer));
    let reply = stripe_webhook_routes(hooks.clone()).oneshot(request).await;
    reply.expect("answer").status()
}

/// Paid pick `n` of $75 (PaymentIntent `pi_Fake<n>`), with a `pending` refund `re` when given.
async fn charged(pool: &PgPool, n: u128, re: Option<&str>) {
    let sqls = [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $1, $1, 7500, $2, false, to_timestamp($3))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $1, 'succeeded', to_timestamp($3), to_timestamp($3))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $1, $1, $1, 1, 'pick-' || $1::text || '-try1',
           'cus_FakeT259', $2, 1, 7500, to_timestamp($3), to_timestamp($3))",
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, stripe_refund_id,
           requested_at) SELECT $1, 'unreachable', 'pending', 1, $4, to_timestamp($3) WHERE $4 IS NOT NULL",
    ];
    for sql in sqls {
        sqlx::query(sql)
            .bind(pick_of(n))
            .bind(format!("pi_Fake{n}"))
            .bind(NOW - 86_400)
            .bind(re)
            .execute(pool)
            .await
            .expect("seed");
    }
}

/// Counts of every row an event can write, and each refund's status.
async fn snapshot(pool: &PgPool) -> Vec<String> {
    let sql = "SELECT 'ledger ' || kind || ' ' || count(*) FROM payments.ledger_entry GROUP BY kind
       UNION ALL SELECT 'alert ' || kind || ' ' || count(*) FROM ops.alert GROUP BY kind
       UNION ALL SELECT 'task ' || kind || ' ' || count(*) FROM core.task GROUP BY kind
       UNION ALL SELECT 'audit ' || action || ' ' || count(*) FROM ops.audit_log GROUP BY action
       UNION ALL SELECT 'dispute ' || stripe_dispute_id FROM payments.dispute
       UNION ALL SELECT 'refund ' || stripe_refund_id || ' ' || status FROM payments.refund
       UNION ALL SELECT 'pick ' || pick_id || ' ' || status FROM payments.charge_attempt
       ORDER BY 1";
    sqlx::query_scalar(sql)
        .fetch_all(pool)
        .await
        .expect("snapshot")
}

fn has(rows: &[String], line: &str) -> bool {
    rows.iter().any(|r| r == line)
}

/// The seven dispatched events, each `(id, type, object)`.
fn events() -> Vec<(&'static str, &'static str, Value)> {
    let refund = |id: &str, status: &str| json!({"id": id, "status": status});
    vec![
        (
            "evt_Fake1",
            "refund.updated",
            refund("re_FakeS", "succeeded"),
        ),
        ("evt_Fake2", "refund.updated", refund("re_FakeF", "failed")),
        (
            "evt_Fake3",
            "charge.refund.updated",
            refund("re_FakeC", "canceled"),
        ),
        ("evt_Fake4", "refund.failed", refund("re_FakeX", "failed")),
        ("evt_Fake5", "refund.updated", refund("re_FakeP", "pending")),
        (
            "evt_Fake6",
            "charge.dispute.created",
            json!({"id": "dp_FakeD", "amount": 7500,
            "payment_intent": "pi_Fake6", "evidence_details": {"due_by": NOW + 864_000}}),
        ),
        (
            "evt_Fake7",
            "charge.dispute.closed",
            json!({"id": "dp_FakeD", "status": "lost"}),
        ),
        (
            "evt_Fake8",
            "radar.early_fraud_warning.created",
            json!({"id": "issfr_FakeW", "payment_intent": "pi_Fake7"}),
        ),
        (
            "evt_Fake9",
            "payment_intent.succeeded",
            json!({"id": "pi_Fake8", "metadata": {"pick_id": pick_of(8).to_string()}}),
        ),
    ]
}

/// TD-FEE-43 (SEC-017, FEE-026, FEE-027, FEE-052): each event reaches its handler once; the same
/// event IDs sent 100 times change nothing after the first.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_each_event_reaches_its_handler_once(pool: PgPool) {
    for (n, re) in [
        (1, "re_FakeS"),
        (2, "re_FakeF"),
        (3, "re_FakeC"),
        (4, "re_FakeX"),
    ] {
        charged(&pool, n, Some(re)).await;
    }
    charged(&pool, 5, Some("re_FakeP")).await;
    for n in [6, 7, 8] {
        charged(&pool, n, None).await;
    }
    let sink = Sink::default();
    let hooks = hooks(pool.clone(), &[("re_FakeS", "succeeded")], sink.clone());
    let mut first = Vec::new();
    for round in 0..100 {
        for (id, kind, object) in events() {
            assert_eq!(send(&hooks, id, kind, object).await, StatusCode::OK, "{id}");
        }
        if round == 0 {
            first = snapshot(&pool).await;
        }
    }
    let rows = snapshot(&pool).await;
    assert_eq!(rows, first, "nothing changes after the first delivery");
    for line in [
        "refund re_FakeS succeeded",
        "refund re_FakeF failed",
        "refund re_FakeC canceled",
        "refund re_FakeX failed",
        "refund re_FakeP pending",
        "ledger refund 1",
        "ledger dispute 1",
        "dispute dp_FakeD",
        "alert payments.dispute_opened 1",
        &format!("alert {DISPUTE_CLOSED_ACTION} 1"),
        &format!("audit {DISPUTE_CLOSED_ACTION} 1"),
        &format!("task {FRAUD_WARNING_KIND} 1"),
    ] {
        assert!(has(&rows, line), "{line} in {rows:?}");
    }
    assert_eq!(*sink.0.lock().expect("lock"), vec![pick_of(8)]);
    let task: Value = sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
        .bind(FRAUD_WARNING_KIND)
        .fetch_one(&pool)
        .await
        .expect("task");
    assert_eq!(task, json!({"pick": pick_of(7)}));
    let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.webhook_event")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(recorded, 9);
}

/// TD-FEE-43 (FEE-026, D-1753): Stripe took refund `re` then failed it; whichever order the two
/// events arrive in, the late `succeeded` is re-read and the row ends `failed`, with no refund left
/// in the ledger and one alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_out_of_order_leaves_the_state_stripe_reports(pool: PgPool) {
    charged(&pool, 1, Some("re_FakeA")).await;
    charged(&pool, 2, Some("re_FakeB")).await;
    let reads = [("re_FakeA", "failed"), ("re_FakeB", "failed")];
    let hooks = hooks(pool.clone(), &reads, Sink::default());
    let ok = json!({"id": "re_FakeA", "status": "succeeded"});
    let failed = json!({"id": "re_FakeA", "status": "failed"});
    assert_eq!(
        send(&hooks, "evt_FakeA2", "refund.failed", failed).await,
        StatusCode::OK
    );
    assert_eq!(
        send(&hooks, "evt_FakeA1", "refund.updated", ok).await,
        StatusCode::OK
    );
    let ok = json!({"id": "re_FakeB", "status": "succeeded"});
    let failed = json!({"id": "re_FakeB", "status": "failed"});
    assert_eq!(
        send(&hooks, "evt_FakeB1", "refund.updated", ok).await,
        StatusCode::OK
    );
    assert_eq!(
        send(&hooks, "evt_FakeB2", "refund.failed", failed).await,
        StatusCode::OK
    );
    let rows = snapshot(&pool).await;
    for line in [
        "refund re_FakeA failed",
        "refund re_FakeB failed",
        "alert payments.refund_failed 2",
    ] {
        assert!(has(&rows, line), "{line} in {rows:?}");
    }
    assert!(
        !rows.iter().any(|r| r.starts_with("ledger refund")),
        "{rows:?}"
    );
}

/// TD-FEE-43 (SEC-017, FEE-026): a database that fails, or a Stripe that cannot be asked, answers
/// 500 with nothing written; the resend is then handled once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_a_database_error_is_resent_and_handled(pool: PgPool) {
    charged(&pool, 1, Some("re_FakeE")).await;
    charged(&pool, 9, None).await;
    let dead = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(1))
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("lazy pool");
    let failed = json!({"id": "re_FakeE", "status": "failed"});
    let down = hooks(dead, &[], Sink::default());
    let event = ("evt_FakeE1", "refund.failed", failed);
    let status = send(&down, event.0, event.1, event.2.clone()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let before = snapshot(&pool).await;
    assert!(has(&before, "refund re_FakeE pending"), "{before:?}");
    let sink = Sink::default();
    let up = hooks(pool.clone(), &[], sink.clone());
    let ok = json!({"id": "re_FakeDown", "status": "succeeded"});
    let status = send(&up, "evt_FakeE2", "refund.updated", ok).await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "Stripe could not be asked"
    );
    let paid = json!({"id": "pi_Fake9", "metadata": {"pick_id": pick_of(9).to_string()}});
    let status = send(&up, "evt_FakeE3", "payment_intent.succeeded", paid).await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the success is not re-read"
    );
    let paid = json!({"id": "pi_FakeNone", "metadata": {"pick_id": pick_of(9).to_string()}});
    let status = send(&up, "evt_FakeE4", "payment_intent.succeeded", paid).await;
    assert_eq!(status, StatusCode::OK, "another PaymentIntent is final");
    assert_eq!(snapshot(&pool).await, before, "none changed anything");
    assert!(sink.0.lock().expect("lock").is_empty());
    for _ in 0..2 {
        let status = send(&up, event.0, event.1, event.2.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    let after = snapshot(&pool).await;
    assert!(has(&after, "refund re_FakeE failed"), "{after:?}");
    assert!(has(&after, "alert payments.refund_failed 1"), "{after:?}");
}

/// TD-FEE-43 (FEE-026, FEE-027, FEE-052): events about a refund, dispute or charge `payments` does
/// not know answer 200 and change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_an_unknown_object_changes_nothing(pool: PgPool) {
    charged(&pool, 1, None).await;
    let before = snapshot(&pool).await;
    let hooks = hooks(pool.clone(), &[("re_FakeU", "succeeded")], Sink::default());
    let unknown = [
        (
            "evt_FakeU1",
            "refund.updated",
            json!({"id": "re_FakeNone", "status": "succeeded"}),
        ),
        (
            "evt_FakeU2",
            "refund.updated",
            json!({"id": "re_FakeU", "status": "succeeded"}),
        ),
        (
            "evt_FakeU3",
            "refund.failed",
            json!({"id": "re_FakeU", "status": "failed"}),
        ),
        (
            "evt_FakeU4",
            "charge.dispute.created",
            json!({"id": "dp_FakeU", "amount": 7500,
            "payment_intent": "pi_FakeNone", "evidence_details": {"due_by": NOW}}),
        ),
        (
            "evt_FakeU5",
            "charge.dispute.closed",
            json!({"id": "dp_FakeU", "status": "won"}),
        ),
        (
            "evt_FakeU6",
            "radar.early_fraud_warning.created",
            json!({"id": "issfr_FakeU", "payment_intent": "pi_FakeNone"}),
        ),
    ];
    for (id, kind, object) in unknown {
        assert_eq!(send(&hooks, id, kind, object).await, StatusCode::OK, "{id}");
    }
    assert_eq!(snapshot(&pool).await, before);
}

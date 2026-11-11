//! TD-FEE-43 (SEC-016, SEC-017, FEE-026, FEE-027, FEE-052; D-1754, D-2071): signed fake Stripe
//! events through the real webhook route on a fresh database. `refund.updated` succeeded, failed
//! and canceled, `charge.refund.updated`, `refund.failed`, `charge.dispute.created`,
//! `charge.dispute.closed`, `radar.early_fraud_warning.created` and `payment_intent.succeeded`
//! each reach their handler once, and the same event ID sent 100 times changes nothing after the
//! first; a failed-then-succeeded pair arriving in the other order leaves the state Stripe reports;
//! a database error, or a Stripe that cannot be asked, answers 500 and the resend is handled; an
//! event for an unknown object answers 200 and changes nothing but, for a refund, its event row; a
//! closed dispute not yet recorded answers 500; a handled event the route cannot read raises one
//! High alert per event ID (D-2073). TD-FEE-53 (SEC-017, PICK-022; T331, D-2289): a success whose
//! `metadata.pick_id` is missing, empty or not a UUID is recorded with one High unreadable alert
//! and releases nothing. TD-FEE-54 (FEE-052, D-2289): an early fraud warning before its pick's
//! evidence row answers 500 with one alert for the pick however often it is resent, then is acted
//! on once the row exists; one matching no pick commits its event row with one alert. All IDs are
//! fake.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, PaymentIntent,
    PaymentsAdapter,
};
use askhava_core::api::{ClientIpPolicy, ManualClock};
use askhava_payments::api::{
    DISPUTE_CLOSED_ACTION, FRAUD_WARNING_ALERT, FRAUD_WARNING_KIND, Releasable, ReleaseError,
    ReleaseSink, STRIPE_WEBHOOK_PATH, StripeAccount, StripeRefund, StripeWebhook,
    UNREADABLE_EVENT_ALERT, stripe_webhook_routes,
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
    opened(pool, n).await;
    paid(pool, n, re).await;
}

/// Pick `n`'s PaymentIntent `pi_Fake<n>` of $75, with no attempt or evidence row yet.
async fn opened(pool: &PgPool, n: u128) {
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $1, $1, 7500, $2, false, to_timestamp($3))",
    )
    .bind(pick_of(n))
    .bind(format!("pi_Fake{n}"))
    .bind(NOW - 86_400)
    .execute(pool)
    .await
    .expect("seed");
}

/// Pick `n`'s succeeded attempt and evidence row, with a `pending` refund `re` when given.
async fn paid(pool: &PgPool, n: u128, re: Option<&str>) {
    let sqls = [
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

/// Counts of every row an event can write (`events <n>` for `payments.webhook_event`), and each
/// refund's status.
async fn snapshot(pool: &PgPool) -> Vec<String> {
    let sql = "SELECT 'ledger ' || kind || ' ' || count(*) FROM payments.ledger_entry GROUP BY kind
       UNION ALL SELECT 'alert ' || kind || ' ' || count(*) FROM ops.alert GROUP BY kind
       UNION ALL SELECT 'task ' || kind || ' ' || count(*) FROM core.task GROUP BY kind
       UNION ALL SELECT 'audit ' || action || ' ' || count(*) FROM ops.audit_log GROUP BY action
       UNION ALL SELECT 'dispute ' || stripe_dispute_id FROM payments.dispute
       UNION ALL SELECT 'refund ' || stripe_refund_id || ' ' || status FROM payments.refund
       UNION ALL SELECT 'pick ' || pick_id || ' ' || status FROM payments.charge_attempt
       UNION ALL SELECT 'events ' || count(*) FROM payments.webhook_event
       ORDER BY 1";
    sqlx::query_scalar(sql)
        .fetch_all(pool)
        .await
        .expect("snapshot")
}

fn has(rows: &[String], line: &str) -> bool {
    rows.iter().any(|r| r == line)
}

/// The snapshot without its `events` line.
fn effects(rows: &[String]) -> Vec<&String> {
    rows.iter().filter(|r| !r.starts_with("events ")).collect()
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
        "events 9",
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
/// in the ledger and one alert per refund, even when a third event ID reports the same failure; a
/// `succeeded` that Stripe now reports `canceled` is recorded `canceled`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_out_of_order_leaves_the_state_stripe_reports(pool: PgPool) {
    charged(&pool, 1, Some("re_FakeA")).await;
    charged(&pool, 2, Some("re_FakeB")).await;
    charged(&pool, 3, Some("re_FakeC")).await;
    let reads = [
        ("re_FakeA", "failed"),
        ("re_FakeB", "failed"),
        ("re_FakeC", "canceled"),
    ];
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
        send(&hooks, "evt_FakeB2", "refund.failed", failed.clone()).await,
        StatusCode::OK
    );
    let again = send(&hooks, "evt_FakeB3", "refund.updated", failed).await;
    assert_eq!(
        again,
        StatusCode::OK,
        "a second event ID for the same failure"
    );
    let ok = json!({"id": "re_FakeC", "status": "succeeded"});
    let status = send(&hooks, "evt_FakeC1", "refund.updated", ok).await;
    assert_eq!(status, StatusCode::OK, "re-read: Stripe now says canceled");
    let rows = snapshot(&pool).await;
    for line in [
        "refund re_FakeA failed",
        "refund re_FakeB failed",
        "refund re_FakeC canceled",
        "alert payments.refund_failed 3",
        "events 6",
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
    let after = snapshot(&pool).await;
    assert_eq!(effects(&after), effects(&before), "none changed anything");
    assert!(
        has(&after, "events 1"),
        "the final refusal is recorded: {after:?}"
    );
    assert!(sink.0.lock().expect("lock").is_empty());
    for _ in 0..2 {
        let status = send(&up, event.0, event.1, event.2.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    let after = snapshot(&pool).await;
    assert!(has(&after, "refund re_FakeE failed"), "{after:?}");
    assert!(has(&after, "alert payments.refund_failed 1"), "{after:?}");
}

/// TD-FEE-43 (FEE-026, FEE-027, FEE-052, D-2073): events about a refund, dispute or charge
/// `payments` does not know answer 200 and change nothing; a refund no row holds still records its
/// event row (its handler ran and changed nothing), and a fraud warning on a charge no pick made
/// records its row with one alert (D-2289). A closed dispute not yet recorded answers 500, so
/// Stripe resends it.
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
            "evt_FakeU6",
            "radar.early_fraud_warning.created",
            json!({"id": "issfr_FakeU", "payment_intent": "pi_FakeNone"}),
        ),
    ];
    for (id, kind, object) in unknown {
        assert_eq!(send(&hooks, id, kind, object).await, StatusCode::OK, "{id}");
    }
    let closed = json!({"id": "dp_FakeU", "status": "won"});
    let status = send(&hooks, "evt_FakeU5", "charge.dispute.closed", closed).await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "resent until recorded"
    );
    let after = snapshot(&pool).await;
    let fraud = format!("alert {FRAUD_WARNING_ALERT} 1");
    let changed: Vec<_> = effects(&after)
        .into_iter()
        .filter(|r| **r != fraud)
        .collect();
    assert_eq!(changed, effects(&before));
    assert!(
        has(&after, &fraud),
        "the warning alerts (D-2289): {after:?}"
    );
    assert!(has(&after, "events 3"), "U2, U3 and U6 only: {after:?}");
}

/// TD-FEE-43 (FEE-027, D-2073): a handled event whose object lacks a field its handler needs (a
/// dispute with no evidence due date, a refund with a status Stripe never documented) answers 200
/// and raises one High alert per event ID, with no dispute or refund change.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_43_an_unreadable_event_alerts_casey_once(pool: PgPool) {
    charged(&pool, 1, Some("re_FakeR")).await;
    let hooks = hooks(pool.clone(), &[], Sink::default());
    let no_due = json!({"id": "dp_FakeR", "amount": 7500, "payment_intent": "pi_Fake1",
        "evidence_details": {"due_by": null}});
    let odd = json!({"id": "re_FakeR", "status": "done"});
    for _ in 0..3 {
        let status = send(
            &hooks,
            "evt_FakeR1",
            "charge.dispute.created",
            no_due.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let status = send(&hooks, "evt_FakeR2", "refund.updated", odd.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    let rows = snapshot(&pool).await;
    for line in [
        &format!("alert {UNREADABLE_EVENT_ALERT} 2"),
        "refund re_FakeR pending",
        "events 2",
    ] {
        assert!(has(&rows, line), "{line} in {rows:?}");
    }
    assert!(!rows.iter().any(|r| r.starts_with("dispute ")), "{rows:?}");
    let severity: String = sqlx::query_scalar("SELECT DISTINCT severity::text FROM ops.alert")
        .fetch_one(&pool)
        .await
        .expect("severity");
    assert_eq!(severity, "high");
}

/// The kind, severity and subject of every alert, oldest first.
async fn alerts(pool: &PgPool) -> Vec<(String, String, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT kind, severity::text, subject_id FROM ops.alert ORDER BY raised_at, kind",
    )
    .fetch_all(pool)
    .await
    .expect("alerts")
}

/// TD-FEE-53 (SEC-017, PICK-022, D-2289): a signed success whose `metadata.pick_id` is missing,
/// empty or not a UUID writes one event row and one High unreadable alert with no subject, answers
/// 200 and releases nothing; its resends change nothing; a well-formed success still releases
/// through the re-fetched PaymentIntent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_53_a_success_naming_no_pick_is_recorded_not_released(pool: PgPool) {
    charged(&pool, 1, None).await;
    let sink = Sink::default();
    let hooks = hooks(pool.clone(), &[], sink.clone());
    let unreadable = [
        ("evt_FakeP1", json!({"id": "pi_Fake1"})),
        (
            "evt_FakeP2",
            json!({"id": "pi_Fake1", "metadata": {"pick_id": ""}}),
        ),
        (
            "evt_FakeP3",
            json!({"id": "pi_Fake1", "metadata": {"pick_id": "not-a-pick"}}),
        ),
    ];
    let mut first = Vec::new();
    for round in 0..3 {
        for (id, object) in &unreadable {
            let status = send(&hooks, id, "payment_intent.succeeded", object.clone()).await;
            assert_eq!(status, StatusCode::OK, "{id}");
        }
        if round == 0 {
            first = snapshot(&pool).await;
        }
    }
    let rows = snapshot(&pool).await;
    assert_eq!(rows, first, "a resend changes nothing");
    let line = format!("alert {UNREADABLE_EVENT_ALERT} 3");
    assert!(has(&rows, &line) && has(&rows, "events 3"), "{rows:?}");
    let high = (UNREADABLE_EVENT_ALERT.to_owned(), "high".to_owned(), None);
    assert_eq!(alerts(&pool).await, vec![high.clone(), high.clone(), high]);
    assert!(sink.0.lock().expect("lock").is_empty(), "nothing released");
    let paid = json!({"id": "pi_Fake1", "metadata": {"pick_id": pick_of(1).to_string()}});
    let status = send(&hooks, "evt_FakeP4", "payment_intent.succeeded", paid).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(*sink.0.lock().expect("lock"), vec![pick_of(1)]);
}

/// TD-FEE-54 (FEE-052, SEC-017, D-2289): a warning whose PaymentIntent has a `pick_intent` row but
/// no evidence row answers 500, writes no event row and raises one High alert for its pick; a
/// second resend raises nothing new; once the evidence row exists the resend answers 200 and
/// queues `picks.fraud_warning` once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_54_a_warning_before_its_evidence_row_is_resent(pool: PgPool) {
    opened(&pool, 1).await;
    let hooks = hooks(pool.clone(), &[], Sink::default());
    let warning = json!({"id": "issfr_FakeE", "payment_intent": "pi_Fake1"});
    let kind = "radar.early_fraud_warning.created";
    let pick_alert = (
        FRAUD_WARNING_ALERT.to_owned(),
        "high".to_owned(),
        Some(pick_of(1)),
    );
    for resend in 0..2 {
        let status = send(&hooks, "evt_FakeW1", kind, warning.clone()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "resend {resend}");
        assert_eq!(
            alerts(&pool).await,
            vec![pick_alert.clone()],
            "resend {resend}"
        );
        let rows = snapshot(&pool).await;
        assert!(has(&rows, "events 0"), "no event row: {rows:?}");
        assert!(!rows.iter().any(|r| r.starts_with("task ")), "{rows:?}");
    }
    paid(&pool, 1, None).await;
    for _ in 0..2 {
        let status = send(&hooks, "evt_FakeW1", kind, warning.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    let rows = snapshot(&pool).await;
    let task = format!("task {FRAUD_WARNING_KIND} 1");
    assert!(has(&rows, &task) && has(&rows, "events 1"), "{rows:?}");
    assert_eq!(alerts(&pool).await, vec![pick_alert], "nothing new");
}

/// TD-FEE-54 (FEE-052, SEC-017, D-2289): a warning whose PaymentIntent matches no `pick_intent`
/// answers 200, commits its event row with one High alert and queues nothing; its resends change
/// nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_54_an_unmatched_warning_alerts_once_and_queues_nothing(pool: PgPool) {
    charged(&pool, 1, None).await;
    let hooks = hooks(pool.clone(), &[], Sink::default());
    let warning = json!({"id": "issfr_FakeN", "payment_intent": "pi_FakeNone"});
    for _ in 0..3 {
        let status = send(
            &hooks,
            "evt_FakeN1",
            "radar.early_fraud_warning.created",
            warning.clone(),
        );
        assert_eq!(status.await, StatusCode::OK);
    }
    let rows = snapshot(&pool).await;
    assert!(has(&rows, "events 1"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.starts_with("task ")), "{rows:?}");
    let high = (FRAUD_WARNING_ALERT.to_owned(), "high".to_owned(), None);
    assert_eq!(alerts(&pool).await, vec![high]);
}

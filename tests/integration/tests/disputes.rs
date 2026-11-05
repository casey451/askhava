//! TD-FEE-14, TD-FEE-15, TD-FEE-23, TD-FEE-26, TD-FEE-33 and TD-PRIV-12 (Stripe dispute case)
//! (FEE-026, FEE-027, FEE-042, FEE-050, FEE-052, PRIV-027; D-229, D-365, D-490, D-491, D-1542 to
//! D-1544): on a fresh database, a refund trigger replayed refunds once, a failed refund retries
//! an hour on with one alert, a disputed charge's refund is blocked with one alert; a dispute
//! webhook twice records one ledger dispute row of the amount plus $15 with one alert and an audit
//! row; a marked dispute's evidence, built from the charge's FEE-042 record with IDs and timestamps
//! only, goes in at due minus 2 days with the countering fee, an unmarked one waits for Casey with
//! the fee shown, and one Casey already submitted is not sent again; an early fraud warning
//! refunds an unreleased charge and only alerts on a released one.
//!
//! Stripe is a scripted fake; the receipt (TD-FEE-34), the FEE-050 condition and the pick and job
//! moves (TD-FEE-39) are T068's and T191's. All IDs are fake.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    DISPUTE_ACTION, DisputeOpened, DisputeStripe, FRAUD_WARNING_ALERT, MoneyEventError,
    REFUND_BLOCKED_ALERT, REFUND_FAILED_ALERT, RETRY_KIND, RefundOutcome, RefundReason,
    RefundTrigger, SUBMIT_KIND, early_fraud_warning, evidence_pack, list_disputes,
    mark_auto_submit, record_dispute, record_owner_submission, refund_failed, request_refund,
    run_dispute_submit, run_refund_retry,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb066_0001);
const PICK: Uuid = Uuid::from_u128(0x066_0042);
const JOB: Uuid = Uuid::from_u128(0x066_0142);
const INTENT: &str = "pi_FakeT066";
const DISPUTE: &str = "dp_FakeT066";
const NOW: i64 = 1_793_800_000;
const DAY: i64 = 86_400;
/// Contact values a leak would carry; no `payments` table holds any, so none may reach Stripe.
const SENTINELS: [&str; 4] = [
    "Pat Fakename",
    "928-555-0100",
    "1 Fake St",
    "pat@example.test",
];

/// Refunds answer from the script (then succeed); evidence submissions are recorded.
#[derive(Default)]
struct FakeStripe {
    refunds: Mutex<VecDeque<Result<String, AdapterError>>>,
    refund_calls: Mutex<u32>,
    submitted: Mutex<Vec<Vec<(&'static str, String)>>>,
}

fn stripe(refunds: Vec<Result<String, AdapterError>>) -> FakeStripe {
    FakeStripe {
        refunds: Mutex::new(refunds.into()),
        ..FakeStripe::default()
    }
}

impl FakeStripe {
    fn refund_calls(&self) -> u32 {
        *self.refund_calls.lock().expect("lock")
    }
}

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

    async fn create_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        assert_eq!(intent, INTENT);
        *self.refund_calls.lock().expect("lock") += 1;
        let next = self.refunds.lock().expect("lock").pop_front();
        next.unwrap_or_else(|| Ok("re_FakeT066".to_owned()))
    }
}

impl DisputeStripe for FakeStripe {
    async fn submit_evidence(
        &self,
        _: &IdempotencyKey,
        dispute: &str,
        form: &[(&'static str, String)],
    ) -> Result<(), AdapterError> {
        assert_eq!(dispute, DISPUTE);
        self.submitted.lock().expect("lock").push(form.to_vec());
        Ok(())
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t066".to_owned())
}

/// One paid pick of $75 with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT066', $4, 1, 7500, to_timestamp($5),
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
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn ledger(pool: &PgPool, kind: &str) -> Vec<i64> {
    sqlx::query_scalar("SELECT amount_cents FROM payments.ledger_entry WHERE kind = $1")
        .bind(kind)
        .fetch_all(pool)
        .await
        .expect("ledger")
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    count(pool, "SELECT count(*) FROM ops.alert WHERE kind = $1", kind).await
}

async fn task_at(pool: &PgPool, kind: &str) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT extract(epoch FROM run_at)::bigint FROM core.task WHERE kind = $1 ORDER BY run_at",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .expect("tasks")
}

fn unreachable() -> (Uuid, RefundTrigger) {
    (PICK, RefundTrigger::Rule(RefundReason::Unreachable))
}

async fn dispute(pool: &PgPool, due_in_days: i64) -> Result<bool, MoneyEventError> {
    let opened = DisputeOpened {
        dispute: DISPUTE,
        payment_intent: INTENT,
        amount_cents: 7500,
        due_at: at(due_in_days * DAY),
    };
    record_dispute(pool, &opened, at(0)).await
}

/// TD-FEE-14 (FEE-026): a refund trigger replayed or duplicated refunds once; nothing charged is
/// never refunded.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_replayed_trigger_refunds_once(pool: PgPool) {
    let fake = stripe(vec![]);
    let none = request_refund(&pool, &fake, &secret(), unreachable(), at(0)).await;
    assert_eq!(none, Err(MoneyEventError::NothingCharged));
    charged(&pool).await;
    let first = request_refund(&pool, &fake, &secret(), unreachable(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Refunded("re_FakeT066".to_owned())));
    for _ in 0..3 {
        let again = request_refund(&pool, &fake, &secret(), unreachable(), at(60)).await;
        assert_eq!(again, Ok(RefundOutcome::Unchanged));
    }
    let retry = json!({"pick": PICK, "try": 1});
    run_refund_retry(&pool, &fake, &secret(), &retry, at(3600))
        .await
        .expect("retry");
    assert_eq!(fake.refund_calls(), 1);
    assert_eq!(ledger(&pool, "refund").await, vec![7500]);
}

/// TD-FEE-14 (FEE-026): a failed refund alerts Casey and is sent again by the task an hour on; a
/// later `refund.failed` from Stripe queues one more retry; the ledger keeps one refund entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_failed_refund_retries_hourly_with_an_alert(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(vec![Err(AdapterError::NotSent)]);
    let first = request_refund(&pool, &fake, &secret(), unreachable(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    assert_eq!(alerts(&pool, REFUND_FAILED_ALERT).await, 1);
    assert_eq!(task_at(&pool, RETRY_KIND).await, vec![NOW + 3600]);
    let retry = json!({"pick": PICK, "try": 1});
    run_refund_retry(&pool, &fake, &secret(), &retry, at(3600))
        .await
        .expect("retry");
    assert_eq!(fake.refund_calls(), 2);
    assert_eq!(ledger(&pool, "refund").await, vec![7500]);
    assert_eq!(
        refund_failed(&pool, "re_FakeT066", at(4000)).await,
        Ok(true)
    );
    for unchanged in ["re_FakeT066", "re_Unknown"] {
        let again = refund_failed(&pool, unchanged, at(4000)).await;
        assert_eq!(again, Ok(false));
    }
    let retry = json!({"pick": PICK, "try": 2});
    run_refund_retry(&pool, &fake, &secret(), &retry, at(7600))
        .await
        .expect("retry");
    assert_eq!(fake.refund_calls(), 3);
    assert_eq!(ledger(&pool, "refund").await, vec![7500]);
    assert_eq!(alerts(&pool, REFUND_FAILED_ALERT).await, 1);
    let bad = json!({"pick": "not-a-pick"});
    assert!(
        run_refund_retry(&pool, &fake, &secret(), &bad, at(0))
            .await
            .is_err()
    );
}

/// TD-FEE-14 (FEE-026): a disputed charge's refund is blocked and flagged once, nothing sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_disputed_charge_refund_is_blocked(pool: PgPool) {
    charged(&pool).await;
    assert_eq!(dispute(&pool, 5).await, Ok(true));
    let fake = stripe(vec![]);
    for _ in 0..2 {
        let blocked = request_refund(&pool, &fake, &secret(), unreachable(), at(0)).await;
        assert_eq!(blocked, Ok(RefundOutcome::Blocked));
    }
    assert_eq!(fake.refund_calls(), 0);
    assert_eq!(alerts(&pool, REFUND_BLOCKED_ALERT).await, 1);
    assert!(ledger(&pool, "refund").await.is_empty());
}

/// TD-FEE-15 (FEE-027): the dispute webhook twice gives one ledger dispute row of the amount plus
/// $15, one alert to Casey and its audit evidence; an unknown charge is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_15_dispute_twice_records_once(pool: PgPool) {
    charged(&pool).await;
    assert_eq!(dispute(&pool, 5).await, Ok(true));
    assert_eq!(dispute(&pool, 5).await, Ok(false));
    assert_eq!(ledger(&pool, "dispute").await, vec![9000]);
    assert_eq!(alerts(&pool, DISPUTE_ACTION).await, 1);
    let audited = "SELECT count(*) FROM ops.audit_log WHERE action = $1";
    assert_eq!(count(&pool, audited, DISPUTE_ACTION).await, 1);
    let unknown = DisputeOpened {
        dispute: "dp_Other",
        payment_intent: "pi_NoSuchCharge",
        amount_cents: 7500,
        due_at: at(DAY),
    };
    let refused = record_dispute(&pool, &unknown, at(0)).await;
    assert_eq!(refused, Err(MoneyEventError::UnknownCharge));
    let malformed = [
        DisputeOpened {
            dispute: "dp_bad id",
            ..unknown
        },
        DisputeOpened {
            dispute: "dp_",
            ..unknown
        },
        DisputeOpened {
            dispute: DISPUTE,
            payment_intent: INTENT,
            amount_cents: 0,
            ..unknown
        },
    ];
    for opened in malformed {
        let refused = record_dispute(&pool, &opened, at(0)).await;
        assert_eq!(refused, Err(MoneyEventError::UnknownCharge));
    }
}

/// TD-FEE-23, TD-FEE-33 (FEE-027, FEE-050): due in 5 days, the submit task is at due minus 2
/// days; marked for auto-submit, the evidence goes in once with the countering fee and is listed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_23_marked_dispute_submits_two_days_before_due(pool: PgPool) {
    charged(&pool).await;
    dispute(&pool, 5).await.expect("dispute");
    assert_eq!(task_at(&pool, SUBMIT_KIND).await, vec![NOW + 3 * DAY]);
    let id: Uuid = sqlx::query_scalar("SELECT id FROM payments.dispute")
        .fetch_one(&pool)
        .await
        .expect("id");
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(mark_auto_submit(&mut conn, DISPUTE, at(0)).await, Ok(true));
    let fake = stripe(vec![]);
    for _ in 0..2 {
        let payload = json!({"dispute": id});
        run_dispute_submit(&pool, &fake, &secret(), &payload, at(3 * DAY))
            .await
            .expect("submit");
    }
    assert_eq!(fake.submitted.lock().expect("lock").len(), 1);
    assert_eq!(ledger(&pool, "countering-fee").await, vec![1500]);
    let listed = list_disputes(&mut conn, at(-DAY)).await.expect("digest");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].submitted_at, Some(at(3 * DAY)));
    assert_eq!(mark_auto_submit(&mut conn, DISPUTE, at(0)).await, Ok(false));
}

/// TD-FEE-23, TD-FEE-33 (FEE-050, D-229): an unmarked dispute is not submitted and waits for Casey
/// with the fee shown; one Casey submitted is never sent again and pays the countering fee once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_23_unmarked_or_owner_submitted_is_not_auto_submitted(pool: PgPool) {
    charged(&pool).await;
    dispute(&pool, 5).await.expect("dispute");
    let id: Uuid = sqlx::query_scalar("SELECT id FROM payments.dispute")
        .fetch_one(&pool)
        .await
        .expect("id");
    let fake = stripe(vec![]);
    let payload = json!({"dispute": id});
    run_dispute_submit(&pool, &fake, &secret(), &payload, at(3 * DAY))
        .await
        .expect("wait");
    let mut conn = pool.acquire().await.expect("conn");
    let waiting = list_disputes(&mut conn, at(-DAY)).await.expect("portal");
    assert!(!waiting[0].auto_submit && waiting[0].submitted_at.is_none());
    assert_eq!(waiting[0].fee_at_stake_cents, 1500);
    let next_week = list_disputes(&mut conn, at(DAY / 2)).await.expect("digest");
    assert_eq!(
        next_week.len(),
        1,
        "an open dispute from an earlier week is listed"
    );
    let after_due = list_disputes(&mut conn, at(6 * DAY)).await.expect("digest");
    assert!(after_due.is_empty());
    assert_eq!(
        record_owner_submission(&mut conn, DISPUTE, at(DAY)).await,
        Ok(true)
    );
    assert_eq!(
        record_owner_submission(&mut conn, DISPUTE, at(DAY)).await,
        Ok(false)
    );
    assert_eq!(mark_auto_submit(&mut conn, DISPUTE, at(0)).await, Ok(false));
    run_dispute_submit(&pool, &fake, &secret(), &payload, at(3 * DAY))
        .await
        .expect("done");
    assert!(fake.submitted.lock().expect("lock").is_empty());
    assert_eq!(ledger(&pool, "countering-fee").await, vec![1500]);
    let answered = list_disputes(&mut conn, at(DAY / 2)).await.expect("digest");
    assert!(
        answered.is_empty(),
        "a submitted dispute from an earlier week is not"
    );
    let bad = json!({"dispute": 7});
    assert!(
        run_dispute_submit(&pool, &fake, &secret(), &bad, at(0))
            .await
            .is_err()
    );
}

/// TD-FEE-26, TD-PRIV-12 (FEE-042, PRIV-027): the evidence pack is built from the charge's
/// evidence record and its refund, IDs and timestamps only, and the submission Stripe receives
/// carries no contact value.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_26_evidence_pack_is_ids_and_timestamps(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(vec![]);
    request_refund(&pool, &fake, &secret(), unreachable(), at(0))
        .await
        .expect("refund");
    dispute(&pool, 5).await.expect("dispute");
    let mut conn = pool.acquire().await.expect("conn");
    let pack = evidence_pack(&mut conn, PICK).await.expect("pack");
    let text = pack
        .form()
        .iter()
        .map(|(_, v)| v.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let label = format!("pick-{PICK}-try1");
    for expected in [
        "cus_FakeT066",
        INTENT,
        label.as_str(),
        "re_FakeT066",
        "$75.00",
    ] {
        assert!(text.contains(expected), "evidence lacks {expected}");
    }
    assert!(
        text.contains(&at(-DAY).strftime("%Y-%m-%d %H:%M:%S UTC").to_string()),
        "evidence lacks the press time"
    );
    mark_auto_submit(&mut conn, DISPUTE, at(0))
        .await
        .expect("mark");
    let id: Uuid = sqlx::query_scalar("SELECT id FROM payments.dispute")
        .fetch_one(&pool)
        .await
        .expect("id");
    let payload = json!({"dispute": id});
    run_dispute_submit(&pool, &fake, &secret(), &payload, at(3 * DAY))
        .await
        .expect("submit");
    let sent = fake.submitted.lock().expect("lock").concat();
    assert!(!sent.is_empty());
    for (field, value) in sent {
        for sentinel in SENTINELS {
            assert!(!value.contains(sentinel), "{field} carries a contact value");
        }
        assert!(
            !value.contains('@') && !value.contains("http"),
            "{field} holds no link or email"
        );
    }
    let missing = evidence_pack(&mut conn, Uuid::from_u128(1)).await;
    assert_eq!(missing, Err(MoneyEventError::NothingCharged));
}

/// TD-FEE-33 (FEE-052): an early fraud warning alerts Casey; an unreleased charge is refunded at
/// once, a released one only listed for his decision.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_33_fraud_warning_refunds_only_unreleased(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(vec![]);
    let listed = early_fraud_warning(&pool, &fake, &secret(), (PICK, true), at(0)).await;
    assert_eq!(listed, Ok(RefundOutcome::Unchanged));
    assert_eq!(fake.refund_calls(), 0);
    let refunded = early_fraud_warning(&pool, &fake, &secret(), (PICK, false), at(0)).await;
    assert_eq!(
        refunded,
        Ok(RefundOutcome::Refunded("re_FakeT066".to_owned()))
    );
    assert_eq!(alerts(&pool, FRAUD_WARNING_ALERT).await, 2);
    let trigger = "SELECT count(*) FROM payments.refund WHERE trigger = $1";
    assert_eq!(count(&pool, trigger, "fraud-warning").await, 1);
}

/// Stripe as its idempotency layer behaves: a key it has seen gets that request's own answer, a
/// new key refunds the charge once and is refused after; the answer to the first request is lost.
#[derive(Default)]
struct KeyedStripe {
    seen: Mutex<HashMap<String, Option<String>>>,
    refunds: Mutex<u32>,
}

impl PaymentsAdapter for KeyedStripe {
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

    async fn create_refund(&self, key: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        let mut seen = self.seen.lock().expect("lock");
        let first = seen.is_empty();
        let answer = seen.entry(key.as_str().to_owned()).or_insert_with(|| {
            let mut refunds = self.refunds.lock().expect("lock");
            *refunds += 1;
            (*refunds == 1).then(|| "re_FakeLost".to_owned())
        });
        let refused = AdapterError::Vendor {
            status: 400,
            code: None,
        };
        match answer.clone() {
            _ if first => Err(AdapterError::OutcomeUnknown),
            Some(refund) => Ok(refund),
            None => Err(refused),
        }
    }
}

/// TD-FEE-14 (FEE-026, D-1545): a refund Stripe made on a try whose answer was lost is found by
/// the hourly retry (it replays the lost try's key) and recorded once, not refused and retried
/// for ever; Stripe refunded the charge once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_refund_with_a_lost_answer_is_recorded_on_retry(pool: PgPool) {
    charged(&pool).await;
    let fake = KeyedStripe::default();
    let first = request_refund(&pool, &fake, &secret(), unreachable(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    let retry = json!({"pick": PICK, "try": 1});
    run_refund_retry(&pool, &fake, &secret(), &retry, at(3600))
        .await
        .expect("retry");
    let status = "SELECT count(*) FROM payments.refund WHERE stripe_refund_id = $1";
    assert_eq!(count(&pool, status, "re_FakeLost").await, 1);
    assert_eq!(ledger(&pool, "refund").await, vec![7500]);
    let again = request_refund(&pool, &fake, &secret(), unreachable(), at(7200)).await;
    assert_eq!(again, Ok(RefundOutcome::Unchanged));
    assert_eq!(*fake.refunds.lock().expect("lock"), 1);
}

/// TD-FEE-23 (FEE-027, FEE-050, D-1545): a dispute opened a day before its due date, so its first
/// submit task is already past, and marked after that task ran, is still sent once, at the mark;
/// one marked after its due date queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_23_late_mark_still_submits(pool: PgPool) {
    charged(&pool).await;
    dispute(&pool, 1).await.expect("dispute");
    let id: Uuid = sqlx::query_scalar("SELECT id FROM payments.dispute")
        .fetch_one(&pool)
        .await
        .expect("id");
    let fake = stripe(vec![]);
    let payload = json!({"dispute": id});
    run_dispute_submit(&pool, &fake, &secret(), &payload, at(0))
        .await
        .expect("unmarked");
    assert!(fake.submitted.lock().expect("lock").is_empty());
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(mark_auto_submit(&mut conn, DISPUTE, at(60)).await, Ok(true));
    assert_eq!(task_at(&pool, SUBMIT_KIND).await, vec![NOW - DAY, NOW + 60]);
    run_dispute_submit(&pool, &fake, &secret(), &payload, at(60))
        .await
        .expect("submit");
    assert_eq!(fake.submitted.lock().expect("lock").len(), 1);
    let late = DisputeOpened {
        dispute: "dp_FakeLate",
        payment_intent: INTENT,
        amount_cents: 7500,
        due_at: at(DAY),
    };
    assert_eq!(record_dispute(&pool, &late, at(0)).await, Ok(true));
    assert_eq!(
        mark_auto_submit(&mut conn, "dp_FakeLate", at(2 * DAY)).await,
        Ok(true)
    );
    assert_eq!(task_at(&pool, SUBMIT_KIND).await.len(), 3);
}

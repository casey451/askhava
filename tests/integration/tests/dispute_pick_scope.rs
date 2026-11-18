//! TD-FEE-73 (T722, D-4506; FEE-050, FEE-027, SEC-006): the owner portal's refund review reads
//! only the reviewed pick's disputes (`disputes_of`) and records Casey's own evidence submission
//! only for that pick's own dispute (`record_pick_owner_submission`): another pick's or an unknown
//! dispute is `NotThisPick` and writes nothing, its own is `Recorded` once with one countering
//! fee, then `AlreadyIn`; a press racing the auto-submit books one fee; a `du_` dispute is handled
//! like a `dp_` one (no prefix check).
//!
//! PostgreSQL through `#[sqlx::test]`; Stripe is a fake whose optional gate holds the auto path
//! after its Stripe call and before it marks the evidence in. Every row is fake (fake UUIDs and
//! fake Stripe IDs); no contact detail is seeded.

use askhava_adapters::api::{AdapterError, IdempotencyKey};
use askhava_payments::api::{
    DISPUTE_FEE_CENTS, DisputeOpened, DisputeStripe, OwnerSubmission, disputes_of,
    mark_auto_submit, record_dispute, record_pick_owner_submission, run_dispute_submit,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;
use std::sync::Mutex;
use tokio::sync::Notify;

const NOW: i64 = 1_793_900_000;
const DAY: i64 = 86_400;

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t722".to_owned())
}

/// Holds the auto path after Stripe took the evidence until the test lets it go.
#[derive(Default)]
struct Gate {
    reached: Notify,
    go: Notify,
}

/// Counts evidence submissions; with a gate, waits on it after each one.
#[derive(Default)]
struct FakeStripe {
    sent: Mutex<Vec<String>>,
    gate: Option<Gate>,
}

impl DisputeStripe for FakeStripe {
    async fn submit_evidence(
        &self,
        _: &IdempotencyKey,
        dispute: &str,
        _: &[(&'static str, String)],
    ) -> Result<(), AdapterError> {
        self.sent.lock().expect("lock").push(dispute.to_owned());
        if let Some(gate) = &self.gate {
            gate.reached.notify_one();
            gate.go.notified().await;
        }
        Ok(())
    }
}

/// A fake paid pick of $75 with its FEE-042 evidence record; returns the pick and its intent.
async fn charged(pool: &PgPool, tag: &str) -> (Uuid, String) {
    let (pick, business, job) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let intent = format!("pi_FakeT722{tag}");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT722', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(&intent)
            .bind(NOW - DAY)
            .execute(pool)
            .await
            .expect("seed");
    }
    (pick, intent)
}

/// Records dispute `id` on `intent`, opened at `opened` seconds after `NOW`, due in 10 days.
async fn open(pool: &PgPool, id: &str, intent: &str, opened: i64) {
    let event = DisputeOpened {
        dispute: id,
        payment_intent: intent,
        amount_cents: 7500,
        due_at: at(10 * DAY),
    };
    assert_eq!(record_dispute(pool, &event, at(opened)).await, Ok(true));
}

/// Picks P (`dp_` and `du_` disputes), Q (one `dp_`) and R (undisputed).
async fn seeded(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let (p, p_intent) = charged(pool, "P").await;
    let (q, q_intent) = charged(pool, "Q").await;
    let (r, _) = charged(pool, "R").await;
    open(pool, "du_FakeT722P", &p_intent, 60).await;
    open(pool, "dp_FakeT722P", &p_intent, 0).await;
    open(pool, "dp_FakeT722Q", &q_intent, 0).await;
    (p, q, r)
}

/// The countering-fee entries, by dispute ID.
async fn countering(pool: &PgPool) -> Vec<(String, i64)> {
    sqlx::query_as(
        "SELECT stripe_id, amount_cents FROM payments.ledger_entry
          WHERE kind = 'countering-fee' ORDER BY stripe_id",
    )
    .fetch_all(pool)
    .await
    .expect("ledger")
}

/// `submitted_at` (epoch), `submitted_by` and `auto_submit` of a dispute.
async fn state(pool: &PgPool, id: &str) -> (Option<i64>, Option<String>, bool) {
    sqlx::query_as(
        "SELECT extract(epoch FROM submitted_at)::bigint, submitted_by, auto_submit
           FROM payments.dispute WHERE stripe_dispute_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("state")
}

/// Marks `id` for auto-submit and runs its submit task 2 days before the due date.
async fn auto_submit(pool: &PgPool, stripe: &FakeStripe, id: &str) {
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(mark_auto_submit(&mut conn, id, at(0)).await, Ok(true));
    drop(conn);
    run(pool, stripe, id).await;
}

async fn run(pool: &PgPool, stripe: &FakeStripe, id: &str) {
    let row: Uuid =
        sqlx::query_scalar("SELECT id FROM payments.dispute WHERE stripe_dispute_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("id");
    let payload = json!({"dispute": row});
    run_dispute_submit(pool, stripe, &secret(), &payload, at(8 * DAY))
        .await
        .expect("submit");
}

async fn press(pool: &PgPool, pick: Uuid, id: &str) -> OwnerSubmission {
    let mut conn = pool.acquire().await.expect("conn");
    record_pick_owner_submission(&mut conn, pick, id, at(DAY))
        .await
        .expect("press")
}

/// TD-FEE-73: each pick sees only its own disputes, oldest first, with the fee at stake.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_73_disputes_of_lists_only_that_picks(pool: PgPool) {
    let (p, q, r) = seeded(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    let ids = |v: Vec<askhava_payments::api::DisputeView>| {
        assert!(v.iter().all(|d| d.fee_at_stake_cents == DISPUTE_FEE_CENTS));
        v.into_iter().map(|d| d.dispute).collect::<Vec<_>>()
    };
    let of_p = ids(disputes_of(&mut conn, p).await.expect("P"));
    assert_eq!(of_p, ["dp_FakeT722P", "du_FakeT722P"]);
    assert_eq!(
        ids(disputes_of(&mut conn, q).await.expect("Q")),
        ["dp_FakeT722Q"]
    );
    assert!(disputes_of(&mut conn, r).await.expect("R").is_empty());
}

/// TD-FEE-73, SEC-006: another pick's or an unknown dispute writes nothing; the marked other
/// dispute is still sent and recorded by the auto-submit.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_73_other_or_unknown_dispute_is_not_this_pick(pool: PgPool) {
    let (p, _, _) = seeded(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(
        mark_auto_submit(&mut conn, "dp_FakeT722Q", at(0)).await,
        Ok(true)
    );
    drop(conn);
    let before = state(&pool, "dp_FakeT722Q").await;
    for id in ["dp_FakeT722Q", "dp_FakeT722Unknown"] {
        assert_eq!(press(&pool, p, id).await, OwnerSubmission::NotThisPick);
    }
    assert_eq!(state(&pool, "dp_FakeT722Q").await, before);
    assert_eq!(before, (None, None, true));
    assert!(countering(&pool).await.is_empty());
    let stripe = FakeStripe::default();
    run(&pool, &stripe, "dp_FakeT722Q").await;
    assert_eq!(*stripe.sent.lock().expect("lock"), ["dp_FakeT722Q"]);
    let after = state(&pool, "dp_FakeT722Q").await;
    assert_eq!(after.1.as_deref(), Some("auto"));
    assert_eq!(countering(&pool).await, [("dp_FakeT722Q".to_owned(), 1500)]);
}

/// TD-FEE-73, FEE-050: P's own `dp_` and `du_` disputes are recorded once each, then already in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_73_own_dispute_records_once(pool: PgPool) {
    let (p, _, _) = seeded(&pool).await;
    for id in ["dp_FakeT722P", "du_FakeT722P"] {
        assert_eq!(press(&pool, p, id).await, OwnerSubmission::Recorded);
        assert_eq!(press(&pool, p, id).await, OwnerSubmission::AlreadyIn);
        let (when, by, auto) = state(&pool, id).await;
        assert_eq!(
            (when, by.as_deref(), auto),
            (Some(NOW + DAY), Some("owner"), false)
        );
    }
    let fees = countering(&pool).await;
    let want = [
        ("dp_FakeT722P".to_owned(), 1500),
        ("du_FakeT722P".to_owned(), 1500),
    ];
    assert_eq!(fees, want);
}

/// TD-FEE-73, FEE-027: after an auto-submit, Casey's press is already in and books nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_73_after_auto_submit_is_already_in(pool: PgPool) {
    let (p, _, _) = seeded(&pool).await;
    auto_submit(&pool, &FakeStripe::default(), "dp_FakeT722P").await;
    assert_eq!(
        press(&pool, p, "dp_FakeT722P").await,
        OwnerSubmission::AlreadyIn
    );
    assert_eq!(
        state(&pool, "dp_FakeT722P").await.1.as_deref(),
        Some("auto")
    );
    assert_eq!(countering(&pool).await, [("dp_FakeT722P".to_owned(), 1500)]);
}

/// TD-FEE-73, FEE-050, D-229: the auto path held after its Stripe call while Casey presses books
/// one countering fee: the press is `Recorded` and the auto path then records nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_73_press_racing_auto_submit_books_one_fee(pool: PgPool) {
    let (p, _, _) = seeded(&pool).await;
    let stripe = FakeStripe {
        gate: Some(Gate::default()),
        ..FakeStripe::default()
    };
    let gate = stripe.gate.as_ref().expect("gate");
    let pressed = async {
        gate.reached.notified().await;
        let answer = press(&pool, p, "dp_FakeT722P").await;
        gate.go.notify_one();
        answer
    };
    let ((), answer) = tokio::join!(auto_submit(&pool, &stripe, "dp_FakeT722P"), pressed);
    assert_eq!(answer, OwnerSubmission::Recorded);
    assert_eq!(stripe.sent.lock().expect("lock").len(), 1);
    assert_eq!(
        state(&pool, "dp_FakeT722P").await.1.as_deref(),
        Some("owner")
    );
    assert_eq!(countering(&pool).await, [("dp_FakeT722P".to_owned(), 1500)]);
}

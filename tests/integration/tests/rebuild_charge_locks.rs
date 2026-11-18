//! T647 (OPS-016, FEE-019, FEE-021; D-4292, TD-OPS-45): `rebuild_charge`'s answer and locks on
//! PostgreSQL. Called inside a caller's open transaction it books a held charge and answers
//! `Booked`, then `AlreadyBooked`; run while `record`'s lock order (the newest attempt row
//! `FOR UPDATE`, then `pick_intent` `FOR SHARE`) is held on another connection it waits, never
//! deadlocks (40P01) and books one evidence row and one `charge` entry; when `record` books the
//! charge while it waits it answers `AlreadyBooked` and books nothing more; an attempt made while
//! it waits is the one it books on. All data is fake.

use std::time::Duration;

use askhava_adapters::api::ListedCharge;
use askhava_payments::api::{Rebuilt, rebuild_charge};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: Uuid = Uuid::from_u128(0xb647_0001);
const INTENT: &str = "pi_FakeT647Intent";

/// Holds fake pick `pick` as a restore kept it: customer, PaymentIntent and try 1.
async fn hold(pool: &PgPool, pick: Uuid) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, 'cus_FakeT647') ON CONFLICT DO NOTHING")
        .bind(BUSINESS).execute(pool).await.expect("customer");
    sqlx::query("INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, now())")
        .bind(pick).bind(BUSINESS).bind(Uuid::from_u128(pick.as_u128() + 0x100)).bind(INTENT).execute(pool).await.expect("intent");
    attempt(pool, pick, 1).await;
}

async fn attempt(db: impl sqlx::PgExecutor<'_>, pick: Uuid, try_no: i32) {
    sqlx::query("INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at) VALUES ($1, $2, gen_random_uuid(), 'declined', now())")
        .bind(pick).bind(try_no).execute(db).await.expect("attempt");
}

/// Stripe's record of pick `pick`'s succeeded charge, as `list_charge_page` keeps it.
fn listed(pick: Uuid) -> ListedCharge {
    ListedCharge {
        id: "ch_FakeT647Charge".to_owned(),
        payment_intent: Some(INTENT.to_owned()),
        pick_id: Some(pick.to_string()),
        amount: 7500,
        amount_refunded: 0,
        balance_transaction: None,
        dispute: None,
    }
}

/// Pick `pick`'s `(evidence rows, the evidence's try, charge entries)`.
async fn booked(pool: &PgPool, pick: Uuid) -> (i64, Option<i32>, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM payments.charge_evidence WHERE pick_id = $1), \
         (SELECT max(try_no) FROM payments.charge_evidence WHERE pick_id = $1), \
         (SELECT count(*) FROM payments.ledger_entry WHERE kind = 'charge' AND stripe_id = 'ch_FakeT647Charge')",
    )
    .bind(pick)
    .fetch_one(pool)
    .await
    .expect("booked")
}

/// Runs the locking `sql` for pick `pick` on `conn`; it must neither fail nor deadlock (40P01).
async fn lock(conn: &mut PgConnection, sql: &'static str, pick: Uuid) {
    sqlx::query(sql)
        .bind(pick)
        .fetch_one(conn)
        .await
        .expect(sql);
}

/// Starts `rebuild_charge` for pick `pick` on its own connection and waits until it is blocked
/// on a row lock.
async fn rebuild_blocked(
    pool: &PgPool,
    pick: Uuid,
) -> tokio::task::JoinHandle<Result<Rebuilt, askhava_payments::api::ChargeError>> {
    let mut other = pool.acquire().await.expect("connection");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *other)
        .await
        .expect("pid");
    let run =
        tokio::spawn(
            async move { rebuild_charge(&mut other, &listed(pick), Timestamp::now()).await },
        );
    for _ in 0..200 {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')",
        )
        .bind(pid)
        .fetch_one(pool)
        .await
        .expect("activity");
        if waiting {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("rebuild_charge never waited on the held lock");
}

/// TD-OPS-45, OPS-016, D-4292: inside a caller's open transaction the first call books the held
/// charge and answers `Booked`, the second `AlreadyBooked`; one evidence row, one `charge` entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_45_inside_a_callers_transaction_booked_then_already_booked(pool: PgPool) {
    let pick = Uuid::from_u128(0x647_0001);
    hold(&pool, pick).await;
    let mut tx = pool.begin().await.expect("caller's transaction");
    let first = rebuild_charge(&mut tx, &listed(pick), Timestamp::now()).await;
    assert_eq!(first.expect("first"), Rebuilt::Booked);
    let again = rebuild_charge(&mut tx, &listed(pick), Timestamp::now()).await;
    assert_eq!(again.expect("again"), Rebuilt::AlreadyBooked);
    tx.commit().await.expect("commit");
    assert_eq!(booked(&pool, pick).await, (1, Some(1), 1));
}

/// TD-OPS-45, FEE-019, FEE-021, D-4292: with `record`'s locks held (the newest attempt row
/// `FOR UPDATE`, then `pick_intent` `FOR SHARE`) `rebuild_charge` waits, no deadlock (40P01) is
/// raised, and one evidence row and one `charge` entry are booked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_45_record_and_rebuild_on_one_pick_never_deadlock(pool: PgPool) {
    let pick = Uuid::from_u128(0x647_0002);
    hold(&pool, pick).await;
    let mut record = pool.begin().await.expect("record's transaction");
    let attempt_row = "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 FOR UPDATE";
    lock(&mut record, attempt_row, pick).await;
    let run = rebuild_blocked(&pool, pick).await;
    let hold_pick = "SELECT pick_id FROM payments.pick_intent WHERE pick_id = $1 FOR SHARE";
    lock(&mut record, hold_pick, pick).await;
    record.commit().await.expect("record commits");
    let answer = run.await.expect("joined").expect("rebuild: no deadlock");
    assert_eq!(answer, Rebuilt::Booked);
    assert_eq!(booked(&pool, pick).await, (1, Some(1), 1));
}

/// Books pick `pick`'s try 1 on `record`'s transaction as `record`'s effects do: its evidence
/// row and its `charge` entry under key `charge.<pick>`.
async fn record_books(record: &mut PgConnection, pick: Uuid) {
    sqlx::query("INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, amount_cents, pressed_at, charged_at, stripe_charge_id) SELECT i.pick_id, i.business_id, i.job_id, a.press_id, a.try_no, 'pick-' || i.pick_id || '-try1', 'cus_FakeT647', i.stripe_intent_id, i.amount_cents, now(), now(), 'ch_FakeT647Charge' FROM payments.pick_intent i JOIN payments.charge_attempt a ON a.pick_id = i.pick_id AND a.try_no = 1 WHERE i.pick_id = $1")
        .bind(pick).execute(&mut *record).await.expect("record's evidence");
    sqlx::query("INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, stripe_id, idempotency_key) SELECT 'charge', business_id, job_id, amount_cents, 'ch_FakeT647Charge', 'charge.' || pick_id FROM payments.pick_intent WHERE pick_id = $1")
        .bind(pick).execute(&mut *record).await.expect("record's charge entry");
}

/// TD-OPS-45, TD-OPS-04, FEE-019, D-4292: `record` holds the newest attempt row and
/// `pick_intent`, books the charge and commits while `rebuild_charge` waits; `rebuild_charge`
/// then sees the evidence under the lock and answers `AlreadyBooked`, never `Booked`, and the
/// pick keeps exactly one evidence row and one `charge` entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_45_record_books_while_rebuild_waits_already_booked(pool: PgPool) {
    let pick = Uuid::from_u128(0x647_0004);
    hold(&pool, pick).await;
    let mut record = pool.begin().await.expect("record's transaction");
    let attempt_row = "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 FOR UPDATE";
    lock(&mut record, attempt_row, pick).await;
    let run = rebuild_blocked(&pool, pick).await;
    let hold_pick = "SELECT pick_id FROM payments.pick_intent WHERE pick_id = $1 FOR SHARE";
    lock(&mut record, hold_pick, pick).await;
    record_books(&mut record, pick).await;
    record.commit().await.expect("record commits");
    let answer = run.await.expect("joined").expect("rebuild: no deadlock");
    assert_eq!(answer, Rebuilt::AlreadyBooked);
    assert_eq!(booked(&pool, pick).await, (1, Some(1), 1));
}

/// TD-OPS-45, D-4292: an attempt made while `rebuild_charge` waits on the `pick_intent` lock
/// (as `confirm_and_pay`'s `begin_attempt` holds it) is seen under that lock; the call starts
/// over and books on the newer attempt.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_45_a_newer_attempt_made_while_waiting_is_booked_on(pool: PgPool) {
    let pick = Uuid::from_u128(0x647_0003);
    hold(&pool, pick).await;
    let mut press = pool.begin().await.expect("press transaction");
    let intent_row = "SELECT pick_id FROM payments.pick_intent WHERE pick_id = $1 FOR UPDATE";
    lock(&mut press, intent_row, pick).await;
    let run = rebuild_blocked(&pool, pick).await;
    attempt(&mut *press, pick, 2).await;
    press.commit().await.expect("press commits");
    let answer = run.await.expect("joined").expect("rebuild");
    assert_eq!(answer, Rebuilt::Booked);
    assert_eq!(booked(&pool, pick).await, (1, Some(2), 1));
}

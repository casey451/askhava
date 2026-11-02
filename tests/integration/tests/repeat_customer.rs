//! TD-FEE-09 (FEE-013, LIC-023; D-069, D-493) and TD-FEE-32's ledger case (FEE-053): repeat work
//! from the same customer is $0 and never uses the free first win; after the 180-day contact purge
//! (PRIV-007) the caller has no other jobs of that customer to pass, so the pick is charged.
//!
//! The customer's other jobs are what the Confirm page (T224) reads from `jobs` and passes in;
//! `payments` never names the customer. All IDs, keys and amounts are fake.

use askhava_payments::api::{
    Appended, BusinessId, EntryKind, FeeJob, FeeMode, FeeQuote, JoinDates, LedgerEntry, NewEntry,
    PeriodJob, PickedFee, RecurringCap, Service, VisitTier, Waiver, WinCheck, fee_at_pick,
    ledger_append, waive_at_confirm,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb061_1001));
const OTHER: BusinessId = BusinessId(Uuid::from_u128(0xb061_1002));

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t061".to_owned())
}

fn job(n: u128) -> Uuid {
    Uuid::from_u128(0x0061_1000 + n)
}

/// Join dates inside (`early`) or long past (not `early`) the 90-day window.
fn joined(early: bool) -> JoinDates {
    let launch = Timestamp::from_second(1_790_000_000).expect("time");
    let offset = if early { 86_400 } else { 86_400 * 400 };
    JoinDates {
        joined: Timestamp::from_second(1_790_000_000 + offset).expect("time"),
        launch,
        trades_opened: Vec::new(),
    }
}

fn picked(cents: u64) -> PickedFee {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    fee_at_pick(FeeQuote::Fixed(cents), None, job, FeeMode::Tiers, None).expect("fee")
}

/// A charged introduction of `business` on `on`, as the Confirm-and-pay task would record it.
async fn charged(db: &mut PgConnection, business: BusinessId, on: Uuid, cents: i64) {
    let key = format!("fake-charge-{on}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Charge,
        business: Some(business.0),
        job: Some(on),
        amount_cents: cents,
        stripe_id: Some("ch_FakeT061"),
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(db, &entry).await.expect("append");
}

async fn waive(
    db: &mut PgConnection,
    (business, on, cents): (BusinessId, Uuid, u64),
    early: bool,
    others: &[Uuid],
) -> Waiver {
    let fee = picked(cents);
    let dates = joined(early);
    let check = WinCheck {
        business,
        job: on,
        picked: &fee,
        dates: &dates,
        same_customer_jobs: others,
    };
    waive_at_confirm(db, &secret(), check).await.expect("waive")
}

async fn kinds(db: &mut PgConnection) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT kind, amount_cents FROM payments.ledger_entry ORDER BY seq")
        .fetch_all(db)
        .await
        .expect("ledger")
}

/// TD-FEE-09 (FEE-013, W23): the same customer picking the same business for another property is
/// $0 and leaves the free first win unused, which a new customer's job then gets; a repeated
/// Confirm appends nothing new.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_09_repeat_is_free_and_keeps_the_win(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    charged(&mut db, BUSINESS, job(1), 2_500).await;
    let repeat = waive(&mut db, (BUSINESS, job(2), 90_000), true, &[job(1)]).await;
    assert!(matches!(repeat, Waiver::Repeat(Appended::New(_))));
    let again = waive(&mut db, (BUSINESS, job(2), 90_000), true, &[job(1)]).await;
    assert!(matches!(again, Waiver::Repeat(Appended::Already(_))));
    let win = waive(&mut db, (BUSINESS, job(3), 90_000), true, &[]).await;
    assert!(matches!(
        win,
        Waiver::FreeFirstWin {
            waived_cents: 7_500,
            ..
        }
    ));
    let after_win = waive(&mut db, (BUSINESS, job(4), 90_000), true, &[job(3)]).await;
    assert!(matches!(after_win, Waiver::Repeat(_)));
    let expected = [
        ("charge".to_owned(), 2_500),
        ("repeat-customer".to_owned(), 0),
        ("free-first-win".to_owned(), 0),
        ("repeat-customer".to_owned(), 0),
    ];
    assert_eq!(kinds(&mut db).await, expected);
}

/// TD-FEE-09 (FEE-013, PRIV-007): after the 180-day purge the customer's earlier job is no longer
/// passed, so the pick is charged; an introduction to another business never counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_09_after_the_purge_or_another_business_is_charged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    charged(&mut db, BUSINESS, job(1), 2_500).await;
    let purged = waive(&mut db, (BUSINESS, job(2), 40_000), false, &[]).await;
    assert_eq!(purged, Waiver::NotWaived);
    let other = waive(&mut db, (OTHER, job(3), 40_000), false, &[job(1)]).await;
    assert_eq!(other, Waiver::NotWaived);
    let own_job = waive(&mut db, (BUSINESS, job(1), 40_000), false, &[job(1)]).await;
    assert_eq!(own_job, Waiver::NotWaived);
    assert_eq!(kinds(&mut db).await, [("charge".to_owned(), 2_500)]);
}

/// TD-FEE-09 (FEE-013, LIC-023, D-493): a second handyman job at the same address with no shared
/// job type, $400 then $500 for the same customer, is allowed and free as a repeat customer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_09_second_handyman_job_is_a_free_repeat(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let first = waive(&mut db, (BUSINESS, job(1), 40_000), false, &[]).await;
    assert_eq!(first, Waiver::NotWaived);
    charged(&mut db, BUSINESS, job(1), 2_500).await;
    let second = waive(&mut db, (BUSINESS, job(2), 50_000), false, &[job(1)]).await;
    assert!(matches!(second, Waiver::Repeat(Appended::New(_))));
}

/// TD-FEE-32 (FEE-053): every entry is for exactly one pick: one row per job and business, each a
/// FEE-028 kind, with no bundled or other charge kind.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_32_one_ledger_line_per_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    charged(&mut db, BUSINESS, job(1), 2_500).await;
    waive(&mut db, (BUSINESS, job(2), 40_000), true, &[job(1)]).await;
    waive(&mut db, (BUSINESS, job(3), 40_000), true, &[]).await;
    waive(&mut db, (BUSINESS, job(3), 40_000), true, &[]).await;
    let per_pick: Vec<i64> = sqlx::query_scalar(
        "SELECT count(*) FROM payments.ledger_entry GROUP BY business_id, job_id",
    )
    .fetch_all(&mut *db)
    .await
    .expect("ledger");
    assert_eq!(per_pick, [1, 1, 1]);
}

/// TD-FEE-09, TD-FEE-32 (FEE-013, FEE-053, D-1422): a Confirm run again for a job already decided
/// keeps that answer and writes no second line, even when the customer's other jobs changed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_09_a_decided_job_keeps_its_answer(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    charged(&mut db, BUSINESS, job(1), 2_500).await;
    let repeat = waive(&mut db, (BUSINESS, job(2), 90_000), true, &[job(1)]).await;
    assert!(matches!(repeat, Waiver::Repeat(Appended::New(_))));
    let purged = waive(&mut db, (BUSINESS, job(2), 90_000), true, &[]).await;
    assert!(matches!(purged, Waiver::Repeat(Appended::Already(_))));
    let win = waive(&mut db, (BUSINESS, job(3), 90_000), true, &[]).await;
    assert!(matches!(
        win,
        Waiver::FreeFirstWin {
            entry: Appended::New(_),
            ..
        }
    ));
    let later = waive(&mut db, (BUSINESS, job(3), 90_000), true, &[job(1)]).await;
    assert!(matches!(
        later,
        Waiver::FreeFirstWin {
            entry: Appended::Already(_),
            waived_cents: 7_500
        }
    ));
    let expected = [
        ("charge".to_owned(), 2_500),
        ("repeat-customer".to_owned(), 0),
        ("free-first-win".to_owned(), 0),
    ];
    assert_eq!(kinds(&mut db).await, expected);
}

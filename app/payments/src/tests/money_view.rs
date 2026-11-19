//! TD-FEE-29 (FEE-046, D-444): the set-aside is 8.6% (a setting) of fees collected, to the cent;
//! Casey's expenses show separately and never become ledger entries; collected, set aside,
//! expenses and left add up; the monthly CSV has those columns. TD-FEE-16's and TD-FEE-33's
//! money-view cases: the ledger lines, the processing fee line among them, come only from the
//! ledger, and the app role can neither change nor delete an expense. The Stripe reconcile
//! (FEE-030) is untouched by this task: `reconcile.rs` and its tests are unchanged.

use askhava_core::api::arizona;
use jiff::Timestamp;
use jiff::civil::{Date, date};
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::api::{
    CSV_HEADER, EntryKind, ExpenseError, LedgerEntry, MoneyView, Month, NewEntry, NewExpense,
    add_expense, ledger_append, money_view_csv, owner_money_view,
};
use crate::money_view::{assemble, csv_of, set_aside_cents};

/// The default setting: 860 basis points, 8.6% (D-444).
const RATE: u16 = 860;
const BUSINESS: Uuid = Uuid::from_u128(0x0210_0001);
const JOB: Uuid = Uuid::from_u128(0x0210_0002);

fn expense(day: Date, vendor: &str, cents: i64) -> NewExpense {
    NewExpense::new(day, vendor, cents, "software", "fake note").unwrap()
}

async fn append(pool: &PgPool, kind: EntryKind, cents: i64, stripe: &str) {
    let entry = LedgerEntry::new(NewEntry {
        kind,
        business: Some(BUSINESS),
        job: Some(JOB),
        amount_cents: cents,
        stripe_id: Some(stripe),
        idempotency_key: &format!("t210.{stripe}"),
    })
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    ledger_append(&mut conn, &entry).await.unwrap();
}

async fn ledger_rows(pool: &PgPool) -> (i64, Timestamp) {
    let r = sqlx::query!(
        r#"SELECT count(*) AS "n!", max(recorded_at) AS "at!" FROM payments.ledger_entry"#
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (r.n, r.at.to_jiff())
}

/// TD-FEE-29 (FEE-046, D-444): 8.6% of fees collected, rounded up to the cent; none on nothing.
#[test]
fn td_fee_29_set_aside_is_the_setting_of_collected_to_the_cent() {
    for (collected, want) in [
        (2_500, 215),
        (7_500, 645),
        (19_900, 1_712),
        (100, 9),
        (1, 1),
    ] {
        assert_eq!(set_aside_cents(collected, RATE), want, "{collected}");
    }
    assert_eq!(set_aside_cents(0, RATE), 0);
    assert_eq!(set_aside_cents(-2_500, RATE), 0);
    assert_eq!(set_aside_cents(19_900, 0), 0, "ruling filed: setting at 0");
    assert_eq!(set_aside_cents(19_900, 10_000), 19_900);
    assert_eq!(set_aside_cents(i64::MAX, 10_000), i64::MAX, "no overflow");
}

/// TD-FEE-29, TD-FEE-33 (FEE-046, FEE-051): the four lines add up; the ledger view is kept whole.
#[test]
fn td_fee_29_collected_set_aside_expenses_and_left_add_up() {
    let ledger = MoneyView {
        charges: 29_900,
        refunds: 2_500,
        disputes: 7_500,
        dispute_fees: 1_500,
        processing_fees: 900,
        ..MoneyView::default()
    };
    let spent = vec![];
    let v = assemble(Month::new(2026, 10).unwrap(), ledger, spent, RATE);
    assert_eq!(
        (v.collected, v.set_aside, v.expenses_total),
        (19_900, 1_712, 0)
    );
    assert_eq!(v.ledger, ledger);
    assert_eq!(v.collected, v.set_aside + v.expenses_total + v.left);
}

/// TD-FEE-29, TD-FEE-16, TD-FEE-33 (FEE-046, FEE-028, FEE-051): an expense is stored outside the
/// ledger and listed by month; the ledger lines (processing fee included) come from the ledger.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_29_expenses_show_separately_and_never_enter_the_ledger(pool: PgPool) {
    append(&pool, EntryKind::Charge, 7_500, "ch_FakeT210a").await;
    append(&pool, EntryKind::Charge, 19_900, "ch_FakeT210b").await;
    append(&pool, EntryKind::Refund, 2_500, "re_FakeT210a").await;
    append(&pool, EntryKind::ProcessingFee, 900, "txn_FakeT210a").await;
    let (rows, now) = ledger_rows(&pool).await;
    let first = now.to_zoned(arizona()).date().first_of_month();
    let earlier = first.yesterday().unwrap();
    for e in [
        expense(first, "Fake Hosting Co", 1_234),
        expense(earlier, "Old Fake", 99),
    ] {
        add_expense(&pool, &e, now).await.unwrap();
    }
    assert_eq!(
        ledger_rows(&pool).await.0,
        rows,
        "no ledger row for an expense"
    );
    let month = Month::of(now);
    let v = owner_money_view(&pool, month, RATE).await.unwrap();
    assert_eq!((v.ledger.charges, v.ledger.refunds), (27_400, 2_500));
    assert_eq!(v.ledger.processing_fees, 900, "TD-FEE-33: the fee line");
    assert_eq!((v.collected, v.set_aside), (24_900, 2_142));
    assert_eq!(v.expenses.len(), 1, "only this month's expense");
    assert_eq!(
        (v.expenses[0].vendor.as_str(), v.expenses_total),
        ("Fake Hosting Co", 1_234)
    );
    assert_eq!(v.left, 24_900 - 2_142 - 1_234);
    let csv = money_view_csv(&pool, month, RATE).await.unwrap();
    assert_eq!(csv, csv_of(&v));
}

/// TD-FEE-29 (FEE-046): the CSV carries the four columns, dollars to the cent, and escapes text.
#[test]
fn td_fee_29_the_monthly_csv_has_the_columns() {
    assert!(CSV_HEADER.ends_with("collected,set_aside,expenses,left"));
    let day = date(2026, 10, 3);
    let spent = vec![crate::api::Expense {
        id: 1,
        spent_on: day,
        vendor: "=Fake, \"Vendor\"".to_owned(),
        amount_cents: 1_205,
        category: "-ads".to_owned(),
        note: String::new(),
    }];
    let ledger = MoneyView {
        charges: 7_500,
        ..MoneyView::default()
    };
    let csv = csv_of(&assemble(
        Month::new(2026, 10).unwrap(),
        ledger,
        spent,
        RATE,
    ));
    let lines: Vec<&str> = csv.split("\r\n").collect();
    assert_eq!(lines[0], CSV_HEADER);
    assert_eq!(
        lines[1],
        "expense,2026-10-03,\"'=Fake, \"\"Vendor\"\"\",'-ads,,,,12.05,"
    );
    assert_eq!(lines[2], "month,2026-10,,,,75.00,6.45,12.05,56.50");
    assert_eq!(lines[3], "", "ends with a line break");
}

/// TD-FEE-29 (FEE-046, R1, R2): each field is checked once, and a future date is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_29_bad_expenses_are_refused(pool: PgPool) {
    let day = date(2026, 10, 3);
    let bad = |d, v: &str, c, k: &str, n: &str| NewExpense::new(d, v, c, k, n).unwrap_err();
    assert_eq!(bad(date(1999, 12, 31), "V", 1, "k", ""), ExpenseError::Date);
    assert_eq!(bad(day, " ", 1, "k", ""), ExpenseError::Vendor);
    assert_eq!(bad(day, &"v".repeat(121), 1, "k", ""), ExpenseError::Vendor);
    assert_eq!(bad(day, "a\nb", 1, "k", ""), ExpenseError::Vendor);
    for cents in [0, -1, 100_000_001] {
        assert_eq!(bad(day, "V", cents, "k", ""), ExpenseError::Amount);
    }
    assert_eq!(bad(day, "V", 1, "", ""), ExpenseError::Category);
    assert_eq!(bad(day, "V", 1, "k", &"n".repeat(501)), ExpenseError::Note);
    assert_eq!(Month::new(1999, 1), Err(ExpenseError::Month));
    assert_eq!(Month::new(2026, 13), Err(ExpenseError::Month));
    assert_eq!(Month::new(2026, 3).unwrap().to_string(), "2026-03");
    let now: Timestamp = "2026-10-03T06:00:00Z".parse().unwrap();
    let tomorrow_in_arizona = expense(day, "Fake", 100);
    assert_eq!(
        add_expense(&pool, &tomorrow_in_arizona, now).await,
        Err(ExpenseError::Date)
    );
}

/// TD-FEE-16 (FEE-046, D-1495, R9): the app role can neither change nor delete an expense.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_the_app_role_cannot_change_an_expense(pool: PgPool) {
    let now: Timestamp = "2026-10-03T19:00:00Z".parse().unwrap();
    add_expense(&pool, &expense(date(2026, 10, 3), "Fake", 100), now)
        .await
        .unwrap();
    let mut tx = as_app(&pool).await;
    let update = sqlx::query!("UPDATE payments.expense SET amount_cents = 1")
        .execute(&mut *tx)
        .await;
    assert!(denied(&update), "UPDATE was allowed");
    drop(tx);
    let mut tx = as_app(&pool).await;
    let delete = sqlx::query!("DELETE FROM payments.expense")
        .execute(&mut *tx)
        .await;
    assert!(denied(&delete), "DELETE was allowed");
}

/// Opens a transaction running as `askhava_app` until it ends (`SET LOCAL ROLE`, R9).
async fn as_app(pool: &PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query!("SET LOCAL ROLE askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

/// True when `outcome` failed with `insufficient_privilege` (SQLSTATE 42501).
fn denied<T>(outcome: &sqlx::Result<T>) -> bool {
    matches!(outcome, Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("42501"))
}

//! TD-FEE-16's ledger cases (FEE-028, FEE-032, D-092): every FEE-028 entry type appends through
//! `ledger_append`, a repeated key appends once, the checked constructor refuses entries the
//! migration would refuse, the table's own checks refuse a bad row written around it, a failed
//! statement is `LedgerError::Database`, and the app role cannot update, delete or truncate a
//! ledger row (SQLSTATE 42501).
//! The Stripe-matching, reconcile and architecture cases of TD-FEE-16 are T067's and T175's.

use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};

use crate::api::EntryKind::{Charge, Credit, FeeFreeJobType, Payout, RepeatCustomer};
use crate::api::LedgerError::{Amount, Key, Parties, StripeId};
use crate::api::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};

const BUSINESS: Uuid = Uuid::from_u128(0xb0b0_0000_0000_0000_0000_0000_0000_0001);
const JOB: Uuid = Uuid::from_u128(0x10b0_0000_0000_0000_0000_0000_0000_0001);

/// A valid fake entry of `kind` under `key`.
fn raw(kind: EntryKind, key: &str) -> NewEntry<'_> {
    let zero = kind.is_zero();
    NewEntry {
        kind,
        business: (kind != EntryKind::Payout).then_some(BUSINESS),
        job: (kind != EntryKind::Payout).then_some(JOB),
        amount_cents: if zero { 0 } else { 7_500 },
        stripe_id: (!zero).then_some("ch_Fake0001"),
        idempotency_key: key,
    }
}

fn entry(kind: EntryKind, key: &str) -> LedgerEntry {
    LedgerEntry::new(raw(kind, key)).unwrap()
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM payments.ledger_entry"#)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// TD-FEE-16 (FEE-028): each of the eleven entry types, the three $0 kinds included, appends.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn every_entry_type_appends(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    for kind in EntryKind::ALL {
        let key = format!("test:{}", kind.code());
        let done = ledger_append(&mut conn, &entry(kind, &key)).await.unwrap();
        assert!(matches!(done, Appended::New(_)), "{kind:?}");
    }
    assert_eq!(count(&pool).await, 11);
    let codes = sqlx::query_scalar!("SELECT kind FROM payments.ledger_entry ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();
    let want: Vec<&str> = EntryKind::ALL.iter().map(|k| k.code()).collect();
    assert_eq!(codes, want);
}

/// TD-FEE-16 (FEE-028, JOB-006): a repeated key with the same entry writes nothing more; with an
/// entry that differs in any one field (kind, business, job, amount, Stripe ID) it is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_repeated_key_appends_once(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let first = entry(EntryKind::Charge, "test:charge:1");
    let Appended::New(id) = ledger_append(&mut conn, &first).await.unwrap() else {
        panic!("the first append is new");
    };
    let again = ledger_append(&mut conn, &first).await.unwrap();
    assert_eq!(again, Appended::Already(id));
    let changes: [fn(&mut NewEntry<'_>); 5] = [
        |r| r.kind = EntryKind::Refund,
        |r| r.business = Some(JOB),
        |r| r.job = Some(BUSINESS),
        |r| r.amount_cents = 2_500,
        |r| r.stripe_id = Some("ch_Fake0002"),
    ];
    for change in changes {
        let mut other = raw(EntryKind::Charge, "test:charge:1");
        change(&mut other);
        let refused = ledger_append(&mut conn, &LedgerEntry::new(other).unwrap()).await;
        assert_eq!(refused, Err(LedgerError::KeyReused));
    }
    assert_eq!(count(&pool).await, 1);
}

/// TD-FEE-16 (FEE-028, FEE-013, FEE-045): `LedgerEntry::new` refuses each rule the table's checks
/// hold (amount, parties, Stripe ID, key), before any statement runs; the table's own refusals are
/// `the_table_refuses_bad_rows`.
#[test]
fn the_constructor_refuses_bad_entries() {
    let bad = |change: fn(&mut NewEntry<'_>), kind: EntryKind| {
        let mut r = raw(kind, "test:bad");
        change(&mut r);
        LedgerEntry::new(r).unwrap_err()
    };
    assert_eq!(bad(|r| r.amount_cents = -1, Charge), Amount);
    assert_eq!(bad(|r| r.amount_cents = 0, Charge), Amount);
    assert_eq!(bad(|r| r.amount_cents = 2_500, RepeatCustomer), Amount);
    assert_eq!(bad(|r| r.business = Some(BUSINESS), Payout), Parties);
    assert_eq!(bad(|r| r.job = Some(JOB), Payout), Parties);
    assert_eq!(bad(|r| r.business = None, Credit), Parties);
    assert_eq!(bad(|r| r.job = None, Charge), Parties);
    assert_eq!(
        bad(|r| r.stripe_id = Some("ch_Fake0001"), FeeFreeJobType),
        StripeId
    );
    assert_eq!(bad(|r| r.stripe_id = Some("ch_"), Charge), StripeId);
    assert_eq!(bad(|r| r.stripe_id = Some("CH_Fake"), Charge), StripeId);
    assert_eq!(bad(|r| r.stripe_id = Some("c_Fake"), Charge), StripeId);
    assert_eq!(
        bad(|r| r.stripe_id = Some("abcdefghi_Fake"), Charge),
        StripeId
    );
    assert_eq!(
        bad(|r| r.stripe_id = Some("nounderscore"), Charge),
        StripeId
    );
    assert_eq!(bad(|r| r.idempotency_key = "", Charge), Key);
    assert_eq!(bad(|r| r.idempotency_key = "has space", Charge), Key);
    let credit = LedgerEntry::new(NewEntry {
        job: None,
        ..raw(Credit, "test:credit")
    })
    .unwrap();
    assert_eq!((credit.kind(), credit.amount_cents()), (Credit, 7_500));
}

/// TD-FEE-16 (FEE-028, D-092): as `askhava_app` a row can be appended and read, never updated,
/// deleted or truncated.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_app_role_cannot_change_a_row(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let mut tx = as_app(&mut conn).await;
    let done = ledger_append(&mut tx, &entry(EntryKind::Charge, "test:role")).await;
    assert!(matches!(done, Ok(Appended::New(_))));
    tx.commit().await.unwrap();
    let mut tx = as_app(&mut conn).await;
    let update = sqlx::query!("UPDATE payments.ledger_entry SET amount_cents = 1")
        .execute(&mut *tx)
        .await;
    assert!(permission_denied(&update), "{update:?}");
    tx.rollback().await.unwrap();
    let mut tx = as_app(&mut conn).await;
    let delete = sqlx::query!("DELETE FROM payments.ledger_entry")
        .execute(&mut *tx)
        .await;
    assert!(permission_denied(&delete), "{delete:?}");
    tx.rollback().await.unwrap();
    let mut tx = as_app(&mut conn).await;
    let truncate = sqlx::query!("TRUNCATE payments.ledger_entry")
        .execute(&mut *tx)
        .await;
    assert!(permission_denied(&truncate), "{truncate:?}");
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool).await, 1);
}

/// Opens a transaction running as `askhava_app` until it ends (`SET LOCAL ROLE`, R9).
async fn as_app(conn: &mut PgConnection) -> Transaction<'_, Postgres> {
    into_app(conn.begin().await.unwrap()).await
}

/// Switches an open transaction to `askhava_app` until it ends (`SET LOCAL ROLE`, R9).
async fn into_app(mut tx: Transaction<'_, Postgres>) -> Transaction<'_, Postgres> {
    sqlx::query!("SET LOCAL ROLE askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

/// True when `outcome` failed with `insufficient_privilege` (SQLSTATE 42501), the grant refusal.
fn permission_denied<T>(outcome: &sqlx::Result<T>) -> bool {
    let Err(sqlx::Error::Database(err)) = outcome else {
        return false;
    };
    err.code().as_deref() == Some("42501")
}

/// Writes a row straight to the table, around `LedgerEntry::new`, to probe its checks.
async fn insert_raw(
    conn: &mut PgConnection,
    kind: &str,
    amount_cents: i64,
    stripe_id: Option<&str>,
    key: &str,
) -> sqlx::Result<u64> {
    sqlx::query!(
        "INSERT INTO payments.ledger_entry
           (kind, business_id, job_id, amount_cents, stripe_id, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6)",
        kind,
        BUSINESS,
        JOB,
        amount_cents,
        stripe_id,
        key,
    )
    .execute(conn)
    .await
    .map(|done| done.rows_affected())
}

/// TD-FEE-16 (FEE-028, FEE-013, D-1383): the table itself refuses a negative amount and a $0 kind
/// with a Stripe ID, so a writer that skips `LedgerEntry::new` still cannot store them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_table_refuses_bad_rows(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let bad = [
        ("charge", -7_500, Some("ch_Fake0001"), "test:raw:negative"),
        (
            "repeat-customer",
            0,
            Some("ch_Fake0001"),
            "test:raw:zero-stripe",
        ),
    ];
    for (kind, amount, stripe_id, key) in bad {
        let err = insert_raw(&mut conn, kind, amount, stripe_id, key)
            .await
            .expect_err("refused");
        let e = err.as_database_error().expect("database error");
        assert_eq!(e.code().as_deref(), Some("23514"), "{kind}");
        assert_eq!(e.constraint(), Some("ledger_entry_amount_check"), "{kind}");
    }
    let good = insert_raw(&mut conn, "repeat-customer", 0, None, "test:raw:zero").await;
    assert_eq!(good.unwrap(), 1);
    assert_eq!(count(&pool).await, 1);
}

/// TD-FEE-16 (FEE-028): a refused insert, and a refused read of the row a repeated key found, are
/// both `LedgerError::Database` (here the grants are narrowed inside a rolled-back transaction).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_statement_is_a_database_error(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let first = entry(EntryKind::Charge, "test:db");
    ledger_append(&mut conn, &first).await.unwrap();
    let mut tx = conn.begin().await.unwrap();
    sqlx::query!("REVOKE INSERT ON payments.ledger_entry FROM askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut tx = into_app(tx).await;
    let fresh = entry(EntryKind::Charge, "test:db:new");
    assert_eq!(
        ledger_append(&mut tx, &fresh).await,
        Err(LedgerError::Database)
    );
    tx.rollback().await.unwrap();
    let mut tx = conn.begin().await.unwrap();
    sqlx::query!("REVOKE SELECT ON payments.ledger_entry FROM askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query!("GRANT SELECT (id, idempotency_key) ON payments.ledger_entry TO askhava_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut tx = into_app(tx).await;
    assert_eq!(
        ledger_append(&mut tx, &first).await,
        Err(LedgerError::Database)
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool).await, 1);
}

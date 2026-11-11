//! TD-FEE-57 (FEE-012, FEE-013, FEE-045, PICK-022; D-2302, D-2832, D-2713): `no_fee_release`
//! clears a pick with no fee only on a $0 ledger decision for the pick's own business and job, as
//! its `payments.pick_fee` row stores them. A waiver for one job of a business never clears a pick
//! of that business on another job, whichever pair the caller passes; a pick with no stored fee,
//! a caller pair other than the stored one, a stored pair with only a charge entry, or a pick with
//! a PaymentIntent is refused. On a fresh database; every ID is fake.

use askhava_payments::api::{
    BusinessId, EntryKind, FeeJob, FeeMode, FeeQuote, LedgerEntry, NewEntry, PeriodJob,
    RecurringCap, ReleaseError, Service, VisitTier, fee_at_pick, fix_fee_at_pick, ledger_append,
    no_fee_release,
};
use askhava_privacy::api::ReleaseBasis;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb337_0001));
const OTHER_BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb337_0002));
const PICK_A: Uuid = Uuid::from_u128(0x337_00a1);
const PICK_B: Uuid = Uuid::from_u128(0x337_00b1);
const JOB_A: Uuid = Uuid::from_u128(0x337_01a1);
const JOB_B: Uuid = Uuid::from_u128(0x337_01b1);
const NOW: i64 = 1_793_700_000;

/// Fixes at `pick` the fee of a firm $1,000 quote in tiers mode on `business` and `job`: $75, or
/// $0 for a fee-free job type (FEE-005, FEE-045).
async fn fix(conn: &mut PgConnection, pick: Uuid, (business, job): (BusinessId, Uuid), free: bool) {
    let period = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: free,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(100_000), None, period, FeeMode::Tiers, None);
    let at = Timestamp::from_second(NOW).expect("time");
    let written = fix_fee_at_pick(conn, pick, business, job, &fee.expect("fee"), at).await;
    assert_eq!(written, Ok(true));
}

/// Appends a ledger entry of `kind` for `business` and `job`: $0 for the $0 kinds, else a $75
/// charge with a fake charge ID.
async fn entry(conn: &mut PgConnection, kind: EntryKind, (business, job): (BusinessId, Uuid)) {
    let key = format!("t337-{}-{}-{job}", kind.code(), business.0);
    let zero = kind.is_zero();
    let raw = NewEntry {
        kind,
        business: Some(business.0),
        job: Some(job),
        amount_cents: if zero { 0 } else { 7500 },
        stripe_id: if zero { None } else { Some("ch_FakeT337") },
        idempotency_key: &key,
    };
    ledger_append(conn, &LedgerEntry::new(raw).expect("entry"))
        .await
        .expect("append");
}

/// `no_fee_release` for `pick` with the caller's pair, as the pick it clears or the refusal.
async fn clear(
    conn: &mut PgConnection,
    pick: Uuid,
    (business, job): (BusinessId, Uuid),
) -> Result<(Uuid, ReleaseBasis), ReleaseError> {
    no_fee_release(conn, pick, (business.0, job))
        .await
        .map(|c| (c.pick(), c.basis()))
}

/// TD-FEE-57 (FEE-012, FEE-045, PICK-022, D-2302): a free first win recorded for pick A's job
/// never clears pick B of the same business on another job, with either pair; pick A is cleared on
/// its own; pick B, once its own fee-free $0 entry is recorded, is cleared.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_waiver_for_one_job_never_clears_another_pick(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    fix(&mut conn, PICK_A, (BUSINESS, JOB_A), false).await;
    fix(&mut conn, PICK_B, (BUSINESS, JOB_B), true).await;
    entry(&mut conn, EntryKind::FreeFirstWin, (BUSINESS, JOB_A)).await;
    for pair in [(BUSINESS, JOB_A), (BUSINESS, JOB_B)] {
        let refused = clear(&mut conn, PICK_B, pair).await;
        assert_eq!(refused, Err(ReleaseError::NoFeeDecision));
    }
    let own = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
    assert_eq!(own, Ok((PICK_A, ReleaseBasis::NoFeeDue)));
    entry(&mut conn, EntryKind::FeeFreeJobType, (BUSINESS, JOB_B)).await;
    let cleared = clear(&mut conn, PICK_B, (BUSINESS, JOB_B)).await;
    assert_eq!(cleared, Ok((PICK_B, ReleaseBasis::NoFeeDue)));
}

/// TD-FEE-57 (FEE-013, PICK-022, D-2302): a pick with no `payments.pick_fee` row is refused, even
/// with a $0 repeat-customer entry for the pair its caller passes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_pick_with_no_stored_fee_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    entry(&mut conn, EntryKind::RepeatCustomer, (BUSINESS, JOB_A)).await;
    let refused = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
    assert_eq!(refused, Err(ReleaseError::NoFeeDecision));
}

/// TD-FEE-57 (FEE-013, PICK-022, D-2302): a caller pair that differs from the pick's stored row,
/// in business or in job, is refused even when that pair has its own $0 entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_pair_other_than_the_stored_one_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    fix(&mut conn, PICK_A, (BUSINESS, JOB_A), false).await;
    for pair in [(OTHER_BUSINESS, JOB_A), (BUSINESS, JOB_B)] {
        entry(&mut conn, EntryKind::RepeatCustomer, pair).await;
        let refused = clear(&mut conn, PICK_A, pair).await;
        assert_eq!(refused, Err(ReleaseError::NoFeeDecision));
    }
}

/// TD-FEE-57 (FEE-012, PICK-022, D-2832): a pick whose own stored pair has only a charge entry,
/// no $0 one, is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_charge_entry_alone_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    fix(&mut conn, PICK_A, (BUSINESS, JOB_A), false).await;
    entry(&mut conn, EntryKind::Charge, (BUSINESS, JOB_A)).await;
    let refused = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
    assert_eq!(refused, Err(ReleaseError::NoFeeDecision));
}

/// TD-FEE-57 (FEE-036, PICK-022, D-1478, D-2832): a pick with its own stored row and a $0 entry
/// for that same pair, but a PaymentIntent, has a fee to pay and is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_pick_with_a_payment_intent_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    fix(&mut conn, PICK_A, (BUSINESS, JOB_A), false).await;
    entry(&mut conn, EntryKind::FreeFirstWin, (BUSINESS, JOB_A)).await;
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1, $2, $3, 7500, 'pi_FakeT337', false, to_timestamp($4))",
    )
    .bind(PICK_A)
    .bind(BUSINESS.0)
    .bind(JOB_A)
    .bind(NOW)
    .execute(&mut *conn)
    .await
    .expect("intent");
    let refused = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
    assert_eq!(refused, Err(ReleaseError::NoFeeDecision));
}

/// TD-FEE-57 (R5): a failed read of the stored fee, or of the ledger, is `Database`, never a
/// clearing; the app role is denied each table in turn inside a rolled-back transaction.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_57_a_failed_read_clears_nothing(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    fix(&mut conn, PICK_A, (BUSINESS, JOB_A), true).await;
    entry(&mut conn, EntryKind::FeeFreeJobType, (BUSINESS, JOB_A)).await;
    for revoke in [
        "REVOKE SELECT ON payments.pick_fee FROM askhava_app",
        "REVOKE SELECT ON payments.ledger_entry FROM askhava_app",
    ] {
        sqlx::query("BEGIN")
            .execute(&mut *conn)
            .await
            .expect("begin");
        sqlx::query(revoke)
            .execute(&mut *conn)
            .await
            .expect("revoke");
        sqlx::query("SET LOCAL ROLE askhava_app")
            .execute(&mut *conn)
            .await
            .expect("role");
        let failed = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
        sqlx::query("ROLLBACK")
            .execute(&mut *conn)
            .await
            .expect("rollback");
        assert_eq!(failed, Err(ReleaseError::Database));
    }
    let cleared = clear(&mut conn, PICK_A, (BUSINESS, JOB_A)).await;
    assert_eq!(cleared, Ok((PICK_A, ReleaseBasis::NoFeeDue)));
}

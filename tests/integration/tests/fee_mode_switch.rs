//! TD-FEE-06 (FEE-007, D-072): on a fresh database, the owner's fee-mode switch is logged in the
//! audit log with the owner as actor, flat mode defaults each job type to its category's set
//! tier, and a business that has not accepted the new authorization version can't confirm until
//! it does. All IDs and amounts are fake.

use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, ConfirmError, Confirmed, FEE_MODE_ACTION, FEE_MODE_ID, FeeJob,
    FeeMode, FeeQuote, FeeTerms, PeriodJob, RecurringCap, Service, VisitTier, accept_authorization,
    fee_at_pick, may_confirm, record_fee_at_confirm, switch_fee_mode,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb060_0006));
const OWNER: Uuid = Uuid::from_u128(0x0a06_0006);
const START: FeeTerms = FeeTerms::new(FeeMode::Tiers, 1);
/// An audit row as (actor kind, actor, action, subject, from, to).
type AuditRow = (
    String,
    Option<Uuid>,
    String,
    Uuid,
    Option<String>,
    Option<String>,
);

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

/// A job in a category whose set tier is $75.
fn medium_job() -> PeriodJob {
    PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::from_dollars(75).expect("tier"),
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    }
}

/// TD-FEE-06 (FEE-007, JOB-005): the switch writes one audit row naming the owner and the move;
/// choosing the mode in force writes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_06_the_switch_is_logged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let same = switch_fee_mode(&mut db, OWNER, START, FeeMode::Tiers).await;
    assert_eq!(same, Ok(None));
    let flat = switch_fee_mode(&mut db, OWNER, START, FeeMode::Flat).await;
    assert_eq!(flat, Ok(Some(FeeTerms::new(FeeMode::Flat, 2))));
    let rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT actor_kind, actor_id, action, subject_id, from_status, to_status \
             FROM ops.audit_log WHERE subject_kind = 'payments.fee_mode'",
    )
    .fetch_all(&pool)
    .await
    .expect("audit");
    let row = (
        "owner".to_owned(),
        Some(OWNER),
        FEE_MODE_ACTION.to_owned(),
        FEE_MODE_ID,
        Some("tiers".to_owned()),
        Some("flat".to_owned()),
    );
    assert_eq!(rows, [row]);
}

/// TD-FEE-06 (FEE-007, D-072): after the switch to flat mode the business can't confirm, and no
/// fee is recorded, until it accepts the new authorization version; the flat fee is then the
/// category's set tier whatever the quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_06_no_confirm_without_the_new_authorization(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT060b")
        .execute(&mut *db)
        .await
        .expect("customer");
    let refused = may_confirm(&mut db, BUSINESS, START).await;
    assert_eq!(refused, Err(ConfirmError::NewAuthorizationNeeded));
    accept_authorization(&mut db, BUSINESS, START, AUTHORIZATION_TEXT, at(0))
        .await
        .expect("accepted");
    assert_eq!(may_confirm(&mut db, BUSINESS, START).await, Ok(()));
    let flat = switch_fee_mode(&mut db, OWNER, START, FeeMode::Flat)
        .await
        .expect("switch")
        .expect("changed");
    let picked = fee_at_pick(
        FeeQuote::Fixed(900_000),
        None,
        medium_job(),
        FeeMode::Flat,
        None,
    )
    .expect("fee");
    assert_eq!(picked.cents(), 7_500);
    let confirmed = Confirmed {
        business: BUSINESS,
        job: Uuid::from_u128(0x0b06_0006),
        charge: Some("ch_FakeT060b"),
    };
    let secret = SecretString::from("fake-id-secret-t060".to_owned());
    let early = record_fee_at_confirm(&mut db, &secret, (&picked, flat), confirmed).await;
    assert_eq!(early, Err(ConfirmError::NewAuthorizationNeeded));
    let count = "SELECT count(*) FROM payments.ledger_entry";
    let rows: i64 = sqlx::query_scalar(count)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 0);
    accept_authorization(
        &mut db,
        BUSINESS,
        flat,
        "Fake flat-mode authorization",
        at(1),
    )
    .await
    .expect("accepted again");
    let late = record_fee_at_confirm(&mut db, &secret, (&picked, flat), confirmed).await;
    assert!(late.is_ok());
    let amount: i64 = sqlx::query_scalar("SELECT amount_cents FROM payments.ledger_entry")
        .fetch_one(&pool)
        .await
        .expect("amount");
    assert_eq!(amount, 7_500);
}

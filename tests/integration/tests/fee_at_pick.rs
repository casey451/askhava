//! TD-FEE-04 (integration cases) and TD-FEE-05 (FEE-005, FEE-006, JOB-016; D-009, D-071, D-357):
//! on a fresh database, the fee is fixed at the pick from the chosen quote row alone, an AI flag
//! or a later edit to another quote changes nothing, the fee is written to the ledger at Confirm,
//! and a day-3 PROBLEM reply about a much higher final bill changes no ledger row.
//!
//! The Stripe charge itself is T062's; here its `ch_…` ID is fake. All IDs and amounts are fake.

use askhava_core::api::Guarded;
use askhava_jobs::api::rules::{CheckIn, Contact, Reply};
use askhava_jobs::api::{JobStatus, connected_reply, create, current, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, Appended, BusinessId, CardError, ConfirmError, Confirmed, FeeJob, FeeMode,
    FeeQuote, FeeTerms, PeriodJob, PickedFee, RecurringCap, Service, VisitTier,
    accept_authorization, fee_at_pick, may_confirm, record_fee_at_confirm,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb060_0001));
const OTHER: Uuid = Uuid::from_u128(0xb060_0002);
const TERMS: FeeTerms = FeeTerms::new(FeeMode::Tiers, 1);
const JOB: PeriodJob = PeriodJob {
    job: FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    },
    service: Service::NotRecurring,
    cap: RecurringCap::DEFAULT,
};
/// The ledger as rows of (kind, business, job, amount, Stripe ID, key).
const LEDGER: &str = "SELECT kind, business_id, job_id, amount_cents, stripe_id, idempotency_key \
                      FROM payments.ledger_entry ORDER BY seq";
type Row = (
    String,
    Option<Uuid>,
    Option<Uuid>,
    i64,
    Option<String>,
    String,
);

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t060".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

/// A business with a Stripe customer and the tiers-mode authorization accepted.
async fn ready_business(db: &mut PgConnection) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT060")
        .execute(&mut *db)
        .await
        .expect("customer");
    accept_authorization(db, BUSINESS, TERMS, AUTHORIZATION_TEXT, at(0))
        .await
        .expect("authorization");
}

/// A firm quote of `cents` by `business` on `job`, read by the AI and confirmed.
async fn quote(db: &mut PgConnection, job: Uuid, business: Uuid, cents: i64) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', $3, false, 'Fake notes', 'Fake quote', true, true, now())",
    )
    .bind(job)
    .bind(business)
    .bind(cents)
    .execute(db)
    .await
    .expect("quote");
}

/// The pick: the fee fixed from the chosen quote row alone.
async fn pick(db: &mut PgConnection, job: Uuid) -> PickedFee {
    let cents: i64 = sqlx::query_scalar(
        "SELECT amount_cents FROM quotes.quote WHERE job_id = $1 AND business_id = $2",
    )
    .bind(job)
    .bind(BUSINESS.0)
    .fetch_one(db)
    .await
    .expect("chosen quote");
    let chosen = FeeQuote::Fixed(u64::try_from(cents).expect("cents"));
    fee_at_pick(chosen, None, JOB, FeeMode::Tiers, None).expect("fee")
}

/// Records the fee at Confirm with a fake charge ID, authorized before the charge.
async fn confirm(db: &mut PgConnection, job: Uuid, picked: &PickedFee) -> Appended {
    let confirmed = Confirmed {
        business: BUSINESS,
        job,
        charge: Some("ch_FakeT060"),
    };
    let authorized = may_confirm(db, BUSINESS, TERMS).await.expect("authorized");
    record_fee_at_confirm(db, &secret(), (picked, authorized), confirmed)
        .await
        .expect("recorded")
}

/// An AI reading of `job` with its major flag set or unset (the reading table is append-only).
async fn ai_major(db: &mut PgConnection, job: Uuid, major: bool) {
    sqlx::query(
        "INSERT INTO ai.reading (subject_id, trade_confidence, job_type_confidence, emergency, \
         urgent, major, spanish) VALUES ($1, 0.9, 0.9, false, false, $2, false)",
    )
    .bind(job)
    .bind(major)
    .execute(db)
    .await
    .expect("reading");
}

async fn ledger(pool: &PgPool) -> Vec<Row> {
    sqlx::query_as(LEDGER)
        .fetch_all(pool)
        .await
        .expect("ledger")
}

/// TD-FEE-04 (FEE-005, D-009): the fee is fixed from the chosen quote; flipping the AI flags on
/// it and editing another quote afterwards leave the fee charged at Confirm unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_04_fixed_at_the_pick_from_the_chosen_quote(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    ready_business(&mut db).await;
    let job = Uuid::from_u128(0x0b06_0004);
    quote(&mut db, job, BUSINESS.0, 45_000).await;
    quote(&mut db, job, OTHER, 400_000).await;
    ai_major(&mut db, job, false).await;
    let picked = pick(&mut db, job).await;
    assert_eq!(picked.cents(), 2_500);
    ai_major(&mut db, job, true).await;
    assert_eq!(pick(&mut db, job).await, picked);
    sqlx::query(
        "UPDATE quotes.quote SET amount_cents = 900000, edited_at = now() WHERE business_id = $1",
    )
    .bind(OTHER)
    .execute(&mut *db)
    .await
    .expect("edit another quote");
    sqlx::query("UPDATE quotes.quote SET ai_read = false WHERE business_id = $1")
        .bind(BUSINESS.0)
        .execute(&mut *db)
        .await
        .expect("flip the AI flag");
    assert_eq!(pick(&mut db, job).await, picked);
    assert!(matches!(
        confirm(&mut db, job, &picked).await,
        Appended::New(_)
    ));
    let rows = ledger(&pool).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!((row.0.as_str(), row.3), ("charge", 2_500));
    assert_eq!((row.1, row.2), (Some(BUSINESS.0), Some(job)));
    assert_eq!(row.4.as_deref(), Some("ch_FakeT060"));
}

/// TD-FEE-05 (FEE-006, JOB-016, D-071): charged at Confirm, once; a day-3 PROBLEM reply about a
/// final bill far above the quote closes the job and changes no ledger row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_05_problem_reply_changes_no_fee(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    ready_business(&mut db).await;
    let job = create(&mut db, at(1), false, Actor::System)
        .await
        .expect("job");
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    quote(&mut db, job, BUSINESS.0, 120_000).await;
    let picked = pick(&mut db, job).await;
    // The business confirms while the job is Payment pending, the charge then connects it.
    for (i, step) in path.windows(2).enumerate() {
        if i == 4 {
            let recorded = confirm(&mut db, job, &picked).await;
            assert!(matches!(recorded, Appended::New(_)));
        }
        let moved = move_to(&mut db, job, (step[0], step[1]), at(2), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    let before = ledger(&pool).await;
    assert_eq!(before.len(), 1);
    assert_eq!((before[0].0.as_str(), before[0].3), ("charge", 7_500));
    assert_eq!(before[0].4.as_deref(), Some("ch_FakeT060"));
    let reply = (Reply::Problem, CheckIn::Day3, Contact::Call);
    let closed = connected_reply(&mut db, job, reply, (at(3 * 24 * 60), Actor::System)).await;
    assert_eq!(closed, Ok(Guarded::Applied(())));
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "closed");
    assert!(matches!(
        confirm(&mut db, job, &picked).await,
        Appended::Already(_)
    ));
    assert_eq!(ledger(&pool).await, before);
}

/// TD-FEE-05 (FEE-006, FEE-045): a fee-free job type records $0 at Confirm with no charge; a
/// charged fee with no charge ID, or one that is not a `ch_…` charge, is refused and writes
/// nothing, as is a $0 fee with an ID or a proof for another business; no authorization is
/// accepted for a business with no Stripe customer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_05_fee_free_and_refused_entries(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let stranger = BusinessId(OTHER);
    let none = accept_authorization(&mut db, stranger, TERMS, AUTHORIZATION_TEXT, at(0)).await;
    assert_eq!(none, Err(CardError::NoCustomer));
    ready_business(&mut db).await;
    let job = Uuid::from_u128(0x0b06_0005);
    let free_job = PeriodJob {
        job: FeeJob {
            fee_free: true,
            ..JOB.job
        },
        ..JOB
    };
    let free = fee_at_pick(
        FeeQuote::Fixed(60_000),
        None,
        free_job,
        FeeMode::Tiers,
        None,
    );
    let free = free.expect("fee");
    let bad = Confirmed {
        business: BUSINESS,
        job,
        charge: Some("not-a-charge"),
    };
    let priced = pick_of(60_000);
    let ok = may_confirm(&mut db, BUSINESS, TERMS)
        .await
        .expect("authorized");
    let confirmed = Confirmed {
        charge: None,
        ..bad
    };
    let pi = Confirmed {
        charge: Some("pi_FakeT060"),
        ..bad
    };
    for (fee, charge) in [
        (&priced, bad),
        (&priced, confirmed),
        (&priced, pi),
        (&free, pi),
    ] {
        let refused = record_fee_at_confirm(&mut db, &secret(), (fee, ok), charge).await;
        assert_eq!(refused, Err(ConfirmError::ChargeId));
    }
    let elsewhere = Confirmed {
        business: stranger,
        ..confirmed
    };
    let refused = record_fee_at_confirm(&mut db, &secret(), (&free, ok), elsewhere).await;
    assert_eq!(refused, Err(ConfirmError::Mismatch));
    assert!(ledger(&pool).await.is_empty());
    let recorded = record_fee_at_confirm(&mut db, &secret(), (&free, ok), confirmed).await;
    assert!(matches!(recorded, Ok(Appended::New(_))));
    let rows = ledger(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].3, rows[0].4.as_deref()),
        ("fee-free-job-type", 0, None)
    );
}

/// The fee fixed at the pick of a firm quote of `cents` in a priced job.
fn pick_of(cents: u64) -> PickedFee {
    fee_at_pick(FeeQuote::Fixed(cents), None, JOB, FeeMode::Tiers, None).expect("fee")
}

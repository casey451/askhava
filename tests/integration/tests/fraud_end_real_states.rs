//! T867 (FEE-052, JOB-014, PICK-022, PICK-046, PICK-047; D-2627, D-5888): an early fraud
//! warning before release ends the pick as fraud-refunded and returns the job to Quotes ready
//! from the states that really occur: a pick still waiting (`awaiting_confirm`) whose job is in
//! Payment pending after its charge succeeded, and a `paid` pick whose job the settle made
//! Connected with no release. A released pick is only listed for Casey, a repeated warning ends
//! nothing twice, and a refunded success holds the Payment pending wait without asking for a
//! release. PostgreSQL and a scripted fake Stripe; every ID, name and number is fake.

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{FRAUD_WARNING_ALERT, PaymentHold, payment_held};
use askhava_picks::api::{
    HELD_SUCCESS_KIND, PICK_LAPSED_TEMPLATE, ended_businesses, fraud_warning, payment_returned,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb867_0001);
const HOMEOWNER: Uuid = Uuid::from_u128(0xc867_0001);
const SECRET: &str = "fake-id-secret-t867";
const ENDED: &str = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'fraud_refunded' AND ended_at IS NOT NULL";
const REFUNDED: &str = "SELECT count(*) FROM payments.refund WHERE pick_id = $1 AND trigger = 'fraud-warning' AND status = 'succeeded'";
const INTENT_ENDED: &str =
    "SELECT count(*) FROM payments.pick_intent WHERE pick_id = $1 AND ended_at IS NOT NULL";
const LAPSED: &str = "SELECT count(*) FROM core.task WHERE kind = 'jobs.homeowner_text' AND payload->>'template' = $2 AND payload->'values'->>'pick' = $1::text";
const HELD_TASKS: &str =
    "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'pick' = $1::text";

/// Refunds succeed; nothing else is called.
struct FakeStripe;

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
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("re_FakeT867".to_owned())
    }
}

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, 0, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

/// A seeded case: the pick, its job.
#[derive(Clone, Copy)]
struct Case {
    pick: Uuid,
    job: Uuid,
}

/// A job moved along its arrows to `status`, its pick stored as `pick_status` with a succeeded
/// $75 charge on an open PaymentIntent `intent`.
async fn seeded(pool: &PgPool, (status, pick_status): (JobStatus, &str), intent: &str) -> Case {
    let at = az(1, 9);
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, false, Actor::System)
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
    for pair in path.windows(2).take_while(|p| p[0] != status) {
        let moved = move_to(&mut db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id")
        .bind(job).bind(BUSINESS).bind(pick_status).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $5, $5)",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT867', $4, 1, 7500, $5, $5)",
    ] {
        sqlx::query(sql)
            .bind(pick)
            .bind(BUSINESS)
            .bind(job)
            .bind(intent)
            .bind(at.to_sqlx())
            .execute(&mut *db)
            .await
            .expect("seed");
    }
    Case { pick, job }
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn count_kind(pool: &PgPool, sql: &'static str, id: Uuid, kind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn job_status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("conn");
    current(&mut db, job).await.expect("job").status.code()
}

async fn pick_status(pool: &PgPool, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("pick status")
}

async fn warn(
    pool: &PgPool,
    case: Case,
    at: Timestamp,
) -> Result<bool, askhava_picks::api::PicksError> {
    let secret = SecretString::from(SECRET.to_owned());
    fraud_warning(
        pool,
        (&FakeStripe, &secret),
        (case.pick, case.job, false),
        at,
    )
    .await
}

/// Ends with the pick fraud-refunded once, its refund and its job back in Quotes ready, one
/// PICK-046 offer queued, and the warned business never offered again.
async fn assert_fraud_ended(pool: &PgPool, case: Case) {
    assert_eq!(count(pool, ENDED, case.pick).await, 1);
    assert_eq!(count(pool, REFUNDED, case.pick).await, 1);
    assert_eq!(job_status(pool, case.job).await, "quotes_ready");
    assert_eq!(count(pool, INTENT_ENDED, case.pick).await, 1);
    let lapsed = count_kind(pool, LAPSED, case.pick, PICK_LAPSED_TEMPLATE).await;
    assert_eq!(lapsed, 1, "one backup offer");
    let mut db = pool.acquire().await.expect("conn");
    let ended = ended_businesses(&mut db, case.job).await.expect("ended");
    assert_eq!(
        ended,
        vec![BUSINESS],
        "the warned business is never offered again"
    );
}

/// FEE-052, JOB-014 case (a): a charge succeeded while the pick waits in Payment pending; the
/// warning ends it fraud-refunded and returns the job with one offer (fails on `main`).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_waiting_pick_in_payment_pending_ends_fraud_refunded(pool: PgPool) {
    let pending = (JobStatus::PaymentPending, "awaiting_confirm");
    let case = seeded(&pool, pending, "pi_FakeT867a").await;
    assert_eq!(warn(&pool, case, az(3, 9)).await, Ok(true));
    assert_fraud_ended(&pool, case).await;
}

/// FEE-052, JOB-014 case (b): the settle made the pick `paid` and the job Connected, and nothing
/// was released; the warning ends the pick, its PaymentIntent and the job's connection (fails on
/// `main`). A repeated warning ends nothing twice and offers no second backup.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_paid_pick_on_a_connected_job_ends_fraud_refunded_once(pool: PgPool) {
    let case = seeded(&pool, (JobStatus::Connected, "paid"), "pi_FakeT867b").await;
    assert_eq!(warn(&pool, case, az(3, 9)).await, Ok(true));
    assert_fraud_ended(&pool, case).await;
    let ended = "SELECT count(*) FROM core.task WHERE kind = 'payments.pick_ended' AND payload->>'pick' = $1::text";
    assert_eq!(count(&pool, ended, case.pick).await, 1);
    assert_eq!(warn(&pool, case, az(3, 10)).await, Ok(false));
    assert_fraud_ended(&pool, case).await;
    assert_eq!(count(&pool, ended, case.pick).await, 1);
}

/// FEE-052: a released pick is listed for Casey (the fraud alert) and changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_released_pick_is_listed_and_changes_nothing(pool: PgPool) {
    let case = seeded(&pool, (JobStatus::Connected, "paid"), "pi_FakeT867c").await;
    sqlx::query("INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, false)")
        .bind(case.pick).bind(case.job).bind(BUSINESS).bind(az(2, 9).to_sqlx()).bind(HOMEOWNER)
        .execute(&pool).await.expect("release row");
    assert_eq!(warn(&pool, case, az(3, 9)).await, Ok(false));
    let alerts = "SELECT count(*) FROM ops.alert WHERE subject_id = $1 AND kind = $2";
    let listed = count_kind(&pool, alerts, case.pick, FRAUD_WARNING_ALERT).await;
    assert_eq!(listed, 1);
    let refunds = "SELECT count(*) FROM payments.refund WHERE pick_id = $1";
    assert_eq!(count(&pool, refunds, case.pick).await, 0);
    assert_eq!(pick_status(&pool, case.pick).await, "paid");
    assert_eq!(job_status(&pool, case.job).await, "connected");
    assert_eq!(count(&pool, INTENT_ENDED, case.pick).await, 0);
}

/// PICK-048 (deferred to T689): a waiting pick whose job is still in Awaiting confirm is refunded
/// but not ended, and its job does not move.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_waiting_pick_before_payment_pending_is_left(pool: PgPool) {
    let early = (JobStatus::AwaitingConfirm, "awaiting_confirm");
    let case = seeded(&pool, early, "pi_FakeT867d").await;
    assert_eq!(warn(&pool, case, az(3, 9)).await, Ok(false));
    assert_eq!(count(&pool, REFUNDED, case.pick).await, 1);
    assert_eq!(pick_status(&pool, case.pick).await, "awaiting_confirm");
    assert_eq!(job_status(&pool, case.job).await, "awaiting_confirm");
}

/// FEE-052, D-5888: a paid pick whose job already left Connected for a final status (Done) is
/// refunded but not ended, and its job does not move; a warning naming another job for the pick
/// ends nothing either.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_paid_pick_on_a_final_job_or_another_job_is_left(pool: PgPool) {
    let case = seeded(&pool, (JobStatus::Connected, "paid"), "pi_FakeT867g").await;
    let mut db = pool.acquire().await.expect("conn");
    let arrow = (JobStatus::Connected, JobStatus::Done);
    let moved = move_to(&mut db, case.job, arrow, az(2, 9), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    drop(db);
    assert_eq!(warn(&pool, case, az(3, 9)).await, Ok(false));
    assert_eq!(count(&pool, REFUNDED, case.pick).await, 1);
    assert_eq!(pick_status(&pool, case.pick).await, "paid");
    assert_eq!(job_status(&pool, case.job).await, "done");
    assert_eq!(count(&pool, INTENT_ENDED, case.pick).await, 0);
    let other = seeded(&pool, (JobStatus::Connected, "paid"), "pi_FakeT867h").await;
    let wrong = Case {
        pick: other.pick,
        job: case.job,
    };
    assert_eq!(warn(&pool, wrong, az(3, 9)).await, Ok(false));
    assert_eq!(count(&pool, REFUNDED, other.pick).await, 1);
    assert_eq!(pick_status(&pool, other.pick).await, "paid");
    assert_eq!(job_status(&pool, other.job).await, "connected");
    assert_eq!(count(&pool, INTENT_ENDED, other.pick).await, 0);
}

/// FEE-052, D-5888: a success whose refund succeeded while its PaymentIntent is open holds the
/// Payment pending wait (`Held`, never `Success`), so no release task is queued and the pick is not
/// ended as `payment_failed`; the same success unrefunded is `Success` and queues one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refunded_success_is_held_without_a_release_task(pool: PgPool) {
    let pending = (JobStatus::PaymentPending, "awaiting_confirm");
    let plain = seeded(&pool, pending, "pi_FakeT867e").await;
    let refunded = seeded(&pool, pending, "pi_FakeT867f").await;
    sqlx::query("INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) VALUES ($1, 'fraud-warning', 'succeeded', 1, $2)")
        .bind(refunded.pick).bind(az(3, 9).to_sqlx()).execute(&pool).await.expect("refund");
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(
        payment_held(&mut db, plain.pick).await,
        Ok(PaymentHold::Success)
    );
    assert_eq!(
        payment_held(&mut db, refunded.pick).await,
        Ok(PaymentHold::Held)
    );
    for case in [plain, refunded] {
        let mut tx = pool.begin().await.expect("tx");
        payment_returned(&mut tx, case.job, az(3, 10))
            .await
            .expect("returned");
        tx.commit().await.expect("commit");
        assert_eq!(pick_status(&pool, case.pick).await, "awaiting_confirm");
        assert_eq!(job_status(&pool, case.job).await, "payment_pending");
    }
    let queued = |case: Case| count_kind(&pool, HELD_TASKS, case.pick, HELD_SUCCESS_KIND);
    assert_eq!(queued(plain).await, 1);
    assert_eq!(queued(refunded).await, 0);
}

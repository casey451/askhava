//! T355: TD-PICK-59 (PICK-022, PRIV-005, FEE-052; D-2627, D-3118): the release claim and the
//! early-fraud-warning end both lock the pick's row first, on two connections, with no sleeps:
//! (a) a claim holding its lock and its release row makes the fraud path wait, which then finds
//! the row and writes no hold and asks for no refund; (b) a fraud path that committed its hold
//! first makes the claim refuse (no release row, no vault read), and the refund goes out once;
//! (c) the 72-hour no-show still ends a released, paid pick, and the plain claim and the plain
//! fraud end behave as before. The claim is driven through `picks::api::claim_still_open`, the
//! lock and re-check `release`'s claim takes first (a test crate cannot mint a `ReleaseProof`,
//! SEC-030, `tests/integration/tests/architecture.rs`). Stripe is a counting fake; every ID and
//! number is fake.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::Guarded;
use askhava_integration::set_app_role;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_payments::api::{Contact, EmailCheck};
use askhava_picks::api::rules::PickStatus;
use askhava_picks::api::{ReleasedPick, claim_still_open, fraud_warning, no_call_at_72_hours};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb355_0001);
const HOMEOWNER: Uuid = Uuid::from_u128(0xc355_0001);
const SECRET: &str = "fake-id-secret-t355";
const HOLDS: &str = "SELECT count(*) FROM picks.fraud_hold WHERE pick_id = $1";
const RELEASES: &str = "SELECT count(*) FROM picks.release WHERE pick_id = $1";
const REFUNDED: &str = "SELECT count(*) FROM payments.refund WHERE pick_id = $1 AND trigger = 'fraud-warning' AND status = 'succeeded'";
const VAULT_READS: &str = "SELECT count(*) FROM ops.audit_log WHERE action = 'vault_read.picks.release' AND $1::uuid IS NOT NULL";

/// Refunds succeed when `up`, else the request is never sent; counts every refund asked for.
struct FakeStripe {
    up: bool,
    refunds: AtomicUsize,
}

impl FakeStripe {
    fn new(up: bool) -> Self {
        Self {
            up,
            refunds: AtomicUsize::new(0),
        }
    }

    fn refunds(&self) -> usize {
        self.refunds.load(Ordering::SeqCst)
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
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        self.refunds.fetch_add(1, Ordering::SeqCst);
        if self.up {
            Ok("re_FakeT355".to_owned())
        } else {
            Err(AdapterError::NotSent)
        }
    }
}

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, 0, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

/// A job moved up to `status`, its paid pick, charge record and open line (as T191's fixture).
async fn seeded(pool: &PgPool, status: JobStatus, intent: &str) -> ReleasedPick<'static> {
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
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(BUSINESS).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $5, $5)",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT355', $4, 1, 7500, $5, $5)",
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($3, $2, $2, '+19285550198', false, $5)",
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
    let job_number = askhava_jobs::api::job_number(&mut db, job)
        .await
        .expect("number");
    ReleasedPick {
        pick,
        job,
        job_number,
        business: BUSINESS,
        business_name: "Fake Plumbing",
        homeowner: HOMEOWNER,
        language: (Language::En, false),
    }
}

/// The claim's release row, as `release`'s claim writes it, on `conn` (inside its transaction).
async fn release_row(conn: &mut sqlx::PgConnection, case: &ReleasedPick<'_>) {
    sqlx::query("INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, false)")
        .bind(case.pick).bind(case.job).bind(case.business).bind(az(2, 9).to_sqlx()).bind(case.homeowner)
        .execute(conn).await.expect("release row");
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn pick_status(pool: &PgPool, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("status")
}

/// Polls (no sleep) until another session of this database waits on a row lock, or `racer` ended
/// without waiting.
async fn wait_for_lock_waiter<T>(pool: &PgPool, racer: &tokio::task::JoinHandle<T>) {
    let sql = "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'";
    loop {
        let waiting: i64 = sqlx::query_scalar(sql)
            .fetch_one(pool)
            .await
            .expect("activity");
        if waiting > 0 || racer.is_finished() {
            return;
        }
        tokio::task::yield_now().await;
    }
}

/// TD-PICK-59 (a) (PICK-022, FEE-052, D-2627): the claim locks the pick and holds its release row
/// open; the fraud path waits on the lock, then finds the row: no hold, no refund asked for.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_59_a_fraud_warning_waits_for_a_claim_and_refunds_nothing(pool: PgPool) {
    let case = seeded(&pool, JobStatus::PaymentPending, "pi_FakeT355a").await;
    let mut claim = pool.begin().await.expect("claim tx");
    let open = claim_still_open(&mut claim, case.pick, PickStatus::Paid).await;
    assert_eq!(open, Ok(true));
    release_row(&mut claim, &case).await;
    let stripe = Arc::new(FakeStripe::new(true));
    let (racer, fake, ids) = (
        pool.clone(),
        Arc::clone(&stripe),
        (case.pick, case.job, false),
    );
    let fraud = tokio::spawn(async move {
        let secret = SecretString::from(SECRET.to_owned());
        fraud_warning(&racer, (&*fake, &secret), ids, az(3, 9)).await
    });
    wait_for_lock_waiter(&pool, &fraud).await;
    assert!(
        !fraud.is_finished(),
        "the fraud path waits on the claim's lock"
    );
    claim.commit().await.expect("claim commits");
    assert_eq!(fraud.await.expect("fraud task"), Ok(false));
    assert_eq!(stripe.refunds(), 0, "the refund client is never called");
    assert_eq!(count(&pool, HOLDS, case.pick).await, 0);
    assert_eq!(count(&pool, RELEASES, case.pick).await, 1);
    assert_eq!(pick_status(&pool, case.pick).await, "paid");
}

/// TD-PICK-59 (b) (PRIV-005, FEE-052, D-2627): the fraud path commits its hold first (its refund
/// failing here, so the pick stays paid); the claim then refuses with nothing written or read;
/// the next warning refunds once and ends the pick; the hold is append-only for the app role.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_59_b_a_committed_hold_refuses_the_claim_and_refunds_once(pool: PgPool) {
    let secret = SecretString::from(SECRET.to_owned());
    let (down, up) = (FakeStripe::new(false), FakeStripe::new(true));
    let case = seeded(&pool, JobStatus::PaymentPending, "pi_FakeT355b").await;
    let ids = (case.pick, case.job, false);
    assert_eq!(
        fraud_warning(&pool, (&down, &secret), ids, az(3, 9)).await,
        Ok(false)
    );
    assert_eq!(count(&pool, HOLDS, case.pick).await, 1);
    assert_eq!(pick_status(&pool, case.pick).await, "paid");
    let mut claim = pool.begin().await.expect("claim tx");
    let open = claim_still_open(&mut claim, case.pick, PickStatus::Paid).await;
    assert_eq!(open, Ok(false), "the claim answers MovedOn");
    claim.commit().await.expect("claim commits");
    assert_eq!(count(&pool, RELEASES, case.pick).await, 0);
    assert_eq!(count(&pool, VAULT_READS, case.pick).await, 0);
    assert_eq!(
        fraud_warning(&pool, (&up, &secret), ids, az(3, 10)).await,
        Ok(true)
    );
    assert_eq!(up.refunds(), 1, "the refund is sent once");
    assert_eq!(count(&pool, REFUNDED, case.pick).await, 1);
    assert_eq!(
        count(&pool, HOLDS, case.pick).await,
        1,
        "a second warning adds none"
    );
    assert_eq!(pick_status(&pool, case.pick).await, "fraud_refunded");
    let mut app = pool.acquire().await.expect("conn");
    set_app_role(&mut app).await.expect("SET ROLE askhava_app");
    for sql in [
        "UPDATE picks.fraud_hold SET held_at = now() WHERE pick_id = $1",
        "DELETE FROM picks.fraud_hold WHERE pick_id = $1",
    ] {
        let denied = sqlx::query(sql).bind(case.pick).execute(&mut *app).await;
        assert!(denied.is_err(), "append-only: {sql}");
    }
}

/// TD-PICK-59 (c) (PICK-022, FEE-052, D-3118): the 72-hour no-show still ends a released, paid
/// pick (and the claim then refuses it); a plain claim is open; a released pick's warning, or a
/// caller's `true` with no release row, holds and refunds nothing; a plain fraud end refunds once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_59_c_the_no_show_and_the_plain_paths_behave_as_before(pool: PgPool) {
    let secret = SecretString::from(SECRET.to_owned());
    let up = FakeStripe::new(true);
    let released = seeded(&pool, JobStatus::Connected, "pi_FakeT355c").await;
    let mut db = pool.acquire().await.expect("conn");
    release_row(&mut db, &released).await;
    let ids = (released.pick, released.job, false);
    assert_eq!(
        fraud_warning(&pool, (&up, &secret), ids, az(3, 9)).await,
        Ok(false)
    );
    assert_eq!(count(&pool, HOLDS, released.pick).await, 0);
    let call = (Contact::Call, EmailCheck::NoReply, az(2, 9), &[][..]);
    let ended = no_call_at_72_hours(&mut db, &released, call, az(5, 9)).await;
    assert_eq!(ended, Ok(true));
    assert_eq!(pick_status(&pool, released.pick).await, "no_show");
    let open = claim_still_open(&mut db, released.pick, PickStatus::Paid).await;
    assert_eq!(open, Ok(false));
    let plain = seeded(&pool, JobStatus::PaymentPending, "pi_FakeT355d").await;
    let open = claim_still_open(&mut db, plain.pick, PickStatus::Paid).await;
    assert_eq!(open, Ok(true));
    let said = (plain.pick, plain.job, true);
    let kept = fraud_warning(&pool, (&up, &secret), said, az(3, 9)).await;
    assert_eq!(kept, Ok(false));
    assert_eq!(count(&pool, HOLDS, plain.pick).await, 0);
    assert_eq!(up.refunds(), 0);
    let ids = (plain.pick, plain.job, false);
    assert_eq!(
        fraud_warning(&pool, (&up, &secret), ids, az(3, 10)).await,
        Ok(true)
    );
    assert_eq!(up.refunds(), 1);
    assert_eq!(count(&pool, REFUNDED, plain.pick).await, 1);
    assert_eq!(count(&pool, HOLDS, plain.pick).await, 1);
    assert_eq!(pick_status(&pool, plain.pick).await, "fraud_refunded");
}

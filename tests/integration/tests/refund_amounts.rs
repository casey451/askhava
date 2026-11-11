//! T333: TD-FEE-59 (FEE-026, FEE-028, FEE-053; D-2303, D-3018), against PostgreSQL: a refund
//! Stripe reports `succeeded` is booked at what Stripe reports. A full refund records the charged
//! $75; $25 of a $75 charge records $25 with one High `payments.refund_amount_mismatch` alert and
//! no "refunded in full" notice; an amount missing then present on the re-read records the re-read
//! amount; an amount missing on both records the charge with one High
//! `payments.refund_amount_unconfirmed` alert (its notice goes); an `eur` refund records the charge,
//! never the foreign figure, with the unconfirmed alert and no notice; the same event twice records
//! once. The webhook's `refund_succeeded`, `refund_in_full` and `picks::api::decide_at_96_hours`
//! are shown on the same cases. Stripe is a scripted fake; every ID, name and number is fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_calls::api::{AnsweredBy, Caller as CallCaller, LoggedCall};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_payments::api::{
    CallRecord, Contact, EmailCheck, MoneyEventError, REFUND_MISMATCH_ALERT,
    REFUND_UNCONFIRMED_ALERT, Refund, RefundAmount, RefundDecision, RefundOutcome, RefundReason,
    RefundTrigger, StripeRefund, UnreachableFacts, refund_failed, refund_in_full, refund_succeeded,
    request_refund,
};
use askhava_picks::api::{AtNinetySix, PicksError, ReleasedPick, call_record, decide_at_96_hours};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: Uuid = Uuid::from_u128(0xb333_0001);
const HOMEOWNER: Uuid = Uuid::from_u128(0xc333_0001);
const EMAILED: &str =
    "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%.unreachable_refunded.' || $1::text";
const OPEN_LINE: &str =
    "SELECT count(*) FROM calls.number_pool WHERE job_id = $1 AND closed_at IS NULL";
const FULL: (Option<i64>, Option<&str>) = (Some(7500), Some("usd"));

/// Stripe answers each refund request from `sent` (none left: a refusal) and each re-read from
/// `reads` (none left: `NotSent`, as the provided `read_refund`).
#[derive(Default)]
struct FakeStripe {
    sent: Mutex<VecDeque<StripeRefund>>,
    reads: Mutex<VecDeque<Result<StripeRefund, AdapterError>>>,
}

impl FakeStripe {
    fn new(sent: &[StripeRefund], reads: Vec<Result<StripeRefund, AdapterError>>) -> Self {
        Self {
            sent: Mutex::new(sent.iter().cloned().collect()),
            reads: Mutex::new(reads.into()),
        }
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
        panic!("the typed refund is used")
    }
    async fn send_refund(&self, _: &IdempotencyKey, _: &str) -> Result<StripeRefund, AdapterError> {
        let next = self.sent.lock().expect("lock").pop_front();
        next.ok_or(AdapterError::Vendor {
            status: 400,
            code: None,
        })
    }
    async fn read_refund(&self, _: &str) -> Result<StripeRefund, AdapterError> {
        let next = self.reads.lock().expect("lock").pop_front();
        next.unwrap_or(Err(AdapterError::NotSent))
    }
}

/// A charged pick's refund `re_…` for `intent`, in `status`, reporting `(amount, currency)`.
fn refund(
    (id, intent): (&str, &str),
    status: &str,
    (amount, currency): (Option<i64>, Option<&str>),
) -> StripeRefund {
    StripeRefund {
        id: id.to_owned(),
        status: status.to_owned(),
        amount,
        currency: currency.map(str::to_owned),
        payment_intent: Some(intent.to_owned()),
    }
}

fn ok(ids: (&str, &str), reported: (Option<i64>, Option<&str>)) -> StripeRefund {
    refund(ids, "succeeded", reported)
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t333".to_owned())
}

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, 0, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn unanswered() -> [CallRecord; 2] {
    [az(2, 10), az(2, 14)].map(|at| {
        call_record(&LoggedCall {
            caller: CallCaller::Business,
            answered: AnsweredBy::Nobody,
            length: SignedDuration::from_secs(20),
            reached_45s: false,
            started_at: Some(at),
            ended_at: at,
        })
    })
}

fn by_call(calls: &[CallRecord]) -> UnreachableFacts<'_> {
    UnreachableFacts {
        released_at: az(2, 9),
        emergency: false,
        calls,
        notice_attempted: true,
        done_or_problem: false,
        contact: Contact::Call,
        email_check: EmailCheck::NoReply,
        charged: true,
        refunded_before: false,
    }
}

const SEED: [&str; 4] = [
    "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
    "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $5, $5)",
    "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT333', $4, 1, 7500, $5, $5)",
    "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($3, $2, $2, '+19285550133', false, $5)",
];

async fn connected_job(db: &mut PgConnection, at: Timestamp) -> Uuid {
    let job = askhava_jobs::api::create(db, at, false, Actor::System)
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
    for pair in path.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
    job
}

/// A Connected job, its paid $75 pick charged through `intent` and its open masked line.
async fn seeded(pool: &PgPool, intent: &str) -> ReleasedPick<'static> {
    let at = az(1, 9);
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, at).await;
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(BUSINESS).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in SEED {
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

/// The amounts of the ledger's `kind` entries for `job`, in order.
async fn booked(pool: &PgPool, kind: &str, job: Uuid) -> Vec<i64> {
    sqlx::query_scalar("SELECT amount_cents FROM payments.ledger_entry WHERE kind = $1 AND job_id = $2 ORDER BY seq")
        .bind(kind).bind(job).fetch_all(pool).await.expect("ledger")
}

/// How many High alerts of `kind` name `pick`.
async fn alerts(pool: &PgPool, kind: &str, pick: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high' AND subject_id = $2",
    )
    .bind(kind)
    .bind(pick)
    .fetch_one(pool)
    .await
    .expect("alerts")
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn job_status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("conn");
    current(&mut db, job).await.expect("job").status.code()
}

async fn requested(pool: &PgPool, stripe: &FakeStripe, pick: Uuid) -> RefundOutcome {
    let trigger = (pick, RefundTrigger::Rule(RefundReason::Unreachable));
    request_refund(pool, stripe, &secret(), trigger, az(9, 9))
        .await
        .expect("refund")
}

/// The 96-hour run on `case`, refunding through `stripe`.
async fn at_96(pool: &PgPool, stripe: &FakeStripe, case: &ReleasedPick<'_>) -> AtNinetySix {
    let calls = unanswered();
    let facts = by_call(&calls);
    decide_at_96_hours(pool, (stripe, &secret()), case, &facts, az(9, 9))
        .await
        .expect("decide")
}

const REFUNDED: AtNinetySix =
    AtNinetySix::Decided(RefundDecision::Refund(RefundReason::Unreachable));

/// TD-FEE-59 (D-2303): each reported amount and currency against a $75 charge.
#[test]
fn td_fee_59_amount_against_the_charge() {
    let ids = ("re_FakeT333p", "pi_FakeT333p");
    let read = |reported| Refund::checked(ok(ids, reported), None).expect("refund");
    let cases = [
        (FULL, RefundAmount::Full),
        ((Some(2500), Some("usd")), RefundAmount::Differs(2500)),
        ((Some(7500), Some("eur")), RefundAmount::Foreign),
        ((None, Some("EUR")), RefundAmount::Foreign),
        ((None, Some("usd")), RefundAmount::Missing),
        ((Some(2500), None), RefundAmount::Missing),
        ((Some(0), Some("usd")), RefundAmount::Missing),
    ];
    for (reported, expected) in cases {
        assert_eq!(
            read(reported).amount_against(7500),
            expected,
            "{reported:?}"
        );
    }
}

/// TD-FEE-59: a full refund records the charged amount, raises no alert and is refunded in full,
/// whether the re-read reports the full amount or cannot be made (`NotSent`).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_a_full_refund_records_the_charge(pool: PgPool) {
    let ids = ("re_FakeT333a", "pi_FakeT333a");
    let case = seeded(&pool, ids.1).await;
    let stripe = FakeStripe::new(&[ok(ids, FULL)], vec![Ok(ok(ids, FULL))]);
    let answer = requested(&pool, &stripe, case.pick).await;
    assert_eq!(answer, RefundOutcome::Refunded(ids.0.to_owned()));
    assert_eq!(booked(&pool, "refund", case.job).await, [7500]);
    for kind in [REFUND_MISMATCH_ALERT, REFUND_UNCONFIRMED_ALERT] {
        assert_eq!(alerts(&pool, kind, case.pick).await, 0);
    }
    for _ in 0..2 {
        assert_eq!(refund_in_full(&pool, &stripe, case.pick).await, Ok(true));
    }
}

/// TD-FEE-59: $25 refunded of a $75 charge records $25 with one High mismatch alert; the 96-hour
/// run closes the job, the line and the homeowner's text as before but queues no "refunded in
/// full" email, and a second run refunds and alerts nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_25_of_75_records_25_and_sends_no_notice(pool: PgPool) {
    let ids = ("re_FakeT333b", "pi_FakeT333b");
    let case = seeded(&pool, ids.1).await;
    let stripe = FakeStripe::new(&[ok(ids, (Some(2500), Some("usd")))], vec![]);
    for _ in 0..2 {
        assert_eq!(at_96(&pool, &stripe, &case).await, REFUNDED);
    }
    assert_eq!(booked(&pool, "refund", case.job).await, [2500]);
    assert_eq!(alerts(&pool, REFUND_MISMATCH_ALERT, case.pick).await, 1);
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT, case.pick).await, 0);
    assert_eq!(job_status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
    assert_eq!(count(&pool, EMAILED, case.pick).await, 0);
    assert_eq!(refund_in_full(&pool, &stripe, case.pick).await, Ok(false));
}

/// TD-FEE-59: an amount missing on Stripe's answer and present on the one re-read records the
/// re-read amount with no alert; an amount with no currency (re-read failing) is never booked at
/// the reported figure: the charge, with the unconfirmed alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_missing_then_present_records_the_reread(pool: PgPool) {
    let ids = ("re_FakeT333c", "pi_FakeT333c");
    let case = seeded(&pool, ids.1).await;
    let missing = ok(ids, (None, None));
    let stripe = FakeStripe::new(&[missing], vec![Ok(ok(ids, FULL))]);
    let answer = requested(&pool, &stripe, case.pick).await;
    assert_eq!(answer, RefundOutcome::Refunded(ids.0.to_owned()));
    assert_eq!(booked(&pool, "refund", case.job).await, [7500]);
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT, case.pick).await, 0);
    let ids = ("re_FakeT333d", "pi_FakeT333d");
    let other = seeded(&pool, ids.1).await;
    let stripe = FakeStripe::new(&[ok(ids, (Some(2500), None))], vec![]);
    requested(&pool, &stripe, other.pick).await;
    assert_eq!(booked(&pool, "refund", other.job).await, [7500]);
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT, other.pick).await, 1);
    assert_eq!(alerts(&pool, REFUND_MISMATCH_ALERT, other.pick).await, 0);
}

/// TD-FEE-59: an amount missing on the answer and on the re-read records the charge with one High
/// unconfirmed alert, and its "refunded" notice still goes (D-2303).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_missing_on_both_records_the_charge(pool: PgPool) {
    let ids = ("re_FakeT333e", "pi_FakeT333e");
    let case = seeded(&pool, ids.1).await;
    let missing = ok(ids, (None, Some("usd")));
    let reads = vec![Ok(missing.clone()), Ok(missing.clone())];
    let stripe = FakeStripe::new(&[missing], reads);
    assert_eq!(at_96(&pool, &stripe, &case).await, REFUNDED);
    assert_eq!(booked(&pool, "refund", case.job).await, [7500]);
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT, case.pick).await, 1);
    assert_eq!(job_status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, EMAILED, case.pick).await, 1);
}

/// TD-FEE-59: an `eur` refund records the charged $75, never the foreign figure, with one High
/// unconfirmed alert; the 96-hour run closes the job and queues no "refunded in full" email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_eur_records_the_charge_and_sends_no_notice(pool: PgPool) {
    let ids = ("re_FakeT333f", "pi_FakeT333f");
    let case = seeded(&pool, ids.1).await;
    let eur = ok(ids, (Some(6900), Some("eur")));
    let stripe = FakeStripe::new(
        std::slice::from_ref(&eur),
        vec![Ok(eur.clone()), Ok(eur.clone())],
    );
    assert_eq!(at_96(&pool, &stripe, &case).await, REFUNDED);
    assert_eq!(booked(&pool, "refund", case.job).await, [7500]);
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT, case.pick).await, 1);
    assert_eq!(job_status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
    assert_eq!(count(&pool, EMAILED, case.pick).await, 0);
    assert_eq!(refund_in_full(&pool, &stripe, case.pick).await, Ok(false));
}

/// TD-FEE-59, D-3018: the webhook's `succeeded` for $25 of $75, delivered twice, books $25 once
/// with one mismatch alert, as the request path does; a later `refund.failed` reverses the $25.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_the_same_event_twice_records_once(pool: PgPool) {
    let ids = ("re_FakeT333g", "pi_FakeT333g");
    let case = seeded(&pool, ids.1).await;
    let stripe = FakeStripe::new(&[refund(ids, "pending", FULL)], vec![]);
    assert_eq!(
        requested(&pool, &stripe, case.pick).await,
        RefundOutcome::Failed
    );
    let read = ok(ids, (Some(2500), Some("usd")));
    let read = Refund::checked(read, None).expect("refund");
    let other = Refund {
        payment_intent: "pi_FakeT333other".to_owned(),
        ..read.clone()
    };
    assert_eq!(refund_succeeded(&pool, &other, az(9, 10)).await, Ok(false));
    assert_eq!(refund_succeeded(&pool, &read, az(9, 10)).await, Ok(true));
    assert_eq!(refund_succeeded(&pool, &read, az(9, 11)).await, Ok(false));
    assert_eq!(
        requested(&pool, &stripe, case.pick).await,
        RefundOutcome::Unchanged
    );
    assert_eq!(booked(&pool, "refund", case.job).await, [2500]);
    assert_eq!(alerts(&pool, REFUND_MISMATCH_ALERT, case.pick).await, 1);
    assert_eq!(refund_failed(&pool, ids.0, az(9, 12)).await, Ok(true));
    assert_eq!(booked(&pool, "refund-reversal", case.job).await, [2500]);
}

/// D-3018: `refund_in_full` answers `true` with no succeeded refund, and a read error other than
/// `NotSent` is `Stripe`, never `true`; the 96-hour run then closes nothing and the next run, with
/// Stripe readable, finishes it with its notice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_an_unreadable_refund_is_never_announced(pool: PgPool) {
    let ids = ("re_FakeT333h", "pi_FakeT333h");
    let case = seeded(&pool, ids.1).await;
    let none = FakeStripe::default();
    assert_eq!(refund_in_full(&pool, &none, case.pick).await, Ok(true));
    let down = vec![Err(AdapterError::OutcomeUnknown)];
    let stripe = FakeStripe::new(&[ok(ids, FULL)], down);
    let calls = unanswered();
    let facts = by_call(&calls);
    let run = decide_at_96_hours(&pool, (&stripe, &secret()), &case, &facts, az(9, 9)).await;
    assert_eq!(run, Err(PicksError::Database));
    assert_eq!(job_status(&pool, case.job).await, "connected");
    assert_eq!(count(&pool, EMAILED, case.pick).await, 0);
    let stripe = FakeStripe::new(&[], vec![Err(AdapterError::OutcomeUnknown)]);
    let read = refund_in_full(&pool, &stripe, case.pick).await;
    assert_eq!(read, Err(MoneyEventError::Stripe));
    assert_eq!(at_96(&pool, &FakeStripe::default(), &case).await, REFUNDED);
    assert_eq!(job_status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, EMAILED, case.pick).await, 1);
}

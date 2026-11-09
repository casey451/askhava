//! T270: TD-FEE-47 (FEE-022, FEE-026, FEE-035, FEE-052, JOB-013, JOB-027; D-1819, D-1836, D-2145
//! to D-2147), against PostgreSQL: the 96-hour close and the fraud-warning end wait for a refund
//! `payments` reads as `Succeeded` (a try held open at a fake Stripe closes and emails nothing),
//! `refund_state` answers each state, and the 72-hour no-show ends a paid or free pick in the
//! transaction of the job's return, rolling all of it back when the pick no longer matches.
//! Stripe is a fake; every ID, name and number is fake.

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_calls::api::{AnsweredBy, Caller as CallCaller, LoggedCall};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_payments::api::{
    CallRecord, Contact, EmailCheck, MoneyEventError, PickRefundState, RefundDecision,
    RefundReason, RefundStatus, RefundTrigger, UnreachableFacts, refund_state, request_refund,
    run_refund_retry,
};
use askhava_picks::api::{
    AtNinetySix, PicksError, ReleasedPick, call_record, decide_at_96_hours, fraud_warning,
    no_call_at_72_hours,
};

use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::sync::Notify;

const BUSINESS: Uuid = Uuid::from_u128(0xb270_0001);
const HOMEOWNER: Uuid = Uuid::from_u128(0xc270_0001);
const SECRET: &str = "fake-id-secret-t270";
const EMAILED: &str =
    "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%.unreachable_refunded.' || $1::text";
const OPEN_LINE: &str =
    "SELECT count(*) FROM calls.number_pool WHERE job_id = $1 AND closed_at IS NULL";

/// A fake Stripe: a refund succeeds when `succeeds`; with `held`, each refund request first says
/// it `entered` and then waits for `release`, so a second run can come while it is open.
struct FakeStripe {
    succeeds: bool,
    held: bool,
    entered: Notify,
    release: Notify,
}

impl FakeStripe {
    fn new(succeeds: bool, held: bool) -> Self {
        Self {
            succeeds,
            held,
            entered: Notify::new(),
            release: Notify::new(),
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
        if self.held {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if self.succeeds {
            Ok("re_FakeT270".to_owned())
        } else {
            Err(AdapterError::NotSent)
        }
    }
}

fn secret() -> SecretString {
    SecretString::from(SECRET.to_owned())
}

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn unanswered() -> [CallRecord; 2] {
    [az(2, 10, 0), az(2, 14, 0)].map(|at| {
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
        released_at: az(2, 9, 0),
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

/// How a seeded pick came to be: its stored status, its PaymentIntent when charged, and the $0
/// ledger kind of a free introduction.
#[derive(Clone, Copy)]
struct Intro {
    status: &'static str,
    intent: Option<&'static str>,
    free_kind: Option<&'static str>,
}

const fn paid(intent: &'static str) -> Intro {
    Intro {
        status: "paid",
        intent: Some(intent),
        free_kind: None,
    }
}

const fn free(kind: &'static str) -> Intro {
    Intro {
        status: "free",
        intent: None,
        free_kind: Some(kind),
    }
}

const CHARGE_ROWS: [&str; 3] = [
    "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
    "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $5, $5)",
    "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT270', $4, 1, 7500, $5, $5)",
];

/// A job moved along its arrows to `status`.
async fn job_at(db: &mut PgConnection, status: JobStatus, at: Timestamp) -> Uuid {
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
    for pair in path.windows(2).take_while(|p| p[0] != status) {
        let moved = move_to(db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
    job
}

/// A job of `status`, its pick as `intro` says (with its charge record or $0 ledger entry) and
/// its open masked line.
async fn seeded(pool: &PgPool, status: JobStatus, intro: Intro) -> ReleasedPick<'static> {
    let at = az(1, 9, 0);
    let mut db = pool.acquire().await.expect("conn");
    let job = job_at(&mut db, status, at).await;
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id")
        .bind(job).bind(BUSINESS).bind(intro.status).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    let charge = intro.intent.map_or(&[][..], |_| &CHARGE_ROWS[..]);
    let line = "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($3, $2, $2, '+19285550199', false, $5)";
    for sql in charge.iter().copied().chain([line]) {
        sqlx::query(sql)
            .bind(pick)
            .bind(BUSINESS)
            .bind(job)
            .bind(intro.intent)
            .bind(at.to_sqlx())
            .execute(&mut *db)
            .await
            .expect("seed");
    }
    if let Some(kind) = intro.free_kind {
        sqlx::query("INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, idempotency_key) VALUES ($1, $2, $3, 0, 'fake.t270.' || $3::text)")
            .bind(kind).bind(BUSINESS).bind(job).execute(&mut *db).await.expect("ledger");
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

async fn pick_status(pool: &PgPool, pick: Uuid) -> (String, bool) {
    sqlx::query_as("SELECT status, ended_at IS NOT NULL FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("pick")
}

async fn state(pool: &PgPool, pick: Uuid) -> PickRefundState {
    let mut db = pool.acquire().await.expect("conn");
    refund_state(&mut db, pick).await.expect("state")
}

/// TD-FEE-47: a second 96-hour run while the first refund request is held open emails nothing,
/// leaves the job Connected with its line open and answers `RefundInFlight`; the held request
/// fails, the FEE-026 retry succeeds, and the next runs close the job and the line and send one
/// decision email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_an_in_flight_refund_closes_nothing(pool: PgPool) {
    let case = seeded(&pool, JobStatus::Connected, paid("pi_FakeT270a")).await;
    let calls = unanswered();
    let facts = by_call(&calls);
    let (held, up, at) = (
        FakeStripe::new(false, true),
        FakeStripe::new(true, false),
        az(9, 9, 0),
    );
    let key = secret();
    let first = decide_at_96_hours(&pool, (&held, &key), &case, &facts, at);
    let second = async {
        held.entered.notified().await;
        let open = decide_at_96_hours(&pool, (&up, &secret()), &case, &facts, at).await;
        assert_eq!(open, Ok(AtNinetySix::RefundInFlight));
        assert_eq!(state(&pool, case.pick).await, PickRefundState::InFlight);
        assert_eq!(job_status(&pool, case.job).await, "connected");
        assert_eq!(count(&pool, EMAILED, case.pick).await, 0);
        assert_eq!(count(&pool, OPEN_LINE, case.job).await, 1);
        held.release.notify_one();
    };
    let (first, ()) = tokio::join!(first, second);
    assert_eq!(first, Ok(AtNinetySix::RefundWaiting { blocked: false }));
    assert_eq!(state(&pool, case.pick).await, PickRefundState::Failed);
    assert_eq!(job_status(&pool, case.job).await, "connected");
    let retry = json!({"pick": case.pick.to_string(), "try": 1});
    let retried = run_refund_retry(&pool, &up, &secret(), &retry, az(9, 10, 0)).await;
    assert!(retried.is_ok());
    assert_eq!(state(&pool, case.pick).await, PickRefundState::Succeeded);
    let refunded = AtNinetySix::Decided(RefundDecision::Refund(RefundReason::Unreachable));
    for _ in 0..2 {
        let down = FakeStripe::new(false, false);
        let done = decide_at_96_hours(&pool, (&down, &secret()), &case, &facts, az(9, 11, 0)).await;
        assert_eq!(done, Ok(refunded));
    }
    assert_eq!(job_status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
    assert_eq!(count(&pool, EMAILED, case.pick).await, 1);
}

/// Sets `pick`'s refund row to `status` (`None`: no row) and opens a dispute on it when `disputed`.
async fn set_refund(pool: &PgPool, pick: Uuid, status: Option<&str>, disputed: bool) {
    if let Some(status) = status {
        sqlx::query("INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) VALUES ($1, 'unreachable', $2, 1, $3)")
            .bind(pick).bind(status).bind(az(9, 9, 0).to_sqlx()).execute(pool).await.expect("refund");
    }
    if disputed {
        sqlx::query("INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at, opened_at) VALUES ('dp_Fake' || replace($1::text, '-', ''), $1, 7500, $2, $2)")
            .bind(pick).bind(az(20, 9, 0).to_sqlx()).execute(pool).await.expect("dispute");
    }
}

/// TD-FEE-47: `refund_state` answers `NoRefund`, `InFlight` (`sending`, `pending`), `Succeeded`,
/// `Failed` (`failed`, `canceled`) and `Blocked` (a `blocked` row, or a dispute on a refund that
/// would still send) from the stored rows; a status this build cannot read is `Failed`, and a failed read is `Database`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_refund_state_answers_each_state(pool: PgPool) {
    use PickRefundState::{Blocked, Failed, InFlight, NoRefund, Succeeded};
    let cases = [
        (None, false, NoRefund),
        (Some("sending"), false, InFlight),
        (Some("pending"), false, InFlight),
        (Some("succeeded"), false, Succeeded),
        (Some("failed"), false, Failed),
        (Some("canceled"), false, Failed),
        (Some("blocked"), false, Blocked),
        (None, true, Blocked),
        (Some("sending"), true, Blocked),
        (Some("failed"), true, Blocked),
        (Some("canceled"), true, Blocked),
        (Some("pending"), true, InFlight),
        (Some("succeeded"), true, Succeeded),
    ];
    for (n, (status, disputed, expected)) in cases.into_iter().enumerate() {
        let intent: &'static str = Box::leak(format!("pi_FakeT270s{n}").into_boxed_str());
        let case = seeded(&pool, JobStatus::Connected, paid(intent)).await;
        set_refund(&pool, case.pick, status, disputed).await;
        assert_eq!(
            state(&pool, case.pick).await,
            expected,
            "{status:?} {disputed}"
        );
        let stored = status.and_then(RefundStatus::from_stored);
        let pure = RefundStatus::pick_state(stored, disputed);
        assert_eq!(pure, expected);
    }
    let odd = seeded(&pool, JobStatus::Connected, paid("pi_FakeT270odd")).await;
    sqlx::query("ALTER TABLE payments.refund DROP CONSTRAINT refund_status_check")
        .execute(&pool)
        .await
        .expect("drop check");
    set_refund(&pool, odd.pick, Some("mystery"), false).await;
    assert_eq!(state(&pool, odd.pick).await, Failed);
    let mut tx = pool.begin().await.expect("tx");
    let broken = sqlx::query("SELECT 1 / 0").execute(&mut *tx).await;
    assert!(broken.is_err());
    let aborted = refund_state(&mut tx, odd.pick).await;
    assert_eq!(aborted, Err(MoneyEventError::Database));
}

/// TD-FEE-47: an early fraud warning while the pick's refund is in flight ends nothing; once
/// Stripe answers `succeeded` the next warning run ends the paid pick and returns the job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_a_fraud_warning_during_an_in_flight_refund_ends_nothing(pool: PgPool) {
    let case = seeded(&pool, JobStatus::PaymentPending, paid("pi_FakeT270f")).await;
    let (held, up) = (FakeStripe::new(true, true), FakeStripe::new(true, false));
    let trigger = (case.pick, RefundTrigger::FraudWarning);
    let key = secret();
    let first = request_refund(&pool, &held, &key, trigger, az(5, 9, 0));
    let ids = (case.pick, case.job, false);
    let second = async {
        held.entered.notified().await;
        let warned = fraud_warning(&pool, (&up, &secret()), ids, az(5, 9, 0)).await;
        assert_eq!(warned, Ok(false));
        assert_eq!(job_status(&pool, case.job).await, "payment_pending");
        assert_eq!(
            pick_status(&pool, case.pick).await,
            ("paid".to_owned(), false)
        );
        held.release.notify_one();
    };
    let (first, ()) = tokio::join!(first, second);
    assert!(first.is_ok());
    let warned = fraud_warning(&pool, (&up, &secret()), ids, az(5, 10, 0)).await;
    assert_eq!(warned, Ok(true));
    assert_eq!(job_status(&pool, case.job).await, "quotes_ready");
    let ended = pick_status(&pool, case.pick).await;
    assert_eq!(ended, ("fraud_refunded".to_owned(), true));
}

/// TD-FEE-47: the fraud warning ends only a `paid` pick: a refunded charge whose pick is `free`
/// rolls the job's return back and answers `MovedOn`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_the_fraud_warning_ends_only_a_paid_pick(pool: PgPool) {
    let odd = Intro {
        status: "free",
        ..paid("pi_FakeT270g")
    };
    let case = seeded(&pool, JobStatus::PaymentPending, odd).await;
    let up = FakeStripe::new(true, false);
    let ids = (case.pick, case.job, false);
    let warned = fraud_warning(&pool, (&up, &secret()), ids, az(5, 9, 0)).await;
    assert_eq!(warned, Err(PicksError::MovedOn));
    assert_eq!(state(&pool, case.pick).await, PickRefundState::Succeeded);
    assert_eq!(job_status(&pool, case.job).await, "payment_pending");
    assert_eq!(
        pick_status(&pool, case.pick).await,
        ("free".to_owned(), false)
    );
}

/// TD-FEE-47: the 72-hour no-show, for call contact and for an email-contact day-3 NO, for a
/// paid, a first-win-free, a repeat-free and a fee-free-job-type introduction: the pick becomes
/// `no_show` with its ended time, the job returns to Quotes ready, the line closes, a second run
/// changes nothing and a re-pick works.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_the_no_show_ends_a_paid_or_free_pick(pool: PgPool) {
    let released = az(2, 9, 0);
    let contacts = [
        (Contact::Call, EmailCheck::NoReply, "pi_FakeT270c"),
        (Contact::Email, EmailCheck::No, "pi_FakeT270e"),
    ];
    for (contact, day3, intent) in contacts {
        for intro in [
            paid(intent),
            free("free-first-win"),
            free("repeat-customer"),
            free("fee-free-job-type"),
        ] {
            let case = seeded(&pool, JobStatus::Connected, intro).await;
            let mut db = pool.acquire().await.expect("conn");
            let facts = (contact, day3, released, &[][..]);
            let moved = no_call_at_72_hours(&mut db, &case, facts, az(5, 9, 0)).await;
            assert_eq!(moved, Ok(true));
            assert_eq!(job_status(&pool, case.job).await, "quotes_ready");
            assert_eq!(
                pick_status(&pool, case.pick).await,
                ("no_show".to_owned(), true)
            );
            assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
            let again = no_call_at_72_hours(&mut db, &case, facts, az(5, 10, 0)).await;
            assert_eq!(again, Ok(false));
            let arrow = (JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
            let repick = move_to(&mut db, case.job, arrow, az(5, 11, 0), Actor::System).await;
            assert_eq!(repick, Ok(Guarded::Applied(())));
            let waiting = sqlx::query("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 2, 'page', 'awaiting_confirm', $3)")
                .bind(case.job).bind(BUSINESS).bind(az(5, 11, 0).to_sqlx()).execute(&mut *db).await;
            assert!(waiting.is_ok());
        }
    }
}

/// TD-FEE-47: a pick that is no longer `paid` or `free` rolls the whole no-show back (the job
/// stays Connected, the line open) and answers `MovedOn`, never a silent success.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_47_a_pick_that_no_longer_matches_rolls_back(pool: PgPool) {
    let gone = Intro {
        status: "cancelled",
        intent: None,
        free_kind: None,
    };
    let case = seeded(&pool, JobStatus::Connected, gone).await;
    let mut db = pool.acquire().await.expect("conn");
    let facts = (Contact::Call, EmailCheck::NoReply, az(2, 9, 0), &[][..]);
    let moved = no_call_at_72_hours(&mut db, &case, facts, az(5, 9, 0)).await;
    assert_eq!(moved, Err(PicksError::MovedOn));
    assert_eq!(job_status(&pool, case.job).await, "connected");
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 1);
    assert_eq!(
        pick_status(&pool, case.pick).await,
        ("cancelled".to_owned(), false)
    );
}

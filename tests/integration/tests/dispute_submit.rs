//! TD-FEE-51 (T314, D-2204; FEE-027, FEE-050): a dispute's auto-submit mark and its submit task,
//! and an owner submission and its countering fee, commit together or not at all; a new dispute
//! queues its `picks.dispute_opened` checks in its own transaction; through the worker (a
//! `Registry` run with the clock passed in) a released dispute with a business call attempt or an
//! authenticated 3-D Secure payment is marked and its evidence sent 2 days before the due date,
//! every other one waits for Casey, a call logged after the dispute opened counts at the due date
//! less 2 days, and Casey's own submission records the fee once and stops the auto-submit.
//!
//! Failures are injected with a test-only trigger on the second write. Stripe is a fake; every
//! row is fake (a fake business, a fake vault contact with a 555 number, fake Stripe IDs).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, ThreeDs};
use askhava_core::api::{Lease, ManualClock, Registry, WorkerId, claim};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    DISPUTE_OPENED_KIND, DisputeOpened, DisputeStripe, MoneyEventError, SUBMIT_KIND, ThreeDsStripe,
    mark_auto_submit, record_dispute, record_owner_submission, run_dispute_submit,
};
use askhava_picks::api::{StepParts, run_dispute_opened};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const NOW: i64 = 1_793_800_000;
const DAY: i64 = 86_400;
const DUE_DAYS: i64 = 10;

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-t314-000".to_owned())
}

/// The 3-D Secure answer it was given, counting reads; evidence submissions are counted.
#[derive(Default)]
struct FakeStripe {
    three_ds: Option<ThreeDs>,
    reads: Mutex<u32>,
    submitted: Mutex<u32>,
}

impl ThreeDsStripe for FakeStripe {
    async fn three_ds(&self, _: &str) -> Result<ThreeDs, AdapterError> {
        *self.reads.lock().expect("lock") += 1;
        self.three_ds.ok_or(AdapterError::NotSent)
    }
}

impl DisputeStripe for FakeStripe {
    async fn submit_evidence(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: &[(&'static str, String)],
    ) -> Result<(), AdapterError> {
        *self.submitted.lock().expect("lock") += 1;
        Ok(())
    }
}

/// `three_ds` is what Stripe answers; `None` builds no Stripe client (dev and CI).
fn parts(three_ds: Option<ThreeDs>) -> Arc<StepParts<FakeStripe>> {
    Arc::new(StepParts {
        stripe: three_ds.map(|t| FakeStripe {
            three_ds: Some(t),
            ..FakeStripe::default()
        }),
        secret: secret(),
        vault: None,
        postal: None,
    })
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://fake@localhost/fake".to_owned());
    Vault::new(&key(0x31), &key(0x32), &url).expect("vault")
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A seeded charged pick: its dispute ID, its PaymentIntent and its masked line.
struct Case {
    dispute: String,
    intent: String,
    line: Uuid,
}

const SEED_PICK: &str = "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id";
const SEED_MONEY: [&str; 3] = [
    "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, 'pi_FakeT314' || replace($1::text, '-', ''), false, $4)",
    "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $4, $4)",
    "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT314', 'pi_FakeT314' || replace($1::text, '-', ''), 1, 7500, $4, $4)",
];
const SEED_LINE: &str = "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($1, $2, $3, '+19285550199', false, $4) RETURNING id";
const SEED_RELEASE: &str = "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, false)";

/// A fake business and a fake vault contact (a fresh 555 number), and the case's number.
async fn people(db: &mut sqlx::PgConnection, seeded: Timestamp) -> (Uuid, Uuid, usize) {
    let business = askhava_businesses::api::create(db, false, seeded)
        .await
        .expect("business");
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(format!("+1928555{:04}", 1700 + n))),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    (business, contact, n)
}

/// [`people`], a job, its paid pick and charge, its line and, when `released`, its release row.
async fn charged(pool: &PgPool, released: bool) -> Case {
    let seeded = at(-2 * DAY);
    let mut db = pool.acquire().await.expect("conn");
    let (business, contact, n) = people(&mut db, seeded).await;
    let job = askhava_jobs::api::create(&mut db, seeded, false, Actor::System)
        .await
        .expect("job");
    let pick: Uuid = sqlx::query_scalar(SEED_PICK)
        .bind(job)
        .bind(business)
        .bind(seeded.to_sqlx())
        .fetch_one(&mut *db)
        .await
        .expect("pick");
    for sql in SEED_MONEY {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(seeded.to_sqlx())
            .execute(&mut *db)
            .await
            .expect("money");
    }
    let line: Uuid = sqlx::query_scalar(SEED_LINE)
        .bind(job)
        .bind(contact)
        .bind(business)
        .bind(seeded.to_sqlx())
        .fetch_one(&mut *db)
        .await
        .expect("line");
    if released {
        sqlx::query(SEED_RELEASE)
            .bind(pick)
            .bind(job)
            .bind(business)
            .bind(seeded.to_sqlx())
            .bind(contact)
            .execute(&mut *db)
            .await
            .expect("release");
    }
    Case {
        dispute: format!("dp_FakeT314n{n}"),
        intent: format!("pi_FakeT314{}", pick.simple()),
        line,
    }
}

/// The case's `charge.dispute.created` (due in [`DUE_DAYS`]) recorded at `now`.
async fn open(pool: &PgPool, case: &Case, now: Timestamp) -> Result<bool, MoneyEventError> {
    let opened = DisputeOpened {
        dispute: &case.dispute,
        payment_intent: &case.intent,
        amount_cents: 7500,
        due_at: at(DUE_DAYS * DAY),
    };
    record_dispute(pool, &opened, now).await
}

/// [`charged`], then its dispute opened at `at(0)`.
async fn disputed(pool: &PgPool, released: bool) -> Case {
    let case = charged(pool, released).await;
    assert_eq!(open(pool, &case, at(0)).await, Ok(true));
    case
}

/// Logs one finished business call on the case's line at `at`.
async fn log_call(pool: &PgPool, case: &Case, at: Timestamp) {
    sqlx::query("INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, length_seconds, reached_45s, ended_at) VALUES ($1, 'CA' || md5($1::text || $2::text), 'business', 'person', 20, false, $2)")
        .bind(case.line).bind(at.to_sqlx()).execute(pool).await.expect("call");
}

/// Runs every task due at `now` through a worker registry holding the two dispute handlers, until
/// none is left due.
async fn worker(
    pool: &PgPool,
    parts: &Arc<StepParts<FakeStripe>>,
    sender: &Arc<FakeStripe>,
    now: Timestamp,
) {
    let mut registry = Registry::new();
    let judge = Arc::clone(parts);
    registry.register(DISPUTE_OPENED_KIND, move |pool, task| {
        run_dispute_opened(pool, Arc::clone(&judge), task, now)
    });
    let send = Arc::clone(sender);
    registry.register(SUBMIT_KIND, move |pool, task| {
        let send = Arc::clone(&send);
        async move { run_dispute_submit(&pool, send.as_ref(), &secret(), &task.payload, now).await }
    });
    let (clock, owner) = (
        ManualClock::new(now),
        WorkerId::new(pool).await.expect("worker"),
    );
    let lease = SignedDuration::from_secs(60);
    for _ in 0..5 {
        let due = claim(pool, owner, now, lease, 20).await.expect("claim");
        if due.is_empty() {
            return;
        }
        for task in due {
            registry
                .run(pool, &clock, owner, task, Lease::WORKER)
                .await
                .expect("run");
        }
    }
}

async fn scalar(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("scalar")
}

async fn marked(pool: &PgPool, case: &Case) -> bool {
    sqlx::query_scalar("SELECT auto_submit FROM payments.dispute WHERE stripe_dispute_id = $1")
        .bind(&case.dispute)
        .fetch_one(pool)
        .await
        .expect("dispute")
}

async fn fees(pool: &PgPool, case: &Case) -> i64 {
    let sql = "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'countering-fee' AND stripe_id = $1";
    scalar(pool, sql, &case.dispute).await
}

async fn tasks_like(pool: &PgPool, pattern: &str) -> i64 {
    scalar(
        pool,
        "SELECT count(*) FROM core.task WHERE dedupe_key LIKE $1",
        pattern,
    )
    .await
}

fn sent(stripe: &FakeStripe) -> u32 {
    *stripe.submitted.lock().expect("lock")
}

/// A test-only fault switch: an insert into `core.task` or `payments.ledger_entry` whose `kind`
/// is listed in `public.t314_fault` fails.
const FAULTS: &str = "CREATE TABLE public.t314_fault (kind text PRIMARY KEY);
CREATE FUNCTION public.t314_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
  IF EXISTS (SELECT 1 FROM public.t314_fault f WHERE f.kind = NEW.kind) THEN
    RAISE EXCEPTION 'injected';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER t314_fail BEFORE INSERT ON core.task FOR EACH ROW EXECUTE FUNCTION public.t314_fail();
CREATE TRIGGER t314_fail BEFORE INSERT ON payments.ledger_entry FOR EACH ROW EXECUTE FUNCTION public.t314_fail();";

/// Makes every insert of `kind` fail, until [`heal`].
async fn inject(pool: &PgPool, kind: &str) {
    sqlx::raw_sql(FAULTS).execute(pool).await.expect("faults");
    sqlx::query("INSERT INTO public.t314_fault (kind) VALUES ($1)")
        .bind(kind)
        .execute(pool)
        .await
        .expect("fault");
}

async fn heal(pool: &PgPool) {
    sqlx::query("DELETE FROM public.t314_fault")
        .execute(pool)
        .await
        .expect("heal");
}

/// TD-FEE-51: a failure after the mark and before its submit task, inside a caller's transaction,
/// leaves no mark and keeps the caller's transaction; the retry marks and queues once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_a_failed_submit_task_leaves_no_mark(pool: PgPool) {
    let case = disputed(&pool, true).await;
    inject(&pool, SUBMIT_KIND).await;
    let mut tx = pool.begin().await.expect("tx");
    assert!(
        mark_auto_submit(&mut tx, &case.dispute, at(DAY))
            .await
            .is_err()
    );
    let alive: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&mut *tx)
        .await
        .expect("caller");
    assert_eq!(alive, 1);
    tx.commit().await.expect("commit");
    assert!(!marked(&pool, &case).await);
    heal(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(
        mark_auto_submit(&mut db, &case.dispute, at(DAY)).await,
        Ok(true)
    );
    assert_eq!(
        mark_auto_submit(&mut db, &case.dispute, at(DAY)).await,
        Ok(false)
    );
    assert!(marked(&pool, &case).await);
    assert_eq!(
        tasks_like(&pool, "payments.dispute_submit.%.marked").await,
        1
    );
}

/// TD-FEE-51: a failure after `submitted_at` and before the countering fee leaves neither; the
/// retry records the fee once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_a_failed_fee_leaves_no_submission(pool: PgPool) {
    let case = disputed(&pool, true).await;
    inject(&pool, "countering-fee").await;
    let mut db = pool.acquire().await.expect("conn");
    assert!(
        record_owner_submission(&mut db, &case.dispute, at(DAY))
            .await
            .is_err()
    );
    let sql = "SELECT count(*) FROM payments.dispute WHERE stripe_dispute_id = $1 AND submitted_at IS NOT NULL";
    assert_eq!(scalar(&pool, sql, &case.dispute).await, 0);
    assert_eq!(fees(&pool, &case).await, 0);
    heal(&pool).await;
    assert_eq!(
        record_owner_submission(&mut db, &case.dispute, at(DAY)).await,
        Ok(true)
    );
    assert_eq!(
        record_owner_submission(&mut db, &case.dispute, at(DAY)).await,
        Ok(false)
    );
    assert_eq!(scalar(&pool, sql, &case.dispute).await, 1);
    assert_eq!(fees(&pool, &case).await, 1);
}

/// TD-FEE-51: a new dispute queues its two checks (now and at due less 2 days) in its own
/// transaction: a failed check insert records no dispute, and a repeated event queues none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_a_new_dispute_queues_its_checks_once(pool: PgPool) {
    let case = charged(&pool, true).await;
    inject(&pool, DISPUTE_OPENED_KIND).await;
    assert!(open(&pool, &case, at(0)).await.is_err());
    let sql = "SELECT count(*) FROM payments.dispute WHERE stripe_dispute_id = $1";
    assert_eq!(scalar(&pool, sql, &case.dispute).await, 0);
    heal(&pool).await;
    assert_eq!(open(&pool, &case, at(0)).await, Ok(true));
    assert_eq!(open(&pool, &case, at(DAY)).await, Ok(false));
    let runs: Vec<Timestamp> = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT run_at FROM core.task WHERE kind = $1 ORDER BY run_at",
    )
    .bind(DISPUTE_OPENED_KIND)
    .fetch_all(&pool)
    .await
    .expect("runs")
    .into_iter()
    .map(|t| t.to_jiff())
    .collect();
    assert_eq!(runs, [at(0), at((DUE_DAYS - 2) * DAY)]);
}

/// TD-FEE-51: through the worker, a released dispute with a business call attempt is marked at
/// once and its evidence sent, with one countering fee, 2 days before the due date.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_a_released_called_dispute_is_sent_two_days_before_due(pool: PgPool) {
    let (parts, sender) = (parts(None), Arc::new(FakeStripe::default()));
    let case = disputed(&pool, true).await;
    log_call(&pool, &case, at(-DAY)).await;
    worker(&pool, &parts, &sender, at(0)).await;
    assert!(marked(&pool, &case).await);
    worker(&pool, &parts, &sender, at((DUE_DAYS - 2) * DAY - 1)).await;
    assert_eq!(sent(&sender), 0);
    worker(&pool, &parts, &sender, at((DUE_DAYS - 2) * DAY)).await;
    assert_eq!((sent(&sender), fees(&pool, &case).await), (1, 1));
    worker(&pool, &parts, &sender, at((DUE_DAYS - 1) * DAY)).await;
    assert_eq!((sent(&sender), fees(&pool, &case).await), (1, 1));
}

/// TD-FEE-51, D-3118: a released dispute with no call attempt is marked only when its stored
/// 3-D Secure result is authenticated (read from Stripe once); not authenticated, or unknown with
/// no Stripe client, it waits for Casey.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_no_call_needs_an_authenticated_payment(pool: PgPool) {
    let sender = Arc::new(FakeStripe::default());
    let answers = [
        (Some(ThreeDs::Authenticated), true),
        (Some(ThreeDs::NotAuthenticated), false),
        (None, false),
    ];
    for (answer, expected) in answers {
        let parts = parts(answer);
        let case = disputed(&pool, true).await;
        worker(&pool, &parts, &sender, at(0)).await;
        worker(&pool, &parts, &sender, at(0)).await;
        assert_eq!(marked(&pool, &case).await, expected, "{answer:?}");
        let reads = parts
            .stripe
            .as_ref()
            .map(|s| *s.reads.lock().expect("lock"));
        assert_eq!(reads, answer.map(|_| 1));
    }
    worker(&pool, &parts(None), &sender, at((DUE_DAYS - 2) * DAY)).await;
    assert_eq!(sent(&sender), 1);
}

/// TD-FEE-51: an unreleased dispute is never marked, even with an authenticated payment.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_an_unreleased_dispute_waits_for_casey(pool: PgPool) {
    let (parts, sender) = (
        parts(Some(ThreeDs::Authenticated)),
        Arc::new(FakeStripe::default()),
    );
    let case = disputed(&pool, false).await;
    worker(&pool, &parts, &sender, at(0)).await;
    worker(&pool, &parts, &sender, at((DUE_DAYS - 2) * DAY)).await;
    assert!(!marked(&pool, &case).await);
    assert_eq!((sent(&sender), fees(&pool, &case).await), (0, 0));
}

/// TD-FEE-51: a business call first logged after the dispute opened is counted by the run at the
/// due date less 2 days, which marks the dispute and sends its evidence then.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_a_later_call_counts_at_due_less_two_days(pool: PgPool) {
    let (parts, sender) = (
        parts(Some(ThreeDs::NotAuthenticated)),
        Arc::new(FakeStripe::default()),
    );
    let case = disputed(&pool, true).await;
    worker(&pool, &parts, &sender, at(0)).await;
    assert!(!marked(&pool, &case).await);
    log_call(&pool, &case, at(DAY)).await;
    worker(&pool, &parts, &sender, at((DUE_DAYS - 2) * DAY)).await;
    assert!(marked(&pool, &case).await);
    assert_eq!((sent(&sender), fees(&pool, &case).await), (1, 1));
}

/// TD-FEE-51, D-229: Casey's own submission records the countering fee once, and the marked
/// dispute's submit task then sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_51_caseys_submission_stops_the_auto_submit(pool: PgPool) {
    let (parts, sender) = (parts(None), Arc::new(FakeStripe::default()));
    let case = disputed(&pool, true).await;
    log_call(&pool, &case, at(-DAY)).await;
    worker(&pool, &parts, &sender, at(0)).await;
    assert!(marked(&pool, &case).await);
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(
        record_owner_submission(&mut db, &case.dispute, at(DAY)).await,
        Ok(true)
    );
    assert_eq!(
        record_owner_submission(&mut db, &case.dispute, at(DAY)).await,
        Ok(false)
    );
    worker(&pool, &parts, &sender, at((DUE_DAYS - 2) * DAY)).await;
    assert_eq!((sent(&sender), fees(&pool, &case).await), (0, 1));
}

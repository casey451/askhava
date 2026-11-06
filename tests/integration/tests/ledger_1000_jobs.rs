//! TD-FEE-16 (the Stage 3 run: 1,000 simulated jobs; FEE-028, FEE-029, FEE-030, FEE-032;
//! `sources/02_build_plan/Build_Plan.md:234`, D-1410): on a fresh database, 1,000 paid jobs over 20
//! businesses get a seeded random mix of $25 / $75 / $199 introduction fees by job size, a charge
//! recorded at Confirm or left unknown, then a refund, a dispute (and a refund it blocks) or a
//! closed job. Every money step goes through `payments::api`; two nightly reconciles later the
//! ledger matches Stripe to the cent, every unknown charge is auto-matched or (on a closed job)
//! auto-refunded once, and the money view and statements are folds of the ledger rows alone.
//!
//! Stripe is a stateful fake in stripe-mock's shapes that replays a repeated idempotency key;
//! the stripe-mock round trip is T132's (D-1410), the append-only case `ledger_append_only.rs`
//! and the only-`payments`-imports-the-adapter case `architecture.rs`. Fake IDs only.

use std::collections::HashMap;
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    CONFIRM_FEE_ACTION, DisputeOpened, EntryKind, LedgerEntry, MoneyTotals, MoneyView, NewEntry,
    ReconcileStripe, RefundOutcome, RefundReason, RefundTrigger, StripeCharge, business_statement,
    ledger_append, money_view, reconcile, record_dispute, request_refund,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESSES: u32 = 20;
const NOW: i64 = 1_794_000_000;

/// What the simulation does to one job after its charge.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    Kept,
    Refunded,
    Disputed,
    Closed,
}

/// One simulated job: its IDs, fee, Stripe fee, whether Confirm recorded the charge, its fate.
struct Job {
    n: u32,
    pick: Uuid,
    job: Uuid,
    business: Uuid,
    intent: String,
    cents: i64,
    fee_cents: i64,
    recorded: bool,
    fate: Fate,
}

/// splitmix64 over a fixed seed, so every run simulates the same 1,000 jobs.
fn jobs() -> Vec<Job> {
    let mut state: u64 = 0x0071_1000_c0ff_ee71;
    let mut next = move |m: u64| {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let z = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) % m
    };
    let job = |n: u32, tier: u64, recorded: u64, fate: u64| {
        let cents = [2500, 7500, 19900][usize::try_from(tier).expect("index")];
        let fate = [Fate::Kept, Fate::Refunded, Fate::Disputed, Fate::Closed]
            [usize::try_from(fate.saturating_sub(1)).expect("index")];
        Job {
            n,
            pick: Uuid::from_u128(0x0710_0000_0000 + u128::from(n)),
            job: Uuid::from_u128(0x0711_0000_0000 + u128::from(n)),
            business: Uuid::from_u128(0xb071_0000 + u128::from(n % BUSINESSES)),
            intent: format!("pi_FakeT071n{n}"),
            cents,
            fee_cents: cents * 29 / 1000 + 30,
            recorded: recorded != 0,
            fate,
        }
    };
    (0..1000)
        .map(|n| job(n, next(3), next(10), next(5)))
        .collect()
}

/// Stripe's charges, refunds and stored answers; a refund refunds its intent's charge in full.
#[derive(Default)]
struct FakeStripe {
    charges: Mutex<Vec<StripeCharge>>,
    refunds: Mutex<Vec<String>>,
    answers: Mutex<HashMap<String, String>>,
}

impl FakeStripe {
    fn charge(&self, job: &Job) {
        self.charges.lock().expect("lock").push(StripeCharge {
            id: format!("ch_FakeT071n{}", job.n),
            payment_intent: job.intent.clone(),
            pick: Some(job.pick),
            amount_cents: job.cents,
            refunded_cents: 0,
            dispute: (job.fate == Fate::Disputed)
                .then(|| (format!("dp_T071n{}", job.n), job.cents)),
            balance_transaction: format!("txn_FakeT071n{}", job.n),
            fee_cents: job.fee_cents,
        });
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
        panic!("the charges are seeded")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("no intent is read here")
    }

    async fn create_refund(&self, key: &IdempotencyKey, pi: &str) -> Result<String, AdapterError> {
        let mut answers = self.answers.lock().expect("lock");
        let answer = answers.entry(key.as_str().to_owned()).or_insert_with(|| {
            self.refunds.lock().expect("lock").push(pi.to_owned());
            let mut charges = self.charges.lock().expect("lock");
            let c = charges.iter_mut().find(|c| c.payment_intent == pi);
            let c = c.expect("refund of a known intent");
            c.refunded_cents = c.amount_cents;
            format!("re_{}", &pi[3..])
        });
        Ok(answer.clone())
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        Ok(self.charges.lock().expect("lock").clone())
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t071".to_owned())
}

/// The pick, its succeeded attempt and its FEE-042 evidence record, as T062 and T189 write them
/// (a closed job's pick is ended), then FEE-006's charge entry at Confirm, under
/// `record_fee_at_confirm`'s key, when the job records it.
async fn seed(pool: &PgPool, job: &Job) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at, ended_at) VALUES ($1, $2, $3, $5, $4, false,
           to_timestamp($6), CASE WHEN $7 THEN to_timestamp($6) END)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($6), to_timestamp($6))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT071', $4, 1, $5, to_timestamp($6),
           to_timestamp($6))",
    ] {
        let q = sqlx::query(sql)
            .bind(job.pick)
            .bind(job.business)
            .bind(job.job);
        let q = q.bind(&job.intent).bind(job.cents).bind(NOW - 86_400);
        q.bind(job.fate == Fate::Closed)
            .execute(pool)
            .await
            .expect("seed");
    }
    let (j, b, ch) = (
        job.job.to_string(),
        job.business.to_string(),
        format!("ch_FakeT071n{}", job.n),
    );
    let key = IdempotencyKey::derive(&secret(), CONFIRM_FEE_ACTION, &[&j, &b]);
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Charge,
        business: Some(job.business),
        job: Some(job.job),
        amount_cents: job.cents,
        stripe_id: Some(&ch),
        idempotency_key: key.as_str(),
    });
    let mut conn = pool.acquire().await.expect("conn");
    if job.recorded {
        ledger_append(&mut conn, &entry.expect("entry"))
            .await
            .expect("append");
    }
}

/// The job's fate after its charge, each through `payments::api` only (FEE-032): a dispute is
/// recorded and then blocks the refund asked for it (FEE-026).
async fn play(pool: &PgPool, stripe: &FakeStripe, job: &Job) {
    if job.fate == Fate::Disputed {
        let dp = format!("dp_T071n{}", job.n);
        let (pi, due_at) = (&job.intent, at(14 * 86_400));
        let opened = DisputeOpened {
            dispute: &dp,
            payment_intent: pi,
            amount_cents: job.cents,
            due_at,
        };
        assert!(record_dispute(pool, &opened, at(0)).await.expect("dispute"));
    }
    if matches!(job.fate, Fate::Refunded | Fate::Disputed) {
        let trigger = (job.pick, RefundTrigger::Rule(RefundReason::Unreachable));
        let outcome = request_refund(pool, stripe, &secret(), trigger, at(0)).await;
        let blocked = matches!(outcome.expect("refund"), RefundOutcome::Blocked);
        assert_eq!(blocked, job.fate == Fate::Disputed);
    }
}

async fn ledger_rows(pool: &PgPool) -> Vec<(String, i64)> {
    let sql = "SELECT kind, amount_cents FROM payments.ledger_entry ORDER BY seq";
    sqlx::query_as(sql).fetch_all(pool).await.expect("ledger")
}

fn total(jobs: &[Job], fates: &[Fate]) -> i64 {
    jobs.iter()
        .filter(|j| fates.contains(&j.fate))
        .map(|j| j.cents)
        .sum()
}

/// FEE-030: the money view is the fold of the ledger rows alone and equals Stripe's gross to the
/// cent with every processing fee; the 20 statements together hold every ledger line once.
async fn check_views(pool: &PgPool, jobs: &[Job], expected: MoneyTotals) {
    let rows = ledger_rows(pool).await;
    let kind = |c: &str| {
        EntryKind::ALL
            .into_iter()
            .find(|k| k.code() == c)
            .expect("kind")
    };
    let fold = MoneyView::from_entries(rows.iter().map(|(c, a)| (kind(c), *a)));
    let window = (
        Timestamp::UNIX_EPOCH,
        Timestamp::now() + SignedDuration::from_hours(24),
    );
    let view = money_view(pool, window).await.expect("view");
    assert_eq!((view, view.gross()), (fold, expected));
    assert_eq!(
        view.processing_fees,
        jobs.iter().map(|j| j.fee_cents).sum::<i64>()
    );
    let mut lines = 0;
    for b in 0..BUSINESSES {
        let id = Uuid::from_u128(0xb071_0000 + u128::from(b));
        let statement = business_statement(pool, id, window)
            .await
            .expect("statement");
        let own = jobs.iter().filter(|j| j.business == id);
        assert_eq!(
            statement.totals.charges,
            own.map(|j| j.cents).sum::<i64>(),
            "business {b}"
        );
        lines += statement.lines.len();
    }
    assert_eq!(lines, rows.len(), "every ledger line is on one statement");
}

/// TD-FEE-16 (FEE-028, FEE-029, FEE-030, FEE-032): 1,000 simulated jobs; the ledger matches
/// Stripe to the cent on two nights, unknown charges are matched or refunded once, and a second
/// night writes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_ledger_matches_stripe_on_1000_jobs(pool: PgPool) {
    let (stripe, jobs) = (FakeStripe::default(), jobs());
    for job in &jobs {
        seed(&pool, job).await;
        stripe.charge(job);
        play(&pool, &stripe, job).await;
    }
    let first = reconcile(&pool, &stripe, &secret(), at(3600))
        .await
        .expect("run");
    let rows = ledger_rows(&pool).await;
    let second = reconcile(&pool, &stripe, &secret(), at(90_000))
        .await
        .expect("rerun");
    assert_eq!(
        ledger_rows(&pool).await,
        rows,
        "a second night writes nothing"
    );
    let closed = jobs.iter().filter(|j| j.fate == Fate::Closed);
    assert!(
        closed.clone().any(|j| !j.recorded),
        "an unknown charge on a closed job"
    );
    let unknown = jobs.iter().filter(|j| !j.recorded).count();
    assert_eq!(
        (first.seen, first.auto_matched, first.refunded),
        (1000, unknown, closed.count())
    );
    assert_eq!((second.auto_matched, second.refunded), (0, 0));
    let expected = MoneyTotals {
        charged: total(
            &jobs,
            &[Fate::Kept, Fate::Refunded, Fate::Disputed, Fate::Closed],
        ),
        refunded: total(&jobs, &[Fate::Refunded, Fate::Closed]),
        disputed: total(&jobs, &[Fate::Disputed]),
    };
    for report in [first, second] {
        assert!(report.matched());
        assert_eq!((report.stripe, report.ledger), (expected, expected));
    }
    let stripe_side = MoneyTotals::of_stripe(stripe.charges.lock().expect("lock").iter());
    assert_eq!(stripe_side, expected, "Stripe's own side, to the cent");
    let refunded = jobs
        .iter()
        .filter(|j| matches!(j.fate, Fate::Refunded | Fate::Closed));
    let sent = stripe.refunds.lock().expect("lock").len();
    assert_eq!(
        sent,
        refunded.count(),
        "each refund sent once, none on a dispute"
    );
    check_views(&pool, &jobs, expected).await;
}

//! TD-FEE-60 (FEE-029, FEE-051, FEE-028; D-2304, D-2303): the nightly reconcile catches up after
//! an outage from the start of its last finished run (`payments.reconcile_checkpoint`) less a day:
//! after 10 days it reads back to that time; after 40 days it reads every charge back to it,
//! skipping none, and raises one information-only High `payments.reconcile_gap` alert. 250
//! charges are read in three pages through `ReconcileStripe::list_charges_page`, each page settled
//! and committed before the next is read; the FEE-030 cent match over the pages equals the match
//! over one page. A partial refund the reconcile itself takes is booked at Stripe's amount with one
//! High alert, a refund in another currency at the charge with one High alert. A failed run (Stripe
//! down, a page failing, a repeated cursor) leaves the checkpoint where it was.
//!
//! Stripe is a fake that implements only `list_charges` (so the provided paged read is the one
//! tested), filters by creation time and records each read's `since`; a probe wraps it to count the
//! ledger at each page read, to answer in one page, or to repeat a cursor. Fake IDs only.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    ChargePage, MISMATCH_ALERT, MoneyEventError, MoneyTotals, PAGE_SIZE, RECONCILE_GAP_ALERT,
    REFUND_MISMATCH_ALERT, REFUND_UNCONFIRMED_ALERT, ReconcileReport, ReconcileStripe,
    StripeCharge, StripeRefund, reconcile,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb334_0001);
const NOW: i64 = 1_795_000_000;
const DAY: i64 = 86_400;

/// Charges with their creation second, each list read's `since`, the reads allowed before Stripe
/// fails, and the amount and currency a refund reports.
#[derive(Default)]
struct FakeStripe {
    charges: Mutex<Vec<(i64, StripeCharge)>>,
    reads: Mutex<Vec<i64>>,
    down_after: Mutex<Option<usize>>,
    refund_reports: Mutex<Option<(i64, &'static str)>>,
}

impl FakeStripe {
    fn add(&self, created: i64, charge: StripeCharge) {
        self.charges.lock().expect("lock").push((created, charge));
    }

    fn reads(&self) -> Vec<i64> {
        self.reads.lock().expect("lock").clone()
    }

    fn all(&self) -> Vec<StripeCharge> {
        let all = self.charges.lock().expect("lock");
        all.iter().map(|(_, c)| c.clone()).collect()
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
        panic!("refunds go through send_refund here")
    }

    /// `succeeded` with the set amount and currency (none by default); the charge shows it refunded.
    async fn send_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<StripeRefund, AdapterError> {
        let reports = *self.refund_reports.lock().expect("lock");
        for (_, c) in self.charges.lock().expect("lock").iter_mut() {
            if c.payment_intent == intent {
                c.refunded_cents = reports.map_or(c.amount_cents, |(a, _)| a.min(c.amount_cents));
            }
        }
        Ok(StripeRefund {
            id: format!("re_{}", &intent[3..]),
            status: "succeeded".to_owned(),
            amount: reports.map(|(a, _)| a),
            currency: reports.map(|(_, c)| c.to_owned()),
            payment_intent: Some(intent.to_owned()),
        })
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, since: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        let mut reads = self.reads.lock().expect("lock");
        reads.push(since.as_second());
        if self
            .down_after
            .lock()
            .expect("lock")
            .is_some_and(|n| reads.len() > n)
        {
            return Err(AdapterError::NotSent);
        }
        let all = self.charges.lock().expect("lock");
        let since = since.as_second();
        Ok(all
            .iter()
            .filter(|(at, _)| *at >= since)
            .map(|(_, c)| c.clone())
            .collect())
    }
}

/// How the probe answers a page read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The fake's provided paged read, noting the page size and the ledger's charges so far.
    Paged,
    /// Every charge in one page.
    OnePage,
    /// Every page names the same next cursor.
    Stuck,
}

struct Probe<'a> {
    inner: &'a FakeStripe,
    pool: PgPool,
    mode: Mode,
    seen: Mutex<Vec<(usize, i64)>>,
}

impl PaymentsAdapter for Probe<'_> {
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
        panic!("no refund is owed in these runs")
    }
}

impl ReconcileStripe for Probe<'_> {
    async fn list_charges(&self, since: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        self.inner.list_charges(since).await
    }

    async fn list_charges_page(
        &self,
        since: Timestamp,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ChargePage, AdapterError> {
        match self.mode {
            Mode::OnePage => Ok(ChargePage {
                charges: self.inner.list_charges(since).await?,
                next: None,
            }),
            Mode::Stuck => Ok(ChargePage {
                charges: Vec::new(),
                next: Some("ch_FakeT334stuck".to_owned()),
            }),
            Mode::Paged => {
                let page = self.inner.list_charges_page(since, after, limit).await?;
                let held = entries(&self.pool, "charge").await;
                self.seen
                    .lock()
                    .expect("lock")
                    .push((page.charges.len(), held));
                Ok(page)
            }
        }
    }
}

fn charge(id: &str, intent: &str) -> StripeCharge {
    StripeCharge {
        id: format!("ch_FakeT334{id}"),
        payment_intent: format!("pi_FakeT334{intent}"),
        pick: None,
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: format!("txn_FakeT334{id}"),
        fee_cents: 248,
    }
}

fn night(n: i64) -> Timestamp {
    Timestamp::from_second(NOW + n * DAY).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t334".to_owned())
}

/// `n` open (or ended) $75 picks of one business on `pi_FakeT334p001` to `pi_FakeT334p<n>`.
async fn picks(pool: &PgPool, n: i32, ended: bool) {
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at, ended_at)
         SELECT md5('t334p' || i)::uuid, $1, md5('t334j' || i)::uuid, 7500,
                'pi_FakeT334p' || lpad(i::text, 3, '0'), false, now(),
                CASE WHEN $3 THEN now() END
           FROM generate_series(1, $2) AS i",
    )
    .bind(BUSINESS)
    .bind(n)
    .bind(ended)
    .execute(pool)
    .await
    .expect("seed picks");
}

/// The charge on pick `i`'s own intent.
fn own(i: usize) -> StripeCharge {
    charge(&format!("c{i:03}"), &format!("p{i:03}"))
}

async fn run(pool: &PgPool, stripe: &FakeStripe, at: Timestamp) -> ReconcileReport {
    reconcile(pool, stripe, &secret(), at).await.expect("run")
}

async fn scalar(pool: &PgPool, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("scalar")
}

async fn entries(pool: &PgPool, kind: &str) -> i64 {
    let sql = "SELECT count(*) FROM payments.ledger_entry WHERE kind = $1";
    scalar(pool, sql, kind).await
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    let sql = "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'";
    scalar(pool, sql, kind).await
}

async fn checkpoint(pool: &PgPool) -> Option<i64> {
    let sql =
        "SELECT extract(epoch FROM run_started_at)::bigint FROM payments.reconcile_checkpoint";
    sqlx::query_scalar(sql)
        .fetch_optional(pool)
        .await
        .expect("checkpoint")
}

/// TD-FEE-60 (FEE-029): after a 10-day outage the run reads back to its checkpoint less a day, so
/// a charge created the day after the last run, 9 days ago, is read and refunded; the checkpoint
/// moves to the run's start.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_a_ten_day_outage_reads_back_to_the_checkpoint_less_a_day(pool: PgPool) {
    let stripe = FakeStripe::default();
    run(&pool, &stripe, night(0)).await;
    assert_eq!(
        stripe.reads(),
        [night(-7).as_second()],
        "no checkpoint: 7 days"
    );
    assert_eq!(checkpoint(&pool).await, Some(night(0).as_second()));
    stripe.add(night(1).as_second(), charge("a", "a"));
    stripe.add(night(-2).as_second(), charge("b", "b"));
    let report = run(&pool, &stripe, night(10)).await;
    assert_eq!(stripe.reads()[1], night(-1).as_second());
    assert_eq!((report.seen, report.refunded), (1, 1), "{report:?}");
    assert_eq!(alerts(&pool, RECONCILE_GAP_ALERT).await, 1);
    assert_eq!(checkpoint(&pool).await, Some(night(10).as_second()));
}

/// TD-FEE-60 (FEE-029, FEE-051): after a 40-day outage the run reads every charge back to its
/// checkpoint less a day, skipping none, and raises one information-only gap alert; the next
/// night reads its usual 7 days and raises none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_a_forty_day_outage_skips_no_charge_and_alerts_once(pool: PgPool) {
    picks(&pool, 5, false).await;
    let stripe = FakeStripe::default();
    run(&pool, &stripe, night(0)).await;
    for (i, day) in [1, 8, 20, 33, 39].into_iter().enumerate() {
        stripe.add(night(day).as_second(), own(i + 1));
    }
    stripe.add(night(-3).as_second(), charge("old", "old"));
    let report = run(&pool, &stripe, night(40)).await;
    assert_eq!(stripe.reads()[1], night(-1).as_second());
    assert_eq!(
        (report.seen, report.auto_matched, report.refunded),
        (5, 5, 0)
    );
    assert!(report.matched(), "{report:?}");
    assert_eq!(report.stripe.charged, 5 * 7500);
    assert_eq!(entries(&pool, "charge").await, 5);
    assert_eq!(alerts(&pool, RECONCILE_GAP_ALERT).await, 1);
    let report = run(&pool, &stripe, night(41)).await;
    assert_eq!(stripe.reads()[2], night(34).as_second());
    assert_eq!(report.seen, 1);
    assert_eq!(
        alerts(&pool, RECONCILE_GAP_ALERT).await,
        1,
        "information once"
    );
    assert_eq!(entries(&pool, "charge").await, 5);
}

/// TD-FEE-60 (FEE-029, FEE-051, FEE-028): 250 charges are read in three pages of at most 100,
/// each page settled and committed before the next is read; the cent match over the pages equals
/// the match over one page and Stripe's totals over every charge.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_two_hundred_fifty_charges_are_read_in_three_pages(pool: PgPool) {
    picks(&pool, 250, false).await;
    let stripe = FakeStripe::default();
    for i in 1..=250 {
        let mut c = own(i);
        if i % 7 == 0 {
            c.refunded_cents = 2500;
        }
        stripe.add(night(0).as_second(), c);
    }
    let paged = Probe {
        inner: &stripe,
        pool: pool.clone(),
        mode: Mode::Paged,
        seen: Mutex::new(Vec::new()),
    };
    let report = reconcile(&pool, &paged, &secret(), night(0))
        .await
        .expect("paged run");
    let pages = paged.seen.lock().expect("lock").clone();
    assert_eq!(
        pages,
        [(100, 0), (100, 100), (50, 200)],
        "committed page by page"
    );
    assert_eq!(PAGE_SIZE, 100);
    assert_eq!((report.seen, report.auto_matched), (250, 250));
    assert!(report.matched(), "{report:?}");
    assert_eq!(report.stripe, MoneyTotals::of_stripe(&stripe.all()));
    assert_eq!(report.stripe.refunded, 35 * 2500);
    assert_eq!(entries(&pool, "charge").await, 250);
    let one = Probe {
        mode: Mode::OnePage,
        ..paged
    };
    let whole = reconcile(&pool, &one, &secret(), night(1))
        .await
        .expect("one-page run");
    assert_eq!(whole.seen, 250);
    assert_eq!((whole.stripe, whole.ledger), (report.stripe, report.ledger));
    assert!(whole.matched(), "{whole:?}");
}

/// TD-FEE-60 (FEE-026, FEE-028, D-2303): a refund the reconcile takes that Stripe reports as $25
/// of $75 is booked at $25 with one High mismatch alert naming the pick, and the match holds; a
/// refund reported in `eur` is booked at the charge with one High unconfirmed alert. The next night
/// sends and alerts nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_a_partial_refund_found_by_the_reconcile_alerts(pool: PgPool) {
    picks(&pool, 1, true).await;
    let stripe = FakeStripe::default();
    stripe.add(night(0).as_second(), own(1));
    *stripe.refund_reports.lock().expect("lock") = Some((2500, "usd"));
    let report = run(&pool, &stripe, night(0)).await;
    assert_eq!((report.refunded, report.open), (1, 0), "{report:?}");
    assert!(report.matched(), "{report:?}");
    let booked = "SELECT sum(amount_cents)::bigint FROM payments.ledger_entry WHERE kind = $1";
    assert_eq!(scalar(&pool, booked, "refund").await, 2500);
    let subject = "SELECT count(*) FROM ops.alert WHERE kind = $1 AND subject_id IS NOT NULL";
    assert_eq!(scalar(&pool, subject, REFUND_MISMATCH_ALERT).await, 1);
    stripe.add(night(0).as_second(), charge("eur", "eur"));
    *stripe.refund_reports.lock().expect("lock") = Some((7000, "eur"));
    let report = run(&pool, &stripe, night(1)).await;
    assert_eq!(report.refunded, 1, "only the new no-pick charge");
    assert_eq!(alerts(&pool, REFUND_UNCONFIRMED_ALERT).await, 1);
    assert_eq!(alerts(&pool, REFUND_MISMATCH_ALERT).await, 1);
    assert_eq!(scalar(&pool, booked, "refund").await, 2500);
}

/// TD-FEE-60 (FEE-029, FEE-028, D-2303, D-3067): a partial refund the reconcile sends through
/// T066 (the pick has its evidence record) is held at the $25 T066 booked, not the $75 charge, so
/// the charge settles: nothing stays open, one refund-amount alert, no ledger-mismatch alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_a_partial_t066_refund_found_by_the_reconcile_settles(pool: PgPool) {
    picks(&pool, 1, true).await;
    for sql in [
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES (md5('t334p1')::uuid, 1, $1, 'succeeded', now(), now())",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES (md5('t334p1')::uuid, $1,
           md5('t334j1')::uuid, $1, 1, 'pick-' || md5('t334p1')::uuid::text || '-try1', 'cus_FakeT334', 'pi_FakeT334p001', 1,
           7500, now(), now())",
    ] {
        sqlx::query(sql)
            .bind(BUSINESS)
            .execute(&pool)
            .await
            .expect("seed");
    }
    let stripe = FakeStripe::default();
    stripe.add(night(0).as_second(), own(1));
    *stripe.refund_reports.lock().expect("lock") = Some((2500, "usd"));
    let report = run(&pool, &stripe, night(0)).await;
    assert_eq!((report.refunded, report.open), (1, 0), "{report:?}");
    assert!(report.matched(), "{report:?}");
    let t066 = "SELECT count(*) FROM payments.refund WHERE status = $1";
    assert_eq!(
        scalar(&pool, t066, "succeeded").await,
        1,
        "sent through T066"
    );
    let booked = "SELECT sum(amount_cents)::bigint FROM payments.ledger_entry WHERE kind = $1";
    assert_eq!(scalar(&pool, booked, "refund").await, 2500);
    assert_eq!(alerts(&pool, REFUND_MISMATCH_ALERT).await, 1);
    assert_eq!(alerts(&pool, MISMATCH_ALERT).await, 0);
    let open = "SELECT count(*) FROM payments.reconcile_open WHERE last_code = $1";
    assert_eq!(scalar(&pool, open, "mismatch").await, 0);
}

/// TD-FEE-60 (FEE-029): a run that fails (Stripe down, the second page failing, a repeated
/// cursor) leaves the checkpoint where it was, the pages it finished stay committed, and the next
/// finished run moves it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_60_a_failed_run_leaves_the_checkpoint(pool: PgPool) {
    picks(&pool, 101, false).await;
    let stripe = FakeStripe::default();
    run(&pool, &stripe, night(0)).await;
    for i in 1..=101 {
        stripe.add(night(0).as_second(), own(i));
    }
    *stripe.down_after.lock().expect("lock") = Some(2);
    let failed = reconcile(&pool, &stripe, &secret(), night(1)).await;
    assert!(matches!(failed, Err(MoneyEventError::Stripe)), "{failed:?}");
    assert_eq!(checkpoint(&pool).await, Some(night(0).as_second()));
    assert_eq!(
        entries(&pool, "charge").await,
        100,
        "the first page stays committed"
    );
    let failed = reconcile(&pool, &stripe, &secret(), night(2)).await;
    assert!(matches!(failed, Err(MoneyEventError::Stripe)), "{failed:?}");
    let stuck = Probe {
        inner: &stripe,
        pool: pool.clone(),
        mode: Mode::Stuck,
        seen: Mutex::new(Vec::new()),
    };
    *stripe.down_after.lock().expect("lock") = None;
    let failed = reconcile(&pool, &stuck, &secret(), night(3)).await;
    assert!(matches!(failed, Err(MoneyEventError::Stripe)), "{failed:?}");
    assert_eq!(checkpoint(&pool).await, Some(night(0).as_second()));
    let report = run(&pool, &stripe, night(4)).await;
    assert_eq!(stripe.reads().last(), Some(&night(-3).as_second()));
    assert!(report.matched(), "{report:?}");
    assert_eq!(entries(&pool, "charge").await, 101);
    assert_eq!(checkpoint(&pool).await, Some(night(4).as_second()));
}

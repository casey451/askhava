//! TD-FEE-64 (FEE-013, FEE-028, FEE-030, FEE-031, FEE-042, FEE-045; D-2623, D-3134, D-3658): on
//! a fresh database, a success recorded by the confirm, by the check round or by the webhook
//! leaves exactly one `charge` ledger row with the intent's `ch_` ID and the pick's cents under
//! the per-pick key `charge.<pick>`, an evidence row holding the same ID and one
//! `payments.receipt` task; a replay adds nothing; a succeeded intent with no `ch_` ID records
//! and releases nothing; the business statement and the money view total the charges; a
//! released paid pick makes the same customer's next job a repeat, but a charge refunded with
//! nothing released (late success, fraud warning) does not; a charge, its refund, a re-pick of
//! the same business on the same job and a second charge leave two `charge` rows, which a
//! reconcile neither doubles nor flags; a fee-free pick of a business with no saved card gets its
//! `fee-free-job-type` entry from `waive_at_confirm` and `no_fee_release` clears it; a charged
//! intent amount that differs from the stored fee is booked as charged and alerts once.
//!
//! Stripe is a scripted fake of `IntentStripe` and `ReconcileStripe` (its intent and charge IDs
//! are derived from the pick); the webhook is signed with a fake secret. All IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, Delivery, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
    WebhookVendor, verify_webhook,
};
use askhava_payments::api::{
    BusinessId, CHARGE_FEE_MISMATCH_ALERT, CHECK_KIND, ChargeOutcome, EntryKind, FeeJob, FeeMode,
    FeeQuote, IntentStripe, JoinDates, LedgerEntry, NewEntry, PeriodJob, PickNow, PickedFee, Press,
    RECEIPT_KIND, ReconcileStripe, RecurringCap, ReleaseError, Service, StripeAccount,
    StripeCharge, UNKNOWN_CHARGE_ALERT, VerifiedStripeEvent, VisitTier, Waiver, WinCheck,
    business_statement, confirm_and_pay, fee_at_pick, fix_fee_at_pick, ledger_append, money_view,
    no_fee_release, paid_release, reconcile, run_charge_check, waive_at_confirm,
};
use askhava_privacy::api::ReleaseBasis;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb359_0001));
const WEBHOOK_SECRET: &str = "whsec_fake_t359_not_a_real_secret";
const NOW: i64 = 1_794_000_000;
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe: a confirm answers `confirm`, a fetch answers `fetch`, a succeeded intent
/// names its `ch_` charge only when `with_charge`; the reconcile lists `charges`.
struct FakeStripe {
    confirm: &'static str,
    fetch: &'static str,
    with_charge: bool,
    charges: Mutex<Vec<StripeCharge>>,
}

fn fake(confirm: &'static str, fetch: &'static str) -> FakeStripe {
    FakeStripe {
        confirm,
        fetch,
        with_charge: true,
        charges: Mutex::new(Vec::new()),
    }
}

fn pick(n: u128) -> Uuid {
    Uuid::from_u128(0x359_0000 + n)
}

fn job(n: u128) -> Uuid {
    Uuid::from_u128(0x359_1000 + n)
}

fn intent_id(pick: Uuid) -> String {
    format!("pi_{}", pick.simple())
}

fn charge_id(pick: Uuid) -> String {
    format!("ch_{}", pick.simple())
}

impl FakeStripe {
    fn answer(&self, intent: &str, status: &str) -> Answer {
        let pick = Uuid::parse_str(intent.trim_start_matches("pi_")).expect("a fake intent");
        let paid = status == "succeeded" && self.with_charge;
        Ok(PaymentIntent {
            id: intent.to_owned(),
            status: status.to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT359".to_owned(),
            metadata: IntentMetadata {
                pick_id: pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: paid.then(|| charge_id(pick)),
        })
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("no confirmed intent is created here (FEE-047)")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Answer {
        self.answer(id, self.fetch)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        pick: &str,
    ) -> Result<String, AdapterError> {
        Ok(intent_id(Uuid::parse_str(pick).expect("pick")))
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, intent: &str, _: &str) -> Answer {
        self.answer(intent, self.confirm)
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        Ok(self.charges.lock().expect("lock").clone())
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t359".to_owned())
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn account() -> StripeAccount {
    StripeAccount {
        live: false,
        account: "acct_testfake".to_owned(),
    }
}

/// The fee `fee_at_pick` fixes for a firm quote of `cents`, fee-free when `fee_free`.
fn fee(cents: u64, fee_free: bool) -> PickedFee {
    let period = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    fee_at_pick(FeeQuote::Fixed(cents), None, period, FeeMode::Tiers, None).expect("fee")
}

/// Stores the fee fixed at `pick` on `job` (FEE-005, D-1871).
async fn fixed(pool: &PgPool, (pick, job): (Uuid, Uuid), picked: &PickedFee) {
    let mut conn = pool.acquire().await.expect("conn");
    fix_fee_at_pick(&mut conn, pick, BUSINESS, job, picked, at(-600))
        .await
        .expect("fee fixed");
}

/// The business's saved Stripe customer (a business with a card).
async fn with_customer(pool: &PgPool) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT359")
        .execute(pool)
        .await
        .expect("customer");
}

/// One "Confirm and pay" press of `pick` on `job` for the $75 fee fixed at the pick.
async fn press(pool: &PgPool, stripe: &FakeStripe, (pick, job): (Uuid, Uuid)) -> ChargeOutcome {
    fixed(pool, (pick, job), &fee(100_000, false)).await;
    let press = Press {
        pick,
        business: BUSINESS,
        job,
        amount_cents: 7500,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(pick.as_u128() + 0x5000),
        emergency: false,
        now: PickNow::Payable,
    };
    confirm_and_pay(pool, stripe, &secret(), &press, at(0))
        .await
        .expect("press")
}

/// A signed `payment_intent.succeeded` event for `intent`, as `adapters` hands it over.
fn succeeded(intent: &str) -> VerifiedStripeEvent {
    let body = json!({"id": "evt_FakeT359", "type": "payment_intent.succeeded",
        "livemode": false, "data": {"object": {"id": intent, "object": "payment_intent"}}})
    .to_string();
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
    mac.update(format!("{NOW}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = format!("t={NOW},v1={sig}");
    let headers = [("Stripe-Signature", header.as_str())];
    let delivery = Delivery {
        url: "https://hooks.example.test/webhooks/stripe",
        body: body.as_bytes(),
        headers: &headers,
    };
    let secret = SecretString::from(WEBHOOK_SECRET.to_owned());
    let hook = verify_webhook(WebhookVendor::Stripe, &secret, delivery, NOW).expect("signed");
    VerifiedStripeEvent::from_webhook(&hook).expect("a Stripe event")
}

async fn webhook(pool: &PgPool, stripe: &FakeStripe, pick: Uuid) -> Result<Uuid, ReleaseError> {
    let event = succeeded(&intent_id(pick));
    let cleared = paid_release(pool, stripe, &account(), &event, pick, at(60)).await?;
    assert_eq!(cleared.basis(), ReleaseBasis::PaidCharge);
    Ok(cleared.pick())
}

async fn check_round(pool: &PgPool, stripe: &FakeStripe, pick: Uuid) {
    let payload = json!({"pick": pick, "try": 1, "round": 0});
    run_charge_check(pool, stripe, &payload, at(1_800))
        .await
        .expect("check");
}

async fn count(pool: &PgPool, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// What one pick's success left: its `charge` rows `(stripe ID, cents, key)`, the evidence's
/// charge ID and its receipt tasks.
async fn booked(pool: &PgPool, pick: Uuid) -> (Vec<(String, i64, String)>, Option<String>, i64) {
    let entries = sqlx::query_as(
        "SELECT e.stripe_id, e.amount_cents, e.idempotency_key FROM payments.ledger_entry e
          JOIN payments.pick_intent i ON i.job_id = e.job_id AND i.business_id = e.business_id
         WHERE i.pick_id = $1 AND e.kind = 'charge' AND e.stripe_id = $2 ORDER BY e.seq",
    )
    .bind(pick)
    .bind(charge_id(pick))
    .fetch_all(pool)
    .await
    .expect("entries");
    let evidence: Option<Option<String>> = sqlx::query_scalar(
        "SELECT stripe_charge_id FROM payments.charge_evidence WHERE pick_id = $1",
    )
    .bind(pick)
    .fetch_optional(pool)
    .await
    .expect("evidence");
    let receipts = "SELECT count(*) FROM core.task WHERE kind = 'payments.receipt'
                      AND payload->>'pick' = $1";
    let receipts = count(pool, receipts, &pick.to_string()).await;
    (entries, evidence.flatten(), receipts)
}

fn one_charge(pick: Uuid) -> (Vec<(String, i64, String)>, Option<String>, i64) {
    let entry = (charge_id(pick), 7500, format!("charge.{pick}"));
    (vec![entry], Some(charge_id(pick)), 1)
}

async fn ledger_rows(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM payments.ledger_entry WHERE $1 = $1",
        "",
    )
    .await
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    count(pool, "SELECT count(*) FROM ops.alert WHERE kind = $1", kind).await
}

/// TD-FEE-64 (FEE-028, FEE-031, FEE-042, D-3134): a success recorded first by the confirm, by
/// the check round or by the webhook leaves one `charge` row with the `ch_` ID and the stored
/// cents under `charge.<pick>`, the evidence holds the same ID and one receipt task is queued;
/// the replayed webhook and check add nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_each_success_writer_books_one_charge(pool: PgPool) {
    with_customer(&pool).await;
    assert_eq!(RECEIPT_KIND, "payments.receipt");
    let paid = fake("succeeded", "succeeded");
    let outcome = press(&pool, &paid, (pick(1), job(1))).await;
    assert_eq!(outcome, ChargeOutcome::Succeeded(intent_id(pick(1))));
    let waiting = fake("processing", "succeeded");
    press(&pool, &waiting, (pick(2), job(2))).await;
    press(&pool, &waiting, (pick(3), job(3))).await;
    assert_eq!(booked(&pool, pick(2)).await, (vec![], None, 0));
    check_round(&pool, &waiting, pick(2)).await;
    assert_eq!(webhook(&pool, &waiting, pick(3)).await, Ok(pick(3)));
    for n in 1..=3 {
        assert_eq!(
            booked(&pool, pick(n)).await,
            one_charge(pick(n)),
            "pick {n}"
        );
    }
    let rows = ledger_rows(&pool).await;
    assert_eq!(rows, 3);
    for n in 1..=3 {
        assert_eq!(webhook(&pool, &waiting, pick(n)).await, Ok(pick(n)));
        check_round(&pool, &waiting, pick(n)).await;
    }
    assert_eq!(ledger_rows(&pool).await, rows, "a replay adds nothing");
    for n in 1..=3 {
        assert_eq!(
            booked(&pool, pick(n)).await,
            one_charge(pick(n)),
            "pick {n}"
        );
    }
    assert_eq!(alerts(&pool, CHARGE_FEE_MISMATCH_ALERT).await, 0);
    let span = (
        at(-86_400),
        Timestamp::from_second(4_000_000_000).expect("t"),
    );
    let statement = business_statement(&pool, BUSINESS.0, span)
        .await
        .expect("statement");
    assert_eq!(
        statement.totals.charges, 22_500,
        "the statement totals the charges"
    );
    let lines = statement
        .lines
        .iter()
        .filter(|l| l.kind == EntryKind::Charge);
    assert_eq!(lines.count(), 3);
    let view = money_view(&pool, span).await.expect("money view");
    assert_eq!(view.charges, 22_500);
}

/// TD-FEE-64 (FEE-042, PICK-022, D-3134): a succeeded intent with no `ch_` ID is no recorded
/// success: the confirm and the check round leave the attempt undecided, the webhook is refused
/// so Stripe resends it, and no evidence, ledger row or receipt is written.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_a_success_with_no_charge_id_releases_nothing(pool: PgPool) {
    with_customer(&pool).await;
    let mut bare = fake("succeeded", "succeeded");
    bare.with_charge = false;
    assert_eq!(
        press(&pool, &bare, (pick(1), job(1))).await,
        ChargeOutcome::Pending
    );
    check_round(&pool, &bare, pick(1)).await;
    assert_eq!(
        webhook(&pool, &bare, pick(1)).await,
        Err(ReleaseError::Stripe)
    );
    let status = "SELECT count(*) FROM payments.charge_attempt WHERE status = 'sent'
                    AND pick_id::text = $1";
    assert_eq!(count(&pool, status, &pick(1).to_string()).await, 1);
    assert_eq!(booked(&pool, pick(1)).await, (vec![], None, 0));
    assert_eq!(ledger_rows(&pool).await, 0);
    let checks = "SELECT count(*) FROM core.task WHERE kind = $1";
    assert!(
        count(&pool, checks, CHECK_KIND).await >= 1,
        "a later fetch is queued"
    );
    let full = fake("processing", "succeeded");
    check_round(&pool, &full, pick(1)).await;
    assert_eq!(booked(&pool, pick(1)).await, one_charge(pick(1)));
}

fn dates() -> JoinDates {
    JoinDates {
        joined: at(0),
        launch: at(-200 * 86_400),
        trades_opened: Vec::new(),
    }
}

/// `waive_at_confirm` for BUSINESS on `job`, with `earlier` the customer's other jobs.
async fn waive(pool: &PgPool, job: Uuid, earlier: &[Uuid], picked: &PickedFee) -> Waiver {
    let dates = dates();
    let check = WinCheck {
        business: BUSINESS,
        job,
        picked,
        dates: &dates,
        same_customer_jobs: earlier,
    };
    let mut conn = pool.acquire().await.expect("conn");
    waive_at_confirm(&mut conn, &secret(), check)
        .await
        .expect("waiver")
}

async fn refunded(pool: &PgPool, pick: Uuid, trigger: &str) {
    sqlx::query(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
         VALUES ($1, $2, 'succeeded', 1, now())",
    )
    .bind(pick)
    .bind(trigger)
    .execute(pool)
    .await
    .expect("refund");
}

/// TD-FEE-64 (FEE-013): after a released, paid pick the same business's pick on another job of
/// the same customer is a repeat; a charge refunded with nothing released (a late success or an
/// unreleased fraud warning) is no introduction, so the next job is not waived; a refund after a
/// release (unreachable) still counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_only_an_actual_introduction_makes_a_repeat(pool: PgPool) {
    with_customer(&pool).await;
    let paid = fake("succeeded", "succeeded");
    for n in 1..=3 {
        press(&pool, &paid, (pick(n), job(n))).await;
        assert_eq!(booked(&pool, pick(n)).await, one_charge(pick(n)));
    }
    let picked = fee(100_000, false);
    let next = waive(&pool, job(11), &[job(1)], &picked).await;
    assert!(matches!(next, Waiver::Repeat(_)), "{next:?}");
    refunded(&pool, pick(2), "late-success").await;
    assert_eq!(
        waive(&pool, job(12), &[job(2)], &picked).await,
        Waiver::NotWaived
    );
    refunded(&pool, pick(3), "fraud-warning").await;
    assert_eq!(
        waive(&pool, job(13), &[job(3)], &picked).await,
        Waiver::NotWaived
    );
    refunded(&pool, pick(1), "unreachable").await;
    let after = waive(&pool, job(14), &[job(1)], &picked).await;
    assert!(matches!(after, Waiver::Repeat(_)), "{after:?}");
}

/// The charge as Stripe's list shows it: `refunded` cents back, no processing fee.
fn listed(pick: Uuid, refunded: i64) -> StripeCharge {
    StripeCharge {
        id: charge_id(pick),
        payment_intent: intent_id(pick),
        pick: Some(pick),
        amount_cents: 7500,
        refunded_cents: refunded,
        dispute: None,
        balance_transaction: format!("txn_{}", pick.simple()),
        fee_cents: 0,
    }
}

/// TD-FEE-64 (FEE-028, FEE-030): a charge, its full refund, a re-pick of the same business on
/// the same job and a second charge leave two `charge` rows, each under its own pick's key, and
/// a reconcile over both appends nothing and raises no `payments.unknown_charge`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_a_second_paid_pick_on_the_same_job_keeps_its_own_charge(pool: PgPool) {
    with_customer(&pool).await;
    let paid = fake("succeeded", "succeeded");
    press(&pool, &paid, (pick(1), job(1))).await;
    refunded(&pool, pick(1), "unreachable").await;
    let key = format!("refund.{}", pick(1));
    let refund = LedgerEntry::new(NewEntry {
        kind: EntryKind::Refund,
        business: Some(BUSINESS.0),
        job: Some(job(1)),
        amount_cents: 7500,
        stripe_id: Some(&intent_id(pick(1))),
        idempotency_key: &key,
    })
    .expect("refund entry");
    let mut conn = pool.acquire().await.expect("conn");
    ledger_append(&mut conn, &refund)
        .await
        .expect("refund booked");
    drop(conn);
    press(&pool, &paid, (pick(2), job(1))).await;
    for n in 1..=2 {
        assert_eq!(
            booked(&pool, pick(n)).await,
            one_charge(pick(n)),
            "pick {n}"
        );
    }
    let charges = "SELECT count(*) FROM payments.ledger_entry WHERE kind = $1";
    assert_eq!(count(&pool, charges, "charge").await, 2);
    let rows = ledger_rows(&pool).await;
    *paid.charges.lock().expect("lock") = vec![listed(pick(1), 7500), listed(pick(2), 0)];
    let report = reconcile(&pool, &paid, &secret(), at(3_600))
        .await
        .expect("reconcile");
    assert_eq!(report.seen, 2);
    assert_eq!(
        ledger_rows(&pool).await,
        rows,
        "the reconcile appends nothing"
    );
    assert_eq!(alerts(&pool, UNKNOWN_CHARGE_ALERT).await, 0);
}

/// TD-FEE-64 (FEE-045, FEE-036, D-1419): a fee-free job-type pick of a business with no saved
/// card gets its `fee-free-job-type` $0 entry from `waive_at_confirm`, once however often it
/// runs, and `no_fee_release` clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_a_fee_free_pick_gets_its_entry_and_is_released(pool: PgPool) {
    let free = fee(40_000, true);
    assert_eq!(free.cents(), 0);
    fixed(&pool, (pick(1), job(1)), &free).await;
    for _ in 0..2 {
        assert_eq!(waive(&pool, job(1), &[], &free).await, Waiver::NotWaived);
    }
    let entries: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind, amount_cents FROM payments.ledger_entry")
            .fetch_all(&pool)
            .await
            .expect("entries");
    assert_eq!(entries, vec![("fee-free-job-type".to_owned(), 0)]);
    let mut conn = pool.acquire().await.expect("conn");
    let cleared = no_fee_release(&mut conn, pick(1), (BUSINESS.0, job(1)))
        .await
        .expect("cleared");
    assert_eq!(cleared.basis(), ReleaseBasis::NoFeeDue);
    assert_eq!(waive(&pool, job(2), &[], &free).await, Waiver::NotWaived);
    assert_eq!(
        ledger_rows(&pool).await,
        1,
        "no stored fee-free row, no entry"
    );
}

/// TD-FEE-64 (FEE-028, FEE-042, D-3658): a success whose checked intent amount differs from the
/// stored fee books the intent amount, the money Stripe took, and raises one High
/// `payments.charge_fee_mismatch` naming the pick; a replay raises nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_64_a_fee_mismatch_books_the_intent_amount_and_alerts(pool: PgPool) {
    with_customer(&pool).await;
    fixed(&pool, (pick(1), job(1)), &fee(10_000, false)).await;
    let seeds = [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, now())",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           VALUES ($1, 1, $1, 'sent', now() - interval '1 hour')",
    ];
    for sql in seeds {
        sqlx::query(sql)
            .bind(pick(1))
            .bind(BUSINESS.0)
            .bind(job(1))
            .bind(intent_id(pick(1)))
            .execute(&pool)
            .await
            .expect("seed");
    }
    let paid = fake("succeeded", "succeeded");
    check_round(&pool, &paid, pick(1)).await;
    check_round(&pool, &paid, pick(1)).await;
    assert_eq!(booked(&pool, pick(1)).await, one_charge(pick(1)));
    assert_eq!(alerts(&pool, CHARGE_FEE_MISMATCH_ALERT).await, 1);
    let subject = "SELECT count(*) FROM ops.alert WHERE subject_id::text = $1";
    assert_eq!(count(&pool, subject, &pick(1).to_string()).await, 1);
}

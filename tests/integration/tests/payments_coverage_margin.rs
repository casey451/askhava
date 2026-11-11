//! T387 (D-3049; FEE-026, FEE-028, FEE-053; D-2303), against PostgreSQL: refund paths of
//! `app/payments/src/refunds.rs` no other test reaches, so `payments` branch coverage has margin
//! over its 95% floor (BUILD_BRIEF.md:481). A `succeeded` refund with no amount whose one re-read
//! is not `succeeded`, or names another PaymentIntent, stands: booked at the charge with one High
//! `payments.refund_amount_unconfirmed` alert. `refund_in_full` on a succeeded refund the ledger
//! holds no entry for reads Stripe: the full charge is `true`, a foreign one `false`, and a read
//! error other than `NotSent` is `MoneyEventError::Stripe`. None of this is behaviour T380 changes
//! (the forgotten-key retry, `charge_already_refunded`, the refusal answer). Stripe is a scripted
//! fake; every ID and number is fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    MoneyEventError, REFUND_UNCONFIRMED_ALERT, RefundOutcome, RefundReason, RefundTrigger,
    StripeRefund, refund_in_full, request_refund,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0x387_0b01);
const REFUND: &str = "re_FakeT387";
const NOW: i64 = 1_793_000_000;
const NO_AMOUNT: (Option<i64>, Option<&str>) = (None, Some("usd"));

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

/// Stripe's refund `REFUND` of `intent` in `status`, reporting `(amount, currency)`.
fn refund(
    intent: &str,
    status: &str,
    (amount, currency): (Option<i64>, Option<&str>),
) -> StripeRefund {
    StripeRefund {
        id: REFUND.to_owned(),
        status: status.to_owned(),
        amount,
        currency: currency.map(str::to_owned),
        payment_intent: Some(intent.to_owned()),
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t387".to_owned())
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

/// One paid pick: its ID, its job and its PaymentIntent.
#[derive(Clone, Copy)]
struct Paid {
    pick: Uuid,
    job: Uuid,
    intent: &'static str,
}

/// Six paid picks, one per case.
const PAID: [Paid; 6] = {
    const INTENTS: [&str; 6] = [
        "pi_FakeT387a",
        "pi_FakeT387b",
        "pi_FakeT387c",
        "pi_FakeT387d",
        "pi_FakeT387e",
        "pi_FakeT387f",
    ];
    let mut paid = [Paid {
        pick: Uuid::nil(),
        job: Uuid::nil(),
        intent: "",
    }; 6];
    let mut n = 0;
    while n < 6 {
        paid[n] = Paid {
            pick: Uuid::from_u128(0x387_0a00 + n as u128),
            job: Uuid::from_u128(0x387_0c00 + n as u128),
            intent: INTENTS[n],
        };
        n += 1;
    }
    paid
};

/// `paid`'s $75 charge with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool, paid: Paid) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT387', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(paid.pick)
            .bind(BUSINESS)
            .bind(paid.job)
            .bind(paid.intent)
            .bind(NOW - 86_400)
            .execute(pool)
            .await
            .expect("seed");
    }
}

/// The amounts of `paid`'s ledger `refund` entries, in order.
async fn booked(pool: &PgPool, paid: Paid) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT amount_cents FROM payments.ledger_entry WHERE kind = 'refund' AND job_id = $1 ORDER BY seq",
    )
    .bind(paid.job)
    .fetch_all(pool)
    .await
    .expect("ledger")
}

/// How many High alerts of `kind` name `paid`'s pick.
async fn alerts(pool: &PgPool, kind: &str, paid: Paid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high' AND subject_id = $2",
    )
    .bind(kind)
    .bind(paid.pick)
    .fetch_one(pool)
    .await
    .expect("alerts")
}

/// `paid`'s refund requested once under the unreachable rule through `stripe`.
async fn requested(pool: &PgPool, stripe: &FakeStripe, paid: Paid) -> RefundOutcome {
    let trigger = (paid.pick, RefundTrigger::Rule(RefundReason::Unreachable));
    request_refund(pool, stripe, &secret(), trigger, at(0))
        .await
        .expect("refund")
}

/// One request case: the pick, Stripe's answer, its re-reads, the ledger `refund` amounts and
/// the unconfirmed alerts it must leave.
type Case = (
    Paid,
    StripeRefund,
    Vec<Result<StripeRefund, AdapterError>>,
    Vec<i64>,
    i64,
);

fn request_cases() -> [Case; 4] {
    let [a, b, c, d, ..] = PAID;
    let usd = |n| (Some(n), Some("usd"));
    let other = "pi_FakeT387other";
    [
        (
            a,
            refund(a.intent, "succeeded", NO_AMOUNT),
            vec![Ok(refund(a.intent, "pending", usd(2500)))],
            vec![7500],
            1,
        ),
        (
            b,
            refund(b.intent, "succeeded", NO_AMOUNT),
            vec![Ok(refund(other, "succeeded", usd(2500)))],
            vec![7500],
            1,
        ),
        (
            c,
            refund(c.intent, "succeeded", NO_AMOUNT),
            vec![Ok(refund(c.intent, "succeeded", usd(2500)))],
            vec![2500],
            0,
        ),
        (
            d,
            refund(d.intent, "succeeded", usd(7500)),
            vec![Ok(refund(d.intent, "succeeded", usd(2500)))],
            vec![7500],
            0,
        ),
    ]
}

/// TD-FEE-59 (FEE-026, FEE-028, FEE-053; D-2303): the request path's one re-read of a `succeeded`
/// answer with no amount. A re-read still `pending`, or `succeeded` for another PaymentIntent,
/// leaves the answer: booked at the $75 charge with one unconfirmed alert. A re-read `succeeded`
/// for the pick's own PaymentIntent replaces it (booked at its $25, no unconfirmed alert). An
/// answer with its amount is never re-read. Every arm of the re-read runs in this one binary.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_a_missing_amount_is_reread_once_and_only_a_matching_success_replaces_it(
    pool: PgPool,
) {
    for (paid, answer, reads, amounts, unconfirmed) in request_cases() {
        charged(&pool, paid).await;
        let stripe = FakeStripe::new(&[answer], reads);
        let outcome = requested(&pool, &stripe, paid).await;
        assert_eq!(outcome, RefundOutcome::Refunded(REFUND.to_owned()));
        assert_eq!(booked(&pool, paid).await, amounts, "{}", paid.intent);
        let alerted = alerts(&pool, REFUND_UNCONFIRMED_ALERT, paid).await;
        assert_eq!(alerted, unconfirmed, "{}", paid.intent);
        let left = stripe.reads.lock().expect("lock").len();
        assert_eq!(
            left,
            usize::from(paid.intent == PAID[3].intent),
            "{}",
            paid.intent
        );
    }
}

/// TD-FEE-59 (FEE-026, FEE-028; D-2067): a `pending` answer is not re-read and books nothing; the
/// request is not refunded yet.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_a_pending_answer_is_not_reread(pool: PgPool) {
    let paid = PAID[4];
    charged(&pool, paid).await;
    let reread = refund(paid.intent, "succeeded", (Some(7500), Some("usd")));
    let stripe = FakeStripe::new(
        &[refund(paid.intent, "pending", NO_AMOUNT)],
        vec![Ok(reread)],
    );
    assert_eq!(requested(&pool, &stripe, paid).await, RefundOutcome::Failed);
    assert!(booked(&pool, paid).await.is_empty());
    assert_eq!(stripe.reads.lock().expect("lock").len(), 1);
}

/// A `succeeded` refund row for the pick whose ledger has no `refund` entry (as when the ledger
/// write is still to come or was reversed), so `refund_in_full` must ask Stripe.
async fn succeeded_without_entry(pool: &PgPool, paid: Paid) {
    charged(pool, paid).await;
    sqlx::query(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, stripe_refund_id, requested_at,
           decided_at) VALUES ($1, 'unreachable', 'succeeded', 1, $2, to_timestamp($3), to_timestamp($3))",
    )
    .bind(paid.pick)
    .bind(REFUND)
    .bind(NOW)
    .execute(pool)
    .await
    .expect("refund row");
}

/// TD-FEE-59 (FEE-026, FEE-053; D-2303, D-3018): `refund_in_full`. No succeeded refund is
/// `true`. With no ledger refund to judge by it takes Stripe's re-read: the whole charge in `usd`
/// is `true`, the charge in `eur` `false`, and a read error other than `NotSent` is
/// `MoneyEventError::Stripe`, never a guessed `true`. A ledger refund at the charge still reads
/// Stripe (`NotSent` is `true`); one at another amount is `false` with no read. Every arm runs in
/// this one binary.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_59_refund_in_full_judges_the_ledger_then_stripe(pool: PgPool) {
    let [first, full, part, .., unrefunded] = PAID;
    charged(&pool, unrefunded).await;
    let none = FakeStripe::default();
    assert_eq!(
        refund_in_full(&pool, &none, unrefunded.pick).await,
        Ok(true)
    );
    succeeded_without_entry(&pool, first).await;
    assert!(booked(&pool, first).await.is_empty());
    let stripe = FakeStripe::new(
        &[],
        vec![
            Ok(refund(first.intent, "succeeded", (Some(7500), Some("usd")))),
            Ok(refund(first.intent, "succeeded", (Some(7500), Some("eur")))),
            Err(AdapterError::OutcomeUnknown),
        ],
    );
    assert_eq!(refund_in_full(&pool, &stripe, first.pick).await, Ok(true));
    assert_eq!(refund_in_full(&pool, &stripe, first.pick).await, Ok(false));
    assert_eq!(
        refund_in_full(&pool, &stripe, first.pick).await,
        Err(MoneyEventError::Stripe)
    );
    for (paid, amount, in_full) in [(full, 7500, true), (part, 2500, false)] {
        charged(&pool, paid).await;
        let answer = refund(paid.intent, "succeeded", (Some(amount), Some("usd")));
        let sent = FakeStripe::new(&[answer], Vec::new());
        assert_eq!(
            requested(&pool, &sent, paid).await,
            RefundOutcome::Refunded(REFUND.to_owned())
        );
        assert_eq!(booked(&pool, paid).await, [amount]);
        assert_eq!(refund_in_full(&pool, &sent, paid.pick).await, Ok(in_full));
    }
}

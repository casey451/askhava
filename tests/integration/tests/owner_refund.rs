//! The owner portal's refund review step and audit log viewer (T205; TD-FEE-14, TD-FEE-74;
//! FEE-026, FEE-050, SEC-006, SEC-043; D-229, D-3883, D-4481, D-4506).
//!
//! A replayed owner refund press sends one refund and books one ledger row and one owner audit
//! row; a disputed charge's refund is blocked and sends nothing; the review step of pick P lists
//! only P's disputes; an evidence press on P naming Q's dispute changes nothing and writes no audit
//! row or countering fee; the press on P's own dispute records once with one audit row about P.
//! PostgreSQL through `#[sqlx::test]`, a fake Stripe, fake IDs only; no contact detail is seeded.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    DisputeOpened, OwnerSubmission, RefundOutcome, StripeRefund, record_dispute,
};
use askhava_portals::api::{
    CookieJar, EvidenceForm, OWNER_EVIDENCE_ACTION, OWNER_REFUND_ACTION, OwnerAuth, OwnerPrincipal,
    OwnerToolError, ReviewNotice, SecondFactor, bootstrap_owner, owner_audit_log_page,
    owner_dispute_evidence, owner_principal, owner_refund, owner_refund_page, owner_sign_in,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PASSWORD: &str = "fake owner pass phrase 2026";
const TOKEN: &str = "fake-bootstrap-token-0123456789abcdef";
const REFUND_BUTTON: &str = "Refund this charge";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_010).expect("time") + SignedDuration::from_secs(secs)
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t205".to_owned())
}

/// Takes every refund with `succeeded` for the asked intent and counts the calls.
#[derive(Default)]
struct FakeStripe {
    sent: Mutex<Vec<String>>,
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
    async fn send_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<StripeRefund, AdapterError> {
        self.sent.lock().expect("lock").push(intent.to_owned());
        Ok(StripeRefund {
            id: "re_FakeT205".to_owned(),
            status: "succeeded".to_owned(),
            amount: Some(7500),
            currency: Some("usd".to_owned()),
            payment_intent: Some(intent.to_owned()),
        })
    }
    async fn read_refund(&self, _: &str) -> Result<StripeRefund, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The bootstrapped owner signed in at 0 with a TOTP code; the session's jar.
async fn signed_in(db: &mut PgConnection) -> CookieJar {
    bootstrap_owner(db, Some(TOKEN), PASSWORD, at(0))
        .await
        .expect("bootstrap");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let code = totp_code(&secret, at(0));
    let auth = OwnerAuth::new("askhava.test", "https://askhava.test").expect("relying party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    owner_sign_in(db, CookieJar::new(), factor, &auth, at(0))
        .await
        .expect("signed in")
}

/// The owner of `jar` read back at `t` seconds.
async fn owner(db: &mut PgConnection, jar: &CookieJar, t: i64) -> OwnerPrincipal {
    owner_principal(db, jar, at(t))
        .await
        .expect("read")
        .expect("live")
}

/// A fake paid $75 pick with its FEE-042 evidence record; the pick and its intent.
async fn charged(pool: &PgPool, tag: &str) -> (Uuid, String) {
    let (pick, business, job) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let intent = format!("pi_FakeT205{tag}");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT205', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(&intent)
            .bind(at(-86_400).as_second())
            .execute(pool)
            .await
            .expect("seed");
    }
    (pick, intent)
}

/// Opens dispute `id` on `intent`, due in 10 days.
async fn open(pool: &PgPool, id: &str, intent: &str) {
    let event = DisputeOpened {
        dispute: id,
        payment_intent: intent,
        amount_cents: 7500,
        due_at: at(10 * 86_400),
    };
    assert_eq!(record_dispute(pool, &event, at(0)).await, Ok(true));
}

/// Ledger rows of `kind`.
async fn ledger_rows(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM payments.ledger_entry WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Owner audit rows of `action` about `pick`.
async fn owner_rows(pool: &PgPool, action: &str, pick: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action = $1 AND actor_kind = 'owner'
            AND subject_kind = 'picks.pick' AND subject_id = $2",
    )
    .bind(action)
    .bind(pick)
    .fetch_one(pool)
    .await
    .expect("audit")
}

/// `submitted_at` (epoch), `submitted_by` and `auto_submit` of a dispute.
async fn state(pool: &PgPool, id: &str) -> (Option<i64>, Option<String>, bool) {
    sqlx::query_as(
        "SELECT extract(epoch FROM submitted_at)::bigint, submitted_by, auto_submit
           FROM payments.dispute WHERE stripe_dispute_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("state")
}

/// TD-FEE-14 (FEE-026, SEC-006, SEC-043): a stale factor refunds nothing; a replayed owner press
/// gives one Stripe refund, one ledger row and one owner audit row about the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_a_replayed_owner_refund_gives_one_refund(pool: PgPool) {
    let (pick, _) = charged(&pool, "R").await;
    let stripe = FakeStripe::default();
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let stale = owner(&mut db, &jar, 301).await;
    let refused = owner_refund(&pool, (&stripe, &secret()), &stale, pick, at(301)).await;
    assert_eq!(refused, Err(OwnerToolError::NeedsFreshFactor));
    assert!(stripe.sent.lock().expect("lock").is_empty());
    step_up(&mut db, 300).await;
    let fresh = owner(&mut db, &jar, 310).await;
    let page = owner_refund_page(&mut db, "fake-csrf", pick, None)
        .await
        .expect("page");
    assert!(page.contains(REFUND_BUTTON));
    let first = owner_refund(&pool, (&stripe, &secret()), &fresh, pick, at(310)).await;
    assert_eq!(first, Ok(RefundOutcome::Refunded("re_FakeT205".to_owned())));
    let again = owner_refund(&pool, (&stripe, &secret()), &fresh, pick, at(320)).await;
    assert_eq!(again, Ok(RefundOutcome::Unchanged));
    assert_eq!(
        stripe.sent.lock().expect("lock").len(),
        1,
        "one Stripe refund"
    );
    assert_eq!(ledger_rows(&pool, "refund").await, 1, "one ledger row");
    assert_eq!(owner_rows(&pool, OWNER_REFUND_ACTION, pick).await, 1);
    let notice = Some(ReviewNotice::Refund(RefundOutcome::Unchanged));
    let page = owner_refund_page(&mut db, "fake-csrf", pick, notice)
        .await
        .expect("page");
    assert!(page.contains("Refunded in full.") && !page.contains(REFUND_BUTTON));
}

/// The session's second factor moved to `t` seconds, as a fresh step-up would.
async fn step_up(db: &mut PgConnection, t: i64) {
    sqlx::query("UPDATE portals.owner_session SET factor_at = $1::timestamptz")
        .bind(at(t).to_string())
        .execute(&mut *db)
        .await
        .expect("fresh factor");
}

/// TD-FEE-14 (FEE-026): a disputed charge's review step offers no refund, and an owner press is
/// `Blocked`: nothing goes to Stripe and no refund row is booked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_14_a_disputed_charge_refund_is_blocked(pool: PgPool) {
    let (pick, intent) = charged(&pool, "D").await;
    open(&pool, "dp_FakeT205D", &intent).await;
    let stripe = FakeStripe::default();
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let page = owner_refund_page(&mut db, "fake-csrf", pick, None)
        .await
        .expect("page");
    assert!(page.contains("disputed") && !page.contains(REFUND_BUTTON));
    let fresh = owner(&mut db, &jar, 60).await;
    let press = owner_refund(&pool, (&stripe, &secret()), &fresh, pick, at(60)).await;
    assert_eq!(press, Ok(RefundOutcome::Blocked));
    assert!(stripe.sent.lock().expect("lock").is_empty(), "nothing sent");
    assert_eq!(ledger_rows(&pool, "refund").await, 0);
}

/// An evidence press on `pick` naming `dispute`, by a fresh owner.
async fn press(pool: &PgPool, jar: &CookieJar, pick: Uuid, dispute: &str) -> OwnerSubmission {
    let mut db = pool.acquire().await.expect("db");
    let fresh = owner(&mut db, jar, 60).await;
    let form = EvidenceForm {
        dispute: dispute.to_owned(),
    };
    owner_dispute_evidence(&mut db, &fresh, (pick, &form), at(60))
        .await
        .expect("press")
}

/// TD-FEE-74 (FEE-050, SEC-006, D-229, D-4506): P's review step lists P's dispute, not Q's; a
/// press on P naming Q's is refused and changes nothing; P's own records once with one owner
/// audit row about P and one countering fee, and a replay writes neither again. The audit log
/// viewer shows the row with its owner actor.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_74_the_evidence_press_is_scoped_to_its_pick(pool: PgPool) {
    let ((p, p_intent), (q, q_intent)) = (charged(&pool, "P").await, charged(&pool, "Q").await);
    open(&pool, "dp_FakeT205P", &p_intent).await;
    open(&pool, "dp_FakeT205Q", &q_intent).await;
    sqlx::query("UPDATE payments.dispute SET auto_submit = true WHERE pick_id = $1")
        .bind(q)
        .execute(&pool)
        .await
        .expect("Q marked for auto-submit");
    let mut db = pool.acquire().await.expect("db");
    let jar = signed_in(&mut db).await;
    let page = owner_refund_page(&mut db, "fake-csrf", p, None)
        .await
        .expect("page");
    assert!(page.contains("dp_FakeT205P") && !page.contains("dp_FakeT205Q"));
    assert!(page.contains("I submitted this dispute's evidence in Stripe myself"));
    let before = state(&pool, "dp_FakeT205Q").await;
    let foreign = press(&pool, &jar, p, "dp_FakeT205Q").await;
    assert_eq!(foreign, OwnerSubmission::NotThisPick);
    assert_eq!(state(&pool, "dp_FakeT205Q").await, before);
    assert_eq!(before, (None, None, true));
    assert_eq!(owner_rows(&pool, OWNER_EVIDENCE_ACTION, p).await, 0);
    assert_eq!(owner_rows(&pool, OWNER_EVIDENCE_ACTION, q).await, 0);
    assert_eq!(ledger_rows(&pool, "countering-fee").await, 0);
    let notice = Some(ReviewNotice::Evidence(foreign));
    let page = owner_refund_page(&mut db, "fake-csrf", p, notice)
        .await
        .expect("page");
    assert!(page.contains("Error: That dispute is not on this charge."));
    for answer in [OwnerSubmission::Recorded, OwnerSubmission::AlreadyIn] {
        assert_eq!(press(&pool, &jar, p, "dp_FakeT205P").await, answer);
        assert_eq!(owner_rows(&pool, OWNER_EVIDENCE_ACTION, p).await, 1);
        assert_eq!(ledger_rows(&pool, "countering-fee").await, 1);
    }
    let log = owner_audit_log_page(&mut db, 0).await.expect("log");
    assert!(log.contains(OWNER_EVIDENCE_ACTION) && log.contains(&format!("picks.pick {p}")));
    assert!(log.contains("<td>Owner</td>"));
}

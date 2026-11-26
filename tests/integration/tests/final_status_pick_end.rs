//! A job's final status ends its waiting picks (T744; FEE-021, PICK-038, QH-007, PICK-048; D-1576,
//! D-2958, D-4748, D-5664), on PostgreSQL. Fake IDs, amounts and Stripe objects only.
//!
//! A job with a pick awaiting Confirm or in Payment pending is cancelled; its `jobs.final_status`
//! task runs through `picks`' handler, which first expires the job's texts queued before the close
//! (the payload's `at`), then ends the pick as `job_final` and its PaymentIntent through
//! `payments`' own pick end. Stripe is a scripted stand-in: each fetch answers `succeeded` and
//! refunds are counted. The webhook is signed with a fake secret.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, Delivery, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
    WebhookVendor, verify_webhook,
};
use askhava_core::api::{Claimed, Guarded, MIGRATOR, WorkerId, claim};
use askhava_jobs::api::JobStatus::{
    AwaitingConfirm, Cancelled, Connected, Done, Intake, Open, PaymentPending, QuotesReady,
};
use askhava_jobs::api::{FINAL_STATUS_KIND, JobStatus, current, move_to};
use askhava_messaging::api::{NewText, Relation, enqueue};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    LATE_KIND, PICK_ENDED_KIND, ReleaseError, StripeAccount, VerifiedStripeEvent, paid_release,
    run_charge_check, run_late_refund,
};
use askhava_picks::api::{
    INACTIVE_TEMPLATE, WindowRules, run_confirm_ends_at, run_confirm_reminder_at, run_final_status,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const INTENT: &str = "pi_FakeT744";
const WEBHOOK_SECRET: &str = "whsec_fake_t744_not_a_real_secret";
const BASE: i64 = 1_793_000_000;

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(BASE + minute * 60).expect("time")
}

/// The scripted Stripe stand-in: every fetch is `succeeded` for `pick`; refunds are counted.
struct FakeStripe {
    pick: Uuid,
    refunds: Mutex<u32>,
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

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        assert_eq!(id, INTENT);
        Ok(PaymentIntent {
            id: INTENT.to_owned(),
            status: "succeeded".to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT744".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: Some("ch_FakeT744".to_owned()),
        })
    }

    async fn create_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        assert_eq!(intent, INTENT);
        *self.refunds.lock().expect("lock") += 1;
        Ok("re_FakeT744".to_owned())
    }
}

/// A job in Awaiting confirm whose one fake business quoted and whose pick has `status`
/// (`awaiting_confirm` or `paid`). Returns job, business, pick.
async fn picked(db: &mut PgConnection, status: &str) -> (Uuid, Uuid, Uuid) {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    walk(db, job, &[Intake, Open, QuotesReady, AwaitingConfirm], 1).await;
    let business = askhava_businesses::api::create(db, false, at(0))
        .await
        .expect("business");
    askhava_businesses::api::add_owner(db, business, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 12000, false, '', 'Fake quote', false, true, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(at(3).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(status)
    .bind(at(10).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    (job, business, pick)
}

/// Case A (D-5664): the pick's $75 PaymentIntent is stored, its one attempt `sent`, as a Confirm
/// press or a PICK-048 auto-confirm charge in flight leaves it.
async fn stored_intent(db: &mut PgConnection, (job, business, pick): (Uuid, Uuid, Uuid)) {
    let sqls = [
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($2, 'cus_FakeT744')",
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
         stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at) \
         VALUES ($1, 1, $2, 'sent', $5)",
    ];
    for sql in sqls {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(INTENT)
            .bind(at(20).to_sqlx())
            .execute(&mut *db)
            .await
            .expect("seed");
    }
}

/// The job's one `jobs.final_status` task, claimed.
async fn final_task(pool: &PgPool, job: Uuid) -> Claimed {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let claimed = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    let mine = |t: &Claimed| t.kind == FINAL_STATUS_KIND && t.payload["job"] == job.to_string();
    let mut tasks: Vec<Claimed> = claimed.expect("claim").into_iter().filter(mine).collect();
    assert_eq!(tasks.len(), 1, "one final-status task");
    tasks.remove(0)
}

/// The pick's status and end reason, and whether its PaymentIntent row is ended (none: no row).
async fn pick_state(pool: &PgPool, pick: Uuid) -> (String, Option<String>, Option<bool>) {
    sqlx::query_as(
        "SELECT p.status, p.end_reason, \
         CASE WHEN i.pick_id IS NOT NULL THEN i.ended_at IS NOT NULL END FROM picks.pick p \
         LEFT JOIN payments.pick_intent i ON i.pick_id = p.id WHERE p.id = $1",
    )
    .bind(pick)
    .fetch_one(pool)
    .await
    .expect("pick")
}

async fn count(pool: &PgPool, sql: &'static str, arg: String) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn tasks_of(pool: &PgPool, kind: &str) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE kind = $1";
    count(pool, sql, kind.to_owned()).await
}

async fn all_tasks(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE $1 <> ''";
    count(pool, sql, "all".to_owned()).await
}

fn ended() -> (String, Option<String>, Option<bool>) {
    let reason = Some("job_final".to_owned());
    ("cancelled".to_owned(), reason, Some(true))
}

/// A signature-checked `payment_intent.succeeded` for the pick's own intent.
fn succeeded() -> VerifiedStripeEvent {
    let body = json!({"id": "evt_FakeT744", "type": "payment_intent.succeeded", "livemode": false,
        "data": {"object": {"id": INTENT, "object": "payment_intent"}}})
    .to_string();
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
    mac.update(format!("{BASE}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = format!("t={BASE},v1={sig}");
    let headers = [("Stripe-Signature", header.as_str())];
    let delivery = Delivery {
        url: "https://hooks.example.test/webhooks/stripe",
        body: body.as_bytes(),
        headers: &headers,
    };
    let secret = SecretString::from(WEBHOOK_SECRET.to_owned());
    let hook = verify_webhook(WebhookVendor::Stripe, &secret, delivery, BASE).expect("signed");
    VerifiedStripeEvent::from_webhook(&hook).expect("event")
}

/// The late-success refund tasks run twice; Stripe is asked for exactly one refund.
async fn refunded_once(pool: &PgPool, stripe: &FakeStripe, pick: Uuid) {
    let late = tasks_of(pool, LATE_KIND).await;
    assert_eq!(late, 1, "one late-success refund task");
    let secret = SecretString::from("fake-id-secret-t744".to_owned());
    for minute in [90, 150] {
        run_late_refund(pool, stripe, &secret, &json!({"pick": pick}), at(minute))
            .await
            .expect("refund task");
    }
    assert_eq!(*stripe.refunds.lock().expect("lock"), 1);
}

/// A fake queued text about `job` from `template`, created at `created` (none: now).
async fn text(db: &mut PgConnection, job: Uuid, template: &str, created: Option<i64>) -> Uuid {
    let new = NewText {
        template_id: template,
        subject: &[job],
        recipient: Uuid::new_v4(),
        relation: Relation::OpenRequest,
        body: "fake body",
    };
    let secret = SecretString::from("fake-outbox-secret-t744".to_owned());
    let id = enqueue(db, &secret, &new).await.expect("queued").0;
    let sql = "UPDATE messaging.outbox SET created_at = COALESCE($2, created_at), \
               not_before = now() + interval '9 hours' WHERE id = $1";
    let created = created.map(|m| at(m).to_sqlx());
    sqlx::query(sql)
        .bind(id)
        .bind(created)
        .execute(&mut *db)
        .await
        .expect("held");
    id
}

async fn outbox_status(pool: &PgPool, id: Uuid) -> String {
    let sql = "SELECT status FROM messaging.outbox WHERE id = $1::uuid";
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row")
}

/// Moves `job` along `path` at `minute` (50: the close's time); each move must apply.
async fn walk(db: &mut PgConnection, job: Uuid, path: &[JobStatus], minute: i64) {
    for arrow in path.windows(2) {
        let applied = move_to(db, job, (arrow[0], arrow[1]), at(minute), Actor::System).await;
        assert!(matches!(applied, Ok(Guarded::Applied(()))), "move");
    }
}

/// Runs the final-status handler on `task`; it must succeed.
async fn run(pool: &PgPool, task: &Claimed) {
    let done = run_final_status(pool.clone(), task.clone()).await;
    done.expect("handled");
}

fn stripe(pick: Uuid) -> FakeStripe {
    let refunds = Mutex::new(0);
    FakeStripe { pick, refunds }
}

fn claimed(payload: Value) -> Claimed {
    let kind = String::new();
    Claimed {
        id: 1,
        kind,
        payload,
        attempts: 1,
    }
}

/// A job with a waiting pick whose intent is stored (case A), walked along `path`. Returns job, pick
/// and the final-status task.
async fn closed_with_intent(pool: &PgPool, path: &[JobStatus]) -> (Uuid, Uuid, Claimed) {
    let mut db = pool.acquire().await.expect("db");
    let ids = picked(&mut db, "awaiting_confirm").await;
    stored_intent(&mut db, ids).await;
    walk(&mut db, ids.0, path, 50).await;
    (ids.0, ids.2, final_task(pool, ids.0).await)
}

/// FEE-021, PICK-038, D-4748: the homeowner cancels while the pick awaits Confirm (no intent yet):
/// the pick ends as `job_final`; its queued window end and reminder then act on nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_cancel_awaiting_confirm_ends_the_pick_and_its_timers_act_on_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, _, pick) = picked(&mut db, "awaiting_confirm").await;
    let from = current(&mut db, job).await.expect("job").timer_from;
    walk(&mut db, job, &[AwaitingConfirm, Cancelled], 50).await;
    run(&pool, &final_task(&pool, job).await).await;
    let state = ("cancelled".to_owned(), Some("job_final".to_owned()), None);
    assert_eq!(pick_state(&pool, pick).await, state);
    let before = all_tasks(&pool).await;
    let window = json!({"pick": pick, "from": from.as_second(), "due": at(70).as_second()});
    let ends = run_confirm_ends_at(pool.clone(), claimed(window), at(70)).await;
    ends.expect("window end");
    let remind = claimed(json!({"pick": pick, "start": from.as_second()}));
    let rules = WindowRules::default();
    let reminded = run_confirm_reminder_at(pool.clone(), rules, remind, at(60)).await;
    reminded.expect("reminder");
    assert_eq!(pick_state(&pool, pick).await, state, "nothing acted");
    assert_eq!(all_tasks(&pool).await, before, "nothing queued after");
    let sql = "SELECT count(*) FROM picks.confirm_reminder WHERE pick_id = $1::text::uuid";
    let reminders = count(&pool, sql, pick.to_string()).await;
    assert_eq!(reminders, 0, "no reminder sent");
}

/// FEE-021, D-2958: a cancel in Payment pending ends the pick and its PaymentIntent once; a late
/// `payment_intent.succeeded` never releases and is refunded once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_cancel_in_payment_pending_ends_the_intent_and_refunds_a_late_success(pool: PgPool) {
    let path = [AwaitingConfirm, PaymentPending, Cancelled];
    let (_, pick, task) = closed_with_intent(&pool, &path).await;
    run(&pool, &task).await;
    run(&pool, &task).await;
    assert_eq!(pick_state(&pool, pick).await, ended());
    assert_eq!(tasks_of(&pool, PICK_ENDED_KIND).await, 1, "ended once");
    let (stripe, live) = (stripe(pick), false);
    let account = StripeAccount {
        live,
        account: "acct_testfake".to_owned(),
    };
    let cleared = paid_release(&pool, &stripe, &account, &succeeded(), pick, at(60)).await;
    assert_eq!(cleared.err(), Some(ReleaseError::PickEnded));
    refunded_once(&pool, &stripe, pick).await;
}

/// PICK-048, FEE-021, D-5664 case A: a close while an auto-confirm charge is in flight on the
/// `awaiting_confirm` pick, its intent stored, ends the pick and the intent; the charge that
/// still succeeds, found by the check round, is refunded once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_close_during_an_auto_confirm_charge_refunds_its_success(pool: PgPool) {
    let (_, pick, task) = closed_with_intent(&pool, &[AwaitingConfirm, Cancelled]).await;
    run(&pool, &task).await;
    assert_eq!(pick_state(&pool, pick).await, ended());
    let stripe = stripe(pick);
    let check = json!({"pick": pick, "try": 1, "round": 0});
    let checked = run_charge_check(&pool, &stripe, &check, at(60)).await;
    checked.expect("check");
    refunded_once(&pool, &stripe, pick).await;
}

/// FEE-021, D-1576: a paid pick is untouched by the final status: no end, its intent left as is.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_paid_pick_is_untouched(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let ids = picked(&mut db, "paid").await;
    stored_intent(&mut db, ids).await;
    walk(
        &mut db,
        ids.0,
        &[AwaitingConfirm, PaymentPending, Connected, Done],
        50,
    )
    .await;
    run(&pool, &final_task(&pool, ids.0).await).await;
    let untouched = ("paid".to_owned(), None, Some(false));
    assert_eq!(pick_state(&pool, ids.2).await, untouched);
    assert_eq!(tasks_of(&pool, PICK_ENDED_KIND).await, 0);
}

/// QH-007, D-1576, D-5664: the first delivery expires a text queued before the close; a replay
/// changes nothing, and the held "no longer active" row and a later text the first delivery's
/// time let through both survive it.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_replay_changes_nothing_and_keeps_the_held_notice(pool: PgPool) {
    let (job, pick, task) = closed_with_intent(&pool, &[AwaitingConfirm, Cancelled]).await;
    let mut db = pool.acquire().await.expect("db");
    let early = text(&mut db, job, "first_quote", Some(40)).await;
    assert_eq!(
        task.payload["at"],
        at(50).to_string(),
        "the close time travels"
    );
    run(&pool, &task).await;
    assert_eq!(outbox_status(&pool, early).await, "expired");
    let held = text(&mut db, job, INACTIVE_TEMPLATE, None).await;
    let later = text(&mut db, job, "first_quote", None).await;
    let before = (all_tasks(&pool).await, pick_state(&pool, pick).await);
    run(&pool, &task).await;
    let after = (all_tasks(&pool).await, pick_state(&pool, pick).await);
    assert_eq!(after, before);
    assert_eq!(before.1, ended());
    for id in [held, later] {
        assert_eq!(outbox_status(&pool, id).await, "queued", "kept");
    }
}

/// D-5664: a payload with no `at` (queued before T744) or an unreadable one expires nothing, never
/// using the handler's clock, and still ends the pick and tells the quoter.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_payload_without_a_close_time_expires_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, _, pick) = picked(&mut db, "awaiting_confirm").await;
    walk(&mut db, job, &[AwaitingConfirm, Cancelled], 50).await;
    let early = text(&mut db, job, "first_quote", Some(40)).await;
    let mut task = final_task(&pool, job).await;
    task.payload.as_object_mut().expect("object").remove("at");
    run(&pool, &task).await;
    task.payload["at"] = json!("not a time");
    run(&pool, &task).await;
    assert_eq!(outbox_status(&pool, early).await, "queued");
    let reason = pick_state(&pool, pick).await.1;
    assert_eq!(reason.as_deref(), Some("job_final"));
    let sql = "SELECT count(*) FROM core.task WHERE payload->>'template' = $1";
    let told = count(&pool, sql, INACTIVE_TEMPLATE.to_owned()).await;
    assert_eq!(told, 1, "told once");
}

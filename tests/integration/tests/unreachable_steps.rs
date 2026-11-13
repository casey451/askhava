//! TD-JOB-14 (T281, D-1845, D-2204; FEE-022, FEE-035, FEE-052, JOB-013, JOB-027, TXT-026,
//! TXT-030): the `picks.unreachable_step` and `picks.fraud_warning` handlers run against
//! PostgreSQL with the clock passed in (advanced by hand, no sleeps). Every row is fake: a fake
//! business, a fake vault contact with a 555 number, fake Stripe IDs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::{Claimed, Guarded};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_payments::api::FRAUD_WARNING_KIND;
use askhava_picks::api::{
    ContactChoice, RELEASE_STEPS, Step, StepParts, UNREACHABLE_STEP_KIND, record_contact_choice,
    run_fraud_warning, run_unreachable_step, schedule_steps,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Refunds succeed when `.0`, else the request is never sent; nothing else is called.
struct FakeStripe(bool);

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
        if self.0 {
            Ok("re_FakeT281".to_owned())
        } else {
            Err(AdapterError::NotSent)
        }
    }
}

/// Fake 555 number prefixes; each seeded contact adds two digits (a phone is unique in the vault).
const PHONE: &str = "+192855501";
const UNTEXTABLE: &str = "+199955501";

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://fake@localhost/fake".to_owned());
    Vault::new(&key(0x28), &key(0x29), &url).expect("vault")
}

fn parts(refunds: bool) -> Arc<StepParts<FakeStripe>> {
    let postal = askhava_messaging::api::PostalAddress::new(
        "PO Box 1234, Lake Havasu City, AZ 86405",
        askhava_messaging::api::MailboxKind::PoBox,
    )
    .expect("postal");
    Arc::new(StepParts {
        stripe: Some(FakeStripe(refunds)),
        secret: SecretString::from("test-only-id-secret-t281-000".to_owned()),
        vault: Some(vault()),
        postal: Some(postal),
    })
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A seeded pick, its job, its masked line and her vault contact.
struct Case {
    pick: Uuid,
    job: Uuid,
    line: Uuid,
    contact: Uuid,
    phone: String,
}

/// How to seed: the job's status, emergency, her email (and email contact choice), her phone and
/// whether the pick was released.
struct Seed {
    status: JobStatus,
    emergency: bool,
    email: bool,
    phone: &'static str,
    released: bool,
}

const REGULAR: Seed = Seed {
    status: JobStatus::Connected,
    emergency: false,
    email: false,
    phone: PHONE,
    released: true,
};

/// The release time of [`REGULAR`]: November 2, 2026, 9:00 a.m. Arizona time.
fn released_at() -> Timestamp {
    az(2, 9, 0)
}

/// A fake business with a public name and her fake vault contact (a fresh 555 number).
async fn people(db: &mut sqlx::PgConnection, seed: &Seed) -> (Uuid, Uuid, String) {
    let at = az(1, 9, 0);
    let business = askhava_businesses::api::create(db, false, at)
        .await
        .expect("business");
    askhava_businesses::api::save_business_name(db, business, ("Fake Plumbing", None), None)
        .await
        .expect("name");
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let phone = format!("{}{n:02}", seed.phone);
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(phone.clone())),
        email: seed.email.then(|| SecretString::from("dana@example.test")),
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    (business, contact, phone)
}

/// A job moved through its arrows to `seed.status`.
async fn job_at(db: &mut sqlx::PgConnection, seed: &Seed) -> Uuid {
    let at = az(1, 9, 0);
    let job = askhava_jobs::api::create(db, at, seed.emergency, Actor::System)
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
    for pair in path.windows(2).take_while(|p| p[0] != seed.status) {
        let moved = move_to(db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
    job
}

/// The paid pick of `business` on `job` with its fake $75 charge.
async fn paid_pick(db: &mut sqlx::PgConnection, (job, business): (Uuid, Uuid)) -> Uuid {
    let at = az(1, 9, 0);
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, 'pi_FakeT281' || replace($1::text, '-', ''), false, $4)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $4, $4)",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT281', 'pi_FakeT281' || replace($1::text, '-', ''), 1, 7500, $4, $4)",
    ] {
        sqlx::query(sql)
            .bind(pick)
            .bind(business)
            .bind(job)
            .bind(at.to_sqlx())
            .execute(&mut *db)
            .await
            .expect("seed");
    }
    pick
}

/// A business, a vault contact, a job moved to `seed.status`, its paid pick and charge, its line
/// and, when released, its release row and contact choice.
async fn seeded(pool: &PgPool, seed: Seed) -> Case {
    let at = az(1, 9, 0);
    let mut db = pool.acquire().await.expect("conn");
    let (business, contact, phone) = people(&mut db, &seed).await;
    let job = job_at(&mut db, &seed).await;
    let pick = paid_pick(&mut db, (job, business)).await;
    let line: Uuid = sqlx::query_scalar("INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($1, $2, $3, '+19285550199', false, $4) RETURNING id")
        .bind(job).bind(contact).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("line");
    if seed.released {
        sqlx::query("INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, $6)")
            .bind(pick).bind(job).bind(business).bind(released_at().to_sqlx()).bind(contact).bind(seed.email)
            .execute(&mut *db).await.expect("release");
        let choice = if seed.email {
            ContactChoice::Email
        } else {
            ContactChoice::Call
        };
        record_contact_choice(&mut db, pick, choice, at)
            .await
            .expect("choice");
    }
    Case {
        pick,
        job,
        line,
        contact,
        phone,
    }
}

/// Logs one finished business call on the line, answered by `who`, at `at` (no recorded start).
async fn log_call(pool: &PgPool, case: &Case, who: &str, at: Timestamp) {
    sqlx::query("INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, length_seconds, reached_45s, ended_at) VALUES ($1, 'CA' || md5($1::text || $2::text), 'business', $3, 20, false, $2)")
        .bind(case.line).bind(at.to_sqlx()).bind(who).execute(pool).await.expect("call");
}

fn task(kind: &str, payload: serde_json::Value) -> Claimed {
    Claimed {
        id: 0,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

async fn step(
    pool: &PgPool,
    parts: &Arc<StepParts<FakeStripe>>,
    case: &Case,
    step: Step,
    now: Timestamp,
) {
    let claimed = task(
        UNREACHABLE_STEP_KIND,
        json!({"pick": case.pick, "step": step.code()}),
    );
    let ran = run_unreachable_step(pool.clone(), Arc::clone(parts), claimed, now).await;
    assert_eq!(ran, Ok(()));
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("conn");
    current(&mut db, job).await.expect("job").status.code()
}

async fn pick_status(pool: &PgPool, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("pick")
}

const STEPS_DUE: &str = "SELECT count(*) FROM core.task WHERE kind = 'picks.unreachable_step' AND payload->>'pick' = $1::text";
const REMINDERS: &str =
    "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%no_call_reminder.' || $1::text || '.%'";
const MISSED_TEXT: &str = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'caller_missed' AND recipient_id = $1";
const MISSED_EMAIL: &str = "SELECT count(*) FROM messaging.email_outbox WHERE template_id = 'caller_missed' AND recipient_id = $1";
const QUESTION: &str = "SELECT count(*) FROM core.task WHERE kind = 'jobs.homeowner_text' AND payload->>'template' = 'emailed_question' AND payload->>'job' = $1::text";
const DECISION_EMAIL: &str =
    "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%.unreachable_refunded.' || $1::text";

/// TD-JOB-14: the release's transaction queues the steps at + 24, + 48, + 72 and + 96 hours,
/// each once, however often it runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_a_release_queues_each_step_once(pool: PgPool) {
    let case = seeded(&pool, REGULAR).await;
    for _ in 0..2 {
        let mut tx = pool.begin().await.expect("tx");
        schedule_steps(&mut tx, case.pick, released_at())
            .await
            .expect("steps");
        tx.commit().await.expect("commit");
    }
    assert_eq!(count(&pool, STEPS_DUE, case.pick).await, 4);
    let due: Vec<Timestamp> = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT run_at FROM core.task WHERE kind = 'picks.unreachable_step' ORDER BY run_at",
    )
    .fetch_all(&pool)
    .await
    .expect("due")
    .into_iter()
    .map(|t| t.to_jiff())
    .collect();
    let hours: Vec<i64> = RELEASE_STEPS.iter().map(|(_, h)| *h).collect();
    let wanted: Vec<Timestamp> = hours
        .iter()
        .map(|h| released_at() + SignedDuration::from_hours(*h))
        .collect();
    assert_eq!(due, wanted);
    let release = include_str!("../../../app/picks/src/release.rs");
    assert!(
        release.contains("schedule_steps(&mut tx, pick, at)"),
        "the claim's transaction queues the steps"
    );
}

/// TD-JOB-14 (TXT-026): the reminders go at 24 and 48 hours, unless a business call came first.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_reminders_unless_a_business_call_came_first(pool: PgPool) {
    let p = parts(true);
    let silent = seeded(&pool, REGULAR).await;
    step(&pool, &p, &silent, Step::Reminders, az(3, 9, 0)).await;
    step(&pool, &p, &silent, Step::Reminders, az(4, 9, 0)).await;
    assert_eq!(count(&pool, REMINDERS, silent.pick).await, 2);
    let called = seeded(&pool, REGULAR).await;
    log_call(&pool, &called, "nobody", az(2, 10, 0)).await;
    step(&pool, &p, &called, Step::Reminders, az(3, 9, 0)).await;
    assert_eq!(count(&pool, REMINDERS, called.pick).await, 0);
    let between = seeded(&pool, REGULAR).await;
    step(&pool, &p, &between, Step::Reminders, az(3, 9, 0)).await;
    assert_eq!(count(&pool, REMINDERS, between.pick).await, 1);
    log_call(&pool, &between, "nobody", az(3, 15, 0)).await;
    step(&pool, &p, &between, Step::Reminders, az(4, 9, 0)).await;
    assert_eq!(count(&pool, REMINDERS, between.pick).await, 1);
}

/// TD-JOB-14 (TXT-030, QH-004): a first unanswered call at 9 p.m. queues the missed-call text
/// only at 8 a.m. Arizona time on a regular job, and at once on an emergency; a number that can't
/// be texted gets the email copy instead, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_the_missed_call_text_at_its_send_time(pool: PgPool) {
    let p = parts(true);
    let regular = seeded(&pool, REGULAR).await;
    log_call(&pool, &regular, "nobody", az(2, 21, 0)).await;
    step(&pool, &p, &regular, Step::BusinessCall, az(2, 21, 1)).await;
    assert_eq!(count(&pool, MISSED_TEXT, regular.contact).await, 0);
    let held = "SELECT count(*) FROM core.task WHERE kind = 'picks.unreachable_step' AND payload->>'step' = 'business_call' AND run_at = $2 AND payload->>'pick' = $1::text";
    let at8: i64 = sqlx::query_scalar(held)
        .bind(regular.pick)
        .bind(az(3, 8, 0).to_sqlx())
        .fetch_one(&pool)
        .await
        .expect("held");
    assert_eq!(at8, 1);
    step(&pool, &p, &regular, Step::BusinessCall, az(3, 8, 0)).await;
    step(&pool, &p, &regular, Step::BusinessCall, az(3, 8, 5)).await;
    assert_eq!(count(&pool, MISSED_TEXT, regular.contact).await, 1);
    let urgent = seeded(
        &pool,
        Seed {
            emergency: true,
            ..REGULAR
        },
    )
    .await;
    log_call(&pool, &urgent, "machine", az(2, 21, 0)).await;
    step(&pool, &p, &urgent, Step::BusinessCall, az(2, 21, 1)).await;
    assert_eq!(count(&pool, MISSED_TEXT, urgent.contact).await, 1);
    let untextable = seeded(
        &pool,
        Seed {
            phone: UNTEXTABLE,
            email: true,
            ..REGULAR
        },
    )
    .await;
    log_call(&pool, &untextable, "nobody", az(2, 10, 0)).await;
    for _ in 0..2 {
        step(&pool, &p, &untextable, Step::BusinessCall, az(2, 10, 1)).await;
    }
    assert_eq!(count(&pool, MISSED_TEXT, untextable.contact).await, 0);
    assert_eq!(count(&pool, MISSED_EMAIL, untextable.contact).await, 1);
}

/// TD-JOB-14 (FEE-035, JOB-013, JOB-027): at + 72 hours a call-contact job with no call returns
/// to Quotes ready as a no-show; an email-contact job gets the emailed question once; its day-3
/// NO from T277 records the no-show (`Step::DayThreeNo`, D-3488).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_the_72_hour_step_by_contact(pool: PgPool) {
    let p = parts(true);
    let by_call = seeded(&pool, REGULAR).await;
    step(&pool, &p, &by_call, Step::SeventyTwo, az(5, 9, 0)).await;
    assert_eq!(status(&pool, by_call.job).await, "quotes_ready");
    assert_eq!(pick_status(&pool, by_call.pick).await, "no_show");
    let by_email = seeded(
        &pool,
        Seed {
            email: true,
            ..REGULAR
        },
    )
    .await;
    for _ in 0..2 {
        step(&pool, &p, &by_email, Step::SeventyTwo, az(5, 9, 0)).await;
    }
    assert_eq!(status(&pool, by_email.job).await, "connected");
    assert_eq!(count(&pool, QUESTION, by_email.job).await, 1);
    step(&pool, &p, &by_email, Step::DayThreeNo, az(5, 12, 0)).await;
    assert_eq!(status(&pool, by_email.job).await, "quotes_ready");
    assert_eq!(pick_status(&pool, by_email.pick).await, "no_show");
}

/// TD-JOB-14 (FEE-022, JOB-027): a failed refund at + 96 hours closes nothing and runs the step
/// again an hour later; that run refunds, closes and emails once; an email YES keeps the fee.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_a_waiting_refund_runs_again_and_decides_once(pool: PgPool) {
    let case = seeded(&pool, REGULAR).await;
    log_call(&pool, &case, "nobody", az(2, 10, 0)).await;
    log_call(&pool, &case, "nobody", az(2, 14, 0)).await;
    step(&pool, &parts(false), &case, Step::NinetySix, az(6, 9, 0)).await;
    assert_eq!(status(&pool, case.job).await, "connected");
    assert_eq!(count(&pool, DECISION_EMAIL, case.pick).await, 0);
    let again = "SELECT count(*) FROM core.task WHERE payload->>'step' = 'at_96' AND run_at = $2 AND payload->>'pick' = $1::text";
    let later: i64 = sqlx::query_scalar(again)
        .bind(case.pick)
        .bind(az(6, 10, 0).to_sqlx())
        .fetch_one(&pool)
        .await
        .expect("again");
    assert_eq!(later, 1);
    for _ in 0..2 {
        step(&pool, &parts(true), &case, Step::NinetySix, az(6, 10, 0)).await;
    }
    assert_eq!(status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, DECISION_EMAIL, case.pick).await, 1);
    assert_eq!(count(&pool, MISSED_TEXT, case.contact).await, 1);
    let agreed = seeded(
        &pool,
        Seed {
            email: true,
            ..REGULAR
        },
    )
    .await;
    log_call(&pool, &agreed, "nobody", az(2, 10, 0)).await;
    log_call(&pool, &agreed, "nobody", az(2, 14, 0)).await;
    step(
        &pool,
        &parts(true),
        &agreed,
        Step::EmailedAnswer(true),
        az(5, 12, 0),
    )
    .await;
    step(&pool, &parts(true), &agreed, Step::NinetySix, az(6, 9, 0)).await;
    assert_eq!(status(&pool, agreed.job).await, "connected");
    let refunds = "SELECT count(*) FROM payments.refund WHERE pick_id = $1";
    assert_eq!(count(&pool, refunds, agreed.pick).await, 0);
}

/// TD-JOB-14 (D-2204): a 96-hour run that refunded and closed but failed before the decision
/// email was queued is retried; the closed job still gets the email, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_a_retry_after_the_close_queues_the_decision_email_once(pool: PgPool) {
    let case = seeded(&pool, REGULAR).await;
    log_call(&pool, &case, "nobody", az(2, 10, 0)).await;
    log_call(&pool, &case, "nobody", az(2, 14, 0)).await;
    step(&pool, &parts(true), &case, Step::NinetySix, az(6, 9, 0)).await;
    assert_eq!(status(&pool, case.job).await, "closed");
    let lost = "DELETE FROM core.task WHERE dedupe_key LIKE '%.unreachable_refunded.' || $1::text";
    sqlx::query(lost)
        .bind(case.pick)
        .execute(&pool)
        .await
        .expect("lose the email");
    assert_eq!(count(&pool, DECISION_EMAIL, case.pick).await, 0);
    for _ in 0..2 {
        step(&pool, &parts(true), &case, Step::NinetySix, az(6, 9, 5)).await;
    }
    assert_eq!(count(&pool, DECISION_EMAIL, case.pick).await, 1);
    let refunds = "SELECT count(*) FROM payments.refund WHERE pick_id = $1";
    assert_eq!(count(&pool, refunds, case.pick).await, 1);
    step(&pool, &parts(true), &case, Step::Reminders, az(6, 9, 5)).await;
    assert_eq!(count(&pool, REMINDERS, case.pick).await, 0);
}

/// TD-JOB-14 (FEE-052, JOB-014): a fraud warning on an unreleased paid pick whose refund is in
/// flight changes nothing and runs again an hour later; once the refund succeeded the job returns
/// to Quotes ready and the pick ends fraud-refunded; a released pick changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_the_fraud_warning_task(pool: PgPool) {
    let unreleased = Seed {
        status: JobStatus::PaymentPending,
        released: false,
        ..REGULAR
    };
    let case = seeded(&pool, unreleased).await;
    sqlx::query("INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) VALUES ($1, 'fraud-warning', 'pending', 1, $2)")
        .bind(case.pick).bind(az(3, 9, 0).to_sqlx()).execute(&pool).await.expect("refund");
    let warn = |pick: Uuid, now: Timestamp| {
        let claimed = task(FRAUD_WARNING_KIND, json!({"pick": pick}));
        run_fraud_warning(pool.clone(), parts(true), claimed, now)
    };
    assert_eq!(warn(case.pick, az(3, 9, 0)).await, Ok(()));
    assert_eq!(status(&pool, case.job).await, "payment_pending");
    let rerun = "SELECT count(*) FROM core.task WHERE kind = 'picks.fraud_warning' AND run_at = $2 AND payload->>'pick' = $1::text";
    let later: i64 = sqlx::query_scalar(rerun)
        .bind(case.pick)
        .bind(az(3, 10, 0).to_sqlx())
        .fetch_one(&pool)
        .await
        .expect("rerun");
    assert_eq!(later, 1);
    sqlx::query("UPDATE payments.refund SET status = 'succeeded' WHERE pick_id = $1")
        .bind(case.pick)
        .execute(&pool)
        .await
        .expect("succeeded");
    assert_eq!(warn(case.pick, az(3, 10, 0)).await, Ok(()));
    assert_eq!(status(&pool, case.job).await, "quotes_ready");
    assert_eq!(pick_status(&pool, case.pick).await, "fraud_refunded");
    let released = seeded(&pool, REGULAR).await;
    assert_eq!(warn(released.pick, az(3, 9, 0)).await, Ok(()));
    assert_eq!(status(&pool, released.job).await, "connected");
    assert_eq!(pick_status(&pool, released.pick).await, "paid");
}

const REFUNDS: &str = "SELECT count(*) FROM payments.refund WHERE pick_id = $1";
const NOTICES: &str = "SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' AND payload->>'pick' = $1::text";

/// Two unanswered business calls 4 hours apart on `case`'s line.
async fn two_calls(pool: &PgPool, case: &Case) {
    log_call(pool, case, "nobody", az(2, 10, 0)).await;
    log_call(pool, case, "nobody", az(2, 14, 0)).await;
}

/// TD-JOB-14 (FEE-022, TXT-030, D-3487): the 96-hour step counts the missed-call notice only once
/// it is queued: at night it waits for 8 a.m., then queues the text and refunds; a number that
/// can't be texted, with no email on file, gets no refund.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_no_refund_without_a_queued_missed_call_notice(pool: PgPool) {
    let night = seeded(&pool, REGULAR).await;
    two_calls(&pool, &night).await;
    step(&pool, &parts(true), &night, Step::NinetySix, az(6, 21, 0)).await;
    assert_eq!(count(&pool, REFUNDS, night.pick).await, 0);
    assert_eq!(count(&pool, MISSED_TEXT, night.contact).await, 0);
    let again = "SELECT count(*) FROM core.task WHERE payload->>'step' = 'at_96' AND run_at = $2 AND payload->>'pick' = $1::text";
    let at8: i64 = sqlx::query_scalar(again)
        .bind(night.pick)
        .bind(az(7, 8, 0).to_sqlx())
        .fetch_one(&pool)
        .await
        .expect("at 8");
    assert_eq!(at8, 1);
    step(&pool, &parts(true), &night, Step::NinetySix, az(7, 8, 0)).await;
    assert_eq!(count(&pool, MISSED_TEXT, night.contact).await, 1);
    assert_eq!(status(&pool, night.job).await, "closed");
    let unreachable = seeded(
        &pool,
        Seed {
            phone: UNTEXTABLE,
            ..REGULAR
        },
    )
    .await;
    two_calls(&pool, &unreachable).await;
    step(
        &pool,
        &parts(true),
        &unreachable,
        Step::NinetySix,
        az(6, 9, 0),
    )
    .await;
    assert_eq!(count(&pool, REFUNDS, unreachable.pick).await, 0);
    assert_eq!(status(&pool, unreachable.job).await, "connected");
}

/// TD-JOB-14 (FEE-022, TXT-030, D-3490): a missed-call text queued at its step stays attempted:
/// she opts out on day 3 and the 96-hour run at night still refunds at once, with no late email
/// copy though an email is on file.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_a_queued_notice_counts_after_an_opt_out(pool: PgPool) {
    let p = parts(true);
    let case = seeded(
        &pool,
        Seed {
            email: true,
            ..REGULAR
        },
    )
    .await;
    two_calls(&pool, &case).await;
    step(&pool, &p, &case, Step::BusinessCall, az(2, 15, 0)).await;
    assert_eq!(count(&pool, MISSED_TEXT, case.contact).await, 1);
    let mut db = pool.acquire().await.expect("conn");
    let channel = (RevokeChannel::Portal, "portal");
    let listed = suppress(&mut db, &vault(), &case.phone, channel, az(4, 9, 0)).await;
    assert_eq!(listed, Ok(true));
    step(&pool, &p, &case, Step::NinetySix, az(6, 21, 0)).await;
    assert_eq!(count(&pool, REFUNDS, case.pick).await, 1);
    assert_eq!(status(&pool, case.job).await, "closed");
    assert_eq!(count(&pool, MISSED_TEXT, case.contact).await, 1);
    assert_eq!(count(&pool, MISSED_EMAIL, case.contact).await, 0);
    assert_eq!(count(&pool, DECISION_EMAIL, case.pick).await, 1);
}

/// TD-JOB-14 (FEE-022, JOB-027, D-3488): a NO to the 72-hour emailed question is only stored: the
/// pick stays, and the 96-hour step still refunds.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_a_no_to_the_emailed_question_keeps_the_refund(pool: PgPool) {
    let case = seeded(
        &pool,
        Seed {
            email: true,
            ..REGULAR
        },
    )
    .await;
    two_calls(&pool, &case).await;
    let no = Step::EmailedAnswer(false);
    step(&pool, &parts(true), &case, no, az(5, 12, 0)).await;
    assert_eq!(status(&pool, case.job).await, "connected");
    assert_eq!(pick_status(&pool, case.pick).await, "paid");
    step(&pool, &parts(true), &case, Step::NinetySix, az(6, 9, 0)).await;
    assert_eq!(count(&pool, REFUNDS, case.pick).await, 1);
    assert_eq!(status(&pool, case.job).await, "closed");
}

/// Ends `first`'s pick as a no-show and its line, then seeds a second business's paid, released
/// pick on the same job with her same contact and its own line (`email` sets its contact choice).
async fn next_pick(pool: &PgPool, first: &Case, email: bool) -> Case {
    let at = az(4, 9, 0);
    let mut db = pool.acquire().await.expect("conn");
    sqlx::query("UPDATE picks.pick SET status = 'no_show', ended_at = $2 WHERE id = $1")
        .bind(first.pick)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("end");
    sqlx::query("UPDATE calls.number_pool SET closed_at = $2 WHERE id = $1")
        .bind(first.line)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("close");
    let business = askhava_businesses::api::create(&mut db, false, at)
        .await
        .expect("business");
    askhava_businesses::api::save_business_name(&mut db, business, ("Fake Roofing", None), None)
        .await
        .expect("name");
    let pick = paid_pick(&mut db, (first.job, business)).await;
    let line: Uuid = sqlx::query_scalar("INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($1, $2, $3, '+19285550188', false, $4) RETURNING id")
        .bind(first.job).bind(first.contact).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("line");
    sqlx::query("INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, $6)")
        .bind(pick).bind(first.job).bind(business).bind(at.to_sqlx()).bind(first.contact).bind(email)
        .execute(&mut *db).await.expect("release");
    let choice = if email {
        ContactChoice::Email
    } else {
        ContactChoice::Call
    };
    record_contact_choice(&mut db, pick, choice, at)
        .await
        .expect("choice");
    Case {
        pick,
        job: first.job,
        line,
        contact: first.contact,
        phone: first.phone.clone(),
    }
}

/// TD-JOB-14 (JOB-027, D-3488): each email-contact pick on one job gets its own 72-hour question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_each_pick_on_a_job_gets_its_own_question(pool: PgPool) {
    let email = Seed {
        email: true,
        ..REGULAR
    };
    let first = seeded(&pool, email).await;
    step(&pool, &parts(true), &first, Step::SeventyTwo, az(5, 9, 0)).await;
    let second = next_pick(&pool, &first, true).await;
    step(&pool, &parts(true), &second, Step::SeventyTwo, az(7, 9, 0)).await;
    assert_eq!(count(&pool, QUESTION, first.job).await, 2);
}

/// TD-JOB-14 (PRIV-005, TXT-030, D-3486, D-3490): an ended pick's steps do nothing on the job's
/// next pick: no missed-call text from the next pick's calls, no question, no move; the next pick
/// reads only its own line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_an_ended_pick_never_acts_on_the_next_one(pool: PgPool) {
    let p = parts(true);
    let first = seeded(&pool, REGULAR).await;
    let second = next_pick(&pool, &first, false).await;
    log_call(&pool, &second, "nobody", az(4, 10, 0)).await;
    for old in [Step::BusinessCall, Step::SeventyTwo, Step::Reminders] {
        step(&pool, &p, &first, old, az(5, 10, 0)).await;
    }
    assert_eq!(count(&pool, MISSED_TEXT, first.contact).await, 0);
    assert_eq!(count(&pool, REMINDERS, first.pick).await, 0);
    assert_eq!(status(&pool, first.job).await, "connected");
    assert_eq!(pick_status(&pool, second.pick).await, "paid");
    step(&pool, &p, &second, Step::BusinessCall, az(5, 10, 0)).await;
    assert_eq!(count(&pool, MISSED_TEXT, second.contact).await, 1);
    let named = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'caller_missed' AND recipient_id = $1 AND body LIKE '%Fake Roofing%'";
    assert_eq!(count(&pool, named, second.contact).await, 1);
}

/// TD-JOB-14 (FEE-022, D-3490): a pick whose refund succeeded on a job that went to Done (her
/// DONE reply) gets no decision email from a later 96-hour step.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_14_no_decision_email_on_a_job_done(pool: PgPool) {
    let case = seeded(&pool, REGULAR).await;
    two_calls(&pool, &case).await;
    sqlx::query("INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) VALUES ($1, 'fraud-warning', 'succeeded', 1, $2)")
        .bind(case.pick).bind(az(5, 9, 0).to_sqlx()).execute(&pool).await.expect("refund");
    let mut db = pool.acquire().await.expect("conn");
    let arrow = (JobStatus::Connected, JobStatus::Done);
    let moved = move_to(&mut db, case.job, arrow, az(5, 10, 0), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    step(&pool, &parts(true), &case, Step::NinetySix, az(6, 9, 0)).await;
    assert_eq!(count(&pool, NOTICES, case.pick).await, 0);
    assert_eq!(status(&pool, case.job).await, "done");
}

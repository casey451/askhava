//! T191: TD-JOB-07, TD-TXT-10, TD-FEE-22, TD-TXT-13, TD-FEE-39 and TD-TXT-30 (FEE-022, FEE-050,
//! FEE-052, JOB-013, JOB-014, JOB-027, TXT-026, TXT-030, QH-004; D-142, D-143, D-236, D-243,
//! D-286, D-336, D-440, D-491): the call log read as the refund rule reads it, the reminder and
//! missed-call times under quiet hours, the 96-hour decision with its refund, close and texts, the
//! 72-hour no-call return, the dispute auto-submit mark and the fraud-warning end. Stripe is a
//! scripted fake; every ID, name and number is fake.

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_calls::api::{AnsweredBy, Caller as CallCaller, LoggedCall};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::{
    AreaCodes, IMPORTANT_TEMPLATES, Kind, Language, SendWindow, keyword, render,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    CallRecord, Contact, DisputeOpened, EmailCheck, RefundDecision, RefundReason, RefundTrigger,
    Unmet, UnreachableFacts, record_dispute, request_refund,
};
use askhava_picks::api::{
    AtNinetySix, MISSED_CALL_TEMPLATE, REMINDER_EMAIL_TEMPLATE, REMINDER_TEMPLATE, ReleasedPick,
    call_record, decide_at_96_hours, decision_template, dispute_auto_submit, fraud_warning,
    missed_call_at, missed_call_text, no_call_at_72_hours, queue_reminders, reminder_template,
    reminder_times, send_at,
};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb191_0001);
const HOMEOWNER: Uuid = Uuid::from_u128(0xc191_0001);
const INTENT: &str = "pi_FakeT191";
const PHONE: &str = "+19285550100";
const SECRET: &str = "fake-id-secret-t191";

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
            Ok("re_FakeT191".to_owned())
        } else {
            Err(AdapterError::NotSent)
        }
    }
}

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn call(at: Timestamp, answered: AnsweredBy) -> CallRecord {
    let logged = LoggedCall {
        caller: CallCaller::Business,
        answered,
        length: SignedDuration::from_secs(20),
        reached_45s: false,
        ended_at: at,
    };
    call_record(&logged)
}

fn facts(calls: &[CallRecord], emergency: bool, contact: Contact) -> UnreachableFacts<'_> {
    UnreachableFacts {
        released_at: az(2, 9, 0),
        emergency,
        calls,
        notice_attempted: true,
        done_or_problem: false,
        contact,
        email_check: EmailCheck::NoReply,
        charged: true,
        refunded_before: false,
    }
}

fn decide(times: [Timestamp; 2], emergency: bool) -> RefundDecision {
    let calls = times.map(|t| call(t, AnsweredBy::Nobody));
    askhava_payments::api::decide_unreachable(&facts(&calls, emergency, Contact::Call), az(9, 9, 0))
}

/// TD-FEE-22: only attempts inside 8 a.m. to 8 p.m. count, 3 hours apart, any hour on an
/// emergency; no notice, or an email YES, means no refund; the reason names the condition.
#[test]
fn td_fee_22_attempt_hours_spacing_notice_and_the_decision_reason() {
    let one = decide([az(3, 7, 50), az(3, 14, 0)], false);
    assert_eq!(one, RefundDecision::NoRefund(Unmet::TooFewAttempts(1)));
    assert_eq!(decision_template(one), Some("unreachable_no_one_attempt"));
    let paid = RefundDecision::Refund(RefundReason::Unreachable);
    assert_eq!(decide([az(3, 10, 0), az(3, 14, 0)], false), paid);
    let close = decide([az(3, 10, 0), az(3, 12, 59)], false);
    assert_eq!(close, RefundDecision::NoRefund(Unmet::AttemptsTooClose));
    assert_eq!(decide([az(3, 23, 0), az(4, 2, 30)], true), paid);
    let calls = [
        call(az(3, 10, 0), AnsweredBy::Nobody),
        call(az(3, 14, 0), AnsweredBy::Machine),
    ];
    let silent = UnreachableFacts {
        notice_attempted: false,
        ..facts(&calls, false, Contact::Call)
    };
    let none = askhava_payments::api::decide_unreachable(&silent, az(9, 9, 0));
    assert_eq!(none, RefundDecision::NoRefund(Unmet::NoMissedCallNotice));
    let yes = UnreachableFacts {
        email_check: EmailCheck::Yes,
        ..facts(&calls, false, Contact::Email)
    };
    let agreed = askhava_payments::api::decide_unreachable(&yes, az(9, 9, 0));
    assert_eq!(agreed, RefundDecision::NoRefund(Unmet::EmailedAndAgreed));
    let templates = [one, paid, close, none, agreed].map(decision_template);
    let first = &[
        "Only 1 of your calls",
        "is refunded",
        "less than 3 hours",
        "no way",
        "emailed",
    ];
    for (template, words) in templates.iter().zip(first) {
        let body = render(
            template.expect("an email"),
            Language::En,
            false,
            &[("job", "41")],
        );
        let body = body.expect("render").body;
        assert!(
            body.contains(words) && body.contains("Ask about a charge"),
            "{body}"
        );
    }
    assert_eq!(decision_template(RefundDecision::NotYet), None);
}

/// TD-TXT-30, TD-FEE-22: one missed-call text after the first unanswered attempt, an important
/// text with no TIMES line; at 23:10 it goes at once on an emergency job, else at 08:00:00.
#[test]
fn td_txt_30_the_missed_call_text_time_and_words() {
    let released = az(2, 9, 0);
    let calls = [
        call(az(2, 23, 10), AnsweredBy::Machine),
        call(az(3, 10, 0), AnsweredBy::Nobody),
    ];
    assert_eq!(missed_call_at(released, &calls), Some(az(2, 23, 10)));
    let answered = [
        call(az(2, 11, 0), AnsweredBy::Person),
        call(az(3, 10, 0), AnsweredBy::Nobody),
    ];
    assert_eq!(missed_call_at(released, &answered), None);
    let table = AreaCodes::embedded().expect("table");
    let night = az(2, 23, 10);
    assert_eq!(
        send_at(&table, (PHONE, Kind::PickStep, true), night),
        Ok(night)
    );
    assert_eq!(
        send_at(&table, (PHONE, Kind::PickStep, false), night),
        Ok(az(3, 8, 0))
    );
    assert!(IMPORTANT_TEMPLATES.contains(&MISSED_CALL_TEMPLATE));
    assert_eq!(keyword("TIMES"), None);
    let values = [
        ("business", "Fake Plumbing"),
        ("job", "41"),
        ("number", "+19285550199"),
    ];
    let text = render(MISSED_CALL_TEMPLATE, Language::En, false, &values).expect("render");
    assert!(!text.body.contains("TIMES"), "{}", text.body);
}

/// TD-TXT-10, TD-TXT-13: reminders at 24:00:00 and 48:00:00, none at 23:59:59, none after an
/// attempt; held to 08:00; Spanish; the email line; the fixed calling hours and no window.
#[test]
fn td_txt_10_td_txt_13_the_no_call_reminders() {
    let released = az(2, 10, 0);
    let day = SignedDuration::from_hours(24);
    assert_eq!(
        reminder_times(released, &[]),
        [released + day, released + day * 2]
    );
    assert!(
        !reminder_times(released, &[]).contains(&(released + day - SignedDuration::from_secs(1)))
    );
    let early = [call(az(2, 15, 0), AnsweredBy::Nobody)];
    assert!(reminder_times(released, &early).is_empty());
    let table = AreaCodes::embedded().expect("table");
    assert_eq!(
        send_at(&table, (PHONE, Kind::Update, false), az(3, 2, 0)),
        Ok(az(3, 8, 0))
    );
    let job = [("job", "41")];
    let es = render(REMINDER_TEMPLATE, Language::Es, true, &job).expect("es");
    assert!(es.body.starts_with("Ask Hava: Trabajo 41"), "{}", es.body);
    assert_eq!(reminder_template(Contact::Email), REMINDER_EMAIL_TEMPLATE);
    for template in [
        reminder_template(Contact::Call),
        reminder_template(Contact::Email),
    ] {
        let body = render(template, Language::En, false, &job)
            .expect("en")
            .body;
        assert!(body.contains("Call between 8 a.m. and 8 p.m. Arizona time (any hour on an emergency job). Your 2 attempts can be on different days."));
        assert!(!body.contains("window"));
        let email_line =
            "The homeowner chose email; your 2 call attempts still count for the refund.";
        assert_eq!(
            body.contains(email_line),
            template == REMINDER_EMAIL_TEMPLATE
        );
    }
}

/// A job of `status` (through its arrows), its paid pick, charge record and open line.
async fn seeded(pool: &PgPool, status: JobStatus, intent: &str) -> ReleasedPick<'static> {
    let at = az(1, 9, 0);
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, false, Actor::System)
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
        let moved = move_to(&mut db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(BUSINESS).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $5, $5)",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT191', $4, 1, 7500, $5, $5)",
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($3, $2, $2, '+19285550199', false, $5)",
    ] {
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

/// The job's stored status code.
async fn status(db: &mut sqlx::PgConnection, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

fn unanswered() -> [CallRecord; 2] {
    [
        call(az(2, 10, 0), AnsweredBy::Nobody),
        call(az(2, 14, 0), AnsweredBy::Nobody),
    ]
}

const EMAILED: &str =
    "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%.unreachable_refunded.' || $1::text";
const OPEN_LINE: &str =
    "SELECT count(*) FROM calls.number_pool WHERE job_id = $1 AND closed_at IS NULL";

/// TD-JOB-07: a failed refund closes nothing and emails nothing; the next run refunds, closes the
/// job (no check-in after), closes the line and emails the business once; the email YES stops
/// refund and close; a job before 96 hours is untouched.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_07_the_unreachable_refund_closes_the_job_once(pool: PgPool) {
    let case = seeded(&pool, JobStatus::Connected, INTENT).await;
    let calls = unanswered();
    let secret = SecretString::from(SECRET.to_owned());
    let yes = UnreachableFacts {
        email_check: EmailCheck::Yes,
        ..facts(&calls, false, Contact::Email)
    };
    let stripe = (&FakeStripe(true), &secret);
    let kept = decide_at_96_hours(&pool, stripe, &case, &yes, az(9, 9, 0)).await;
    let agreed = RefundDecision::NoRefund(Unmet::EmailedAndAgreed);
    assert_eq!(kept, Ok(AtNinetySix::Decided(agreed)));
    let by_call = facts(&calls, false, Contact::Call);
    let early = decide_at_96_hours(&pool, stripe, &case, &by_call, az(5, 8, 59)).await;
    assert_eq!(early, Ok(AtNinetySix::Decided(RefundDecision::NotYet)));
    let down = (&FakeStripe(false), &secret);
    let failed = decide_at_96_hours(&pool, down, &case, &by_call, az(9, 9, 0)).await;
    assert_eq!(failed, Ok(AtNinetySix::RefundWaiting { blocked: false }));
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(status(&mut db, case.job).await, "connected");
    assert_eq!(count(&pool, EMAILED, case.pick).await, 0);
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 1);
    let paid = AtNinetySix::Decided(RefundDecision::Refund(RefundReason::Unreachable));
    for _ in 0..2 {
        let done = decide_at_96_hours(&pool, stripe, &case, &by_call, az(9, 9, 0)).await;
        assert_eq!(done, Ok(paid));
    }
    let job = current(&mut db, case.job).await.expect("job").status;
    assert_eq!(job.code(), "closed");
    assert!(!job.takes_check_ins());
    assert_eq!(count(&pool, EMAILED, case.pick).await, 1);
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
    let refunds =
        "SELECT count(*) FROM payments.refund WHERE pick_id = $1 AND trigger = 'unreachable'";
    assert_eq!(count(&pool, refunds, case.pick).await, 1);
}

/// TD-JOB-07: a run that refunded but stopped before the close finishes on the next run (the pick
/// now reads as refunded before): the job closes and the business is emailed once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_07_a_half_finished_run_closes_and_emails_once(pool: PgPool) {
    let case = seeded(&pool, JobStatus::Connected, INTENT).await;
    let secret = SecretString::from(SECRET.to_owned());
    let trigger = RefundTrigger::Rule(RefundReason::Unreachable);
    let refunded = request_refund(
        &pool,
        &FakeStripe(true),
        &secret,
        (case.pick, trigger),
        az(9, 9, 0),
    )
    .await;
    assert!(refunded.is_ok());
    let calls = unanswered();
    let again = UnreachableFacts {
        refunded_before: true,
        ..facts(&calls, false, Contact::Call)
    };
    let paid = AtNinetySix::Decided(RefundDecision::Refund(RefundReason::Unreachable));
    for _ in 0..2 {
        let stripe = (&FakeStripe(false), &secret);
        let done = decide_at_96_hours(&pool, stripe, &case, &again, az(9, 10, 0)).await;
        assert_eq!(done, Ok(paid));
    }
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(status(&mut db, case.job).await, "closed");
    assert_eq!(count(&pool, EMAILED, case.pick).await, 1);
    assert_eq!(count(&pool, OPEN_LINE, case.job).await, 0);
}

/// TD-JOB-07: call contact with no call in 72 hours returns to Quotes ready (pick a no-show);
/// email contact stays Connected until the day-3 NO, which returns it with the no-show recorded.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_07_no_call_in_72_hours_by_contact(pool: PgPool) {
    let case = seeded(&pool, JobStatus::Connected, INTENT).await;
    let mut db = pool.acquire().await.expect("conn");
    let released = az(2, 9, 0);
    let email = (Contact::Email, EmailCheck::NoReply, released, &[][..]);
    let kept = no_call_at_72_hours(&mut db, &case, email, az(5, 9, 0)).await;
    assert_eq!(kept, Ok(false));
    let call = (Contact::Call, EmailCheck::NoReply, released, &[][..]);
    let soon = no_call_at_72_hours(&mut db, &case, call, az(5, 8, 59)).await;
    assert_eq!(soon, Ok(false));
    assert_eq!(status(&mut db, case.job).await, "connected");
    let back = no_call_at_72_hours(&mut db, &case, call, az(5, 9, 0)).await;
    assert_eq!(back, Ok(true));
    assert_eq!(status(&mut db, case.job).await, "quotes_ready");
    let shows = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'no_show'";
    assert_eq!(count(&pool, shows, case.pick).await, 1);
    let mailed = seeded(&pool, JobStatus::Connected, "pi_FakeT191c").await;
    let no = (Contact::Email, EmailCheck::No, released, &[][..]);
    let returned = no_call_at_72_hours(&mut db, &mailed, no, az(5, 10, 0)).await;
    assert_eq!(returned, Ok(true));
    assert_eq!(status(&mut db, mailed.job).await, "quotes_ready");
    assert_eq!(count(&pool, shows, mailed.pick).await, 1);
    assert_eq!(count(&pool, OPEN_LINE, mailed.job).await, 0);
}

/// TD-TXT-30, TD-TXT-10: the missed-call text is queued once, only when owed and her number may
/// be texted (an opted-out number is "never sent"); the two reminders are queued once each.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_30_the_missed_call_text_and_reminders_are_queued_once(pool: PgPool) {
    let case = seeded(&pool, JobStatus::Connected, INTENT).await;
    let mut db = pool.acquire().await.expect("conn");
    let secret = SecretString::from(SECRET.to_owned());
    let released = az(2, 9, 0);
    let missed = [call(az(2, 10, 0), AnsweredBy::Machine)];
    let number = "+19285550199";
    let opted = (&case, number, SendWindow::OptedOut);
    let none = missed_call_text(&mut db, &secret, opted, (released, &missed)).await;
    assert_eq!(none, Ok(false));
    let picked_up = [call(az(2, 10, 0), AnsweredBy::Person)];
    let open = (&case, number, SendWindow::Now);
    let talked = missed_call_text(&mut db, &secret, open, (released, &picked_up)).await;
    assert_eq!(talked, Ok(false));
    let texts = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'caller_missed' AND recipient_id = $1";
    assert_eq!(count(&pool, texts, HOMEOWNER).await, 0);
    for _ in 0..2 {
        let sent = missed_call_text(&mut db, &secret, open, (released, &missed)).await;
        assert_eq!(sent, Ok(true));
    }
    assert_eq!(count(&pool, texts, HOMEOWNER).await, 1);
    let first = queue_reminders(&mut db, &case, (released, Contact::Call, &[])).await;
    assert_eq!(first, Ok(2));
    let again = queue_reminders(&mut db, &case, (released, Contact::Call, &[])).await;
    assert_eq!(again, Ok(0));
    let reminders = "SELECT count(*) FROM core.task WHERE dedupe_key LIKE '%no_call_reminder.' || $1::text || '.%'";
    assert_eq!(count(&pool, reminders, case.pick).await, 2);
}

/// TD-FEE-39: a dispute is marked for auto-submit only with a release and a call attempt or 3DS.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_39_auto_submit(pool: PgPool) {
    seeded(&pool, JobStatus::PaymentPending, INTENT).await;
    let opened = DisputeOpened {
        dispute: "dp_FakeT191",
        payment_intent: INTENT,
        amount_cents: 7500,
        due_at: az(20, 9, 0),
    };
    assert_eq!(record_dispute(&pool, &opened, az(5, 9, 0)).await, Ok(true));
    let mut db = pool.acquire().await.expect("conn");
    for unmet in [(false, 1, true), (true, 0, false)] {
        assert_eq!(
            dispute_auto_submit(&mut db, "dp_FakeT191", unmet, az(5, 9, 0)).await,
            Ok(false)
        );
    }
    assert_eq!(
        dispute_auto_submit(&mut db, "dp_FakeT191", (true, 1, false), az(5, 9, 0)).await,
        Ok(true)
    );
}

/// TD-FEE-39: an early fraud warning on an unreleased charge refunds it once (a `fraud-warning`
/// refund row), ends the pick as fraud-refunded and returns the job to Quotes ready; a released
/// pick, a disputed (blocked) or failed refund and a job past Payment pending change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_39_the_fraud_warning(pool: PgPool) {
    let secret = SecretString::from(SECRET.to_owned());
    let (up, down) = ((&FakeStripe(true), &secret), (&FakeStripe(false), &secret));
    let disputed = seeded(&pool, JobStatus::PaymentPending, INTENT).await;
    let opened = DisputeOpened {
        dispute: "dp_FakeT191",
        payment_intent: INTENT,
        amount_cents: 7500,
        due_at: az(20, 9, 0),
    };
    assert_eq!(record_dispute(&pool, &opened, az(5, 9, 0)).await, Ok(true));
    let ids = (disputed.pick, disputed.job, false);
    assert_eq!(fraud_warning(&pool, up, ids, az(5, 9, 0)).await, Ok(false));
    let ended = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'fraud_refunded'";
    assert_eq!(count(&pool, ended, disputed.pick).await, 0);
    let fresh = seeded(&pool, JobStatus::PaymentPending, "pi_FakeT191b").await;
    let released = fraud_warning(&pool, up, (fresh.pick, fresh.job, true), az(5, 9, 0)).await;
    assert_eq!(released, Ok(false));
    let ids = (fresh.pick, fresh.job, false);
    assert_eq!(
        fraud_warning(&pool, down, ids, az(5, 9, 0)).await,
        Ok(false)
    );
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(status(&mut db, fresh.job).await, "payment_pending");
    assert_eq!(count(&pool, ended, fresh.pick).await, 0);
    assert_eq!(fraud_warning(&pool, up, ids, az(5, 10, 0)).await, Ok(true));
    assert_eq!(status(&mut db, fresh.job).await, "quotes_ready");
    assert_eq!(count(&pool, ended, fresh.pick).await, 1);
    let refunds = "SELECT count(*) FROM payments.refund WHERE pick_id = $1 AND trigger = 'fraud-warning' AND status = 'succeeded'";
    assert_eq!(count(&pool, refunds, fresh.pick).await, 1);
    let connected = seeded(&pool, JobStatus::Connected, "pi_FakeT191d").await;
    let ids = (connected.pick, connected.job, false);
    assert_eq!(fraud_warning(&pool, up, ids, az(5, 9, 0)).await, Ok(false));
    assert_eq!(status(&mut db, connected.job).await, "connected");
    assert_eq!(count(&pool, ended, connected.pick).await, 0);
}

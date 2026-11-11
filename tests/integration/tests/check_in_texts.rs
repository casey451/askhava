//! TD-JOB-13 (T278, D-1842; JOB-015, JOB-016, JOB-017, JOB-027, TXT-029, TXT-040): the check-in,
//! MORE, feedback, PROBLEM-reply, unreachable-closed and emailed-question texts reach the
//! homeowner. A release of a Connected job queues its day-3 and day-14 check-ins from the later of
//! the release and the picked quote's earliest visit; each template renders in English and
//! Spanish and `intake`'s sender sends it (none refused as a bad payload); the unreachable close
//! queues `unreachable_closed` once, in the close's transaction. TD-TXT-25's scan and TD-CON-24's
//! guard run unchanged in `quotes` and `intake`. Fake numbers (555-01xx), IDs and words only.

use std::collections::BTreeSet;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_businesses::api::AccountStatus;
use askhava_calls::api::{AnsweredBy, Caller, LoggedCall};
use askhava_core::api::{IdSecret, LinkToken};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JOB_TEXT_TEMPLATES, JobStart, JobTextKeys, JobTextStep, Opened, TextIn, begin_job,
    job_text_body, job_text_step, parse_job_text, receive_text,
};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{
    CHECK_IN_KIND, JobStatus, TEXT_KIND, current, job_number, move_to, schedule_check_ins,
};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_payments::api::{Contact, EmailCheck, UnreachableFacts, decide_unreachable};
use askhava_picks::api::{
    AtNinetySix, ReleasedPick, UNREACHABLE_CLOSED_TEMPLATE, call_record, check_ins_at_release,
    decide_at_96_hours,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, DayPart, Format, OfferTown, Price, QuoteForm, QuoteRules, Shape,
    Submitted, UPDATE_TEXTS, check, parse_quote, scan, send_cards, submit_quote,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 10:00 Arizona on November 2, 2026: the release.
const DAY: &str = "2026-11-02T17:00:00Z";
const PHONE: &str = "+19285550151";
const OTHER_PHONE: &str = "+19285550152";
const BUSINESS_NAME: &str = "Fake Plumbing";
/// JOB-015's first line, word for word, after the opening.
const FIRST_LINE: &str = "Ask Hava: Reply DONE, PROBLEM, or NO if they never reached you. Not done yet? Reply LATER. More answers: reply MORE.";
/// The seven templates T278 makes sendable (`problem_reply` was T097's, D-1811).
const NEW: [&str; 7] = [
    "check_in_day3",
    "check_in_day14",
    "check_in_more",
    "feedback_request",
    "problem_reply",
    "unreachable_closed",
    "emailed_question",
];

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn days(n: i64) -> Timestamp {
    ts(DAY) + SignedDuration::from_hours(24 * n)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x67), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-278".to_owned())
}

fn keys() -> JobTextKeys {
    JobTextKeys::new(IdSecret::new(1, secret()).expect("id"), secret(), vault())
}

/// A homeowner with `phone` texts in (consent text, request) and gets a plumbing job; returns it, in Intake.
async fn her_job(db: &mut PgConnection, phone: &str) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    job
}

/// Walks `job` along its arrows through each status of `path` (JOB-002).
async fn walk(db: &mut PgConnection, job: Uuid, path: &[JobStatus]) {
    for pair in path.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{pair:?}");
    }
}

/// A Connected job of hers with no quote stored.
async fn connected(db: &mut PgConnection) -> Uuid {
    let job = her_job(db, PHONE).await;
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    walk(db, job, &path).await;
    job
}

/// The AC repair job's card and an active, verified R-39 business serving Lake Havasu City
/// that ticked its job type, as matching reads them.
fn card_for(business: Uuid) -> (CardJob, CardBusiness) {
    let card = CardJob {
        towns: vec![OfferTown {
            code: "lake_havasu_city".to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![Job {
            category: "hvac_repair".to_owned(),
            job_type: "AC not cooling".to_owned(),
            license: LicenseKind::Roc,
            handyman_allowed: false,
            listed: true,
        }],
        text: String::new(),
    };
    let holder = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    (card, holder)
}

/// Her job carded to a new business, quoted with `visit` as its earliest visit (Open to Quotes
/// ready, the first-quote move); returns `(job, business)`.
async fn quoted(db: &mut PgConnection, visit: jiff::civil::Date) -> (Uuid, Uuid) {
    let job = her_job(db, OTHER_PHONE).await;
    walk(db, job, &[JobStatus::Intake, JobStatus::Open]).await;
    let business = askhava_businesses::api::create(db, false, ts(DAY))
        .await
        .expect("business");
    let (card, holder) = card_for(business);
    let map = ClassMap::shipped().expect("license map");
    let sent = send_cards(db, map, job, &card, &[holder], ts(DAY)).await;
    assert_eq!(sent.expect("cards").len(), 1);
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
        ai_read: false,
        earliest_visit: Some((visit, DayPart::Morning)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(db, (job, business), &quote, ts(DAY)).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    (job, business)
}

/// The job's queued check-ins: `(template, run_at)`, day 3 first when there is one.
async fn check_ins(db: &mut PgConnection, job: Uuid) -> Vec<(String, Timestamp)> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT payload->>'template', (extract(epoch FROM run_at))::bigint FROM core.task \
         WHERE kind = $1 AND payload->>'job' = $2 ORDER BY run_at",
    )
    .bind(CHECK_IN_KIND)
    .bind(job.to_string())
    .fetch_all(db)
    .await
    .expect("check-ins");
    let at = |s: i64| Timestamp::from_second(s).expect("time");
    rows.into_iter().map(|(t, s)| (t, at(s))).collect()
}

/// TD-JOB-13 (JOB-015, D-1811 (2)): a release of a Connected job queues day 3 from the later of
/// the release and the earliest visit (start of that Arizona day) and day 14 from the release,
/// once; a job not yet Connected gets none; with no visit day 3 counts from the release.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_13_a_release_that_connects_schedules_the_check_ins(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, business) = quoted(&mut db, jiff::civil::date(2026, 11, 7)).await;
    let early = check_ins_at_release(&mut db, (job, business), ts(DAY)).await;
    assert_eq!(early, Ok(false));
    assert!(check_ins(&mut db, job).await.is_empty());
    let path = [
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    walk(&mut db, job, &path).await;
    for _ in 0..2 {
        let mut tx = pool.begin().await.expect("tx");
        let queued = check_ins_at_release(&mut tx, (job, business), ts(DAY)).await;
        assert_eq!(queued, Ok(true));
        tx.commit().await.expect("commit");
    }
    let visit_day3 = ts("2026-11-10T07:00:00Z");
    let expected = vec![
        ("check_in_day3".to_owned(), visit_day3),
        ("check_in_day14".to_owned(), days(14)),
    ];
    assert_eq!(check_ins(&mut db, job).await, expected);
    let plain = connected(&mut db).await;
    let queued = check_ins_at_release(&mut db, (plain, Uuid::new_v4()), ts(DAY)).await;
    assert_eq!(queued, Ok(true));
    let expected = vec![
        ("check_in_day3".to_owned(), days(3)),
        ("check_in_day14".to_owned(), days(14)),
    ];
    assert_eq!(check_ins(&mut db, plain).await, expected);
}

/// The event a `jobs.homeowner_text` task would carry for `template` on `job` now.
async fn event(db: &mut PgConnection, job: Uuid, template: &str) -> Value {
    let version = current(db, job).await.expect("job").version;
    let number = job_number(db, job).await.expect("number");
    json!({"job": job, "version": version, "template": template,
        "values": {"job": number, "business": BUSINESS_NAME}})
}

/// TD-JOB-13 (JOB-015, JOB-017, JOB-027, TXT-029, TXT-040, D-1817): every new template renders in
/// English and Spanish with nothing left unfilled, holds its fixed words, and passes TD-TXT-25's
/// scan as a listed template.
#[test]
fn td_job_13_the_new_templates_render_in_both_languages() {
    let link = "https://askhava.com/f/fake-token";
    for template in NEW {
        assert!(
            JOB_TEXT_TEMPLATES.contains(&template),
            "{template} not sendable"
        );
        let payload = json!({"job": Uuid::nil(), "version": 1, "template": template,
            "values": {"job": 1234, "business": BUSINESS_NAME}});
        let text = parse_job_text(&payload).expect("a known template");
        for language in [Language::En, Language::Es] {
            let body = job_text_body(&text, language, Some(link)).expect("rendered");
            assert!(body.starts_with("Ask Hava: "), "{template}");
            assert!(!body.contains('{'), "{template}: {body}");
            let shape = UPDATE_TEXTS.iter().find(|(id, _)| *id == template);
            let shape = shape.map_or(Shape::Capped, |(_, s)| *s);
            assert_eq!(check(&body, shape), Vec::new(), "{template} {language:?}");
        }
        let english = job_text_body(&text, Language::En, Some(link)).expect("en");
        let fixed = match template {
            "check_in_day3" | "check_in_day14" => FIRST_LINE.to_owned(),
            "unreachable_closed" => format!(
                "Ask Hava: {BUSINESS_NAME} couldn't reach you. Text us any time to start again."
            ),
            "emailed_question" => format!(
                "Ask Hava: Did {BUSINESS_NAME} email you and agree a time? Text back YES or NO."
            ),
            "feedback_request" => link.to_owned(),
            "check_in_more" => "OTHER: you hired someone else.".to_owned(),
            _ => "Ask Hava: Thanks. The Ask Hava owner reads every problem report".to_owned(),
        };
        assert!(english.contains(&fixed), "{template}: {english}");
    }
    let more = job_text_body(
        &parse_job_text(&json!({"job": Uuid::nil(), "version": 1,
            "template": "check_in_more", "values": {"job": 1}}))
        .expect("more"),
        Language::En,
        None,
    )
    .expect("more");
    for answer in ["DONE:", "PROBLEM:", "NO:", "LATER:", "ONGOING:", "OTHER:"] {
        assert!(more.contains(answer), "MORE lists {answer}");
    }
    assert_eq!(scan().expect("scan"), Vec::new());
}

/// TD-JOB-13 (JOB-015, JOB-017, TXT-029, SEC-037): `intake`'s sender sends each new template to
/// her (none refused as a bad payload); the feedback request carries a live `/f/` link to her own
/// job, and nothing else carries a link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_13_the_sender_sends_each_new_template(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected(&mut db).await;
    for template in NEW {
        let payload = event(&mut db, job, template).await;
        let text = parse_job_text(&payload).expect("not a bad payload");
        let step = job_text_step(&mut db, &keys(), &text, ts(DAY)).await;
        let Ok(JobTextStep::Queued(queued)) = step else {
            panic!("{template} not queued: {step:?}");
        };
        let body: String = sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
            .bind(queued.outbox_id)
            .fetch_one(&mut *db)
            .await
            .expect("body");
        assert!(body.starts_with("Ask Hava: "), "{template}");
        let link = body.split_whitespace().find(|w| w.starts_with("https://"));
        if template != "feedback_request" {
            assert_eq!(link, None, "{template}");
            continue;
        }
        let token = link
            .and_then(|l| l.strip_prefix("https://askhava.com/f/"))
            .expect("feedback link");
        let token = LinkToken::parse(token).expect("token");
        assert_eq!(resolve(&mut db, &token).await.expect("resolve"), Some(job));
    }
}

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
            Ok("re_FakeT278".to_owned())
        } else {
            Err(AdapterError::NotSent)
        }
    }
}

/// A Connected job with a paid pick, its charge record and open line, as T191's tests seed it.
async fn released(pool: &PgPool) -> ReleasedPick<'static> {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected(&mut db).await;
    let business = Uuid::from_u128(0xb278_0001);
    let at = ts(DAY);
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, 'pi_FakeT278', false, $4)",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at) VALUES ($1, 1, $2, 'succeeded', $4, $4)",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, 'pick-' || $1::text || '-try1', 'cus_FakeT278', 'pi_FakeT278', 1, 7500, $4, $4)",
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($3, $2, $2, '+19285550198', false, $4)",
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
    ReleasedPick {
        pick,
        job,
        job_number: job_number(&mut db, job).await.expect("number"),
        business,
        business_name: BUSINESS_NAME,
        homeowner: Uuid::from_u128(0xc278_0001),
        language: (Language::En, false),
    }
}

/// The `unreachable_closed` events queued for `job`: `(version, values)`.
async fn closed_texts(pool: &PgPool, job: Uuid) -> Vec<(i64, Value)> {
    sqlx::query_as(
        "SELECT (payload->>'version')::bigint, payload->'values' FROM core.task \
         WHERE kind = $1 AND payload->>'job' = $2 AND payload->>'template' = $3",
    )
    .bind(TEXT_KIND)
    .bind(job.to_string())
    .bind(UNREACHABLE_CLOSED_TEMPLATE)
    .fetch_all(pool)
    .await
    .expect("events")
}

/// TD-JOB-13 (JOB-027, D-1818): a failed refund queues no close text; the refund's close queues
/// `unreachable_closed` once, at the closed job's version, with the job number and the business's
/// public name only, and a second run queues nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_13_the_unreachable_close_queues_its_text_once(pool: PgPool) {
    let case = released(&pool).await;
    let call = |hours: i64| {
        let at = ts(DAY) + SignedDuration::from_hours(hours);
        call_record(&LoggedCall {
            caller: Caller::Business,
            answered: AnsweredBy::Nobody,
            length: SignedDuration::from_secs(20),
            reached_45s: false,
            started_at: Some(at),
            ended_at: at,
        })
    };
    let calls = [call(1), call(5)];
    let facts = UnreachableFacts {
        released_at: ts(DAY),
        emergency: false,
        calls: &calls,
        notice_attempted: true,
        done_or_problem: false,
        contact: Contact::Call,
        email_check: EmailCheck::NoReply,
        charged: true,
        refunded_before: false,
    };
    let now = days(7);
    assert!(matches!(
        decide_unreachable(&facts, now),
        askhava_payments::api::RefundDecision::Refund(_)
    ));
    let secret = secret();
    let down = decide_at_96_hours(&pool, (&FakeStripe(false), &secret), &case, &facts, now).await;
    assert_eq!(down, Ok(AtNinetySix::RefundWaiting { blocked: false }));
    assert!(closed_texts(&pool, case.job).await.is_empty());
    for _ in 0..2 {
        let up = (&FakeStripe(true), &secret);
        let done = decide_at_96_hours(&pool, up, &case, &facts, now).await;
        assert!(matches!(done, Ok(AtNinetySix::Decided(_))), "{done:?}");
    }
    let mut db = pool.acquire().await.expect("conn");
    let row = current(&mut db, case.job).await.expect("job");
    assert_eq!(row.status.code(), "closed");
    let values = json!({"job": case.job_number, "business": BUSINESS_NAME});
    let expected = vec![(i64::from(row.version), values)];
    assert_eq!(closed_texts(&pool, case.job).await, expected);
}

/// TD-JOB-13 (JOB-015, D-1812): the check-ins `jobs` queues name only the two check-in
/// templates, both of which the sender takes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_13_queued_check_ins_name_sendable_templates(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected(&mut db).await;
    schedule_check_ins(&mut db, job, ts(DAY), None)
        .await
        .expect("scheduled");
    for (template, _) in check_ins(&mut db, job).await {
        assert!(
            JOB_TEXT_TEMPLATES.contains(&template.as_str()),
            "{template}"
        );
    }
}

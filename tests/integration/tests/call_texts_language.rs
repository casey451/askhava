//! T590 (TXT-027, TXT-030, TXT-039; D-4052 item B22): the who's-calling text and the missed-call
//! text and its email copy go in her request's language. With Spanish conversations on, a Spanish
//! request's texts are the Spanish templates (the who's-calling text names an unnamed business by
//! the Spanish `generic_name`); with the switch off they are English; with no language source
//! (`run_whos_calling`, `run_unreachable_step`) or a failed read they are English. Fake numbers
//! (555-01xx, and 999 for a number that can't be texted), names, words and IDs only.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_calls::api::{
    JobLanguage, WHOS_CALLING_KIND, WhosCalling, generic_name, queue_whos_calling,
    run_whos_calling, run_whos_calling_in,
};
use askhava_core::api::{Claimed, Guarded};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, job_language, receive_text};
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::{
    Language, SpanishRecords, spanish_switch, spanish_template_versions, turn_spanish_on,
};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    ContactChoice, Step, StepParts, UNREACHABLE_STEP_KIND, record_contact_choice,
    run_unreachable_step, run_unreachable_step_in,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const SPANISH: &str = "Hola, necesito ayuda, mi lavabo gotea";
const NAME: &str = "Sample Pipe Co 590";
const LINE: &str = "+19285550199";
const ES_WHOS_CALLING: &str = "Si no contesta, llámele de vuelta a este número.";
const EN_WHOS_CALLING: &str = "If you miss it, call them back on this number.";
const ES_MISSED: &str = "intentó llamarle por la solicitud";
const EN_MISSED: &str = "tried to call you about request";

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// How each case reads her language: a real `intake` read, none, or a failed read.
#[derive(Clone, Copy)]
enum Source {
    Read,
    Unset,
    Fails,
}

/// The composition's source: `intake::api::job_language`, `None` on a failed read.
fn read_language(
    db: &mut PgConnection,
    job: Uuid,
) -> Pin<Box<dyn Future<Output = Option<Language>> + Send + '_>> {
    Box::pin(async move { job_language(db, job).await.ok() })
}

/// A source whose read always fails.
fn failed_read(
    _: &mut PgConnection,
    _: Uuid,
) -> Pin<Box<dyn Future<Output = Option<Language>> + Send + '_>> {
    Box::pin(async { None })
}

const READ: JobLanguage = read_language;
const FAILS: JobLanguage = failed_read;

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://fake@localhost/fake".to_owned());
    Vault::new(&key(0x59), &key(0x60), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-t590-000".to_owned())
}

/// A fake vault contact with a fresh number under `prefix`, and an email when `email`.
async fn contact(db: &mut PgConnection, prefix: &str, email: bool) -> Uuid {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(format!("{prefix}{n:02}"))),
        email: email.then(|| SecretString::from("dana@example.test")),
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// A Spanish request opened by text (Spanish conversations on while it opens) and its one job,
/// moved to Connected; with `switch` the stored "Spanish conversations" switch is turned on.
async fn spanish_job(db: &mut PgConnection, switch: bool) -> Uuid {
    let her = contact(db, "+192855501", false).await;
    let inbound = Inbound {
        words: SPANISH,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: az(1, 9, 0),
    };
    let settings = Settings {
        spanish_conversations_on: true,
        ..Settings::default()
    };
    let handled = receive_text(db, &secret(), &msg, settings).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: SPANISH,
        emergency: false,
        trade: "plumbing",
        at: az(1, 9, 0),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    connect(db, job).await;
    if switch {
        spanish_on(db).await;
    }
    job
}

/// Moves `job` through its arrows to Connected.
async fn connect(db: &mut PgConnection, job: Uuid) {
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for pair in path.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), az(1, 9, 0), Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
    }
}

/// Turns Spanish conversations on, unless already on, with a fake review of every template.
async fn spanish_on(db: &mut PgConnection) {
    if spanish_switch(db).await.expect("switch").is_on() {
        return;
    }
    let records = SpanishRecords {
        reviewed_on: Some("2026-11-01".parse().expect("date")),
        reviewer: "FAKE-T590-REVIEWER".to_owned(),
        reviewed: spanish_template_versions().expect("versions"),
        note_on: Some("2026-11-01".parse().expect("date")),
        note_reference: "FAKE-T590-CAMPAIGN".to_owned(),
        sample: "FAKE-T590-SAMPLE Hola, recibimos su solicitud.".to_owned(),
    };
    let on = turn_spanish_on(db, Uuid::from_u128(0x590), &records, az(1, 9, 0)).await;
    assert!(on.is_ok(), "switch on");
}

/// A fake business, named [`NAME`] when `named`.
async fn business(db: &mut PgConnection, named: bool) -> Uuid {
    let id = askhava_businesses::api::create(db, false, az(1, 9, 0))
        .await
        .expect("business");
    if named {
        askhava_businesses::api::save_business_name(db, id, (NAME, None), None)
            .await
            .expect("name");
    }
    id
}

/// The masked line of `job` between her and `business`; its ID.
async fn open_line(db: &mut PgConnection, (job, homeowner, business): (Uuid, Uuid, Uuid)) -> Uuid {
    sqlx::query_scalar("INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($1, $2, $3, $4, false, $5) RETURNING id")
        .bind(job).bind(homeowner).bind(business).bind(LINE).bind(az(1, 9, 0).to_sqlx())
        .fetch_one(&mut *db).await.expect("line")
}

fn task(kind: &str, payload: serde_json::Value) -> Claimed {
    Claimed {
        id: 0,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

/// Queues and runs the who's-calling task for an unnamed business on a Spanish job (`switch` on
/// or off) through `source`; her one text's body.
async fn whos_calling(pool: &PgPool, switch: bool, source: Source) -> String {
    let mut db = pool.acquire().await.expect("conn");
    let job = spanish_job(&mut db, switch).await;
    let business = business(&mut db, false).await;
    let homeowner = contact(&mut db, "+192855501", false).await;
    open_line(&mut db, (job, homeowner, business)).await;
    let call = WhosCalling {
        pick: Uuid::new_v4(),
        job,
        job_number: askhava_jobs::api::job_number(&mut db, job)
            .await
            .expect("n"),
        business,
        homeowner,
        email_contact: false,
        emergency: false,
    };
    let now = az(2, 10, 0);
    assert!(
        queue_whos_calling(&mut db, &call, now)
            .await
            .expect("queue")
    );
    let payload = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'pick' = $2::text",
    )
    .bind(WHOS_CALLING_KIND)
    .bind(call.pick)
    .fetch_one(pool)
    .await
    .expect("task");
    let claimed = task(WHOS_CALLING_KIND, payload);
    let keys = (&vault(), &secret());
    let ran = match source {
        Source::Read => run_whos_calling_in(pool.clone(), keys, READ, claimed, now).await,
        Source::Fails => run_whos_calling_in(pool.clone(), keys, FAILS, claimed, now).await,
        Source::Unset => run_whos_calling(pool.clone(), keys, claimed, now).await,
    };
    assert_eq!(ran, Ok(()));
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE recipient_id = $1")
        .bind(homeowner)
        .fetch_one(pool)
        .await
        .expect("text")
}

/// The business-call step calls no Stripe method.
struct NoStripe;

impl PaymentsAdapter for NoStripe {
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
        panic!("nothing is refunded here")
    }
}

fn parts() -> Arc<StepParts<NoStripe>> {
    let postal = askhava_messaging::api::PostalAddress::new(
        "PO Box 1234, Lake Havasu City, AZ 86405",
        askhava_messaging::api::MailboxKind::PoBox,
    )
    .expect("postal");
    Arc::new(StepParts {
        stripe: Some(NoStripe),
        secret: secret(),
        vault: Some(vault()),
        postal: Some(postal),
    })
}

/// A paid, released pick of a named business on a Spanish job (`switch` on or off) with one
/// unanswered business call; her contact is untextable (with an email) when `untextable`. Runs
/// the business-call step through `source`; her contact.
async fn missed_call(pool: &PgPool, switch: bool, untextable: bool, source: Source) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let job = spanish_job(&mut db, switch).await;
    let business = business(&mut db, true).await;
    let prefix = if untextable {
        "+199955501"
    } else {
        "+192855501"
    };
    let homeowner = contact(&mut db, prefix, untextable).await;
    let line = open_line(&mut db, (job, homeowner, business)).await;
    let pick = released_pick(&mut db, (job, business, homeowner), untextable).await;
    sqlx::query("INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, length_seconds, reached_45s, ended_at) VALUES ($1, 'CA' || md5($1::text), 'business', 'nobody', 20, false, $2)")
        .bind(line).bind(az(2, 10, 0).to_sqlx()).execute(&mut *db).await.expect("call");
    let payload = json!({"pick": pick, "step": Step::BusinessCall.code()});
    let claimed = task(UNREACHABLE_STEP_KIND, payload);
    let (now, p) = (az(2, 10, 1), parts());
    let ran = match source {
        Source::Read => run_unreachable_step_in(pool.clone(), p, READ, claimed, now).await,
        Source::Fails => run_unreachable_step_in(pool.clone(), p, FAILS, claimed, now).await,
        Source::Unset => run_unreachable_step(pool.clone(), p, claimed, now).await,
    };
    assert_eq!(ran, Ok(()));
    homeowner
}

/// The paid pick of `business` on `job`, released to `homeowner` with her contact choice.
async fn released_pick(
    db: &mut PgConnection,
    (job, business, homeowner): (Uuid, Uuid, Uuid),
    email: bool,
) -> Uuid {
    let at = az(1, 9, 0);
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    sqlx::query("INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, $6)")
        .bind(pick).bind(job).bind(business).bind(az(2, 9, 0).to_sqlx()).bind(homeowner).bind(email)
        .execute(&mut *db).await.expect("release");
    let choice = if email {
        ContactChoice::Email
    } else {
        ContactChoice::Call
    };
    record_contact_choice(db, pick, choice, at)
        .await
        .expect("choice");
    pick
}

/// Her missed-call text's body.
async fn missed_text(pool: &PgPool, homeowner: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = 'caller_missed' AND recipient_id = $1")
        .bind(homeowner).fetch_one(pool).await.expect("missed text")
}

/// Her missed-call email copy's plain text.
async fn missed_email(pool: &PgPool, homeowner: Uuid) -> String {
    sqlx::query_scalar("SELECT plain FROM messaging.email_outbox WHERE template_id = 'caller_missed' AND recipient_id = $1")
        .bind(homeowner).fetch_one(pool).await.expect("missed email")
}

/// TXT-027, TXT-039: with the switch on, a Spanish request's who's-calling text is the Spanish
/// template naming an unnamed business by the Spanish generic name.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_whos_calling_text_is_spanish_for_a_spanish_request_with_the_switch_on(pool: PgPool) {
    let body = whos_calling(&pool, true, Source::Read).await;
    assert!(body.contains(ES_WHOS_CALLING), "Spanish template: {body}");
    assert!(body.contains(generic_name(Language::Es)), "Spanish name");
    assert!(!body.contains(EN_WHOS_CALLING), "no English words");
}

/// TXT-039: with the switch off, the same request's who's-calling text is English.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_whos_calling_text_is_english_with_the_switch_off(pool: PgPool) {
    let body = whos_calling(&pool, false, Source::Read).await;
    assert!(body.contains(EN_WHOS_CALLING), "English template: {body}");
    assert!(body.contains(generic_name(Language::En)), "English name");
}

/// D-4052 (B22): with no language source, or a failed read, the who's-calling text is English.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_whos_calling_text_is_english_with_no_source_or_a_failed_read(pool: PgPool) {
    for source in [Source::Unset, Source::Fails] {
        let body = whos_calling(&pool, true, source).await;
        assert!(body.contains(EN_WHOS_CALLING), "English template: {body}");
        assert!(body.contains(generic_name(Language::En)), "English name");
    }
}

/// TXT-030, TXT-039: with the switch on, a Spanish request's missed-call text and its email copy
/// are the Spanish template.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_missed_call_text_and_email_are_spanish_with_the_switch_on(pool: PgPool) {
    let texted = missed_call(&pool, true, false, Source::Read).await;
    let body = missed_text(&pool, texted).await;
    assert!(
        body.contains(ES_MISSED) && body.contains(NAME),
        "Spanish text: {body}"
    );
    let emailed = missed_call(&pool, true, true, Source::Read).await;
    let plain = missed_email(&pool, emailed).await;
    assert!(plain.contains(ES_MISSED), "Spanish email copy: {plain}");
    assert!(!plain.contains(EN_MISSED), "no English words");
}

/// TXT-039: with the switch off, the missed-call text and its email copy are English.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_missed_call_text_and_email_are_english_with_the_switch_off(pool: PgPool) {
    let texted = missed_call(&pool, false, false, Source::Read).await;
    assert!(missed_text(&pool, texted).await.contains(EN_MISSED));
    let emailed = missed_call(&pool, false, true, Source::Read).await;
    assert!(missed_email(&pool, emailed).await.contains(EN_MISSED));
}

/// D-3489, D-4052 (B22): with no language source, or a failed read, the missed-call text and its
/// email copy are English.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_missed_call_text_and_email_are_english_with_no_source_or_a_failed_read(pool: PgPool) {
    for source in [Source::Unset, Source::Fails] {
        let texted = missed_call(&pool, true, false, source).await;
        assert!(missed_text(&pool, texted).await.contains(EN_MISSED));
        let emailed = missed_call(&pool, true, true, source).await;
        assert!(missed_email(&pool, emailed).await.contains(EN_MISSED));
    }
}

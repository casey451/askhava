//! TD-TXT-29 and TD-QH-04 (T318; TXT-027, TXT-014, TXT-015, QH-004, PICK-018; D-1687, D-3118):
//! the who's-calling text after a release, through `calls::api` against PostgreSQL. Fake numbers
//! (555-01xx), names and IDs only.
//!
//! Each case seeds a business (with or without a public name), her vault contact, a job (regular
//! or emergency) and the job's masked line, queues the pick's task with `queue_whos_calling` and
//! runs `run_whos_calling` as the worker would, then reads `messaging.outbox` and `core.task`.

use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_calls::api::{
    WHOS_CALLING_EMAIL_TEMPLATE, WHOS_CALLING_KIND, WHOS_CALLING_TEMPLATE, WhosCalling,
    WhosCallingSent, generic_name, queue_whos_calling, run_whos_calling, send_whos_calling,
};
use askhava_core::api::Claimed;
use askhava_messaging::api::{IMPORTANT_TEMPLATES, Language};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const LINE: &str = "+19285550199";
const NAME: &str = "Fake Plumbing";
const CALL_BACK: &str = "If you miss it, call them back on this number. It works for 30 days.";
const IN_WRITING: &str =
    "Ask for the price and any changes in writing before work starts. Don't pay in full up front.";
const EMAIL_WORDS: &str = "They'll email you at the address you chose.";
const OUTBOX: &str = "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1";

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Arizona wall time on November `day`, 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let civil = date(2026, 11, day).at(hour, minute, 0, 0);
    civil.to_zoned(zone).expect("time").timestamp()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://fake@localhost/fake".to_owned());
    Vault::new(&key(0x31), &key(0x18), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-t318-000".to_owned())
}

/// How to seed: emergency job, email contact, a public name, and a line.
struct Seed {
    emergency: bool,
    email: bool,
    named: bool,
    line: bool,
}

const REGULAR: Seed = Seed {
    emergency: false,
    email: false,
    named: true,
    line: true,
};

/// A business, her fake vault contact, a job and (when asked) its masked line; the pick's call.
async fn seeded(pool: &PgPool, seed: &Seed) -> WhosCalling {
    let at = az(1, 9, 0);
    let mut db = pool.acquire().await.expect("conn");
    let business = askhava_businesses::api::create(&mut db, false, at)
        .await
        .expect("business");
    if seed.named {
        askhava_businesses::api::save_business_name(&mut db, business, (NAME, None), None)
            .await
            .expect("name");
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(format!("+192855501{n:02}"))),
        email: seed.email.then(|| SecretString::from("dana@example.test")),
        address: None,
        onsite_contact: None,
    };
    let homeowner = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let job = askhava_jobs::api::create(&mut db, at, seed.emergency, Actor::System)
        .await
        .expect("job");
    let call = WhosCalling {
        pick: Uuid::new_v4(),
        job,
        job_number: askhava_jobs::api::job_number(&mut db, job)
            .await
            .expect("n"),
        business,
        homeowner,
        email_contact: seed.email,
        emergency: seed.emergency,
    };
    if seed.line {
        open_line(pool, &call).await;
    }
    call
}

/// The job's masked line between her and the business, as `picks.line_retry` would open it.
async fn open_line(pool: &PgPool, call: &WhosCalling) {
    sqlx::query("INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) VALUES ($1, $2, $3, $4, false, $5)")
        .bind(call.job).bind(call.homeowner).bind(call.business).bind(LINE).bind(az(1, 9, 0).to_sqlx())
        .execute(pool).await.expect("line");
}

/// Runs the pick's task as the worker would, at `now`.
async fn run(pool: &PgPool, call: &WhosCalling, now: Timestamp) -> bool {
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 AND payload->>'pick' = $2::text ORDER BY run_at LIMIT 1")
            .bind(WHOS_CALLING_KIND).bind(call.pick).fetch_one(pool).await.expect("task");
    let task = Claimed {
        id: 0,
        kind: WHOS_CALLING_KIND.to_owned(),
        payload,
        attempts: 1,
    };
    run_whos_calling(pool.clone(), (&vault(), &secret()), task, now)
        .await
        .is_ok()
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Her one outbox text: template, body and whether it is exempt from the quiet hours.
async fn text_of(pool: &PgPool, call: &WhosCalling) -> (String, String, bool) {
    sqlx::query_as("SELECT template_id, body, quiet_exempt_until IS NOT NULL FROM messaging.outbox WHERE recipient_id = $1")
        .bind(call.homeowner).fetch_one(pool).await.expect("text")
}

/// TD-TXT-29: the text names the business and the masked number, holds the call-back and "in
/// writing" lines and no call window; it is an important text, and a repeated release or run
/// sends it once (D-3118).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_the_text_names_the_business_and_number_once(pool: PgPool) {
    let call = seeded(&pool, &REGULAR).await;
    let mut db = pool.acquire().await.expect("conn");
    assert!(
        queue_whos_calling(&mut db, &call, az(2, 10, 0))
            .await
            .expect("queue")
    );
    assert!(
        !queue_whos_calling(&mut db, &call, az(2, 10, 0))
            .await
            .expect("again")
    );
    assert!(run(&pool, &call, az(2, 10, 0)).await);
    assert!(run(&pool, &call, az(2, 10, 5)).await);
    assert_eq!(count(&pool, OUTBOX, call.homeowner).await, 1);
    let (template, body, _) = text_of(&pool, &call).await;
    assert_eq!(template, WHOS_CALLING_TEMPLATE);
    for words in [
        NAME,
        LINE,
        CALL_BACK,
        IN_WRITING,
        &call.job_number.to_string(),
    ] {
        assert!(body.contains(words), "{words} missing");
    }
    for window in ["window", "between", "a.m.", "p.m."] {
        assert!(!body.contains(window), "{window} in the text");
    }
    assert!(IMPORTANT_TEMPLATES.contains(&WHOS_CALLING_TEMPLATE));
    assert!(IMPORTANT_TEMPLATES.contains(&WHOS_CALLING_EMAIL_TEMPLATE));
}

/// TD-TXT-29: an email-contact homeowner still gets a text, with the email words and never "call".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_an_email_contact_gets_the_email_words_by_text(pool: PgPool) {
    let seed = Seed {
        email: true,
        ..REGULAR
    };
    let call = seeded(&pool, &seed).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = send_whos_calling(&mut db, (&vault(), &secret()), &call, az(2, 10, 0)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Queued));
    let (template, body, _) = text_of(&pool, &call).await;
    assert_eq!(template, WHOS_CALLING_EMAIL_TEMPLATE);
    assert!(body.contains(EMAIL_WORDS) && body.contains(IN_WRITING) && body.contains(NAME));
    assert!(!body.to_lowercase().contains("call"), "{body}");
}

/// TD-TXT-29: a business with no public name gets the generic wording.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_a_business_with_no_name_gets_the_generic_words(pool: PgPool) {
    let seed = Seed {
        named: false,
        ..REGULAR
    };
    let call = seeded(&pool, &seed).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = send_whos_calling(&mut db, (&vault(), &secret()), &call, az(2, 10, 0)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Queued));
    let (_, body, _) = text_of(&pool, &call).await;
    assert!(body.contains(generic_name(Language::En)), "{body}");
}

/// TD-TXT-29, D-3118: no line yet fails the task and sends nothing; once a line retry opens the
/// line, the next run sends it once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_a_later_line_retry_sends_it_once(pool: PgPool) {
    let seed = Seed {
        line: false,
        ..REGULAR
    };
    let call = seeded(&pool, &seed).await;
    let mut db = pool.acquire().await.expect("conn");
    assert!(
        queue_whos_calling(&mut db, &call, az(2, 10, 0))
            .await
            .expect("queue")
    );
    assert!(!run(&pool, &call, az(2, 10, 0)).await);
    assert_eq!(count(&pool, OUTBOX, call.homeowner).await, 0);
    open_line(&pool, &call).await;
    assert!(run(&pool, &call, az(2, 10, 1)).await);
    assert!(run(&pool, &call, az(2, 10, 9)).await);
    assert_eq!(count(&pool, OUTBOX, call.homeowner).await, 1);
}

/// TD-QH-04: on a job chosen as an emergency, at 23:10 the who's-calling text goes at once,
/// stored exempt from the quiet hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_04_an_emergency_jobs_text_goes_at_23_10(pool: PgPool) {
    let seed = Seed {
        emergency: true,
        ..REGULAR
    };
    let call = seeded(&pool, &seed).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = send_whos_calling(&mut db, (&vault(), &secret()), &call, az(2, 23, 10)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Queued));
    let (template, _, exempt) = text_of(&pool, &call).await;
    assert_eq!(template, WHOS_CALLING_TEMPLATE);
    assert!(exempt);
}

/// TD-QH-04: on a regular job the same text at 23:10 is held to 08:00:00: no text, one task at
/// the window's start, and that task sends it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_04_a_regular_jobs_text_is_held_to_8_am(pool: PgPool) {
    let call = seeded(&pool, &REGULAR).await;
    let mut db = pool.acquire().await.expect("conn");
    let eight = az(3, 8, 0);
    let sent = send_whos_calling(&mut db, (&vault(), &secret()), &call, az(2, 23, 10)).await;
    assert_eq!(sent, Ok(WhosCallingSent::Held { until: eight }));
    assert_eq!(count(&pool, OUTBOX, call.homeowner).await, 0);
    let due: Timestamp = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT run_at FROM core.task WHERE kind = $1 AND payload->>'pick' = $2::text",
    )
    .bind(WHOS_CALLING_KIND)
    .bind(call.pick)
    .fetch_one(&pool)
    .await
    .expect("held task")
    .to_jiff();
    assert_eq!(due, eight);
    assert!(run(&pool, &call, eight).await);
    let (_, _, exempt) = text_of(&pool, &call).await;
    assert!(!exempt);
}

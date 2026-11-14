//! A pick `payments` ended offers the homeowner the next quote, through the production worker
//! registry (T328; TD-PICK-54: FEE-018, FEE-041, QH-004; PICK-046, PICK-035, CON-028; D-2286,
//! D-3468, D-3579).
//!
//! The registry is `askhava_server::worker_registry` from dev stand-in settings, with the job-text
//! keys, on a manual clock; the composition root is compiled in place as
//! `emergency_notices.rs` does (D-2853's exception, logged for this file in D-3569). A homeowner
//! opens a request (consent text and record) and a plumbing job with three quotes from named
//! businesses; quote 1 is picked on the page. The `payments.pick_ended` event is queued as
//! `payments` queues it, every due task runs through `Registry::run`, and the outbox is read.
//! Every number (555-01xx), name, ID and word is fake.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Clock, IdSecret, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, brand, claim,
    enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, JobTextKeys, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_payments::api::PICK_ENDED_KIND;
use askhava_picks::api::{ConfirmSettings, PagePick, page_pick};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t328-not-a-real-key";
const PHONE: &str = "+19285550171";
/// 10:00 a.m. Arizona; 10:40 p.m. Arizona; 8:00 a.m. Arizona the next morning.
const DAY: &str = "2026-11-02T17:00:00Z";
const NIGHT: &str = "2026-11-03T05:40:00Z";
const EIGHT: &str = "2026-11-03T15:00:00Z";
const NAMES: [&str; 3] = ["Sample Pipe Co A", "Sample Pipe Co B", "Sample Pipe Co C"];

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn business(n: usize) -> Uuid {
    Uuid::from_u128(0x328_0000 + u128::try_from(n).expect("small"))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x58), &key(0x68), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The production registry from dev stand-in settings with the job-text keys, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t328-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let link = IdSecret::new(1, secret()).expect("link key");
    let keys = Arc::new(JobTextKeys::new(link, secret(), vault()));
    let parts = (Some(keys), ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            registry
                .run(pool, clock, owner, task, Lease::WORKER)
                .await
                .expect("run");
        }
    }
}

/// Three named businesses, each with a confirmed $190 quote on `job`, in arrival order.
async fn quoted(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    for (i, name) in NAMES.iter().enumerate() {
        let id = business(i + 1);
        sqlx::query(
            "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
             VALUES ($1, 'active', now(), now()) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("business");
        sqlx::query(
            "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
             VALUES ($1, $2, now()) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(*name)
        .execute(&mut *db)
        .await
        .expect("name");
        let received = at + SignedDuration::from_mins(i64::try_from(i).expect("small"));
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(id)
        .bind(received.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
}

/// A homeowner texts in (consent text and record), her job (an emergency when `emergency`) walks
/// to Quotes ready with three quotes; returns the job.
async fn quotes_ready(db: &mut PgConnection, emergency: bool, at: Timestamp) -> Uuid {
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
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
        received_at: at,
    };
    let handled = receive_text(db, &secret(), &msg, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    walk_to_quotes_ready(db, job, at).await;
    job
}

/// Stores `job`'s facts and walks it from Intake to Quotes ready with its three quotes.
async fn walk_to_quotes_ready(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{step:?}");
    }
    quoted(db, job, at).await;
}

/// The page pick of quote `quote` at `at`; the queue is then emptied, so only what the ended
/// pick queues runs.
async fn picked(db: &mut PgConnection, job: Uuid, quote: usize, at: Timestamp) -> Uuid {
    let Ok(PagePick::Picked(pick)) = page_pick(db, (job, false), quote, Actor::System, at).await
    else {
        panic!("a page pick");
    };
    sqlx::query("DELETE FROM core.task")
        .execute(&mut *db)
        .await
        .expect("fixture: an empty queue");
    pick
}

/// `payments.pick_ended` for `pick` of `job` by the business of quote `quote`, as `payments`
/// queues it (FEE-041's fee limit); `again` queues a second delivery under its own key.
async fn ended(db: &mut PgConnection, (job, pick, quote): (Uuid, Uuid, usize), again: bool) {
    let payload = json!({"pick": pick, "business": business(quote), "job": job,
        "reason": "fee_limit"});
    let mut task = NewTask::new(PICK_ENDED_KIND, payload, ts(DAY));
    let key = if again {
        format!("{pick}.again")
    } else {
        pick.to_string()
    };
    task.dedupe_key = Some(key);
    enqueue(db, &task).await.expect("event");
}

/// The bodies of the queued PICK-046 texts, oldest first.
async fn lapsed(db: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE template_id = 'pick_lapsed' ORDER BY created_at, id",
    )
    .fetch_all(db)
    .await
    .expect("outbox")
}

async fn pick_status(db: &mut PgConnection, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(db)
        .await
        .expect("pick")
}

/// The English text names the ended business, lists the other two quotes by their page numbers
/// (never its own), links her quote page and ends with the Terms line.
fn assert_offers_the_rest(body: &str) {
    let short = brand().fill("{brand_short}", false).expect("brand");
    let opening = format!("{short}: {} couldn't take it this time.", NAMES[0]);
    assert!(body.starts_with(&opening), "{body}");
    assert!(
        body.contains(&format!("\n2. {}: $190.\n", NAMES[1])),
        "{body}"
    );
    assert!(
        body.contains(&format!("\n3. {}: $190.\n", NAMES[2])),
        "{body}"
    );
    assert!(
        !body.contains(&format!("1. {}:", NAMES[0])),
        "PICK-035: {body}"
    );
    let page = format!("https://{}/q/", brand().domain());
    assert!(body.contains(&page), "{body}");
    assert!(
        body.ends_with(&format!("{}/terms", brand().domain())),
        "{body}"
    );
}

/// TD-PICK-54 (FEE-041, FEE-018, QH-004, PICK-046): at night a `payments.pick_ended` event ends
/// the pick's wait and returns the job to Quotes ready at once, and the homeowner's PICK-046 text
/// with the other quotes is held to 8 a.m.; a repeat of the event sends nothing twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_54_a_regular_job_gets_the_next_quote_text_at_8_am_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, 1, ts(NIGHT)).await;
    let clock = Arc::new(ManualClock::new(ts(NIGHT)));
    let registry = registry(clock.clone());
    ended(&mut db, (job, pick, 1), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(pick_status(&mut db, pick).await, "payment_failed");
    let status: String = sqlx::query_scalar("SELECT status FROM jobs.job WHERE id = $1")
        .bind(job)
        .fetch_one(&mut *db)
        .await
        .expect("job");
    assert_eq!(status, "quotes_ready", "the wait ended");
    assert!(lapsed(&mut db).await.is_empty(), "nothing at night");
    clock.set(ts(EIGHT) - SignedDuration::from_secs(1));
    run_due(&pool, &registry, &clock).await;
    assert!(lapsed(&mut db).await.is_empty(), "nothing at 7:59:59");
    clock.set(ts(EIGHT));
    run_due(&pool, &registry, &clock).await;
    let sent = lapsed(&mut db).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_offers_the_rest(&sent[0]);
    ended(&mut db, (job, pick, 1), true).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(
        lapsed(&mut db).await.len(),
        1,
        "a repeat sends nothing twice"
    );
}

/// TD-PICK-54 (QH-004): on a job the homeowner chose as an emergency the PICK-046 text goes at
/// once at night.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_54_an_emergency_job_gets_it_at_once_at_night(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, true, ts(DAY)).await;
    let pick = picked(&mut db, job, 1, ts(NIGHT)).await;
    let clock = Arc::new(ManualClock::new(ts(NIGHT)));
    let registry = registry(clock.clone());
    ended(&mut db, (job, pick, 1), false).await;
    run_due(&pool, &registry, &clock).await;
    let sent = lapsed(&mut db).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_offers_the_rest(&sent[0]);
}

/// TD-PICK-54 (PICK-046, TXT-039): a Spanish request gets the Spanish text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_54_a_spanish_request_gets_the_spanish_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    sqlx::query("UPDATE intake.conversation SET language = 'es'")
        .execute(&mut *db)
        .await
        .expect("fixture: a Spanish request");
    let pick = picked(&mut db, job, 1, ts(DAY)).await;
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    ended(&mut db, (job, pick, 1), false).await;
    run_due(&pool, &registry, &clock).await;
    let sent = lapsed(&mut db).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let opening = format!("{} no pudo tomarlo esta vez.", NAMES[0]);
    assert!(sent[0].contains(&opening), "{}", sent[0]);
    assert!(
        sent[0].contains(&format!("\n2. {}: $190.\n", NAMES[1])),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains("Al elegir, acepta los Términos"),
        "{}",
        sent[0]
    );
}

/// TD-PICK-54 (PICK-046, PICK-035): two ended picks on one job each get their own text, each
/// leaving out its own business's quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_54_two_ended_picks_on_one_job_each_get_theirs(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    let first = picked(&mut db, job, 1, ts(DAY)).await;
    ended(&mut db, (job, first, 1), false).await;
    run_due(&pool, &registry, &clock).await;
    let second = picked(&mut db, job, 2, ts(DAY)).await;
    ended(&mut db, (job, second, 2), false).await;
    run_due(&pool, &registry, &clock).await;
    let sent = lapsed(&mut db).await;
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_offers_the_rest(&sent[0]);
    let opening = format!("{} couldn't take it this time.", NAMES[1]);
    assert!(sent[1].contains(&opening), "{}", sent[1]);
    assert!(
        sent[1].contains(&format!("\n1. {}: $190.\n", NAMES[0])),
        "{}",
        sent[1]
    );
    assert!(
        !sent[1].contains(&format!("2. {}:", NAMES[1])),
        "{}",
        sent[1]
    );
}

/// TD-PICK-54 (CON-028): an opted-out number gets no PICK-046 text, while the pick still ends.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_54_an_opted_out_number_gets_no_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, 1, ts(DAY)).await;
    let channel = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), PHONE, channel, ts(DAY))
        .await
        .expect("suppress");
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    ended(&mut db, (job, pick, 1), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(pick_status(&mut db, pick).await, "payment_failed");
    assert!(lapsed(&mut db).await.is_empty(), "no text after an opt-out");
}

//! The withdrawn-pick notice, the opt-out-with-a-live-job event and the safety-call task through
//! the production worker registry (T330, D-2288, D-3134; TD-PICK-55: PICK-032, CON-029;
//! TD-CON-31: CON-028, CON-029; TD-CALL-19: CALL-019, CALL-022).
//!
//! The registry is `askhava_server::worker_registry`, built from dev stand-in settings with the
//! footer postal setting, both vault keys and the Twilio settings, so every T330 handler runs with
//! the composition's own parts. The layer table lets a test crate depend on modules but not on
//! `askhava-server` (D-736), so the composition root is compiled in place, as `task_registry.rs`
//! and `emergency_notices.rs` do (D-2853, D-3096, D-3463). Due tasks are claimed on a manual clock
//! and run through `Registry::run`. Every ID, number, name, address and word is fake.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create};
use askhava_core::api::{
    Clock, Guarded, Lease, ManualClock, NewTask, Registry, Settings, SettingsError, WorkerId,
    claim, enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    JobStart, OPTED_OUT_LIVE_KIND, Opened, PublishedTerms, TextIn, answer_text_pick, begin_job,
    on_opt_out, receive_text,
};
use askhava_jobs::api::{JobStatus, job_number, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_picks::api::{ConfirmSettings, PICK_WITHDRAWN_KIND, WITHDRAWN_TEMPLATE};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t330-not-a-real-key";
const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";
const EMAIL: &str = "dana.testperson@example.invalid";
const NUMBERS: &str = "+19285550110,+19285550111";
const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};

fn key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// The vault the registry builds from the same fake keys.
fn vault() -> Vault {
    let url = SecretString::from("postgres://fake@localhost/fake".to_owned());
    let (seal, index) = (key(0x30).into(), key(0x31).into());
    Vault::new(&seal, &index, &url).expect("vault")
}

/// Dev stand-in settings with the postal line, both vault keys and the Twilio settings.
fn pairs(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t330-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let base = [
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
        ("ASKHAVA_POSTAL_ADDRESS", POSTAL.into()),
        ("VAULT_KEY", key(0x30)),
        ("VAULT_INDEX_KEY", key(0x31)),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            format!("AC{}", "0".repeat(32)),
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token".into()),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            format!("VA{}1", "0".repeat(31)),
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "2".into()),
        ("ASKHAVA_MASKED_NUMBERS", NUMBERS.into()),
    ];
    let extra = extra.iter().map(|(k, v)| (*k, (*v).to_owned()));
    base.into_iter()
        .chain(extra)
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
}

/// The production registry from [`pairs`], on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    let pairs = pairs(&[]);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let parts = (None, ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// A daytime instant after every task's due time.
fn later() -> Timestamp {
    "2027-01-04T18:00:00Z".parse().expect("time")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
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

/// Queues a task of `kind` with `payload` and no dedupe key, as a repeated event.
async fn queue(pool: &PgPool, kind: &'static str, payload: Value) {
    let mut db = pool.acquire().await.expect("conn");
    enqueue(&mut db, &NewTask::new(kind, payload, at(0)))
        .await
        .expect("queue");
}

/// The payloads of `kind`'s tasks, oldest first.
async fn payloads(pool: &PgPool, kind: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(kind)
        .fetch_all(pool)
        .await
        .expect("tasks")
}

/// The withdrawn-pick business notices queued, by their payload.
async fn withdrawn_notices(pool: &PgPool) -> Vec<Value> {
    let all = payloads(pool, "messaging.business_notice").await;
    let template = json!(WITHDRAWN_TEMPLATE);
    all.into_iter()
        .filter(|p| p["template"] == template)
        .collect()
}

/// A business with an owner, a "Confirm and billing" member and a cards-only member; the business
/// and the three mailbox IDs.
async fn business(pool: &PgPool, seed: u128) -> (Uuid, [Uuid; 3]) {
    let mut db = pool.acquire().await.expect("conn");
    let t = at(0);
    let b = create(&mut db, false, t).await.expect("business");
    let boxes = [seed, seed + 1, seed + 2].map(Uuid::from_u128);
    let owner = add_owner(&mut db, b, boxes[0], t).await.expect("owner");
    let by = Acting {
        business: b,
        member: owner,
    };
    for (mailbox, roles) in [(boxes[1], (false, true)), (boxes[2], (true, false))] {
        let roles = Roles::new(roles.0, roles.1).expect("roles");
        add_member(&mut db, by, mailbox, roles, t)
            .await
            .expect("member");
    }
    (b, boxes)
}

/// Her vault contact, with the fake email when `email`.
async fn contact(pool: &PgPool, email: bool, phone: &str) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(phone)),
        email: email.then(|| SecretString::from(EMAIL)),
        address: None,
        onsite_contact: None,
    };
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

fn text(contact: Uuid, words: &str, minute: i64, opt_out: bool) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128((contact.as_u128() << 32) + u128::from(minute.unsigned_abs())),
        contact,
        inbound: Inbound {
            words,
            opt_out,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// Opens her request and one plumbing job in Quotes ready, quoted by `quoters` in order; the
/// request and the job.
async fn job_ready(pool: &PgPool, her: Uuid, quoters: &[Uuid]) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("conn");
    let first = text(
        her,
        "my kitchen sink is leaking under the cabinet",
        0,
        false,
    );
    let secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(&mut db, &secret, &first, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let start = JobStart {
        conversation,
        words: first.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
        panic!("a new job");
    };
    let types = ["leak repair".to_owned()];
    record_job_facts(&mut db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(&mut db, job, step, at(2), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    quote(pool, job, quoters).await;
    (conversation, job)
}

/// Each of `quoters` quotes on `job`, confirmed, in order (fake wording).
async fn quote(pool: &PgPool, job: Uuid, quoters: &[Uuid]) {
    for (n, quoter) in (3_i64..).zip(quoters) {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(quoter)
        .bind(at(n).to_sqlx())
        .execute(pool)
        .await
        .expect("quote");
    }
}

/// Her "<words>" text pick at `minute`.
async fn yes(pool: &PgPool, her: Uuid, words: &str, minute: i64) {
    let mut db = pool.acquire().await.expect("conn");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let msg = text(her, words, minute, false);
    answer_text_pick(&mut db, &secret, &msg, (IntakeSettings::default(), &TERMS))
        .await
        .expect("text pick");
}

/// How many texts are queued to `contact`.
async fn texts_to(pool: &PgPool, contact: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1")
        .bind(contact)
        .fetch_one(pool)
        .await
        .expect("texts")
}

/// The plain parts of the CON-028 emails stored for `contact`.
async fn still_open_emails(pool: &PgPool, contact: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT plain FROM messaging.email_outbox WHERE template_id = $1 AND recipient_id = $2",
    )
    .bind(askhava_intake::api::STILL_OPEN_EMAIL)
    .bind(contact)
    .fetch_all(pool)
    .await
    .expect("emails")
}

/// TD-PICK-55 (PICK-032): a second "<number> YES" replaces the waiting pick; through the worker
/// the first business's owner and "Confirm and billing" member, never its cards-only member, get
/// one withdrawn-pick notice naming the job; the same event again sends nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_55_a_replaced_pick_tells_its_business_once(pool: PgPool) {
    let (first, boxes) = business(&pool, 0x330_0100).await;
    let (second, _) = business(&pool, 0x330_0200).await;
    let her = contact(&pool, false, "+19285550120").await;
    let (_, job) = job_ready(&pool, her, &[first, second]).await;
    for (words, minute) in [("1 YES", 10), ("1 YES", 11), ("2 YES", 12)] {
        yes(&pool, her, words, minute).await;
    }
    let events = payloads(&pool, PICK_WITHDRAWN_KIND).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["reason"], json!("replaced"));
    let clock = Arc::new(ManualClock::new(later()));
    let worker = registry(clock.clone());
    run_due(&pool, &worker, &clock).await;
    let notices = withdrawn_notices(&pool).await;
    assert_eq!(notices.len(), 1, "one notice");
    let number = job_number(&mut pool.acquire().await.expect("conn"), job)
        .await
        .expect("number");
    assert_eq!(notices[0]["business"], json!(first));
    assert_eq!(notices[0]["pick"], events[0]["pick"]);
    assert_eq!(notices[0]["to"], json!([boxes[0], boxes[1]]));
    assert_eq!(notices[0]["values"], json!({"job": number}));
    queue(&pool, PICK_WITHDRAWN_KIND, events[0].clone()).await;
    run_due(&pool, &worker, &clock).await;
    assert_eq!(
        withdrawn_notices(&pool).await.len(),
        1,
        "a repeat sends nothing"
    );
}

/// TD-PICK-55: an event with a reason other than `replaced` or `paused` tells nobody.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_55_an_unknown_reason_tells_nobody(pool: PgPool) {
    let (b, _) = business(&pool, 0x330_0300).await;
    let ids = json!({"pick": Uuid::from_u128(1), "job": Uuid::from_u128(2), "business": b});
    let mut payload = ids.clone();
    payload["reason"] = json!("expired");
    queue(&pool, PICK_WITHDRAWN_KIND, payload).await;
    let clock = Arc::new(ManualClock::new(later()));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    assert!(withdrawn_notices(&pool).await.is_empty());
}

/// TD-CON-31 (CON-028, CON-029): a sole-word CANCEL from her number with a waiting pick withdraws
/// it and, through the worker, its business is told once (`paused`) and she gets the CON-028 line
/// by email once, never a text; the same event again stores and sends nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_31_cancel_withdraws_tells_the_business_and_emails_her_once(pool: PgPool) {
    let (b, boxes) = business(&pool, 0x330_0400).await;
    let her = contact(&pool, true, "+19285550121").await;
    let (_, job) = job_ready(&pool, her, &[b]).await;
    yes(&pool, her, "1 YES", 10).await;
    yes(&pool, her, "1 YES", 11).await;
    let before = texts_to(&pool, her).await;
    let stop = text(her, "CANCEL", 12, true);
    let mut db = pool.acquire().await.expect("conn");
    let done = on_opt_out(&mut db, &stop)
        .await
        .expect("intake")
        .expect("live");
    drop(db);
    assert!(done.paused);
    let clock = Arc::new(ManualClock::new(later()));
    let worker = registry(clock.clone());
    run_due(&pool, &worker, &clock).await;
    let notices = withdrawn_notices(&pool).await;
    assert_eq!(notices.len(), 1, "the business told once");
    assert_eq!(notices[0]["business"], json!(b));
    assert_eq!(notices[0]["to"], json!([boxes[0], boxes[1]]));
    let number = job_number(&mut pool.acquire().await.expect("conn"), job)
        .await
        .expect("number");
    let line = format!("Your request {number} is still open. Close it on your quote page.");
    let emails = still_open_emails(&pool, her).await;
    assert_eq!(emails.len(), 1, "one email");
    assert!(emails[0].contains(&line), "the CON-028 line");
    assert_eq!(
        texts_to(&pool, her).await,
        before,
        "no text follows the opt-out"
    );
    let event = payloads(&pool, OPTED_OUT_LIVE_KIND).await.remove(0);
    queue(&pool, OPTED_OUT_LIVE_KIND, event).await;
    run_due(&pool, &worker, &clock).await;
    assert_eq!(
        still_open_emails(&pool, her).await.len(),
        1,
        "a repeat stores nothing"
    );
    assert_eq!(withdrawn_notices(&pool).await.len(), 1, "no second notice");
    assert_eq!(texts_to(&pool, her).await, before);
}

/// TD-CON-31 (CON-028): an opt-out that is not CANCEL, from a number with no email on file, keeps
/// the waiting pick, tells no business and stores no email and no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_31_no_email_on_file_sends_nothing(pool: PgPool) {
    let (b, _) = business(&pool, 0x330_0500).await;
    let her = contact(&pool, false, "+19285550122").await;
    job_ready(&pool, her, &[b]).await;
    yes(&pool, her, "1 YES", 10).await;
    yes(&pool, her, "1 YES", 11).await;
    let before = texts_to(&pool, her).await;
    let stop = text(her, "STOP", 12, true);
    let mut db = pool.acquire().await.expect("conn");
    let done = on_opt_out(&mut db, &stop)
        .await
        .expect("intake")
        .expect("live");
    drop(db);
    assert!(!done.paused);
    let clock = Arc::new(ManualClock::new(later()));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    assert_eq!(payloads(&pool, OPTED_OUT_LIVE_KIND).await.len(), 1);
    assert!(
        still_open_emails(&pool, her).await.is_empty(),
        "no email on file"
    );
    assert!(withdrawn_notices(&pool).await.is_empty(), "the pick stands");
    assert_eq!(texts_to(&pool, her).await, before);
}

/// Whether each pick of `job` was withdrawn by a pause, oldest first.
async fn paused_picks(pool: &PgPool, job: Uuid) -> Vec<bool> {
    sqlx::query_scalar(
        "SELECT status = 'cancelled' AND end_reason IS NOT DISTINCT FROM 'paused' \
         FROM picks.pick WHERE job_id = $1 ORDER BY picked_at, id",
    )
    .bind(job)
    .fetch_all(pool)
    .await
    .expect("picks")
}

/// TD-CON-31 (CON-029): the handler itself withdraws the waiting pick of a job still paused by
/// the homeowner and its business is told once; a job whose pause has ended keeps its pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_31_the_handler_withdraws_only_while_the_job_is_paused(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(later()));
    let worker = registry(clock.clone());
    for (n, ended) in [(0_u128, false), (1, true)] {
        let (b, _) = business(&pool, 0x330_0900 + n * 0x10).await;
        let her = contact(&pool, false, &format!("+1928555013{n}")).await;
        let (conversation, job) = job_ready(&pool, her, &[b]).await;
        yes(&pool, her, "1 YES", 10).await;
        yes(&pool, her, "1 YES", 11).await;
        let inbox = Uuid::from_u128(0x330_0a00 + n);
        sqlx::query(
            "INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at, \
             ended_at) VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN $4 END)",
        )
        .bind(conversation)
        .bind(job)
        .bind(inbox)
        .bind(at(12).to_sqlx())
        .bind(ended)
        .execute(&pool)
        .await
        .expect("pause");
        let payload = json!({
            "conversation": conversation, "jobs": [job], "paused": true, "inbox": inbox,
        });
        queue(&pool, OPTED_OUT_LIVE_KIND, payload).await;
        run_due(&pool, &worker, &clock).await;
        assert_eq!(paused_picks(&pool, job).await, vec![!ended], "{n}");
        let told = withdrawn_notices(&pool).await;
        let ours: Vec<_> = told.iter().filter(|p| p["business"] == json!(b)).collect();
        assert_eq!(ours.len(), usize::from(!ended), "{n}");
    }
}

/// Whether task `kind` finished (done, not dead-lettered, no error).
async fn finished(pool: &PgPool, kind: &str) -> Vec<(bool, bool, Option<String>)> {
    sqlx::query_as(
        "SELECT done_at IS NOT NULL, dead_at IS NOT NULL, last_error FROM core.task \
         WHERE kind = $1 ORDER BY id",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .expect("tasks")
}

/// TD-CALL-19 (CALL-019, CALL-022, D-3134): the production worker registers `calls.safety_call`
/// under the Twilio settings; with the switch off (the default, the only state until T407) each
/// task is run by that handler, places no call, writes no call audit and completes, never a dead
/// letter; the switch is read on each run, so a second task does the same.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_19_a_safety_call_task_completes_with_the_switch_off(pool: PgPool) {
    use askhava_calls::api::SAFETY_CALL_KIND;
    let clock = Arc::new(ManualClock::new(later()));
    let worker = registry(clock.clone());
    let listed = format!("{worker:?}");
    assert!(
        listed.contains(&format!("\"{SAFETY_CALL_KIND}\"")),
        "registered"
    );
    for n in 1..=2_u128 {
        let payload = json!({
            "contact": Uuid::from_u128(0x330_0600 + n),
            "message": Uuid::from_u128(0x330_0700 + n),
            "arrived_at": later().to_string(),
            "alert": Uuid::from_u128(0x330_0800 + n),
        });
        queue(&pool, SAFETY_CALL_KIND, payload).await;
    }
    run_due(&pool, &worker, &clock).await;
    let runs = finished(&pool, SAFETY_CALL_KIND).await;
    assert_eq!(runs, vec![(true, false, None), (true, false, None)]);
    let calls: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE action LIKE 'calls.safety_call%'",
    )
    .fetch_one(&pool)
    .await
    .expect("audit");
    assert_eq!(calls, 0, "no call placed or claimed");
}

/// TD-CALL-19 (CALL-019, CALL-022): a malformed safety-call payload is refused, not completed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_19_a_malformed_safety_call_is_refused(pool: PgPool) {
    use askhava_calls::api::SAFETY_CALL_KIND;
    queue(&pool, SAFETY_CALL_KIND, json!({"contact": "not an id"})).await;
    let clock = Arc::new(ManualClock::new(later()));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    let runs = finished(&pool, SAFETY_CALL_KIND).await;
    assert_eq!(runs.len(), 1);
    assert!(!runs[0].0, "not completed");
    assert!(runs[0].2.is_some(), "a fixed error sentence");
}

/// CALL-022, D-3134: `ASKHAVA_MAIN_NUMBER` is optional, read in E.164, and a bad value stops
/// start-up naming the key, never the value.
#[test]
fn the_main_number_setting_is_checked_and_never_echoed() {
    let unset = Settings::from_pairs(pairs(&[])).expect("settings");
    assert_eq!(unset.main_number(), None);
    let set = Settings::from_pairs(pairs(&[("ASKHAVA_MAIN_NUMBER", " +19285550199 ")]));
    assert_eq!(set.expect("settings").main_number(), Some("+19285550199"));
    let bad: BTreeSet<&str> = ["9285550199", "+0928555019", "+1928555O199", "+1234567", ""].into();
    for value in bad.into_iter().filter(|v| !v.is_empty()) {
        let err = Settings::from_pairs(pairs(&[("ASKHAVA_MAIN_NUMBER", value)])).unwrap_err();
        assert_eq!(
            err,
            SettingsError::Invalid("ASKHAVA_MAIN_NUMBER"),
            "{value}"
        );
        assert!(!err.to_string().contains(value), "never echoed");
    }
}

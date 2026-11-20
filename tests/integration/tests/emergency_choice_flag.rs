//! INT-029's emergency choice marks the request's jobs an emergency in `jobs`, and each emergency
//! job of a request is released once (T747; INT-029, PICK-044, PICK-043, FEE-045; D-4748, D-3189,
//! D-2331).
//!
//! Each case runs on a fresh database with fake IDs and sample words only: the homeowner's "1" at
//! the urgent choice (`intake::api::answer_choice`) sets `jobs.job.emergency` false to true on the
//! first job and the ALSO job, never back; the pick notice then starts `jobs`' 60 clock minutes
//! with one reminder at 30 (PICK-044); a pick with unsettled facts is not refused (T340, D-2331);
//! HOLD is refused (PICK-043, T456); and a request with two emergency cards queues one
//! `intake.emergency_released` event per job, each sending its own card once.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, Roles, add_owner, apply, create, request_welcome,
    save_business_name, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Claimed, Guarded};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    CardsOutKeys, EMERGENCY_RELEASED_KIND, JobStart, Opened, TextIn, ZipStep, answer_choice,
    answer_zip, ask_zip, begin_job, receive_text, release_emergency, run_emergency_released,
};
use askhava_jobs::api::{JobStatus, TIMER_KIND, current, mark_emergency, move_to};
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_picks::api::{
    CONFIRM_ENDS_KIND, ConfirmSettings, PagePick, TimeRefusal, WindowRules, emergency_plan,
    emergency_reminders, hold_pick, notice_sent, page_pick,
};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, NewContact, Vault, scrub, store_contact,
};
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, Place, build_card, card_digest, keep_card,
    keep_description,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const CLEANING: &str = "house_cleaning";
const REGULAR: &str = "regular cleaning";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-9001,Sample Shine One LLC,,,2019-03-04\n";
const WORDS: &str = "please clean my whole house every other week";

/// 2 p.m. Arizona time on November 9, 2026, plus `minutes`.
fn at(minutes: i64) -> Timestamp {
    let base: Timestamp = "2026-11-09T14:00:00-07:00".parse().expect("time");
    base + SignedDuration::from_mins(minutes)
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-747".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x67), &url).expect("vault")
}

fn text(contact: Uuid, words: &str, minutes: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minutes),
    }
}

/// Opens `her` request and its first job, not an emergency, for `trade`.
async fn open(db: &mut PgConnection, her: Uuid, trade: &str) -> (Uuid, Uuid) {
    let handled = receive_text(db, &secret(), &text(her, WORDS, 0), Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    (conversation, also(db, conversation, trade).await)
}

/// Opens another job of request `conversation` for `trade` (the ALSO reply's second job).
async fn also(db: &mut PgConnection, conversation: Uuid, trade: &str) -> Uuid {
    let start = JobStart {
        conversation,
        words: WORDS,
        emergency: false,
        trade,
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    job
}

/// `her` answers the urgent choice with `words` ("1" emergency, "2" regular).
async fn choose(db: &mut PgConnection, her: Uuid, conversation: Uuid, words: &str) -> bool {
    let answered = answer_choice(db, &text(her, words, 2), conversation).await;
    answered.expect("choice").released
}

async fn flag(db: &mut PgConnection, job: Uuid) -> bool {
    current(db, job).await.expect("job").emergency
}

async fn moved(db: &mut PgConnection, job: Uuid, steps: &[(JobStatus, JobStatus)], when: i64) {
    for step in steps {
        let done = move_to(db, job, *step, at(when), Actor::System).await;
        assert!(matches!(done, Ok(Guarded::Applied(()))), "move");
    }
}

const TO_QUOTES: [(JobStatus, JobStatus); 2] = [
    (JobStatus::Intake, JobStatus::Open),
    (JobStatus::Open, JobStatus::QuotesReady),
];

/// INT-029, PICK-044, D-4748: "1" marks both jobs of the request an emergency, bumping the version
/// and timer version with a timer under the new fence; "2" afterwards, a second mark and a
/// regular choice elsewhere change nothing, and a final job is never marked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_choice_marks_every_job_and_nothing_clears_it(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let her = Uuid::from_u128(0x0747_0001);
    let (conversation, first) = open(&mut db, her, "plumbing").await;
    let second = also(&mut db, conversation, "landscape_maintenance").await;
    let before = current(&mut db, first).await.expect("job");
    assert!(!before.emergency && !flag(&mut db, second).await);
    choose(&mut db, her, conversation, "1").await;
    let after = current(&mut db, first).await.expect("job");
    assert!(after.emergency && flag(&mut db, second).await);
    assert_eq!(after.version, before.version + 1);
    assert_eq!(after.timer_version, before.timer_version + 1);
    let fence: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND payload->>'job' = $2::text \
         AND (payload->>'version')::int = $3 AND (payload->>'timer')::int = $4",
    )
    .bind(TIMER_KIND)
    .bind(first)
    .bind(after.version)
    .bind(after.timer_version)
    .fetch_one(&mut *db)
    .await
    .expect("timer");
    assert_eq!(fence, 1, "the timer under the new fence");
    choose(&mut db, her, conversation, "2").await;
    assert!(flag(&mut db, first).await && flag(&mut db, second).await);
    let again = mark_emergency(&mut db, first, at(3)).await;
    assert_eq!(again, Ok(false));
    assert_eq!(
        current(&mut db, first).await.expect("job").version,
        after.version
    );
    let other = Uuid::from_u128(0x0747_0002);
    let (regular, job) = open(&mut db, other, "plumbing").await;
    choose(&mut db, other, regular, "2").await;
    assert!(!flag(&mut db, job).await, "regular changes nothing");
    let closed = Uuid::from_u128(0x0747_0003);
    let (request, job) = open(&mut db, closed, "plumbing").await;
    moved(
        &mut db,
        job,
        &[(JobStatus::Intake, JobStatus::Cancelled)],
        1,
    )
    .await;
    choose(&mut db, closed, request, "1").await;
    assert!(!flag(&mut db, job).await, "a final job is never marked");
}

/// A chosen emergency job in Awaiting confirm since `at(10)` with one waiting page pick.
async fn chosen_and_picked(pool: &PgPool, n: u128) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("conn");
    let her = Uuid::from_u128(0x0747_0100 + n);
    let (conversation, job) = open(&mut db, her, "plumbing").await;
    choose(&mut db, her, conversation, "1").await;
    let mut steps = TO_QUOTES.to_vec();
    steps.push((JobStatus::QuotesReady, JobStatus::AwaitingConfirm));
    moved(&mut db, job, &steps, 10).await;
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(Uuid::from_u128(0xb747_0000 + n))
    .bind(at(10).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    (job, pick)
}

async fn due(pool: &PgPool, kind: &str, job: Uuid) -> Vec<Timestamp> {
    let rows: Vec<jiff_sqlx::Timestamp> = sqlx::query_scalar(
        "SELECT run_at FROM core.task WHERE kind = $1 AND payload->>'job' = $2::text \
         AND done_at IS NULL ORDER BY run_at",
    )
    .bind(kind)
    .bind(job)
    .fetch_all(pool)
    .await
    .expect("tasks");
    rows.into_iter().map(|t| t.to_jiff()).collect()
}

/// PICK-044, PICK-043 (T456): on the chosen emergency the pick notice starts no business-hours
/// window but `jobs`' 60 clock minutes, ending in one Confirm-end task, with one reminder at 30
/// minutes; "Hold for 2 more hours" is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_pick_gets_60_minutes_one_reminder_and_no_hold(pool: PgPool) {
    let (job, pick) = chosen_and_picked(&pool, 1).await;
    let mut db = pool.acquire().await.expect("conn");
    let sent = notice_sent(&mut db, pick, at(10), ConfirmSettings::default()).await;
    assert_eq!(sent, Ok(None), "no business-hours end");
    assert_eq!(due(&pool, CONFIRM_ENDS_KIND, job).await, [at(70)]);
    assert!(due(&pool, TIMER_KIND, job).await.contains(&at(70)));
    let plan = emergency_plan(at(10), false, WindowRules::default());
    let reminders = emergency_reminders(&plan);
    assert_eq!(reminders.len(), 1);
    assert_eq!((plan.confirm_by, reminders[0].at), (at(70), at(40)));
    let held = hold_pick(&mut db, pick, Roles::BOTH, at(15), WindowRules::default()).await;
    assert_eq!(held, Ok(Err(TimeRefusal::Emergency)));
}

/// FEE-005, FEE-045, D-2331 (T340): the chosen emergency job with no facts row is picked (a
/// non-emergency one is refused for the same unsettled facts).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn unsettled_facts_never_refuse_the_chosen_emergency(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    load_embedded(&mut db).await.expect("catalog");
    let her = Uuid::from_u128(0x0747_0201);
    let (conversation, job) = open(&mut db, her, "plumbing").await;
    choose(&mut db, her, conversation, "1").await;
    moved(&mut db, job, &TO_QUOTES, 2).await;
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, period, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 90000, NULL, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0xb747_0201))
    .bind(at(3).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, at(5)).await;
    assert!(matches!(picked, Ok(PagePick::Picked(_))), "{picked:?}");
    tx.commit().await.expect("commit");
}

/// One active cleaning business in the town with an owner (a fake mailbox).
async fn business(db: &mut PgConnection) {
    let t: Timestamp = "2026-11-01T17:00:00Z".parse().expect("time");
    let b = create(db, false, t).await.expect("business");
    add_owner(db, b, Uuid::new_v4(), t).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    request_welcome(db, b, t).await.expect("welcome");
    welcome_sent(db, b, t).await.expect("sent");
    save_business_name(db, b, ("Sample Shine One LLC", None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(CLEANING, REGULAR);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
}

/// Stores `job`'s scrubbed words and its card carrying the emergency mark.
async fn emergency_card(db: &mut PgConnection, conversation: Uuid, job: Uuid) {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let words = "Whole house cleaning every other week, three bedrooms";
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    let ids = (conversation, job);
    let kept = keep_description(db, ids, Uuid::new_v4(), &scrubbed, at(1)).await;
    assert_eq!(kept, Ok(true));
    let parts = CardParts {
        category: "House cleaning".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: scrubbed.as_str().to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: CLEANING.to_owned(),
        candidates: vec![REGULAR.to_owned()],
        emergency: true,
    };
    keep_card(db, ids, &card_digest(card.text()), &card, &facts, at(1))
        .await
        .expect("kept");
}

/// The open `intake.emergency_released` tasks' payloads, oldest first.
async fn releases(db: &mut PgConnection) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(EMERGENCY_RELEASED_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

async fn cards(db: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM quotes.job_card WHERE job_id = $1")
        .bind(job)
        .fetch_one(db)
        .await
        .expect("cards")
}

/// A request with consent and its ZIP in, two jobs (the second as an ALSO or SPLIT opens it) and
/// an emergency card stored for each.
async fn two_emergency_jobs(db: &mut PgConnection) -> (Uuid, Uuid, [Uuid; 2]) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550147".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let (conversation, first) = open(db, her, CLEANING).await;
    let second = also(db, conversation, "landscape_maintenance").await;
    let first_ask = text(her, "", 1);
    ask_zip(
        db,
        &secret(),
        (&first_ask, conversation),
        Settings::default(),
    )
    .await
    .expect("asked");
    let reply = text(her, "86403", 1);
    let zip = answer_zip(db, &secret(), (&reply, conversation), Settings::default()).await;
    assert!(matches!(zip.expect("zip"), ZipStep::Stored(_)));
    for job in [first, second] {
        emergency_card(db, conversation, job).await;
    }
    (her, conversation, [first, second])
}

/// INT-029, D-3189, D-4748: the choice queues one release per emergency job, each carrying its
/// job; each run sends that job's card once, a rerun and a second release add nothing, and an
/// event queued before T747 (no `job`) still sends the request's first emergency card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_split_with_two_emergency_jobs_releases_each_card_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    import_city_list(&mut db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    business(&mut db).await;
    let (her, conversation, jobs) = two_emergency_jobs(&mut db).await;
    assert!(choose(&mut db, her, conversation, "1").await, "released");
    let payloads = releases(&mut db).await;
    let named: BTreeSet<String> = payloads
        .iter()
        .map(|p| p["job"].as_str().expect("a job").to_owned())
        .collect();
    let expected: BTreeSet<String> = jobs.iter().map(Uuid::to_string).collect();
    assert_eq!((payloads.len(), named), (2, expected));
    let again = release_emergency(&mut db, conversation, at(3)).await;
    assert_eq!(again, Ok(false), "each job once");
    assert_eq!(releases(&mut db).await.len(), 2);
    let keys = Arc::new(CardsOutKeys::new(secret(), vault()));
    let legacy = serde_json::json!({ "conversation": conversation });
    for payload in payloads.iter().chain(payloads.iter()).chain([&legacy]) {
        let task = Claimed {
            id: 1,
            kind: EMERGENCY_RELEASED_KIND.to_owned(),
            payload: payload.clone(),
            attempts: 1,
        };
        let ran = run_emergency_released(pool.clone(), keys.clone(), task).await;
        assert!(ran.is_ok(), "{ran:?}");
    }
    for job in jobs {
        assert_eq!(cards(&mut db, job).await, 1, "one card each, never twice");
        assert!(flag(&mut db, job).await);
    }
}

//! Worker handlers on the worker clock, not the wall clock (T600; D-4196, D-4310, D-3447's
//! pattern; PICK-001, INT-005, INT-019, INT-027, INT-041, BIZ-029, PRIV-007, PRIV-009, SEC-035).
//! Fake numbers (555-01xx), IDs and words only.
//!
//! Each case runs a handler's `*_at` version at [`ahead`], a 10:00 Arizona time more than two days
//! after the wall clock, as a test-control clock advance does, and checks the step it does there:
//! a wall-clock read would see the card shown in the future, the reply box still open, the wait
//! not yet over, or would stamp its rows at the wrong time. The last case runs each `(pool, task)`
//! wrapper once, which still reads the wall clock.

use askhava_businesses::api::{
    BOX_CLOSED_KIND, COMPLAINT_HOLD_KIND, NewComplaint, create, record_complaint, run_box_closed,
    run_box_closed_at, run_complaint_holds, run_complaint_holds_at,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::Claimed;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, AUTO_SENT_ACTION, CARD_TIMER_KIND, CARDS_APPROVED_KIND, CardsOutKeys,
    JobStart, NO_REPLY_KIND, Opened, Preview, REMINDER_TEMPLATE, Shown, TextIn, ZipStep,
    answer_zip, ask_zip, begin_job, receive_text, run_address_needed_at, run_card_timer,
    run_card_timer_at, run_cards_approved, run_cards_approved_at, run_no_reply_at, show_card,
};
use askhava_integration::set_app_role;
use askhava_jobs::api::PURGE_DUE_KIND;
use askhava_picks::api::ADDRESS_NEEDED_KIND;
use askhava_privacy::api::{
    NewContact, PHOTO_DELETE_KIND, Vault, run_purge_due, run_purge_due_at, store_contact,
};
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, JobCard, Place, build_card, card_digest, keep_card,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use std::sync::Arc;

const CLEANING: &str = "house_cleaning";
const WORDS: &str = "please clean my whole house every other week";

/// 17:00 UTC (10:00 Arizona) on the third day after today: always more than two days ahead of
/// the wall clock, and inside every texting window.
fn ahead() -> Timestamp {
    let day = Timestamp::now().as_second().div_euclid(86_400) + 3;
    Timestamp::from_second(day * 86_400 + 17 * 3_600).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-600".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x56), &key(0x66), &url).expect("vault")
}

fn keys() -> CardsOutKeys {
    CardsOutKeys::new(secret(), vault())
}

fn claimed(kind: &str, payload: Value) -> Claimed {
    Claimed {
        id: 600,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

fn text(contact: Uuid, words: &str, at: Timestamp) -> TextIn<'_> {
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: at,
    }
}

async fn setup(pool: &PgPool) -> PgConnection {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    db
}

/// A homeowner texts in at `at`, gets a house-cleaning job and answers the ZIP question;
/// (her contact, request, job).
async fn request(db: &mut PgConnection, phone: &str, at: Timestamp) -> (Uuid, Uuid, Uuid) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let handled = receive_text(db, &secret(), &text(her, WORDS, at), Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: WORDS,
        emergency: false,
        trade: CLEANING,
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let ids = (&text(her, "", at), conversation);
    ask_zip(db, &secret(), ids, Settings::default())
        .await
        .expect("asked");
    let reply = text(her, "86403", at);
    let zip = answer_zip(db, &secret(), (&reply, conversation), Settings::default()).await;
    assert!(matches!(zip.expect("zip"), ZipStep::Stored(_)));
    (her, conversation, job)
}

/// Stores the job's card at `at` and shows it to her (INT-041), which queues its timer.
async fn shown(db: &mut PgConnection, (conversation, job): (Uuid, Uuid), at: Timestamp) {
    let parts = CardParts {
        category: "House cleaning".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "Whole house cleaning every other week, three bedrooms".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card: JobCard = build_card(&parts, DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: CLEANING.to_owned(),
        candidates: vec!["regular cleaning".to_owned()],
        emergency: false,
    };
    let digest = card_digest(card.text());
    keep_card(db, (conversation, job), &digest, &card, &facts, at)
        .await
        .expect("kept");
    let preview = Preview {
        conversation,
        job,
        card: &card,
        link: "https://example.com/q/fake-token",
        wait: AUTO_SEND_AFTER,
        at,
    };
    let done = show_card(db, &secret(), &preview).await.expect("shown");
    assert!(matches!(done, Shown::Texted(_)));
}

/// The payload of `job`'s latest card timer: the watch first, then the wait it starts.
async fn latest_timer(pool: &PgPool, job: Uuid) -> Value {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY id DESC LIMIT 1",
    )
    .bind(CARD_TIMER_KIND)
    .bind(job.to_string())
    .fetch_one(pool)
    .await
    .expect("timer")
}

/// Marks the card text the timer watches as sent at `at`, as the outbox sender would.
async fn delivered(pool: &PgPool, timer: &Value, at: Timestamp) {
    sqlx::query(
        "UPDATE messaging.outbox SET status = 'sent', sent_at = $2, provider_sid = 'SM00000000000000000000000000000600', \
         body = NULL WHERE id = $1::uuid",
    )
    .bind(timer["outbox"].as_str().expect("outbox"))
    .bind(at.to_sqlx())
    .execute(pool)
    .await
    .expect("sent");
}

/// The payload of the first queued `kind` task about `job`.
async fn queued(pool: &PgPool, kind: &str, job: Uuid) -> Value {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY id LIMIT 1",
    )
    .bind(kind)
    .bind(job.to_string())
    .fetch_one(pool)
    .await
    .expect("queued task")
}

async fn is_true(pool: &PgPool, sql: &'static str, id: Uuid, at: Timestamp) -> bool {
    let row: Option<bool> = sqlx::query_scalar(sql)
        .bind(id)
        .bind(at.to_sqlx())
        .fetch_optional(pool)
        .await
        .expect("query");
    row == Some(true)
}

/// (a) and (d), PICK-001, INT-041, D-4196, D-4310: the card timer run at `ahead` approves the card
/// by the system actor, and the cards-approved step run then stamps `shown_card.sent_at` at that
/// time, which opens each business's 24-hour quote window.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_card_timer_and_cards_approved_run_on_the_worker_clock(pool: PgPool) {
    let at = ahead();
    let mut db = setup(&pool).await;
    let (_, conversation, job) = request(&mut db, "+19285550161", at).await;
    shown(&mut db, (conversation, job), at).await;
    let mut due = at;
    for _ in 0..2 {
        let payload = latest_timer(&pool, job).await;
        if due == at {
            delivered(&pool, &payload, at).await;
        }
        due = Timestamp::from_microsecond(payload["due"].as_i64().expect("due")).expect("due");
        let timer = claimed(CARD_TIMER_KIND, payload);
        run_card_timer_at(pool.clone(), timer, due)
            .await
            .expect("timer");
    }
    assert!(due >= at + AUTO_SEND_AFTER, "the wait ran its 30 minutes");
    let by_system = "SELECT count(*) = 1 FROM ops.audit_log WHERE action = $1 \
                     AND actor_kind = 'system'";
    let n: bool = sqlx::query_scalar(by_system)
        .bind(AUTO_SENT_ACTION)
        .fetch_one(&pool)
        .await
        .expect("audit");
    assert!(n, "the card is approved by the system actor");
    let approved = queued(&pool, CARDS_APPROVED_KIND, job).await;
    let sent_at = due + SignedDuration::from_mins(9);
    let task = claimed(CARDS_APPROVED_KIND, approved);
    run_cards_approved_at(pool.clone(), Arc::new(keys()), task, sent_at)
        .await
        .expect("sent");
    let stamp =
        "SELECT sent_at = $2 FROM quotes.shown_card WHERE job_id = $1 AND sent_at IS NOT NULL";
    assert!(
        is_true(&pool, stamp, job, sent_at).await,
        "sent_at is the worker's time"
    );
}

/// (d), INT-019, INT-027, D-4310: the no-reply reminder run 24 hours after a text at `ahead`
/// queues the reminder, and the address-needed step asks for her name and address at its time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_no_reply_and_address_steps_run_on_the_worker_clock(pool: PgPool) {
    let at = ahead();
    let mut db = setup(&pool).await;
    let (_, _, job) = request(&mut db, "+19285550162", at).await;
    let remind = claimed(NO_REPLY_KIND, queued(&pool, NO_REPLY_KIND, job).await);
    let next_day = at + SignedDuration::from_hours(24);
    run_no_reply_at(pool.clone(), secret(), remind, next_day)
        .await
        .expect("reminder step");
    let reminders: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
            .bind(REMINDER_TEMPLATE)
            .fetch_one(&pool)
            .await
            .expect("outbox");
    assert_eq!(reminders, 1, "the INT-019 reminder is queued");
    let pick = Uuid::from_u128(0x600);
    let ask = claimed(ADDRESS_NEEDED_KIND, json!({ "job": job, "pick": pick }));
    run_address_needed_at(pool.clone(), secret(), ask, next_day)
        .await
        .expect("asked");
    let mark = "SELECT bool_and(answered_at = $2) FROM intake.intake_answer \
                WHERE question_id = 'address_wait' AND choice = replace($1::text, '-', '')";
    assert!(
        is_true(&pool, mark, pick, next_day).await,
        "asked at the worker's time"
    );
    let asks: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE $1 = ANY(subject_ids)")
            .bind(pick)
            .fetch_one(&pool)
            .await
            .expect("outbox");
    assert_eq!(asks, 1, "the name-and-address ask is queued");
}

/// (b), BIZ-029, D-4196: a complaint received 120 hours before `ahead` is listed at `ahead`, so
/// the box-closed task finishes rather than retrying until it dead-letters.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_reply_box_closes_on_the_worker_clock(pool: PgPool) {
    let at = ahead();
    let mut db = pool.acquire().await.expect("conn").detach();
    let business = create(&mut db, false, at).await.expect("business");
    let new = NewComplaint {
        business,
        job: Uuid::from_u128(0x601),
        kind: "no_show",
        words: "",
    };
    let received = at - SignedDuration::from_hours(120);
    let id = record_complaint(&mut db, &new, received)
        .await
        .expect("complaint")
        .id;
    let task = claimed(BOX_CLOSED_KIND, json!({ "complaint": id }));
    run_box_closed_at(pool.clone(), task, at)
        .await
        .expect("listed");
    let listed = "SELECT listed_at = $2 FROM businesses.complaint WHERE id = $1";
    assert!(
        is_true(&pool, listed, id, at).await,
        "listed at the worker's time"
    );
}

/// (c), PRIV-007, PRIV-009, D-4196: a contact-stage purge task's complaint hold events and a
/// photo-stage task's object delete are queued at `ahead`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn holds_and_photo_deletes_are_stamped_on_the_worker_clock(pool: PgPool) {
    let at = ahead();
    let job = Uuid::from_u128(0x602);
    let mut db = pool.acquire().await.expect("conn").detach();
    let business = create(&mut db, false, at).await.expect("business");
    let new = NewComplaint {
        business,
        job,
        kind: "no_show",
        words: "",
    };
    record_complaint(&mut db, &new, at)
        .await
        .expect("complaint");
    let contact = claimed(PURGE_DUE_KIND, json!({ "job": job, "stage": "contact" }));
    let events = run_complaint_holds_at(pool.clone(), contact, at).await;
    assert_eq!(events.expect("holds"), 1);
    let hold = "SELECT bool_and(run_at = $2) FROM core.task WHERE kind = $3 AND payload->>'job' = $1::text";
    let held: Option<bool> = sqlx::query_scalar(hold)
        .bind(job)
        .bind(at.to_sqlx())
        .bind(COMPLAINT_HOLD_KIND)
        .fetch_one(&pool)
        .await
        .expect("events");
    assert_eq!(held, Some(true), "hold events at the worker's time");
    sqlx::query(
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at) \
         VALUES (gen_random_uuid(), $1, 1, 'photos/fake-600', now())",
    )
    .bind(job)
    .execute(&pool)
    .await
    .expect("photo");
    let photos = claimed(PURGE_DUE_KIND, json!({ "job": job, "stage": "photos" }));
    run_purge_due_at(pool.clone(), photos, at)
        .await
        .expect("purged");
    let delete = "SELECT bool_and(run_at = $1) FROM core.task WHERE kind = $2";
    let queued_at: Option<bool> = sqlx::query_scalar(delete)
        .bind(at.to_sqlx())
        .bind(PHOTO_DELETE_KIND)
        .fetch_one(&pool)
        .await
        .expect("delete task");
    assert_eq!(
        queued_at,
        Some(true),
        "the object's delete at the worker's time"
    );
}

/// D-3447, D-4196: each `(pool, task)` wrapper still runs, on the wall clock.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_wall_clock_wrappers_still_run(pool: PgPool) {
    let job = Uuid::from_u128(0x603);
    let unknown = json!({ "complaint": Uuid::from_u128(0x604) });
    assert!(
        run_box_closed(pool.clone(), claimed(BOX_CLOSED_KIND, unknown))
            .await
            .is_ok()
    );
    let photos = json!({ "job": job, "stage": "photos" });
    let holds = run_complaint_holds(pool.clone(), claimed(PURGE_DUE_KIND, photos.clone())).await;
    assert_eq!(holds.expect("not a contact stage"), 0);
    assert!(
        run_purge_due(pool.clone(), claimed(PURGE_DUE_KIND, photos))
            .await
            .is_ok()
    );
    let bad = claimed(CARDS_APPROVED_KIND, json!({}));
    assert!(
        run_cards_approved(pool.clone(), Arc::new(keys()), bad)
            .await
            .is_err()
    );
    assert!(
        run_card_timer(pool, claimed(CARD_TIMER_KIND, json!({})))
            .await
            .is_err()
    );
}

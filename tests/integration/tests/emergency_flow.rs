//! The urgent and 911-list emergency flows end to end in the database (TD-INT-12, TD-INT-13,
//! TD-INT-15, TD-INT-16: INT-004 to INT-006, INT-029, INT-030, INT-033, D-164, D-436, D-491).
//!
//! Each case runs `intake::api` on a fresh database with fake IDs only: the first text with its
//! consent record, the ZIP and area check stood in by an `intake.property` row (T040's step), the
//! "How soon?" answer, the night count over real `businesses` rows, and the choice; an emergency
//! is released as one `intake.emergency_released` task in `core.task`, the event `quotes` turns
//! into cards (T045, T150).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, Ticks, apply, create, set_job_types, set_ticks, set_towns,
};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    Answered, CHOICE_TEMPLATE, EMERGENCY_RELEASED_KIND, Handled, NEEDED_BY_TEMPLATE,
    NIGHT_NONE_TEMPLATE, NIGHT_ONE_TEMPLATE, TextIn, Trade, answer_choice, answer_how_soon,
    how_soon, offer_emergency_choice, receive_text, release_emergency, safe_hold,
};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const HVAC: Trade<'static> = Trade {
    category: "hvac_repair",
    town: TOWN,
    label: "HVAC",
};

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn az(time: &str) -> Timestamp {
    format!("2026-11-02T{time}:00-07:00").parse().expect("time")
}

fn text(contact: Uuid, at: Timestamp, n: u128, words: &str) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x3_9000 + n),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at,
    }
}

fn templates(texts: &[askhava_intake::api::Queued]) -> Vec<&'static str> {
    texts.iter().map(|q| q.template).collect()
}

/// One active HVAC business in the town with the after-hours tick (BIZ-007).
async fn after_hours_business(db: &mut Db) {
    let id = create(db, false, az("09:00")).await.expect("create");
    let done = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, id, Event::Checked(done), Actor::System, az("09:01"))
        .await
        .expect("active");
    let ticks = Ticks {
        overflow_on: false,
        speaks_spanish: false,
        insurance: None,
        after_hours: true,
    };
    let actor = Actor::Business(id);
    set_ticks(db, id, ticks, actor).await.expect("ticks");
    set_towns(db, id, &[TOWN], actor).await.expect("towns");
    let job = JobTypeChoice::new("hvac_repair", "no cooling");
    let allowed: BTreeSet<_> = [job.clone()].into();
    set_job_types(db, id, &[job], &allowed, actor)
        .await
        .expect("job types");
}

/// The ZIP and area check's stored town (T040's step, stood in here).
async fn area_done(db: &mut Db, conversation: Uuid, at: Timestamp) {
    sqlx::query!(
        "INSERT INTO intake.property (homeowner_id, contact_id, town, created_at) \
         SELECT homeowner_id, gen_random_uuid(), $2, $3 FROM intake.conversation WHERE id = $1",
        conversation,
        TOWN,
        at.to_sqlx() as _
    )
    .execute(&mut **db)
    .await
    .expect("property");
}

async fn released(db: &mut Db) -> i64 {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM core.task WHERE kind = $1",
        EMERGENCY_RELEASED_KIND
    )
    .fetch_one(&mut **db)
    .await
    .expect("tasks")
}

async fn first(db: &mut Db, contact: Uuid, at: Timestamp, words: &str) -> Handled {
    receive_text(
        db,
        &secret(),
        &text(contact, at, contact.as_u128(), words),
        Settings::default(),
    )
    .await
    .expect("first")
}

async fn how(db: &mut Db, msg: &TextIn<'_>, id: Uuid) -> Answered {
    answer_how_soon(db, &secret(), (msg, id), &HVAC, Settings::default())
        .await
        .expect("how soon")
}

/// TD-INT-13, TD-INT-15 (INT-030, INT-006, INT-033, INT-029, D-436, D-491): "AC died, 92 inside"
/// at 22:40 gets no safety text; "1" gets the night count (one business, singular) then the
/// choice; "1" again chooses emergency and the job is released at once, before any trade question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_13_urgent_at_night_count_choice_and_release(mut db: Db) {
    after_hours_business(&mut db).await;
    let contact = Uuid::from_u128(0x39A);
    let opened = first(&mut db, contact, az("22:40"), "AC died, 92 inside").await;
    assert_eq!(templates(&opened.texts), ["consent"]);
    let id = opened.conversation.expect("request");
    area_done(&mut db, id, az("22:41")).await;
    let today = text(contact, az("22:42"), 1, "1");
    let asked = how(&mut db, &today, id).await;
    assert_eq!(
        templates(&asked.texts),
        [NIGHT_ONE_TEMPLATE, CHOICE_TEMPLATE]
    );
    assert_eq!(
        how_soon(&mut db, id).await.expect("read").as_deref(),
        Some("today")
    );
    assert_eq!(released(&mut db).await, 0);
    let pick = text(contact, az("22:43"), 2, "1");
    let chose = answer_choice(&mut db, &pick, id).await.expect("choice");
    assert_eq!((chose.read, chose.released), (Some("emergency"), true));
    assert_eq!(released(&mut db).await, 1);
    let again = release_emergency(&mut db, id, az("22:44"))
        .await
        .expect("again");
    assert!(!again, "released once");
}

/// TD-INT-12 (INT-029, INT-005, D-164): an emergency chosen at 23:00 after a 911 match waits for
/// SAFE and for the ZIP; it is released as soon as the ZIP is in, before the trade questions.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_12_release_waits_for_safe_and_the_zip(mut db: Db) {
    after_hours_business(&mut db).await;
    let contact = Uuid::from_u128(0x39B);
    let opened = first(&mut db, contact, az("23:00"), "smell gas by the furnace").await;
    assert_eq!(templates(&opened.texts), ["emergency_911", "consent"]);
    let id = opened.conversation.expect("request");
    assert!(safe_hold(&mut db, id).await.expect("hold"));
    let early = text(contact, az("23:01"), 1, "1");
    let chose = answer_choice(&mut db, &early, id).await.expect("choice");
    assert!(!chose.released, "no card before SAFE (INT-005)");
    let safe = text(contact, az("23:02"), 2, "SAFE");
    receive_text(&mut db, &secret(), &safe, Settings::default())
        .await
        .expect("safe");
    assert!(!safe_hold(&mut db, id).await.expect("hold"));
    assert!(
        !release_emergency(&mut db, id, az("23:03"))
            .await
            .expect("no zip")
    );
    area_done(&mut db, id, az("23:04")).await;
    assert!(
        release_emergency(&mut db, id, az("23:04"))
            .await
            .expect("zip in")
    );
    assert_eq!(released(&mut db).await, 1);
}

/// TD-INT-15 (INT-033, INT-006, D-271): at 22:40 with no after-hours business only the zero text
/// goes, no choice is offered and the job runs as regular (never released).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_15_no_after_hours_business_no_choice(mut db: Db) {
    let contact = Uuid::from_u128(0x39C);
    let opened = first(&mut db, contact, az("22:40"), "AC died, 92 inside").await;
    let id = opened.conversation.expect("request");
    area_done(&mut db, id, az("22:41")).await;
    let asked = how(&mut db, &text(contact, az("22:42"), 1, "1"), id).await;
    assert_eq!(templates(&asked.texts), [NIGHT_NONE_TEMPLATE]);
    assert!(
        !release_emergency(&mut db, id, az("22:43"))
            .await
            .expect("regular")
    );
    assert_eq!(released(&mut db).await, 0);
}

/// TD-INT-15, TD-INT-13, TD-INT-16 (INT-006, INT-030, D-292): at 14:00 "1" gets only the daytime
/// choice; "2" and "3" get nothing; "4" asks the day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_16_daytime_choice_and_answer_four(mut db: Db) {
    after_hours_business(&mut db).await;
    let cases = [
        ("1", vec![CHOICE_TEMPLATE]),
        ("2", vec![]),
        ("3", vec![]),
        ("4", vec![NEEDED_BY_TEMPLATE]),
    ];
    for (n, (answer, want)) in (0u128..).zip(cases) {
        let contact = Uuid::from_u128(0x3A0 + n);
        let opened = first(&mut db, contact, az("14:00"), "AC died, 92 inside").await;
        let id = opened.conversation.expect("request");
        let reply = text(
            contact,
            az("14:01") + SignedDuration::from_mins(1),
            0x10 + n,
            answer,
        );
        let asked = how(&mut db, &reply, id).await;
        assert_eq!(templates(&asked.texts), want, "answer {answer}");
    }
}

/// TD-INT-16, TD-INT-13 (INT-005, INT-006, INT-030, D-982): after a 911 match the choice is never
/// offered before SAFE; it goes once SAFE is in; answer 4 after a 911 match still asks the day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_16_no_choice_before_safe_and_answer_four_after_a_match(mut db: Db) {
    let contact = Uuid::from_u128(0x3B0);
    let opened = first(&mut db, contact, az("14:00"), "smell gas").await;
    let id = opened.conversation.expect("request");
    let early = text(contact, az("14:01"), 0x20, "2");
    let held = how(&mut db, &early, id).await;
    assert!(held.texts.is_empty(), "no choice before SAFE (INT-005)");
    assert!(held.choice_after_safe);
    let too_soon =
        offer_emergency_choice(&mut db, &secret(), (&early, id), &HVAC, Settings::default())
            .await
            .expect("held");
    assert!(too_soon.is_empty());
    let safe = text(contact, az("14:02"), 0x21, "safe");
    receive_text(&mut db, &secret(), &safe, Settings::default())
        .await
        .expect("safe");
    let after = text(contact, az("14:03"), 0x22, "safe");
    let offered =
        offer_emergency_choice(&mut db, &secret(), (&after, id), &HVAC, Settings::default())
            .await
            .expect("offer");
    assert_eq!(templates(&offered), [CHOICE_TEMPLATE]);

    let day = Uuid::from_u128(0x3B1);
    let opened = first(&mut db, day, az("14:00"), "smell gas").await;
    let id = opened.conversation.expect("request");
    let safe = text(day, az("14:01"), 0x23, "SAFE");
    receive_text(&mut db, &secret(), &safe, Settings::default())
        .await
        .expect("safe");
    let four = how(&mut db, &text(day, az("14:02"), 0x24, "4"), id).await;
    assert_eq!(templates(&four.texts), [NEEDED_BY_TEMPLATE]);
    assert!(!four.choice_after_safe);
    let two = how(&mut db, &text(day, az("14:03"), 0x25, "2"), id).await;
    assert_eq!(templates(&two.texts), [CHOICE_TEMPLATE]);
}

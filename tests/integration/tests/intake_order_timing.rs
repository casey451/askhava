//! TD-INT-46 (T672, D-4360, D-4822): steps 6 to 8 of INT-043's order through the route's entry,
//! `intake::api::receive_raw_reply`, each case asserting its exact texts in the "Intake order"
//! table's order (`docs/spec/intake.md`) up to its own last step. After the coverage line "How
//! soon?" is asked (INT-030); answer 2 goes to the trade questions (INT-039), each answer stored
//! for the card (CARD-007) and the position found by replaying the stored answers by question ID;
//! a gas-smell answer gets the INT-004 text, the SAFE hold and its audit row and holds the order
//! until SAFE (INT-005); answer 4 asks and stores the day; answer 1 offers emergency or regular
//! (INT-006), Emergency storing the card and releasing the job once before the trade questions
//! (INT-029); on the 911 path no choice comes before SAFE; at night the count comes first and a
//! count of 0 offers no choice (INT-033); an Emergency choice made before the ZIP is released
//! when the ZIP comes in; TXT-041's Spanish line follows the first reply. PostgreSQL, fake data
//! only, the clock set (11:00 Arizona, or 21:00 for the night cases).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Acting, Checks, EMERGENCY_YES_LIFE, Event, JobTypeChoice, add_owner, apply, create,
    record_alert_answer, request_welcome, save_business_name, send_emergency_test, set_after_hours,
    set_emergency_emails, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    CHOICE_TEMPLATE, COUNT_MANY_TEMPLATE, EMERGENCY_RELEASED_KIND, HOW_SOON_TEMPLATE, Handled,
    NEEDED_BY_TEMPLATE, NIGHT_MANY_TEMPLATE, NIGHT_NONE_TEMPLATE, RawReply, Reply,
    SPANISH_NOT_YET_TEMPLATE, TRADE_QUESTION_TEMPLATE, TextIn, ZIP_QUESTION_TEMPLATE,
    receive_raw_reply,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
/// A Lake Havasu City ZIP (INT-048).
const HOME_ZIP: &str = "86403";
/// A first text naming one trade (`handyman`) and one job type of it, door repair (D-4441).
const DOOR: &str = "my front door is sticking badly";
/// The fake city list the two handyman businesses are matched on.
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7301,Sample Fixit One LLC,,,2019-03-04\nCL-7302,Sample Fixit Two LLC,,,2019-03-04\n";
const NAMES: [&str; 2] = ["One", "Two"];
/// The minute of 21:00 Arizona on the tests' clock, which starts at 11:00.
const NIGHT: i64 = 600;
const Q: &str = TRADE_QUESTION_TEMPLATE;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x71), &key(0x72), &url).expect("vault")
}

/// `minute` minutes after 11:00 Arizona on a fixed day.
fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T11:00:00-07:00".parse().expect("time");
    start + SignedDuration::from_mins(minute)
}

/// The templates `words` from fake contact `contact` at `minute` queued, in order, through the
/// route's entry.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x72_0000 + contact * 1000 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    };
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The `n`th (0 or 1) active, welcomed handyman business serving the town, ticking door repair;
/// with `night`, its owner covers after-hours emergencies (BIZ-007, D-2010).
async fn business(db: &mut PgConnection, n: usize, night: bool) {
    let b = create(db, false, at(-60)).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up as a handyman");
    let member = add_owner(db, b, Uuid::new_v4(), at(-60)).await;
    let owner = Acting {
        business: b,
        member: member.expect("owner"),
    };
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(db, b, Event::Checked(ok), Actor::System, at(-60)).await;
    checked.expect("active");
    request_welcome(db, b, at(-60)).await.expect("welcome");
    welcome_sent(db, b, at(-60)).await.expect("sent");
    let name = format!("Sample Fixit {} LLC", NAMES[n]);
    let named = save_business_name(db, b, (&name, None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
    if night {
        let on = set_emergency_emails(db, owner, true, at(-59)).await;
        on.expect("on");
        send_emergency_test(db, owner, at(-59)).await.expect("test");
        let yes = record_alert_answer(db, owner, true, at(-59)).await;
        yes.expect("yes");
        let life = (at(-58), EMERGENCY_YES_LIFE);
        set_after_hours(db, b, true, life).await.expect("tick");
    }
}

/// The catalog, the fake city list and both handyman businesses (after-hours ones with `night`),
/// then the app role.
async fn setup(db: &mut PgConnection, night: bool) {
    load_embedded(db).await.expect("catalog");
    let listed = import_city_list(db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
    business(db, 0, night).await;
    business(db, 1, night).await;
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// Fake contact `contact` sends [`DOOR`] and her ZIP from `minute` on; returns her request once
/// the coverage line and "How soon?" went (steps 1 to 6).
async fn asked_how_soon(db: &mut PgConnection, contact: u128, minute: i64) -> Uuid {
    let first = sent(db, contact, DOOR, minute).await;
    assert_eq!(first, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
    let zip = sent(db, contact, HOME_ZIP, minute + 1).await;
    assert_eq!(zip, [COUNT_MANY_TEMPLATE, HOW_SOON_TEMPLATE]);
    request(db, contact).await
}

/// As [`asked_how_soon`] at minute 0, then the request's first `trade_reading` row is made
/// `hvac_repair`, so its trade questions are the HVAC set (the one with a smell list and a
/// question marked for emergency jobs); no licensed fake HVAC business is set up here, so the
/// coverage line before it is the handyman one.
async fn asked_how_soon_hvac(db: &mut PgConnection, contact: u128) -> Uuid {
    let id = asked_how_soon(db, contact, 0).await;
    plant(db, id, ("trade_reading", "hvac_repair"), -1).await;
    id
}

/// Fake contact `contact`'s newest request.
async fn request(db: &mut PgConnection, contact: u128) -> Uuid {
    sqlx::query_scalar(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 ORDER BY c.opened_at DESC LIMIT 1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(db)
    .await
    .expect("request")
}

/// Request `id`'s stored answers to the questions named like `like`, as "question=choice", oldest
/// first.
async fn answers(db: &mut PgConnection, id: Uuid, like: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT question_id || '=' || choice FROM intake.intake_answer \
         WHERE conversation_id = $1 AND question_id LIKE $2 ORDER BY answered_at, id",
    )
    .bind(id)
    .bind(like)
    .fetch_all(db)
    .await
    .expect("answers")
}

/// Writes request `id`'s answer row `question` with `choice` at `minute`.
async fn plant(db: &mut PgConnection, id: Uuid, (question, choice): (&str, &str), minute: i64) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, $2, $3, to_timestamp($4))",
    )
    .bind(id)
    .bind(question)
    .bind(choice)
    .bind(at(minute).as_second())
    .execute(db)
    .await
    .expect("row");
}

/// One count: `sql` with request `id` bound.
async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(id).fetch_one(db).await;
    n.expect("count")
}

/// How many `intake.emergency_released` events are queued.
async fn released(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(EMERGENCY_RELEASED_KIND)
        .fetch_one(db)
        .await
        .expect("tasks")
}

/// TD-INT-46 (INT-043, INT-030, INT-039, CARD-007): regular. Answer 2 goes to the trade questions;
/// an unusable reply gets the question again; each answer is stored, a stored AI follow-up the
/// replay cannot ask shifts nothing, and the name-and-address question follows the last one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_regular_answer_2_then_the_trade_questions(mut db: Db) {
    setup(&mut db, false).await;
    let id = asked_how_soon(&mut db, 0x0555_0401, 0).await;
    let again = sent(&mut db, 0x0555_0401, "soonish", 2).await;
    assert_eq!(again, [HOW_SOON_TEMPLATE], "unreadable");
    assert_eq!(sent(&mut db, 0x0555_0401, "2", 3).await, [Q]);
    assert_eq!(
        answers(&mut db, id, "how_soon").await,
        ["how_soon=few_days"]
    );
    assert_eq!(sent(&mut db, 0x0555_0401, "maybe", 4).await, [Q], "again");
    assert!(answers(&mut db, id, "bank.%").await.is_empty());
    plant(&mut db, id, ("bank.follow_up", "text"), 4).await;
    assert_eq!(sent(&mut db, 0x0555_0401, "door repair", 5).await, [Q]);
    assert_eq!(sent(&mut db, 0x0555_0401, "1", 6).await, [Q]);
    assert_eq!(sent(&mut db, 0x0555_0401, "not sure", 7).await, [Q]);
    let last = sent(&mut db, 0x0555_0401, "it sticks at the top", 8).await;
    assert!(!last.contains(&Q), "the set is done: {last:?}");
    let stored = [
        "bank.follow_up=text",
        "bank.what=2",
        "bank.count=1",
        "bank.materials=not_sure",
        "bank.details=text",
    ];
    assert_eq!(answers(&mut db, id, "bank.%").await, stored);
    assert!(answers(&mut db, id, "emergency_choice").await.is_empty());
}

/// TD-INT-46 (INT-039, INT-004, INT-005): a gas-smell answer queues the INT-004 text, writes the
/// SAFE hold and the `intake.emergency_matched` audit row and holds the order; SAFE resumes at the
/// next trade question, with no choice (the 911 text came after "How soon?").
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_a_gas_smell_answer_holds_the_order_until_safe(mut db: Db) {
    setup(&mut db, false).await;
    let id = asked_how_soon_hvac(&mut db, 0x0555_0402).await;
    assert_eq!(sent(&mut db, 0x0555_0402, "3", 2).await, [Q]);
    assert_eq!(sent(&mut db, 0x0555_0402, "1", 3).await, [Q]);
    assert_eq!(
        sent(&mut db, 0x0555_0402, "5", 4).await,
        [Q],
        "what kind of smell"
    );
    assert_eq!(sent(&mut db, 0x0555_0402, "1", 5).await, [SAFETY_TEMPLATE]);
    let hold = "SELECT count(*) FROM intake.emergency_event WHERE conversation_id = $1 \
                AND matched = 'gas' AND text_queued_at IS NOT NULL AND safe_at IS NULL";
    assert_eq!(count(&mut db, hold, id).await, 1);
    let audit = "SELECT count(*) FROM ops.audit_log WHERE action = 'intake.emergency_matched' \
                 AND subject_id = $1";
    assert_eq!(count(&mut db, audit, id).await, 1);
    assert!(sent(&mut db, 0x0555_0402, "2", 6).await.is_empty(), "held");
    assert_eq!(sent(&mut db, 0x0555_0402, "SAFE", 7).await, [Q]);
    assert_eq!(count(&mut db, hold, id).await, 0);
    let stored = ["bank.system_type=1", "bank.symptom=5", "bank.smell_kind=1"];
    assert_eq!(answers(&mut db, id, "bank.%").await, stored);
    assert_eq!(sent(&mut db, 0x0555_0402, "2", 8).await, [Q]);
    assert_eq!(answers(&mut db, id, "bank.%").await.len(), 4, "the age");
}

/// TD-INT-46 (INT-030, CARD-007): answer 4 asks the day and stores it ("Needed by Fri"), then the
/// trade questions; an empty answer is never stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_answer_4_asks_the_day_and_stores_it(mut db: Db) {
    setup(&mut db, false).await;
    let id = asked_how_soon(&mut db, 0x0555_0403, 0).await;
    assert_eq!(
        sent(&mut db, 0x0555_0403, "4", 2).await,
        [NEEDED_BY_TEMPLATE]
    );
    assert_eq!(sent(&mut db, 0x0555_0403, "by Friday please", 3).await, [Q]);
    assert_eq!(answers(&mut db, id, "needed_by").await, ["needed_by=fri"]);
    assert_eq!(answers(&mut db, id, "emergency_choice").await.len(), 0);
}

/// TD-INT-46 (INT-006, INT-029): urgent. Answer 1 offers the choice; Emergency stores the card
/// with the emergency mark and queues `intake.emergency_released` once, before the trade
/// questions (only those marked for an emergency job); Regular releases nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_urgent_emergency_releases_once_and_regular_never(mut db: Db) {
    setup(&mut db, false).await;
    let id = asked_how_soon_hvac(&mut db, 0x0555_0404).await;
    assert_eq!(sent(&mut db, 0x0555_0404, "1", 2).await, [CHOICE_TEMPLATE]);
    assert_eq!(
        sent(&mut db, 0x0555_0404, "hmm", 3).await,
        [CHOICE_TEMPLATE]
    );
    assert_eq!(released(&mut db).await, 0);
    assert_eq!(sent(&mut db, 0x0555_0404, "Emergency", 4).await, [Q]);
    assert_eq!(released(&mut db).await, 1);
    let card = "SELECT count(*) FROM quotes.shown_card WHERE request_id = $1 AND emergency";
    assert_eq!(count(&mut db, card, id).await, 1);
    let last = sent(&mut db, 0x0555_0404, "1", 5).await;
    assert!(
        !last.contains(&Q),
        "one question marked for emergencies: {last:?}"
    );
    assert_eq!(answers(&mut db, id, "bank.%").await, ["bank.symptom=1"]);
    assert_eq!(released(&mut db).await, 1, "once");
    let other = asked_how_soon(&mut db, 0x0555_0405, 10).await;
    assert_eq!(sent(&mut db, 0x0555_0405, "1", 12).await, [CHOICE_TEMPLATE]);
    assert_eq!(sent(&mut db, 0x0555_0405, "regular", 13).await, [Q]);
    let chosen = answers(&mut db, other, "emergency_choice").await;
    assert_eq!(chosen, ["emergency_choice=regular"]);
    assert_eq!(count(&mut db, card, other).await, 0);
    assert_eq!(released(&mut db).await, 1, "regular releases nothing");
}

/// TD-INT-46 (INT-005, INT-006, INT-025): the 911 path. SAFE comes before the ZIP, and any "How
/// soon?" answer but 4 then offers the choice; a 911 text while the choice is open holds it (an
/// answer then does nothing) until SAFE, which offers it again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_on_the_911_path_no_choice_until_safe(mut db: Db) {
    setup(&mut db, false).await;
    let gas = "I smell gas and my front door is sticking badly";
    let first = sent(&mut db, 0x0555_0406, gas, 0).await;
    assert_eq!(first, [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
    assert_eq!(
        sent(&mut db, 0x0555_0406, "SAFE", 1).await,
        [ZIP_QUESTION_TEMPLATE]
    );
    let zip = sent(&mut db, 0x0555_0406, HOME_ZIP, 2).await;
    assert_eq!(zip, [COUNT_MANY_TEMPLATE, HOW_SOON_TEMPLATE]);
    assert_eq!(sent(&mut db, 0x0555_0406, "3", 3).await, [CHOICE_TEMPLATE]);
    let id = request(&mut db, 0x0555_0406).await;
    assert_eq!(
        sent(&mut db, 0x0555_0406, "now I see smoke", 4).await,
        [SAFETY_TEMPLATE]
    );
    assert!(sent(&mut db, 0x0555_0406, "emergency", 5).await.is_empty());
    assert!(answers(&mut db, id, "emergency_choice").await.is_empty());
    assert_eq!(
        sent(&mut db, 0x0555_0406, "SAFE", 6).await,
        [CHOICE_TEMPLATE]
    );
    assert_eq!(released(&mut db).await, 0);
    sent(&mut db, 0x0555_0406, "emergency", 7).await;
    assert_eq!(released(&mut db).await, 1);
}

/// TD-INT-46 (INT-033, INT-006): at 21:00 Arizona with 2 after-hours businesses the count text
/// comes before the choice; with none the zero text goes, no choice is offered, the job is stored
/// as regular and the trade questions follow.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_at_night_the_count_comes_before_the_choice(mut db: Db) {
    setup(&mut db, true).await;
    asked_how_soon(&mut db, 0x0555_0407, NIGHT).await;
    let urgent = sent(&mut db, 0x0555_0407, "1", NIGHT + 2).await;
    assert_eq!(urgent, [NIGHT_MANY_TEMPLATE, CHOICE_TEMPLATE]);
}

/// TD-INT-46 (INT-033): the night count of 0, as the case above says.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_at_night_with_no_business_no_choice_is_offered(mut db: Db) {
    setup(&mut db, false).await;
    let id = asked_how_soon(&mut db, 0x0555_0408, NIGHT).await;
    let urgent = sent(&mut db, 0x0555_0408, "1", NIGHT + 2).await;
    assert_eq!(urgent, [NIGHT_NONE_TEMPLATE, Q]);
    let chosen = answers(&mut db, id, "emergency_choice").await;
    assert_eq!(chosen, ["emergency_choice=regular"]);
    assert_eq!(released(&mut db).await, 0);
}

/// TD-INT-46 (INT-029): an Emergency choice stored before the ZIP releases the job when the ZIP
/// comes in, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_the_zip_after_an_emergency_choice_releases_the_job(mut db: Db) {
    setup(&mut db, false).await;
    sent(&mut db, 0x0555_0409, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0409).await;
    plant(&mut db, id, ("how_soon", "today"), 1).await;
    plant(&mut db, id, ("emergency_choice", "emergency"), 1).await;
    assert_eq!(released(&mut db).await, 0, "no ZIP yet");
    let zip = sent(&mut db, 0x0555_0409, HOME_ZIP, 2).await;
    assert_eq!(zip.first(), Some(&COUNT_MANY_TEMPLATE));
    assert_eq!(released(&mut db).await, 1);
    sent(&mut db, 0x0555_0409, "1", 3).await;
    assert_eq!(released(&mut db).await, 1, "once");
}

/// TD-INT-46 (TXT-041): while Spanish conversations are off, a Spanish first text gets the fixed
/// Spanish line once, right after the first reply and before the ZIP question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_46_the_spanish_line_follows_the_first_reply(mut db: Db) {
    setup(&mut db, false).await;
    let first = sent(
        &mut db,
        0x0555_0410,
        "Hola, la puerta de entrada no cierra",
        0,
    )
    .await;
    let line = [
        CONSENT_TEMPLATE,
        SPANISH_NOT_YET_TEMPLATE,
        ZIP_QUESTION_TEMPLATE,
    ];
    assert_eq!(first, line);
    let again = sent(&mut db, 0x0555_0410, "Hola, ¿sigue ahí?", 1).await;
    assert!(!again.contains(&SPANISH_NOT_YET_TEMPLATE), "{again:?}");
}

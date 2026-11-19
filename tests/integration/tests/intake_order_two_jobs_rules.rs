//! TD-INT-54 (T717, D-4441, correcting T711; INT-032, INT-009, INT-043, INT-017, INT-025,
//! CON-031): INT-032's two jobs through the route's entry, `intake::api::receive_raw_reply`.
//! `two_jobs` comes once, right after the first ZIP question (after SAFE on the 911 path, after
//! the INT-037 answer on the alarm path); ALSO or TAMBIÉN opens the second job with property
//! `none`, answers no step and gets its coverage line (at once after step 5, else right after the
//! first job's) and the open question again; nothing during a SAFE hold, on a second ALSO, after a
//! one-trade message or while the name-and-address question is open; SPLIT only as the first
//! reply after the turn that sent a SPLIT offer; no ALSO or SPLIT is kept as words. PostgreSQL,
//! fake data and 555 numbers only (no phone number appears here at all).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{
    CONSENT_TEMPLATE, Inbound, NAME_ADDRESS_TEMPLATE, SAFETY_TEMPLATE, Settings,
};
use askhava_intake::api::{
    BEEP_TEMPLATE, COUNT_MANY_TEMPLATE, Handled, NOT_COVERED_TEMPLATE, RawReply, Reply, TextIn,
    ZIP_QUESTION_TEMPLATE, receive_raw_reply,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
/// A Lake Havasu City ZIP (INT-048).
const HOME_ZIP: &str = "86403";
/// A first text naming two trades, `handyman` first, then `plumbing_repair`.
const TWO_TRADES: &str = "the front door sticks and the toilet runs";
/// A first text naming one trade (`handyman`).
const DOOR: &str = "my front door is sticking badly";
/// A first text naming one trade (`plumbing_repair`).
const TOILET: &str = "the toilet runs and it is loud";
/// A 911-list first text naming two trades.
const GAS_TWO: &str = "I smell gas, the front door sticks and the toilet runs";
/// An alarm-beep first text naming two trades.
const BEEP_TWO: &str = "my smoke alarm is beeping, the front door sticks and the toilet runs";
/// The `two_jobs` template ID (INT-032).
const TWO_JOBS: &str = "two_jobs";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7171,Sample Fixit One LLC,,,2019-03-04\nCL-7172,Sample Fixit Two LLC,,,2019-03-04\n";
const NAMES: [&str; 2] = ["One", "Two"];
/// Request `id`'s job links, by name.
const LINKS: &str = "SELECT question_id FROM intake.intake_answer WHERE conversation_id = $1 \
                     AND question_id LIKE 'job.%' ORDER BY question_id";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// `words` from fake contact `contact` at `minute`, through the route's entry; the templates
/// queued, in order.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x73_0000 + contact * 100 + minute.unsigned_abs() as u128),
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

/// The catalog and the fake city list; then the app role.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// The `n`th (0 or 1) active, welcomed handyman business serving the town, ticking door repair.
async fn business(db: &mut PgConnection, n: usize) {
    let b = create(db, false, at(0)).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up as a handyman");
    add_owner(db, b, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at(0))
        .await
        .expect("active");
    request_welcome(db, b, at(0)).await.expect("welcome");
    welcome_sent(db, b, at(0)).await.expect("sent");
    let name = format!("Sample Fixit {} LLC", NAMES[n]);
    save_business_name(db, b, (&name, None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
}

/// Both handyman businesses.
async fn businesses(db: &mut PgConnection) {
    business(db, 0).await;
    business(db, 1).await;
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

/// Request `id`'s job links, by name.
async fn links(db: &mut PgConnection, id: Uuid) -> Vec<String> {
    let rows = sqlx::query_scalar(LINKS).bind(id).fetch_all(db).await;
    rows.expect("links")
}

/// The job ID of request `id`'s job link `name`.
async fn linked(db: &mut PgConnection, id: Uuid, name: &str) -> Uuid {
    let choice: String = sqlx::query_scalar(
        "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 AND question_id = $2",
    )
    .bind(id)
    .bind(name)
    .fetch_one(db)
    .await
    .expect("link");
    Uuid::parse_str(&choice).expect("job ID")
}

/// The kept words of request `id`, as words only.
async fn kept(db: &mut PgConnection, id: Uuid) -> Vec<String> {
    sqlx::query_scalar("SELECT words FROM quotes.job_description WHERE request_id = $1")
        .bind(id)
        .fetch_all(db)
        .await
        .expect("kept")
}

/// How many `two_jobs` texts were queued to fake contact `contact`.
async fn two_jobs_sent(db: &mut PgConnection, contact: u128) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE template_id = 'two_jobs' AND recipient_id = $1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(db)
    .await
    .expect("count")
}

/// Writes request `id`'s step marker `question` with `choice` at `minute`, as the order writes it.
async fn marker(db: &mut PgConnection, id: Uuid, (question, choice): (&str, &str), minute: i64) {
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
    .expect("marker");
}

/// TD-INT-54 (INT-032, CON-031, INT-043): on the clear path the two-trade first text gets the
/// consent text, the ZIP question and right after it `two_jobs`; no later reply gets it again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_two_jobs_once_right_after_the_zip_question(mut db: Db) {
    setup(&mut db).await;
    businesses(&mut db).await;
    let first = sent(&mut db, 0x0555_0301, TWO_TRADES, 0).await;
    assert_eq!(first, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE, TWO_JOBS]);
    let unusable = sent(&mut db, 0x0555_0301, "the blue house by the lake", 1).await;
    assert!(!unusable.contains(&TWO_JOBS), "{unusable:?}");
    let zip = sent(&mut db, 0x0555_0301, HOME_ZIP, 2).await;
    assert!(!zip.contains(&TWO_JOBS), "{zip:?}");
    assert_eq!(two_jobs_sent(&mut db, 0x0555_0301).await, 1);
}

/// TD-INT-54 (INT-032, INT-025, INT-005): on the 911 path `two_jobs` waits for SAFE; an ALSO
/// during the SAFE hold gets nothing and opens nothing; SAFE gets the ZIP question then `two_jobs`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_on_the_911_path_two_jobs_follows_the_zip_after_safe(mut db: Db) {
    setup(&mut db).await;
    let first = sent(&mut db, 0x0555_0302, GAS_TWO, 0).await;
    assert_eq!(first, [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
    let id = request(&mut db, 0x0555_0302).await;
    let before = links(&mut db, id).await;
    assert_eq!(before.len(), 1, "{before:?}");
    let also = sent(&mut db, 0x0555_0302, "ALSO", 1).await;
    assert!(also.is_empty(), "{also:?}");
    assert_eq!(
        links(&mut db, id).await,
        before,
        "nothing opens during the hold"
    );
    let safe = sent(&mut db, 0x0555_0302, "SAFE", 2).await;
    assert_eq!(safe, [ZIP_QUESTION_TEMPLATE, TWO_JOBS]);
    assert_eq!(two_jobs_sent(&mut db, 0x0555_0302).await, 1);
    assert_eq!(kept(&mut db, id).await, [GAS_TWO]);
}

/// TD-INT-54 (INT-032, INT-037): on the alarm path `two_jobs` waits for the INT-037 answer and
/// follows the ZIP question it brings.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_on_the_alarm_path_two_jobs_follows_the_answer(mut db: Db) {
    setup(&mut db).await;
    let first = sent(&mut db, 0x0555_0303, BEEP_TWO, 0).await;
    assert_eq!(first, [CONSENT_TEMPLATE, BEEP_TEMPLATE]);
    let chirp = sent(&mut db, 0x0555_0303, "just chirping", 1).await;
    assert_eq!(chirp, [ZIP_QUESTION_TEMPLATE, TWO_JOBS]);
    assert_eq!(two_jobs_sent(&mut db, 0x0555_0303).await, 1);
}

/// TD-INT-54 (INT-032, INT-009, INT-017): ALSO before the ZIP opens exactly one more job with
/// property `none`, never her other request's same-trade job, answers no step and gets the ZIP
/// question again; a second ALSO opens nothing; the ZIP answer then tells the first job's coverage
/// and right after it the second's; no ALSO is kept as words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_also_before_the_zip_opens_one_job_told_after_the_first(mut db: Db) {
    setup(&mut db).await;
    businesses(&mut db).await;
    sent(&mut db, 0x0555_0304, TOILET, 0).await;
    let earlier = request(&mut db, 0x0555_0304).await;
    let old = links(&mut db, earlier).await;
    assert_eq!(old, ["job.plumbing_repair.none"], "{old:?}");
    let old_job = linked(&mut db, earlier, &old[0]).await;
    sqlx::query("RESET ROLE")
        .execute(&mut *db)
        .await
        .expect("reset");
    sqlx::query("UPDATE intake.conversation SET closed_at = now() WHERE id = $1")
        .bind(earlier)
        .execute(&mut *db)
        .await
        .expect("the earlier request closes; its job stays active");
    set_app_role(&mut db).await.expect("role");
    sent(&mut db, 0x0555_0304, TWO_TRADES, 10).await;
    let id = request(&mut db, 0x0555_0304).await;
    let also = sent(&mut db, 0x0555_0304, "ALSO", 11).await;
    assert_eq!(also, [ZIP_QUESTION_TEMPLATE], "the open question again");
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
    let second = linked(&mut db, id, "job.plumbing_repair.none").await;
    assert_ne!(second, old_job, "another request's job is never joined");
    sent(&mut db, 0x0555_0304, "also", 12).await;
    assert_eq!(
        links(&mut db, id).await,
        both,
        "a second ALSO opens nothing"
    );
    let zip = sent(&mut db, 0x0555_0304, HOME_ZIP, 13).await;
    assert_eq!(
        zip[..2],
        [COUNT_MANY_TEMPLATE, NOT_COVERED_TEMPLATE],
        "{zip:?}"
    );
    assert_eq!(kept(&mut db, id).await, [TWO_TRADES]);
}

/// Answers request `id`'s name-and-address question as `take_name_address` marks it.
async fn address_answered(db: &mut PgConnection, id: Uuid, minute: i64) {
    marker(db, id, ("address_contact", "stored"), minute).await;
}

/// TD-INT-54 (INT-032, INT-027, INT-017): ALSO while the name-and-address question is open opens
/// nothing; after step 5, once it is answered, ALSO opens exactly one more job with property
/// `none` and gets its coverage line at once; TAMBIÉN does the same.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_also_or_tambien_after_step_5_tells_coverage_at_once(mut db: Db) {
    setup(&mut db).await;
    businesses(&mut db).await;
    for (contact, word) in [(0x0555_0305, "ALSO"), (0x0555_0306, "TAMBIÉN")] {
        sent(&mut db, contact, TWO_TRADES, 0).await;
        let zip = sent(&mut db, contact, HOME_ZIP, 1).await;
        assert_eq!(zip, [COUNT_MANY_TEMPLATE, NAME_ADDRESS_TEMPLATE], "{word}");
        let id = request(&mut db, contact).await;
        sent(&mut db, contact, word, 2).await;
        assert_eq!(links(&mut db, id).await, ["job.handyman.none"], "{word}");
        address_answered(&mut db, id, 3).await;
        let opened = sent(&mut db, contact, word, 4).await;
        assert_eq!(opened, [NOT_COVERED_TEMPLATE], "{word}");
        let both = ["job.handyman.none", "job.plumbing_repair.none"];
        assert_eq!(links(&mut db, id).await, both, "{word}");
        assert_eq!(kept(&mut db, id).await, [TWO_TRADES], "{word}");
    }
}

/// TD-INT-54 (INT-032): ALSO after a one-trade message opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_also_after_one_trade_opens_nothing(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0307, DOOR, 0).await;
    sent(&mut db, 0x0555_0307, "ALSO", 1).await;
    let id = request(&mut db, 0x0555_0307).await;
    assert_eq!(links(&mut db, id).await, ["job.handyman.none"]);
    assert_eq!(kept(&mut db, id).await, [DOOR]);
}

/// TD-INT-54 (INT-032, D-1020): SPLIT as the first reply after the turn that sent a SPLIT offer
/// (its marker, at that turn's time) opens exactly one more job, answers no step and gets the ZIP
/// question again; SPLIT again opens none; no SPLIT is kept as words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_split_first_after_its_offer_opens_one_job(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0308, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0308).await;
    marker(&mut db, id, ("split_offer", "plumbing_repair"), 1).await;
    let split = sent(&mut db, 0x0555_0308, "SPLIT", 2).await;
    assert_eq!(split, [ZIP_QUESTION_TEMPLATE], "no step answered");
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
    sent(&mut db, 0x0555_0308, "SPLIT", 3).await;
    assert_eq!(links(&mut db, id).await, both, "only the first reply");
    assert_eq!(kept(&mut db, id).await, [DOOR]);
}

/// TD-INT-54 (INT-032, D-1020): SPLIT or DIVIDIR as a later reply after the offer, or with no
/// offer, opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_54_split_later_or_with_no_offer_opens_nothing(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0309, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0309).await;
    marker(&mut db, id, ("split_offer", "plumbing_repair"), 1).await;
    sent(&mut db, 0x0555_0309, "the blue house", 2).await;
    sent(&mut db, 0x0555_0309, "DIVIDIR", 3).await;
    assert_eq!(links(&mut db, id).await, ["job.handyman.none"]);
    sent(&mut db, 0x0555_0310, DOOR, 0).await;
    sent(&mut db, 0x0555_0310, "SPLIT", 1).await;
    let none = request(&mut db, 0x0555_0310).await;
    assert_eq!(
        links(&mut db, none).await,
        ["job.handyman.none"],
        "no offer"
    );
    assert_eq!(kept(&mut db, none).await, [DOOR]);
}

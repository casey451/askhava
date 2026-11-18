//! TD-INT-45 (T671, D-4360): a homeowner's texts after the consent text walk INT-043's order
//! through the route's entry, `intake::api::receive_raw_reply`: the ZIP question right after the
//! consent text (CON-031), the ZIP and area check (INT-049, INT-022, PRIV-028), the trade check
//! and coverage line (INT-016, INT-017), her first job (INT-009) and the opening words kept once
//! (PRIV-008). Texts are asserted only up to the coverage line, so step 11's interim place is not.
//! A two-trade first message asserts only jobs and `trade_reading` rows (INT-032 is T711's).
//! PostgreSQL, fake data and 555 numbers only (no phone number appears here at all).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    COUNT_MANY_TEMPLATE, Handled, NOT_COVERED_TEMPLATE, OUT_OF_AREA_TEMPLATE, RawReply, Reply,
    TextIn, ZIP_AGAIN_TEMPLATE, ZIP_QUESTION_TEMPLATE, receive_raw_reply,
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
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
/// A Lake Havasu City ZIP and an Arizona ZIP of no service-area town (INT-048, INT-022).
const HOME_ZIP: &str = "86403";
const AWAY_ZIP: &str = "86401";
/// A first text naming one trade (`handyman`, job type `door repair`).
const DOOR: &str = "my front door is sticking badly";
/// A first text naming two trades, `handyman` first, then `plumbing_repair`.
const TWO_TRADES: &str = "the front door sticks and the toilet runs";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7101,Sample Fixit One LLC,,,2019-03-04\nCL-7102,Sample Fixit Two LLC,,,2019-03-04\n";
const NAMES: [&str; 2] = ["One", "Two"];

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x67), &key(0x68), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x67_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// `words` from fake contact `contact` at `minute`, through the route's entry.
async fn send(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Reply {
    let msg = text(contact, words, minute);
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// The templates `words` queued, in order (a later ZIP may land on step 11's question).
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    match send(db, contact, words, minute).await {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The catalog and the fake city list, optionally with handyman off; then the app role.
async fn setup(db: &mut PgConnection, handyman_on: bool) {
    load_embedded(db).await.expect("catalog");
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    if !handyman_on {
        sqlx::query("UPDATE catalog.category SET enabled = false WHERE code = 'handyman'")
            .execute(&mut *db)
            .await
            .expect("off");
    }
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// The `n`th (0 or 1) active, welcomed business signed up as a handyman (LIC-031), serving the
/// town and ticking door repair.
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

/// Request `id`'s job links and `trade_reading` rows (first-named first).
const LINKS: &str = "SELECT question_id FROM intake.intake_answer WHERE conversation_id = $1 \
                     AND question_id LIKE 'job.%' ORDER BY question_id";
const READINGS: &str = "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 \
                        AND question_id = 'trade_reading' ORDER BY answered_at, id";

/// The one text column of `sql`'s rows for request `id`.
async fn strings(db: &mut PgConnection, sql: &'static str, id: Uuid) -> Vec<String> {
    let rows = sqlx::query_scalar(sql).bind(id).fetch_all(db).await;
    rows.expect(sql)
}

/// The count `sql` gives for request `id`.
async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(id).fetch_one(db).await;
    n.expect(sql)
}

async fn first_job(db: &mut PgConnection, id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT job_id FROM intake.conversation WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .expect("job")
}

/// The kept words of request `id` as (job, words).
async fn kept(db: &mut PgConnection, id: Uuid) -> Vec<(Uuid, String)> {
    sqlx::query_as("SELECT job_id, words FROM quotes.job_description WHERE request_id = $1")
        .bind(id)
        .fetch_all(db)
        .await
        .expect("kept")
}

/// TD-INT-45 (CON-031, INT-009, PRIV-008): a first text gets the consent text then the ZIP question
/// as its own text, stores its `trade_reading` row, opens her job with property `none` and keeps
/// its scrubbed words once; the step-marker rows carry no inbox ID.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_a_first_text_gets_consent_then_the_zip_question(mut db: Db) {
    setup(&mut db, true).await;
    let got = sent(&mut db, 0x0555_0101, DOOR, 0).await;
    assert_eq!(got, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
    let id = request(&mut db, 0x0555_0101).await;
    assert_eq!(strings(&mut db, READINGS, id).await, ["handyman"]);
    assert_eq!(strings(&mut db, LINKS, id).await, ["job.handyman.none"]);
    let job = first_job(&mut db, id).await.expect("her job");
    assert_eq!(kept(&mut db, id).await, [(job, DOOR.to_owned())]);
    let markers = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                   AND inbox_id IS NULL AND question_id IN ('zip', 'trade_reading', 'job.handyman.none')";
    assert_eq!(
        count(&mut db, markers, id).await,
        3,
        "no inbox ID on a step marker"
    );
}

/// TD-INT-45 (INT-049, INT-017, PRIV-028, PRIV-008, PRIV-016): a service-area ZIP stores the
/// property (vault contact, town, ZIP), keeps no words, and texts the count of the two covering
/// businesses for the same job; the step markers carry no inbox ID; the dispatcher logs nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_a_town_zip_stores_the_property_and_tells_the_count(mut db: Db) {
    setup(&mut db, true).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0102, DOOR, 0).await;
    let got = sent(&mut db, 0x0555_0102, HOME_ZIP, 1).await;
    assert_eq!(got.first(), Some(&COUNT_MANY_TEMPLATE), "{got:?}");
    let line = "SELECT body FROM messaging.outbox WHERE template_id = 'coverage_count_many' \
                AND recipient_id = $1";
    let line = strings(&mut db, line, Uuid::from_u128(0x0555_0102)).await;
    assert!(line[0].contains("the 2 handyman businesses"), "{line:?}");
    let id = request(&mut db, 0x0555_0102).await;
    let property = "SELECT concat_ws(' ', p.contact_id, p.town, p.zip) FROM intake.property p \
                    JOIN intake.conversation c ON c.homeowner_id = p.homeowner_id WHERE c.id = $1";
    let expected = format!("{} {TOWN} {HOME_ZIP}", Uuid::from_u128(0x0555_0102));
    assert_eq!(strings(&mut db, property, id).await, [expected]);
    let job = first_job(&mut db, id).await.expect("job");
    assert_eq!(
        kept(&mut db, id).await,
        [(job, DOOR.to_owned())],
        "no ZIP kept"
    );
    assert_eq!(
        strings(&mut db, LINKS, id).await,
        ["job.handyman.none"],
        "one job"
    );
    let tagged = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                  AND inbox_id IS NOT NULL AND question_id <> 'heard'";
    assert_eq!(
        count(&mut db, tagged, id).await,
        0,
        "step markers carry no inbox ID"
    );
    let source = include_str!("../../../app/intake/src/order_dispatch.rs");
    for logger in [
        "tracing::",
        "info!",
        "warn!",
        "error!",
        "debug!",
        "println!",
    ] {
        assert!(!source.contains(logger), "{logger}");
    }
}

/// TD-INT-45 (INT-022, INT-049): an Arizona ZIP of no service-area town closes the request with
/// INT-022's reply; a reply with no ZIP is asked once more, then waited on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_out_of_area_closes_and_no_zip_asks_once_more(mut db: Db) {
    setup(&mut db, true).await;
    sent(&mut db, 0x0555_0103, DOOR, 0).await;
    assert_eq!(
        sent(&mut db, 0x0555_0103, AWAY_ZIP, 1).await,
        [OUT_OF_AREA_TEMPLATE]
    );
    let id = request(&mut db, 0x0555_0103).await;
    let closed = "SELECT count(*) FROM intake.conversation WHERE id = $1 AND closed_at IS NOT NULL";
    assert_eq!(count(&mut db, closed, id).await, 1);
    sent(&mut db, 0x0555_0104, DOOR, 0).await;
    let again = sent(&mut db, 0x0555_0104, "it is the one by the lake", 1).await;
    assert_eq!(again, [ZIP_AGAIN_TEMPLATE]);
    let waited = sent(&mut db, 0x0555_0104, "the blue house", 2).await;
    assert!(waited.is_empty(), "{waited:?}");
}

/// TD-INT-45 (INT-025, INT-043): on a 911-list first text the ZIP is asked only after SAFE.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_on_the_911_path_the_zip_comes_after_safe(mut db: Db) {
    setup(&mut db, true).await;
    let first = sent(&mut db, 0x0555_0105, "I smell gas in the kitchen", 0).await;
    assert_eq!(first, [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
    let waiting = sent(&mut db, 0x0555_0105, "what now", 1).await;
    assert!(!waiting.contains(&ZIP_QUESTION_TEMPLATE), "{waiting:?}");
    let safe = sent(&mut db, 0x0555_0105, "SAFE", 2).await;
    assert_eq!(safe, [ZIP_QUESTION_TEMPLATE]);
}

/// TD-INT-45 (INT-016, INT-017, OPS-041): a trade in no open category gets the INT-017 reply and
/// a demand row once her town is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_a_trade_not_open_gets_the_not_covered_reply(mut db: Db) {
    setup(&mut db, false).await;
    sent(&mut db, 0x0555_0106, DOOR, 0).await;
    let got = sent(&mut db, 0x0555_0106, HOME_ZIP, 1).await;
    assert_eq!(got, [NOT_COVERED_TEMPLATE]);
    let id = request(&mut db, 0x0555_0106).await;
    let demand = "SELECT trade FROM intake.recruiting_demand WHERE conversation_id = $1";
    assert_eq!(strings(&mut db, demand, id).await, ["handyman"]);
}

/// TD-INT-45 (JOB-019, INT-049): "cancel my request?" is never read as a step answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_a_close_intent_is_never_a_step_answer(mut db: Db) {
    setup(&mut db, true).await;
    sent(&mut db, 0x0555_0107, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0107).await;
    let reply = send(&mut db, 0x0555_0107, "cancel my request?", 1).await;
    if let Reply::Handled(handled) = &reply {
        let got: Vec<_> = handled.texts.iter().map(|q| q.template).collect();
        assert!(!got.contains(&ZIP_AGAIN_TEMPLATE), "{got:?}");
    }
    let asks = "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 \
                AND question_id = 'zip'";
    assert_eq!(count(&mut db, asks, id).await, 1, "only the first ask");
}

/// TD-INT-45 (INT-043, INT-009): two ZIP replies at once answer the step once: one property, one
/// coverage line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_two_texts_at_once_answer_a_step_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    setup(&mut db, true).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0108, DOOR, 0).await;
    let mut other = pool.acquire().await.expect("conn");
    set_app_role(&mut other).await.expect("role");
    let (a, b) = tokio::join!(
        sent(&mut db, 0x0555_0108, HOME_ZIP, 1),
        sent(&mut other, 0x0555_0108, "86404", 2)
    );
    let counted = [a, b]
        .iter()
        .flatten()
        .filter(|t| **t == COUNT_MANY_TEMPLATE)
        .count();
    assert_eq!(counted, 1);
    let id = request(&mut db, 0x0555_0108).await;
    let properties = "SELECT count(*) FROM intake.property p JOIN intake.conversation c \
                      ON c.homeowner_id = p.homeowner_id WHERE c.id = $1";
    assert_eq!(count(&mut db, properties, id).await, 1);
}

/// TD-INT-45 (INT-009, PRIV-008): a homeowner with a property from an earlier request, whose job
/// of the same trade is still active, gets one new job for the new request (never that job, never
/// a second after the ZIP), with her opening words on it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_a_returning_homeowner_gets_one_job_per_request(mut db: Db) {
    setup(&mut db, true).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0109, DOOR, 0).await;
    sent(&mut db, 0x0555_0109, HOME_ZIP, 1).await;
    let earlier = request(&mut db, 0x0555_0109).await;
    let old_job = first_job(&mut db, earlier).await.expect("old job");
    let close =
        "UPDATE intake.conversation SET closed_at = now() WHERE id = $1 RETURNING 1::bigint";
    sqlx::query("RESET ROLE")
        .execute(&mut *db)
        .await
        .expect("reset");
    assert_eq!(
        count(&mut db, close, earlier).await,
        1,
        "the earlier request only"
    );
    set_app_role(&mut db).await.expect("role");
    let words = "the back door is sticking now too";
    sent(&mut db, 0x0555_0109, words, 10).await;
    sent(&mut db, 0x0555_0109, HOME_ZIP, 11).await;
    let id = request(&mut db, 0x0555_0109).await;
    assert_ne!(id, earlier);
    let job = first_job(&mut db, id).await.expect("new job");
    assert_ne!(job, old_job, "another request's job is never joined");
    assert_eq!(strings(&mut db, LINKS, id).await, ["job.handyman.none"]);
    assert_eq!(kept(&mut db, id).await, [(job, words.to_owned())]);
}

/// TD-INT-45 (INT-009; INT-032's text is T711's): a two-trade first message opens exactly one job,
/// for the first trade, and stores both `trade_reading` rows. Texts are not asserted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_two_trades_open_one_job_for_the_first(mut db: Db) {
    setup(&mut db, true).await;
    send(&mut db, 0x0555_0110, TWO_TRADES, 0).await;
    let id = request(&mut db, 0x0555_0110).await;
    assert_eq!(
        strings(&mut db, READINGS, id).await,
        ["handyman", "plumbing_repair"]
    );
    assert_eq!(strings(&mut db, LINKS, id).await, ["job.handyman.none"]);
}

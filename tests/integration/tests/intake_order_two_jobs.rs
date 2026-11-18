//! TD-INT-45's two-job cases (T711, D-4439, INT-032, INT-009, D-1020): a first message naming two
//! trades through the route's entry, `intake::api::receive_raw_reply`, opens one job and gets the
//! `two_jobs` text; ALSO or TAMBIÉN opens the second trade's job (before the ZIP the early way,
//! after it through `begin_job`), only once and never while the name-and-address question is open;
//! SPLIT opens one more job only as the first reply after a SPLIT offer; no ALSO or SPLIT reply is
//! kept as words. Texts are asserted only up to the coverage line. PostgreSQL, fake data and 555
//! numbers only (no phone number appears here at all).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::brand;
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Inbound, Settings};
use askhava_intake::api::{
    Handled, RawReply, Reply, TextIn, ZIP_QUESTION_TEMPLATE, receive_raw_reply, trade_label,
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
/// A first text naming two trades, `plumbing_repair` first, then `handyman`.
const TOILET_FIRST: &str = "the toilet runs and the back door sticks";
/// The `two_jobs` template ID (INT-032).
const TWO_JOBS: &str = "two_jobs";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7111,Sample Fixit One LLC,,,2019-03-04\nCL-7112,Sample Fixit Two LLC,,,2019-03-04\n";
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
    Vault::new(&key(0x71), &key(0x72), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// `words` from fake contact `contact` at `minute`, through the route's entry; the templates
/// queued, in order.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x71_0000 + contact * 100 + minute.unsigned_abs() as u128),
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

/// The body of the `two_jobs` text queued to fake contact `contact`.
async fn two_jobs_body(db: &mut PgConnection, contact: u128) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE template_id = 'two_jobs' AND recipient_id = $1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_all(db)
    .await
    .expect("body")
}

/// TD-INT-45 (INT-032, CON-031): a two-trade first message opens one job, for the first trade,
/// and gets the consent text, INT-032's `two_jobs` text with both trades' names, then the ZIP
/// question; a one-trade message never gets it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_two_trades_get_the_two_jobs_text(mut db: Db) {
    setup(&mut db).await;
    let got = sent(&mut db, 0x0555_0201, TWO_TRADES, 0).await;
    assert_eq!(got, [CONSENT_TEMPLATE, TWO_JOBS, ZIP_QUESTION_TEMPLATE]);
    let id = request(&mut db, 0x0555_0201).await;
    assert_eq!(links(&mut db, id).await, ["job.handyman.none"]);
    let words = format!(
        "{}: That's 2 jobs. We'll start with handyman. Reply ALSO to start {} too.",
        brand().short_name(),
        trade_label("plumbing_repair")
    );
    assert_eq!(two_jobs_body(&mut db, 0x0555_0201).await, [words]);
    let one = sent(&mut db, 0x0555_0202, DOOR, 0).await;
    assert_eq!(one, [CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE]);
}

/// TD-INT-45 (INT-032, INT-009, D-4439): ALSO before the ZIP opens exactly one more job, for the
/// second trade, with property `none`, never joining the same-trade job of her other request;
/// a second ALSO opens nothing; no ALSO is kept as words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_also_before_the_zip_opens_the_second_job_the_early_way(mut db: Db) {
    setup(&mut db).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0203, DOOR, 0).await;
    sent(&mut db, 0x0555_0203, HOME_ZIP, 1).await;
    let earlier = request(&mut db, 0x0555_0203).await;
    let old_job = linked(&mut db, earlier, "job.handyman.none").await;
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
    sent(&mut db, 0x0555_0203, TOILET_FIRST, 10).await;
    let id = request(&mut db, 0x0555_0203).await;
    assert_ne!(id, earlier);
    sent(&mut db, 0x0555_0203, "ALSO", 11).await;
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
    let second = linked(&mut db, id, "job.handyman.none").await;
    assert_ne!(second, old_job, "another request's job is never joined");
    sent(&mut db, 0x0555_0203, "also", 12).await;
    assert_eq!(
        links(&mut db, id).await,
        both,
        "a second ALSO opens nothing"
    );
    assert_eq!(kept(&mut db, id).await, [TOILET_FIRST]);
}

/// TD-INT-45 (INT-032, INT-027, INT-009): ALSO while the name-and-address question is open opens
/// nothing; once it is answered (its `address_contact` marker, as `take_name_address` writes it),
/// ALSO opens exactly one more job through `begin_job`, keyed with her new property.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_also_after_the_zip_opens_the_second_job_through_begin_job(mut db: Db) {
    setup(&mut db).await;
    business(&mut db, 0).await;
    business(&mut db, 1).await;
    sent(&mut db, 0x0555_0204, TWO_TRADES, 0).await;
    sent(&mut db, 0x0555_0204, HOME_ZIP, 1).await;
    let id = request(&mut db, 0x0555_0204).await;
    sent(&mut db, 0x0555_0204, "ALSO", 2).await;
    assert_eq!(
        links(&mut db, id).await,
        ["job.handyman.none"],
        "question open"
    );
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, 'address_contact', 'stored', to_timestamp($2))",
    )
    .bind(id)
    .bind(at(3).as_second())
    .execute(&mut *db)
    .await
    .expect("answered");
    sent(&mut db, 0x0555_0204, "ALSO", 4).await;
    let got = links(&mut db, id).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[1].starts_with("job.plumbing_repair."), "{got:?}");
    assert_ne!(
        got[1], "job.plumbing_repair.none",
        "keyed with her property"
    );
    assert_eq!(kept(&mut db, id).await, [TWO_TRADES]);
}

/// TD-INT-45 (INT-032): TAMBIÉN opens the second job as ALSO does; ALSO after a one-trade message
/// opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_tambien_opens_it_and_also_after_one_trade_does_not(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0205, TWO_TRADES, 0).await;
    sent(&mut db, 0x0555_0205, "TAMBIÉN", 1).await;
    let id = request(&mut db, 0x0555_0205).await;
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
    sent(&mut db, 0x0555_0206, DOOR, 0).await;
    sent(&mut db, 0x0555_0206, "ALSO", 1).await;
    let one = request(&mut db, 0x0555_0206).await;
    assert_eq!(links(&mut db, one).await, ["job.handyman.none"]);
    assert_eq!(kept(&mut db, one).await, [DOOR]);
}

/// Writes the step marker the trade check leaves when it sends the SPLIT offer for request `id`,
/// naming the trade SPLIT opens (`quotes::api::request_coverage` reads one category per job, so
/// no fake business set gives `Coverage::Split` through the route today; D-4488).
async fn split_offered(db: &mut PgConnection, id: Uuid, minute: i64) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, 'split_offer', 'plumbing_repair', to_timestamp($2))",
    )
    .bind(id)
    .bind(at(minute).as_second())
    .execute(db)
    .await
    .expect("offer");
}

/// TD-INT-45 (INT-032, D-1020): SPLIT as the first reply after a SPLIT offer opens exactly one
/// more job, for the offer's trade; SPLIT again, SPLIT after another reply and SPLIT with no
/// offer open none; no SPLIT is kept as words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_split_right_after_a_split_offer_opens_one_more_job(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0207, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0207).await;
    split_offered(&mut db, id, 1).await;
    sent(&mut db, 0x0555_0207, "SPLIT", 2).await;
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
    sent(&mut db, 0x0555_0207, "SPLIT", 3).await;
    assert_eq!(links(&mut db, id).await, both, "only right after the offer");
    assert_eq!(kept(&mut db, id).await, [DOOR]);
    sent(&mut db, 0x0555_0208, DOOR, 0).await;
    let later = request(&mut db, 0x0555_0208).await;
    split_offered(&mut db, later, 1).await;
    sent(&mut db, 0x0555_0208, "the blue house", 2).await;
    sent(&mut db, 0x0555_0208, "DIVIDIR", 3).await;
    assert_eq!(links(&mut db, later).await, ["job.handyman.none"]);
    sent(&mut db, 0x0555_0209, DOOR, 0).await;
    sent(&mut db, 0x0555_0209, "SPLIT", 1).await;
    let none = request(&mut db, 0x0555_0209).await;
    assert_eq!(
        links(&mut db, none).await,
        ["job.handyman.none"],
        "no offer"
    );
}

/// TD-INT-45 (INT-032, D-1020): DIVIDIR right after a SPLIT offer opens the job as SPLIT does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_45_dividir_right_after_a_split_offer_opens_it_too(mut db: Db) {
    setup(&mut db).await;
    sent(&mut db, 0x0555_0210, DOOR, 0).await;
    let id = request(&mut db, 0x0555_0210).await;
    split_offered(&mut db, id, 1).await;
    sent(&mut db, 0x0555_0210, "dividir", 2).await;
    let both = ["job.handyman.none", "job.plumbing_repair.none"];
    assert_eq!(links(&mut db, id).await, both);
}

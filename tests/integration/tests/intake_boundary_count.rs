//! TD-INT-53's boundary counts (T716, D-4777, D-5333, D-5400; CARD-001, LIC-043, INT-017,
//! PRIV-008): the coverage count carries the stored (trade, job type) pairs, each matched under
//! its own trade's license (`quotes::api::pair_coverage`), so a license-boundary job type named
//! under another trade never counts a business without that license; a two-word "water heater"
//! keeps plumbing's "water heater replacement" and its count; and a job opened on the ZIP's turn
//! keeps the first text's scrubbed words. PostgreSQL, fake data and 555 numbers only.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    COUNT_ONE_TEMPLATE, Handled, RawReply, Reply, TextIn, receive_raw_reply,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::{ROC_HEADER, import_city_list, import_roc_list};
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{CardFacts, Coverage, pair_coverage};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const HOME_ZIP: &str = "86403";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7401,Sample Latch One LLC,,,2019-03-04\n";
/// Fake ROC numbers: electrical (R-11) and plumbing (R-37).
const ROC: [(&str, &str); 2] = [("ROC900211", "R-11"), ("ROC900237", "R-37")];
const DOOR: (&str, &str) = ("handyman", "door repair");
const HEATER: (&str, &str) = ("plumbing_repair", "water heater replacement");

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_300_000 + minute * 60).expect("time")
}

/// Catalog and fake lists, as the owner role.
async fn lists(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    let rows: Vec<String> = ROC
        .iter()
        .map(|(n, c)| format!("{n},{c},Active,2012-05-01"))
        .collect();
    let roc = format!("{ROC_HEADER}\n{}\n", rows.join("\n"));
    import_roc_list(db, date(2026, 11, 1), &roc)
        .await
        .expect("roc list");
}

/// An active, welcomed business serving the town, ticking `ticked`, holding the ROC `numbers`
/// (each with an Active check) and, when `handyman`, signed up as a handyman matched on the city
/// list (LIC-031).
async fn business(
    db: &mut PgConnection,
    (numbers, handyman): (&[&str], bool),
    ticked: (&str, &str),
) {
    let b = create(db, false, at(0)).await.expect("business");
    for number in numbers {
        sqlx::query(
            "INSERT INTO licensing.license_check (business_id, source, license_number, \
             list_date, status, checked_at) VALUES ($1, 'roc', $2, '2026-11-01', 'active', now())",
        )
        .bind(b)
        .bind(*number)
        .execute(&mut *db)
        .await
        .expect("check");
    }
    if handyman {
        signed_up_handyman(db, b).await;
    }
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
    save_business_name(db, b, ("Sample Latch One LLC", None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(ticked.0, ticked.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
}

/// Business `b` signed up as a handyman (LIC-031).
async fn signed_up_handyman(db: &mut PgConnection, b: Uuid) {
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(db)
    .await
    .expect("handyman");
}

/// The count of a handyman job whose stored pairs are door repair and electrical's breaker trips.
async fn handyman_breaker(db: &mut PgConnection) -> Coverage {
    let pairs = [
        ("handyman".to_owned(), "door repair".to_owned()),
        ("electrical".to_owned(), "breaker trips".to_owned()),
    ];
    let facts = CardFacts {
        category: "handyman".to_owned(),
        candidates: pairs.iter().map(|(_, j)| j.clone()).collect(),
        emergency: false,
    };
    pair_coverage(db, ((TOWN, Some(HOME_ZIP)), &facts), &pairs)
        .await
        .expect("coverage")
}

/// D-4777 (CARD-001, LIC-043, D-105): "handyman needed, breaker panel keeps tripping" never counts
/// a handyman without the electrical license; a city-list handyman also holding R-11 counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_a_boundary_job_type_needs_its_own_license(mut db: Db) {
    lists(&mut db).await;
    business(&mut db, (&[], true), DOOR).await;
    let without = handyman_breaker(&mut db).await;
    assert!(!matches!(without, Coverage::Send(_)), "{without:?}");
    business(&mut db, (&[ROC[0].0], true), DOOR).await;
    assert_eq!(handyman_breaker(&mut db).await, Coverage::Send(1));
}

fn text_in(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x74_0000 + contact * 100 + minute.unsigned_abs() as u128),
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

/// The templates `words` from fake contact `contact` queued, through the route's entry.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x75), &key(0x76), &url).expect("vault");
    let raw = SecretString::from(words.to_owned());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let msg = text_in(contact, words, minute);
    let reply = receive_raw_reply(db, &secret, (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The kept words of fake contact `contact`'s newest request.
async fn kept(db: &mut PgConnection, contact: u128) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT d.words FROM quotes.job_description d JOIN intake.conversation c \
         ON c.id = d.request_id JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 ORDER BY d.recorded_at",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_all(db)
    .await
    .expect("kept")
}

/// D-4777, D-4618, INT-007: a two-word "water heater" opens its job on the ZIP's turn and keeps
/// plumbing's "water heater replacement": the R-37 holder counts, the handyman does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_water_heater_keeps_its_licensed_job_type(mut db: Db) {
    lists(&mut db).await;
    business(&mut db, (&[ROC[1].0], false), HEATER).await;
    business(&mut db, (&[], true), HEATER).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    sent(&mut db, 0x0555_0401, "water heater", 0).await;
    let got = sent(&mut db, 0x0555_0401, HOME_ZIP, 1).await;
    assert_eq!(got.first(), Some(&COUNT_ONE_TEMPLATE), "{got:?}");
    assert_eq!(kept(&mut db, 0x0555_0401).await, ["water heater"]);
}

/// D-5400 (PRIV-008, INT-007): a job opened on the ZIP's turn keeps the first text's words,
/// scrubbed as a job opened at the first text keeps them: no 555 number, and only one row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_the_zip_turn_keeps_the_scrubbed_first_words(mut db: Db) {
    lists(&mut db).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    sent(&mut db, 0x0555_0402, "plumber 928-555-0142", 0).await;
    sent(&mut db, 0x0555_0402, HOME_ZIP, 1).await;
    let words = kept(&mut db, 0x0555_0402).await;
    assert_eq!(words.len(), 1, "{words:?}");
    assert!(words[0].contains("plumber"), "{words:?}");
    assert!(!words[0].contains("555"), "{words:?}");
}

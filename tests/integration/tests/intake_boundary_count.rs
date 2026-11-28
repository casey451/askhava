//! TD-INT-53's boundary counts (T716, D-4777, D-5333, D-5400; CARD-001, LIC-043, INT-017,
//! PRIV-008): the coverage count carries the stored (trade, job type) pairs, each matched under
//! its own trade's license (`quotes::api::pair_coverage`), so a license-boundary job type named
//! under another trade never counts a business without that license; a two-word "water heater"
//! keeps plumbing's "water heater replacement" and its count; and a job opened on the ZIP's turn
//! keeps the first text's scrubbed words. The card send uses the counted pairs, each pair's trade
//! pause included (D-5606), and an under-13 purge takes a short first text's held words (D-5617).
//! PostgreSQL, fake data and 555 numbers only.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::{category, load_embedded};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, COUNT_MANY_TEMPLATE, COUNT_ONE_TEMPLATE, CardStep, CardsOutKeys, CardsSent,
    Handled, Preview, RawReply, Reply, TextIn, WHICH_FITS_TEMPLATE, answer_card, jobs_of_contact,
    receive_raw_reply, send_approved, show_card,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::{ROC_HEADER, import_city_list, import_roc_list};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardFacts, CardParts, Coverage, DEFAULT_MIN_WORDS, Place, build_card, card_digest, keep_card,
    pair_coverage,
};
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
        inbox_id: Uuid::from_u128(
            (contact ^ 0x74_0000).wrapping_mul(100) + u128::from(minute.unsigned_abs()),
        ),
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

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x75), &key(0x76), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// The templates `words` from fake contact `contact` queued, through the route's entry.
async fn sent(db: &mut PgConnection, contact: u128, words: &str, minute: i64) -> Vec<&'static str> {
    let vault = vault();
    let raw = SecretString::from(words.to_owned());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let msg = text_in(contact, words, minute);
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        Reply::UnderAge(_, Some(queued)) => vec![queued.template],
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

/// Fake contact `contact`'s newest request and its job.
async fn request_job(db: &mut PgConnection, contact: u128) -> (Uuid, Uuid) {
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 ORDER BY c.opened_at DESC LIMIT 1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(&mut *db)
    .await
    .expect("request");
    let jobs = jobs_of_contact(db, Uuid::from_u128(contact)).await;
    let jobs = jobs.expect("jobs");
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    (conversation, jobs[0])
}

/// Keeps a card for `job` whose stored facts name door repair only (not the stored pairs), shows
/// it, she replies SEND and the send runs; returns the businesses holding its card.
async fn card_sent(db: &mut PgConnection, contact: u128, minute: i64) -> Vec<Uuid> {
    let (conversation, job) = request_job(db, contact).await;
    let parts = CardParts {
        category: "Handyman".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "Door repair, the breaker keeps tripping too".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: DOOR.0.to_owned(),
        candidates: vec![DOOR.1.to_owned()],
        emergency: false,
    };
    let digest = card_digest(card.text());
    let ids = (conversation, job);
    let kept = keep_card(db, ids, &digest, &card, &facts, at(minute)).await;
    kept.expect("kept");
    let preview = Preview {
        conversation,
        job,
        card: &card,
        link: "https://example.invalid/q/fake-token",
        wait: AUTO_SEND_AFTER,
        at: at(minute),
    };
    show_card(db, &secret(), &preview).await.expect("shown");
    let send = text_in(contact, "SEND", minute + 1);
    let step = answer_card(db, &secret(), &send, ids)
        .await
        .expect("answered");
    assert!(!matches!(step, CardStep::Nothing), "{step:?}");
    let keys = CardsOutKeys::new(secret(), vault());
    let out = send_approved(db, &keys, (conversation, job, &digest), at(minute + 2)).await;
    assert!(matches!(out, Ok(CardsSent::Sent { .. })), "{out:?}");
    sqlx::query_scalar("SELECT business_id FROM quotes.job_card WHERE job_id = $1 ORDER BY 1")
        .bind(job)
        .fetch_all(db)
        .await
        .expect("holders")
}

/// A fake homeowner in the vault with the 555 number `phone`; returns her contact ID.
async fn her(db: &mut PgConnection, phone: &str) -> u128 {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new)
        .await
        .expect("contact")
        .as_u128()
}

/// Drives `first`, the ZIP and `answer` to "Which fits best?" for fake contact `contact`;
/// returns the template that answered the choice first.
async fn fit_request(
    db: &mut PgConnection,
    contact: u128,
    (first, answer): (&str, &str),
) -> &'static str {
    sent(db, contact, first, 0).await;
    let asked = sent(db, contact, HOME_ZIP, 1).await;
    assert_eq!(asked, [WHICH_FITS_TEMPLATE], "{asked:?}");
    let got = sent(db, contact, answer, 2).await;
    got.first().copied().expect("a coverage line")
}

/// "handyman needed, breaker keeps tripping" (D-4777's case).
const HANDYMAN_BREAKER: &str = "handyman needed, my breaker keeps tripping";

/// Fake contact `contact`'s "need a handyman for door repair" and ZIP, whose coverage line is
/// returned; then the request's newest stored set becomes handyman's door repair and
/// electrical's breaker trips (as `order_dispatch` stores a set: `job_type.<trade>` rows, one
/// `answered_at`), the pairs [`handyman_breaker`] counts.
async fn door_then_breaker(db: &mut PgConnection, contact: u128) -> &'static str {
    sent(db, contact, "need a handyman for door repair", 0).await;
    let got = sent(db, contact, HOME_ZIP, 1).await;
    let (conversation, _) = request_job(db, contact).await;
    for (question, choice) in [
        ("job_type.handyman", "door_repair"),
        ("job_type.electrical", "breaker_trips"),
    ] {
        sqlx::query(
            "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, \
             answered_at) VALUES ($1, $2, $3, to_timestamp($4))",
        )
        .bind(conversation)
        .bind(question)
        .bind(choice)
        .bind(at(2).as_second() as f64)
        .execute(&mut *db)
        .await
        .expect("pair");
    }
    got.first().copied().expect("a coverage line")
}

/// D-5606 (CARD-001, LIC-043, INT-017, D-105): the send uses the counted pairs, so the card goes
/// to exactly the businesses the coverage line counts. "handyman needed, my breaker keeps
/// tripping", the ZIP and "door repair" (INT-014's `Chosen`: door repair alone) counts both
/// handymen and both get the card; a handyman job whose stored pairs hold electrical's breaker
/// trips counts only the R-11 handyman and only it gets the card, never the unlicensed one,
/// though the stored card's facts name door repair only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_the_send_reaches_exactly_the_counted_businesses(mut db: Db) {
    lists(&mut db).await;
    business(&mut db, (&[], true), DOOR).await;
    business(&mut db, (&[ROC[0].0], true), DOOR).await;
    let licensed: Uuid = sqlx::query_scalar(
        "SELECT business_id FROM licensing.license_check WHERE license_number = $1",
    )
    .bind(ROC[0].0)
    .fetch_one(&mut *db)
    .await
    .expect("licensed");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let chose = her(&mut db, "+19285550163").await;
    let told = fit_request(&mut db, chose, (HANDYMAN_BREAKER, "door repair")).await;
    assert_eq!(told, COUNT_MANY_TEMPLATE);
    assert_eq!(card_sent(&mut db, chose, 3).await.len(), 2);
    let both = her(&mut db, "+19285550164").await;
    assert_eq!(door_then_breaker(&mut db, both).await, COUNT_MANY_TEMPLATE);
    assert_eq!(handyman_breaker(&mut db).await, Coverage::Send(1));
    assert_eq!(card_sent(&mut db, both, 3).await, [licensed]);
}

/// D-5606 (CARD-016, OPS-003): with the electrical trade paused by the owner, a handyman job
/// whose stored pairs hold electrical's breaker trips counts nobody and its card goes to nobody,
/// the R-11 handyman included; unpaused, the same pairs count it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_53_a_paused_pair_trade_counts_and_sends_to_nobody(mut db: Db) {
    lists(&mut db).await;
    business(&mut db, (&[ROC[0].0], true), DOOR).await;
    let electrical = category(&mut db, "electrical").await.expect("read");
    let pause = Switch::Trade(electrical.expect("stored").id);
    let owner = Actor::Owner(Uuid::from_u128(0x0716_0ce0));
    let contact = her(&mut db, "+19285550165").await;
    assert_eq!(
        door_then_breaker(&mut db, contact).await,
        COUNT_ONE_TEMPLATE
    );
    assert_eq!(handyman_breaker(&mut db).await, Coverage::Send(1));
    set_switch(&mut db, pause, true, owner).await.expect("on");
    let paused = handyman_breaker(&mut db).await;
    assert!(!matches!(paused, Coverage::Send(_)), "{paused:?}");
    assert!(card_sent(&mut db, contact, 3).await.is_empty());
}

/// Rows of `quotes.job_description` held under fake contact `contact`'s newest request's own ID.
async fn held(db: &mut PgConnection, contact: u128) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM quotes.job_description d JOIN intake.conversation c \
         ON c.id = d.request_id AND c.id = d.job_id JOIN intake.homeowner h \
         ON h.id = c.homeowner_id WHERE h.contact_id = $1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(db)
    .await
    .expect("held")
}

/// Consent records of fake contact `contact`.
async fn consents(db: &mut PgConnection, contact: u128) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h \
         ON h.id = r.homeowner_id WHERE h.contact_id = $1",
    )
    .bind(Uuid::from_u128(contact))
    .fetch_one(db)
    .await
    .expect("consents")
}

/// D-5617 (INT-038, PRIV-008): a two-word trade-naming first text holds its scrubbed words under
/// the request's own ID; "I'm 12" before the ZIP purges them with the request, and the consent
/// record stays.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_an_under_13_purge_takes_the_held_first_words(mut db: Db) {
    lists(&mut db).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let contact = 0x0555_0405;
    sent(&mut db, contact, "water heater", 0).await;
    assert_eq!(held(&mut db, contact).await, 1, "held before the ZIP");
    assert_eq!(consents(&mut db, contact).await, 1, "consent recorded");
    sent(&mut db, contact, "I'm 12", 1).await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM quotes.job_description")
        .fetch_one(&mut *db)
        .await
        .expect("left");
    assert_eq!(left, 0, "no held words left");
    assert_eq!(
        consents(&mut db, contact).await,
        1,
        "the consent record stays"
    );
}

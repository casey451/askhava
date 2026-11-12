//! The ZIP and area check (TD-INT-38: INT-049, INT-048, INT-022, INT-019; TD-PRIV-13's schema
//! case: PRIV-028; T251, D-1521).
//!
//! Pure cases read ZIPs and place them in the compiled-in markets; database cases open a fake
//! request with `receive_text` and run `ask_zip` and `answer_zip` on a fresh database, in English
//! and Spanish. Fake IDs and words only; the ZIPs are public area codes of the market file.

use std::sync::atomic::{AtomicU64, Ordering};

use askhava_jobs::api::{JobStatus, current};
use askhava_messaging::api::Language;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{
    JobStart, OUT_OF_AREA_TEMPLATE, Opened, TextIn, ZIP_AGAIN_TEMPLATE, ZIP_QUESTION_TEMPLATE,
    ZipPlace, ZipStep, answer_zip, ask_zip, begin_job, conversation, place_zip, read_zip,
    receive_text, zip_body,
};
use crate::rules::{Inbound, Settings};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";

/// A new fake ID for each call.
fn fresh() -> Uuid {
    static NEXT: AtomicU64 = AtomicU64::new(0x2510_0000);
    Uuid::from_u128(u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + SignedDuration::from_mins(minute)
}

fn spanish() -> Settings {
    Settings {
        spanish_conversations_on: true,
        safety_call_on: false,
    }
}

fn text_in(contact: Uuid, minute: i64, words: &str) -> TextIn<'_> {
    TextIn {
        inbox_id: fresh(),
        contact,
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

/// A fake request with a plumbing job from `words`, asked the ZIP question: (contact, request,
/// job).
async fn asked(db: &mut Db, words: &str, settings: Settings) -> (Uuid, Uuid, Uuid) {
    let contact = fresh();
    let first = text_in(contact, 0, words);
    let handled = receive_text(db, &secret(), &first, settings).await;
    let id = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation: id,
        words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a job");
    };
    let msg = text_in(contact, 2, "");
    let queued = ask_zip(db, &secret(), (&msg, id), settings).await;
    assert_eq!(queued.expect("asked").template, ZIP_QUESTION_TEMPLATE);
    (contact, id, job)
}

async fn reply(db: &mut Db, (contact, id): (Uuid, Uuid), minute: i64, words: &str) -> ZipStep {
    let msg = text_in(contact, minute, words);
    answer_zip(db, &secret(), (&msg, id), Settings::default())
        .await
        .expect("answered")
}

/// Every property row of the homeowner of `contact`: (contact ID, town, ZIP).
async fn properties(db: &mut Db, contact: Uuid) -> Vec<(Uuid, String, Option<String>)> {
    sqlx::query_as("SELECT contact_id, town, zip FROM intake.property WHERE contact_id = $1")
        .bind(contact)
        .fetch_all(&mut **db)
        .await
        .expect("properties")
}

/// The (template, body) texts queued to `contact`, oldest first.
async fn texts(db: &mut Db, contact: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT template_id, body FROM messaging.outbox WHERE recipient_id = $1 \
         AND template_id <> 'consent' ORDER BY created_at, id",
    )
    .bind(contact)
    .fetch_all(&mut **db)
    .await
    .expect("outbox")
}

fn body(template: &str, language: Language) -> String {
    zip_body(template, language, true)
}

/// TD-INT-38 (INT-049): one 5-digit ZIP is read from the reply; none, two different ones or a
/// longer digit run is not a ZIP.
#[test]
fn td_int_38_one_zip_is_read_from_a_reply() {
    for (words, zip) in [
        ("86403", Some("86403")),
        ("It's 86404.", Some("86404")),
        ("86403-1234", Some("86403")),
        ("86403 yes 86403", Some("86403")),
        ("no idea", None),
        ("86403 or 86404", None),
        ("864031", None),
        ("8640", None),
    ] {
        assert_eq!(read_zip(words), zip, "{words}");
    }
}

/// TD-INT-38 (INT-049, WORD-008, TXT-039): each ZIP text opens with the brand's short name in its
/// language, says what to send, fits one English segment, and is English while Spanish is off.
#[test]
fn td_int_38_the_zip_texts_in_english_and_spanish() {
    let opening = |es: bool| {
        let b = askhava_core::api::brand();
        format!(
            "{}: ",
            if es {
                b.es().short_name()
            } else {
                b.short_name()
            }
        )
    };
    for (id, en, es) in [
        (
            ZIP_QUESTION_TEMPLATE,
            "What's the ZIP code where the work is?",
            "código postal",
        ),
        (
            ZIP_AGAIN_TEMPLATE,
            "Just the 5 numbers, like 86403.",
            "Solo los 5 números",
        ),
    ] {
        let english = zip_body(id, Language::En, true);
        assert!(
            english.starts_with(&opening(false)) && english.contains(en),
            "{english}"
        );
        assert!(
            english.len() <= 160 && english.is_ascii(),
            "one GSM segment: {english}"
        );
        let spanish = zip_body(id, Language::Es, true);
        assert!(
            spanish.starts_with(&opening(true)) && spanish.contains(es),
            "{spanish}"
        );
        assert_eq!(zip_body(id, Language::Es, false), english, "Spanish off");
    }
}

/// TD-INT-38 (INT-048, INT-022): a town ZIP has its town and a location; the reviewed PO-box ZIP
/// has the town and none; Kingman's ZIP is Arizona but out of the area; 99999 is unknown.
#[test]
fn td_int_38_zips_are_placed_by_the_market_data() {
    let town = |located| ZipPlace::Town {
        town: TOWN.to_owned(),
        located,
    };
    assert_eq!(place_zip("86403"), Ok(town(true)));
    assert_eq!(place_zip("86405"), Ok(town(false)));
    assert_eq!(place_zip("86401"), Ok(ZipPlace::OutOfArea));
    assert_eq!(place_zip("99999"), Ok(ZipPlace::Unknown));
}

/// TD-INT-38 (INT-049, PRIV-028): a service-area ZIP stores exactly the contact, the town and the
/// ZIP, and the step moves on; the PO-box ZIP stores the town with no location; the question is
/// the English template.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_a_town_zip_is_stored_on_the_property(mut db: Db) {
    for (zip, located) in [("86404", true), ("86405", false)] {
        let (contact, id, _) = asked(&mut db, "my sink is leaking", Settings::default()).await;
        let step = reply(&mut db, (contact, id), 3, &format!("the house is in {zip}")).await;
        let place = ZipPlace::Town {
            town: TOWN.to_owned(),
            located,
        };
        assert_eq!(step, ZipStep::Stored(place));
        let stored = properties(&mut db, contact).await;
        assert_eq!(
            stored,
            vec![(contact, TOWN.to_owned(), Some(zip.to_owned()))]
        );
        let sent = texts(&mut db, contact).await;
        let question = (
            ZIP_QUESTION_TEMPLATE.to_owned(),
            body(ZIP_QUESTION_TEMPLATE, Language::En),
        );
        assert_eq!(sent, vec![question]);
    }
}

/// TD-INT-38 (INT-022): an Arizona ZIP in no service-area town closes the request early with the
/// out-of-area reply and stores no property.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_an_arizona_zip_outside_the_area_closes_early(mut db: Db) {
    let (contact, id, job) = asked(&mut db, "my sink is leaking", Settings::default()).await;
    let step = reply(&mut db, (contact, id), 3, "86401").await;
    let ZipStep::OutOfArea(Some(queued)) = step else {
        panic!("{step:?}");
    };
    assert_eq!(queued.template, OUT_OF_AREA_TEMPLATE);
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert!(row.closed_at.is_some(), "closed early");
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status, JobStatus::ClosedEarly);
    assert!(properties(&mut db, contact).await.is_empty());
}

/// TD-INT-38 (INT-049, INT-019): no ZIP, two ZIPs or an unknown well-formed ZIP is asked once
/// more; a further unusable reply closes nothing and asks nothing again; a usable one still counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_an_unusable_reply_is_asked_once_more_then_waited_on(mut db: Db) {
    for words in ["I'm not sure", "86403 or 86404", "99999"] {
        let (contact, id, _) = asked(&mut db, "my sink is leaking", Settings::default()).await;
        let step = reply(&mut db, (contact, id), 3, words).await;
        assert!(matches!(step, ZipStep::AskedAgain(_)), "{words}: {step:?}");
        assert_eq!(
            reply(&mut db, (contact, id), 4, words).await,
            ZipStep::Waiting
        );
        assert_eq!(
            reply(&mut db, (contact, id), 5, "lol").await,
            ZipStep::Waiting
        );
        let row = conversation(&mut db, id).await.expect("read").expect("row");
        assert!(row.closed_at.is_none(), "nothing closes");
        let sent: Vec<String> = texts(&mut db, contact)
            .await
            .into_iter()
            .map(|t| t.0)
            .collect();
        assert_eq!(sent, [ZIP_QUESTION_TEMPLATE, ZIP_AGAIN_TEMPLATE]);
        assert!(matches!(
            reply(&mut db, (contact, id), 6, "86403").await,
            ZipStep::Stored(_)
        ));
    }
}

/// TD-INT-38 (INT-049, TXT-039): a Spanish request gets the Spanish question and re-ask.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_spanish_question_and_re_ask(mut db: Db) {
    let (contact, id, _) = asked(&mut db, "Hola, mi lavabo gotea mucho", spanish()).await;
    let msg = text_in(contact, 3, "no sé");
    let step = answer_zip(&mut db, &secret(), (&msg, id), spanish()).await;
    assert!(matches!(step.expect("answered"), ZipStep::AskedAgain(_)));
    let sent: Vec<String> = texts(&mut db, contact)
        .await
        .into_iter()
        .map(|t| t.1)
        .collect();
    let expected = [ZIP_QUESTION_TEMPLATE, ZIP_AGAIN_TEMPLATE].map(|t| body(t, Language::Es));
    assert_eq!(sent, expected);
}

/// TD-INT-38 (INT-049, PRIV-016): the stored ZIP is in no answer row, audit value or outbox text,
/// and the step's file writes no log line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_the_zip_goes_nowhere_but_the_property(mut db: Db) {
    let (contact, id, _) = asked(&mut db, "my sink is leaking", Settings::default()).await;
    reply(&mut db, (contact, id), 3, "86404").await;
    let found: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM intake.intake_answer a WHERE a::text LIKE '%86404%') + \
         (SELECT count(*) FROM ops.audit_log l WHERE l::text LIKE '%86404%') + \
         (SELECT count(*) FROM messaging.outbox o WHERE o::text LIKE '%86404%') + \
         (SELECT count(*) FROM core.task t WHERE t::text LIKE '%86404%')",
    )
    .fetch_one(&mut *db)
    .await
    .expect("count");
    assert_eq!(found, 0);
    let source = include_str!("../zip_step.rs");
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

/// TD-PRIV-13 (PRIV-028, D-1519): `intake.property` has only its ID, the homeowner, the vault
/// contact ID, the town, the 5-digit ZIP and the time; no address column.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_13_the_property_holds_only_contact_town_and_zip(mut db: Db) {
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = 'intake' AND table_name = 'property' ORDER BY column_name",
    )
    .fetch_all(&mut *db)
    .await
    .expect("columns");
    let expected = [
        "contact_id",
        "created_at",
        "homeowner_id",
        "id",
        "town",
        "zip",
    ];
    assert_eq!(columns, expected);
    let bad = sqlx::query(
        "INSERT INTO intake.property (homeowner_id, contact_id, town, zip, created_at) \
         VALUES (gen_random_uuid(), gen_random_uuid(), 'lake_havasu_city', '8640', now())",
    )
    .execute(&mut *db)
    .await;
    assert!(bad.is_err(), "only five digits");
}

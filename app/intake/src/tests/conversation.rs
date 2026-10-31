//! What opens a job, duplicates, exact answers, the name and address and the under-18 reply
//! (TD-INT-02, TD-INT-11, TD-INT-14, TD-INT-20: INT-007 to INT-012, INT-027, INT-032, INT-038).
//!
//! Pure cases call the rules; database cases run `receive_text` and the conversation functions on
//! a fresh database with fake IDs and fake text only. The two-trade cases are TD-INT-35's (T042).

use askhava_messaging::api::{Language, render};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{
    Answer, JobStart, Opened, Recorded, Taken, TextIn, ask_name_address, begin_job,
    consent_records, conversation, hear, joining_job, receive_text, record_answer,
    stop_if_under_18, take_name_address,
};
use crate::rules::{
    AgeStop, Inbound, JOIN_WINDOW, NAME_ADDRESS_TEMPLATE, Settings, accepts_word_answer, age_stop,
    cancel_like, joins_open_job, opens_job, pick_option, split_name_address,
};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + SignedDuration::from_mins(minute)
}

fn inbound(words: &str) -> Inbound<'_> {
    Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    }
}

fn text_in(contact: Uuid, minute: i64, inbound: Inbound<'_>) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x400 + minute as u128),
        contact,
        inbound,
        received_at: at(minute),
    }
}

/// A fake open request from one text, and the homeowner row's ID.
async fn open_request(db: &mut Db, who: u128, words: &str) -> (Uuid, Uuid) {
    let msg = text_in(Uuid::from_u128(who), 0, inbound(words));
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let id = handled.conversation.expect("request");
    let row = conversation(db, id).await.expect("read").expect("row");
    (id, row.homeowner_id)
}

fn start<'a>(conversation: Uuid, words: &'a str, trade: &'a str, minute: i64) -> JobStart<'a> {
    JobStart {
        conversation,
        words,
        emergency: false,
        trade,
        at: at(minute),
    }
}

async fn answers(db: &mut Db, id: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query!(
        "SELECT question_id, choice FROM intake.intake_answer WHERE conversation_id = $1",
        id
    )
    .fetch_all(&mut **db)
    .await
    .expect("answers")
    .into_iter()
    .map(|r| (r.question_id, r.choice))
    .collect()
}

/// TD-INT-02 (INT-007, INT-008, D-138): "ME", a bare ZIP and "ok sure" open nothing, a cancel-like
/// message opens nothing even with an emergency word, and an emergency keyword opens at any length.
#[test]
fn td_int_02_short_and_cancel_like_messages_open_nothing() {
    for words in ["ME", "86403", "ok sure", "yes", ""] {
        assert!(!opens_job(words, false), "{words}");
    }
    assert!(opens_job("sink is leaking", false));
    assert!(opens_job("gas smell", true));
    assert!(opens_job("fuego", true));
    for words in [
        "cancel",
        "CANCEL my request please",
        "stop",
        "cancelar",
        "never mind the sink",
        "cancel the AC repair job now",
    ] {
        assert!(cancel_like(words), "{words}");
        assert!(!opens_job(words, false), "{words}");
    }
    assert!(!opens_job("cancel gas leak", true));
    assert!(!cancel_like("the leak will not stop and the floor is wet"));
    assert!(opens_job(
        "the leak will not stop and the floor is wet",
        false
    ));
}

/// TD-INT-02 (INT-010): a second message within 1:59:59 joins; 2:00:00 does not (T042's rule).
#[test]
fn td_int_02_the_join_window_ends_at_two_hours() {
    let last = at(0);
    let secs = |s: i64| last + SignedDuration::from_secs(s);
    assert!(joins_open_job(last, secs(7199), JOIN_WINDOW));
    assert!(!joins_open_job(last, secs(7200), JOIN_WINDOW));
    assert!(!joins_open_job(last, secs(-1), JOIN_WINDOW));
    assert!(joins_open_job(
        last,
        secs(7200),
        SignedDuration::from_hours(3)
    ));
}

/// TD-INT-02 (INT-011, INT-012): "ok" inside "broke" is not an answer; word answers are accepted.
#[test]
fn td_int_02_answers_match_exactly_and_word_answers_count() {
    let options = [("yes", "ok"), ("no", "no"), ("ac", "ac")];
    assert_eq!(pick_option("ok", &options), Some("yes"));
    assert_eq!(pick_option("the AC one", &options), Some("ac"));
    assert_eq!(pick_option("Yes! ok", &[("yes", "yes")]), Some("yes"));
    assert_eq!(pick_option("my pipe broke", &options), None);
    assert_eq!(pick_option("no, ok fine", &options), Some("no"));
    for words in ["the AC one", "Yes!", "Maria", "my email is a@example.com"] {
        assert!(accepts_word_answer(words), "{words}");
    }
    assert!(!accepts_word_answer("  "));
}

/// TD-INT-02 (INT-007, INT-009, INT-032): the job-opening path refuses a short message, opens one
/// job per homeowner, property and trade, and opens the ALSO reply's second job for another trade.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_02_one_active_job_per_trade(mut db: Db) {
    let (id, _) = open_request(&mut db, 0xA1, "my sink is leaking").await;
    let me = begin_job(&mut db, &start(id, "ME", "plumbing", 1)).await;
    assert_eq!(me, Ok(Opened::Refused));
    let first = begin_job(&mut db, &start(id, "my sink is leaking", "plumbing", 2)).await;
    let Ok(Opened::Created(job)) = first else {
        panic!("{first:?}");
    };
    let again = begin_job(&mut db, &start(id, "the sink leaks again", "plumbing", 3)).await;
    assert_eq!(again, Ok(Opened::Existing(job)));
    let also = begin_job(&mut db, &start(id, "also the AC is warm", "hvac", 4)).await;
    assert!(matches!(also, Ok(Opened::Created(second)) if second != job));
    let bad = begin_job(&mut db, &start(id, "water is leaking", "Bad Trade", 5)).await;
    assert!(bad.is_err());
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert_eq!(row.job_id, Some(job));
}

/// TD-INT-02 (INT-007, D-138): a one-word emergency message opens a job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_02_an_emergency_keyword_opens_a_job_at_any_length(mut db: Db) {
    let (id, _) = open_request(&mut db, 0xA2, "gas").await;
    let mut gas = start(id, "gas", "plumbing", 1);
    assert_eq!(begin_job(&mut db, &gas).await, Ok(Opened::Refused));
    gas.emergency = true;
    assert!(matches!(
        begin_job(&mut db, &gas).await,
        Ok(Opened::Created(_))
    ));
}

/// TD-INT-02 (INT-010): a message 1:59:59 after the last homeowner message joins the open job, even
/// as another trade; a message at 2:00:00 does not (T042's rule); a newer message restarts it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_02_a_follow_up_inside_the_window_joins(mut db: Db) {
    let (id, _) = open_request(&mut db, 0xA3, "my sink is leaking").await;
    let Ok(Opened::Created(job)) =
        begin_job(&mut db, &start(id, "my sink is leaking", "plumbing", 0)).await
    else {
        panic!("no job");
    };
    let last = at(0);
    for (secs, joined) in [(7199, Some(job)), (7200, None)] {
        let found = joining_job(
            &mut db,
            id,
            last + SignedDuration::from_secs(secs),
            JOIN_WINDOW,
        )
        .await;
        assert_eq!(found, Ok(joined), "{secs}");
    }
    let msg = text_in(Uuid::from_u128(0xA3), 60, inbound("and the AC is warm"));
    hear(&mut db, id, &msg).await.expect("heard");
    assert_eq!(
        joining_job(
            &mut db,
            id,
            at(60) + SignedDuration::from_secs(7199),
            JOIN_WINDOW
        )
        .await,
        Ok(Some(job))
    );
}

/// TD-INT-02 (INT-011, INT-012): an answer counts only for an open request, matched exactly; a
/// free-answer question takes any words and stores no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_02_answers_count_only_for_an_open_request(mut db: Db) {
    let (id, _) = open_request(&mut db, 0xA4, "my sink is leaking").await;
    let options = [("yes", "ok"), ("ac", "ac")];
    let ask = |words, options| Answer {
        conversation: id,
        question: "which_one",
        options,
        words,
        reply: (Uuid::from_u128(0x900), at(1)),
    };
    let broke = record_answer(&mut db, &ask("my pipe broke", Some(&options))).await;
    assert_eq!(broke, Ok(Recorded::Unclear));
    let ac = record_answer(&mut db, &ask("the AC one", Some(&options))).await;
    assert_eq!(ac, Ok(Recorded::Stored("ac".into())));
    let free = record_answer(&mut db, &ask("my email is a@example.com", None)).await;
    assert_eq!(free, Ok(Recorded::Stored("text".into())));
    let stored = answers(&mut db, id).await;
    assert_eq!(stored.iter().filter(|(q, _)| q == "which_one").count(), 2);
    sqlx::query!(
        "UPDATE intake.conversation SET closed_at = opened_at WHERE id = $1",
        id
    )
    .execute(&mut *db)
    .await
    .expect("close");
    let late = record_answer(&mut db, &ask("the AC one", Some(&options))).await;
    assert_eq!(late, Ok(Recorded::Closed));
}

/// TD-INT-11, TD-INT-14 (INT-027, D-238, D-437): the question is not asked before the area check,
/// carries the who-sees-it line in both languages and asks nothing about owning or renting; the
/// answer is stored in the vault, nothing of it in `intake`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_11_the_name_and_address_question_and_its_vault_answer(mut db: Db) {
    let (id, homeowner) = open_request(&mut db, 0xA5, "my sink is leaking").await;
    let msg = text_in(Uuid::from_u128(0xA5), 5, inbound("86403"));
    let early = ask_name_address(&mut db, &secret(), &msg, id, false).await;
    assert_eq!(early, Ok(None));
    sqlx::query!(
        "INSERT INTO intake.property (homeowner_id, contact_id, town, created_at) \
         VALUES ($1, $2, 'lake_havasu_city', now())",
        homeowner,
        Uuid::from_u128(0xA5)
    )
    .execute(&mut *db)
    .await
    .expect("property");
    let asked = ask_name_address(&mut db, &secret(), &msg, id, false).await;
    assert_eq!(
        asked.expect("ask").map(|q| q.template),
        Some(NAME_ADDRESS_TEMPLATE)
    );
    for language in [Language::En, Language::Es] {
        let text = render(NAME_ADDRESS_TEMPLATE, language, true, &[])
            .expect("render")
            .body;
        let line = if language == Language::En {
            "Only the business you pick gets this, after you pick it. We never ask about immigration status or ID."
        } else {
            "Solo el negocio que usted elija la recibe, después de que lo elija."
        };
        assert!(text.contains(line), "{text}");
        for word in ["own", "rent", "landlord", "alquil", "propietario"] {
            assert!(!text.to_lowercase().contains(word), "{text}");
        }
    }
    let key = SecretString::from("1:MTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTE=".to_owned());
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let index = SecretString::from("1:MjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjI=".to_owned());
    let vault = Vault::new(&key, &index, &url).expect("keys");
    let raw = SecretString::from("Zelda 12 Test Street".to_owned());
    let inbox = (id, Uuid::from_u128(0x901));
    let taken = take_name_address(&mut db, &vault, inbox, &raw, at(6)).await;
    let Ok(Taken::Stored(contact)) = taken else {
        panic!("{taken:?}");
    };
    let kept = answers(&mut db, id).await;
    let hex = contact.simple().to_string();
    assert!(kept.contains(&("address_contact".to_owned(), Some(hex))));
    let text = format!("{kept:?}").to_lowercase();
    assert!(!text.contains("zelda") && !text.contains("test street"));
    let none = SecretString::from("Zelda".to_owned());
    let again = take_name_address(&mut db, &vault, inbox, &none, at(7)).await;
    assert_eq!(again, Ok(Taken::Again));
    assert_eq!(
        split_name_address("Zelda Q 12 Test St"),
        Some(("Zelda Q".into(), "12 Test St".into()))
    );
}

/// TD-INT-20 (INT-038): the stated-age and school rules, and someone else's age as a normal request.
#[test]
fn td_int_20_age_rules() {
    for (words, stop) in [
        ("I'm 15 and our sink leaks", AgeStop::Under18),
        ("I am 16 years old", AgeStop::Under18),
        ("tengo 15 años", AgeStop::Under18),
        ("im in high school and the sink leaks", AgeStop::Under18),
        ("I'm in middle school", AgeStop::Under18),
        ("I go to junior high", AgeStop::Under18),
        ("I'm 11", AgeStop::Under13),
        ("tengo 12 años", AgeStop::Under13),
    ] {
        assert_eq!(age_stop(words), Some(stop), "{words}");
    }
    for words in [
        "my 15-year-old broke the door",
        "I'm 45 and the AC is out",
        "I'm 15 minutes away",
        "my son is in high school and the sink leaks",
        "the high school gym roof leaks",
        "I'm 18",
    ] {
        assert_eq!(age_stop(words), None, "{words}");
    }
}

/// TD-INT-20 (INT-038, INT-004, D-385): "I'm 15 and our sink leaks" gets one under-18 reply and
/// the request closes with no job; "I'm 15 and I smell gas" gets the 911 text first; "I'm 11" is
/// deleted except the consent record; "my 15-year-old broke the door" is a normal request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_an_under_18_sender_gets_one_reply_and_the_request_closes(mut db: Db) {
    let settings = Settings::default();
    let words = "I'm 15 and our sink leaks";
    let msg = text_in(Uuid::from_u128(0xB1), 0, inbound(words));
    let handled = receive_text(&mut db, &secret(), &msg, settings)
        .await
        .expect("open");
    let id = handled.conversation.expect("request");
    let stop = stop_if_under_18(&mut db, &secret(), &msg, id, false)
        .await
        .expect("stop");
    assert_eq!(
        stop.map(|(s, q)| (s, q.template)),
        Some((AgeStop::Under18, "under_18"))
    );
    let again = stop_if_under_18(&mut db, &secret(), &msg, id, false)
        .await
        .expect("again");
    assert_eq!(again, None);
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert!(row.closed_at.is_some() && row.job_id.is_none());
    assert_eq!(
        begin_job(&mut db, &start(id, words, "plumbing", 1)).await,
        Ok(Opened::Refused)
    );

    let gas = Inbound {
        emergency: true,
        ..inbound("I'm 15 and I smell gas")
    };
    let msg = text_in(Uuid::from_u128(0xB2), 0, gas);
    let handled = receive_text(&mut db, &secret(), &msg, settings)
        .await
        .expect("gas");
    let order: Vec<_> = handled.texts.iter().map(|q| q.template).collect();
    assert_eq!(order, ["emergency_911", "consent"]);
    let gas_id = handled.conversation.expect("request");
    let stop = stop_if_under_18(&mut db, &secret(), &msg, gas_id, false)
        .await
        .expect("stop");
    assert_eq!(stop.map(|(_, q)| q.template), Some("under_18"));

    let young = text_in(Uuid::from_u128(0xB3), 0, inbound("I'm 11"));
    let handled = receive_text(&mut db, &secret(), &young, settings)
        .await
        .expect("young");
    let young_id = handled.conversation.expect("request");
    let ids = (young_id, Uuid::from_u128(0x902));
    crate::models::insert_answer(&mut db, ids, ("how_soon", "today"), at(1))
        .await
        .expect("answer");
    let stop = stop_if_under_18(&mut db, &secret(), &young, young_id, false)
        .await
        .expect("stop");
    assert_eq!(stop.map(|(s, _)| s), Some(AgeStop::Under13));
    assert!(answers(&mut db, young_id).await.is_empty());
    let records = consent_records(&mut db, young_id).await.expect("records");
    assert_eq!(records.len(), 1);

    let other = text_in(
        Uuid::from_u128(0xB4),
        0,
        inbound("my 15-year-old broke the door"),
    );
    let handled = receive_text(&mut db, &secret(), &other, settings)
        .await
        .expect("other");
    let other_id = handled.conversation.expect("request");
    let none = stop_if_under_18(&mut db, &secret(), &other, other_id, false).await;
    assert_eq!(none, Ok(None));
    assert!(matches!(
        begin_job(
            &mut db,
            &start(other_id, "my 15-year-old broke the door", "carpentry", 1)
        )
        .await,
        Ok(Opened::Created(_))
    ));
}

/// TD-INT-20 (INT-038): the Spanish reply stays Spanish while Spanish conversations are off.
#[test]
fn td_int_20_the_spanish_reply_stays_on() {
    let es = render("under_18", Language::Es, false, &[])
        .expect("render")
        .body;
    assert_eq!(
        es,
        "Ask Hava: Lo sentimos, debe tener 18 años o más para usar Ask Hava. Pídale a un padre o adulto que nos escriba."
    );
    let en = render("under_18", Language::En, false, &[])
        .expect("render")
        .body;
    assert_eq!(
        en,
        "Ask Hava: Sorry, you must be 18 or older to use Ask Hava. Please ask a parent or adult to text us."
    );
}

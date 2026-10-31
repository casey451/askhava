//! The 911-list screen, the SAFE hold, the INT-037 question, "How soon?" and the night count's
//! texts (TD-INT-01, TD-INT-10, TD-INT-13, TD-INT-15, TD-INT-16: INT-002 to INT-006, INT-024 to
//! INT-026, INT-030, INT-033, INT-037, D-436).
//!
//! Pure cases call the matcher and the offer rule; database cases run `on_text` and
//! `receive_answer` on a fresh database with fake IDs only. The whole urgent-emergency flow with
//! a business in the night count is `tests/integration/tests/emergency_flow.rs`.

use askhava_messaging::api::{Language, Registry, render};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{
    Asked, CHOICE_TEMPLATE, Handled, NIGHT_MANY_TEMPLATE, NIGHT_NONE_TEMPLATE, NIGHT_ONE_TEMPLATE,
    Screened, TextIn, emergency_events, is_night, offer, on_text, receive_answer, safe_hold,
};
use crate::emergency::{Check, Choice, Hazard, Urgency, check, choice, is_safe_reply, urgency};
use crate::rules::{CONSENT_TEMPLATE, Inbound, SAFETY_TEMPLATE, Settings};
use crate::service::BEEP_TEMPLATE;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

/// TD-INT-01 (INT-002, INT-004, D-436): the 911 list in English and Spanish.
const MATCHES: &[(&str, Hazard)] = &[
    ("smell gas", Hazard::Gas),
    ("gas leak", Hazard::Gas),
    ("gas line is hissing", Hazard::Gas),
    ("rotten egg smell", Hazard::Gas),
    ("huele a gas", Hazard::Gas),
    ("propane smell by the tank", Hazard::Gas),
    ("CO alarm going off", Hazard::CoAlarm),
    ("la alarma está sonando", Hazard::CoAlarm),
    ("smoke in the kitchen", Hazard::SmokeFire),
    ("outlet caught fire", Hazard::SmokeFire),
    ("en llamas", Hazard::SmokeFire),
    ("outlet is sparking", Hazard::Electrical),
    ("breaker panel is smoking", Hazard::Electrical),
    ("power line down in the yard", Hazard::PowerLine),
];

/// TD-INT-01, TD-INT-10 (INT-002, INT-026, INT-037, D-436): no stem matches these.
const CLEAR: &[&str] = &[
    "pipe burst, water everywhere",
    "AC died, 92 inside",
    "no heat",
    "dizzy and head hurts",
    "gas water heater not working",
    "need a gas line for my new stove",
    "fire pit install",
    "my smoke detector chirps",
    "pool shock",
    "the breaker panel is buzzing and hot to the touch",
    "my sink is leaking",
];

/// TD-INT-01 (INT-002, INT-037): every 911-list phrase matches its entry, nothing else does, and
/// "smoke detector beeping" gets the INT-037 question.
#[test]
fn td_int_01_the_911_list_matches_by_stem() {
    for (words, hazard) in MATCHES {
        assert_eq!(check(words, false), Check::Match(*hazard), "{words}");
    }
    for words in CLEAR {
        assert_eq!(check(words, false), Check::Clear, "{words}");
    }
    assert_eq!(check("smoke detector beeping", false), Check::BeepQuestion);
    assert_eq!(
        check("CO detector beeping, low battery", false),
        Check::Clear
    );
    assert_eq!(check("smoke alarm beeps every minute", false), Check::Clear);
}

/// TD-INT-01 (INT-002, D-165): street names never match: not in the name-and-address answer, and
/// not as an address pattern inside another message, while a real hazard beside one still does.
#[test]
fn td_int_01_addresses_are_not_hazards() {
    for answer in [
        "Pat, 100 Smoketree Test Ave",
        "Burst Test Ln",
        "Smoketree Ave",
    ] {
        assert_eq!(check(answer, true), Check::Clear, "{answer}");
        assert_eq!(check(answer, false), Check::Clear, "{answer}");
    }
    let real = check("I smell gas at 100 Smoketree Test Ave", false);
    assert_eq!(real, Check::Match(Hazard::Gas));
    assert_eq!(check("smell gas", true), Check::Clear);
}

/// TD-INT-10 (INT-025, D-128, D-419): the safe replies, and what does not count.
#[test]
fn td_int_10_safe_replies() {
    for words in [
        "safe",
        "SAFE",
        "seguro",
        "segura",
        "we're safe",
        "we're fine",
        "we're ok",
    ] {
        assert!(is_safe_reply(words), "{words}");
    }
    for words in ["ok", "OK", "yes", "Yes"] {
        assert!(is_safe_reply(words), "{words}");
    }
    for words in [
        "not safe",
        "no seguro",
        "help",
        "the smell is worse",
        "yes the gas is on",
    ] {
        assert!(!is_safe_reply(words), "{words}");
    }
}

/// TD-INT-13, TD-INT-16 (INT-030, INT-006, D-208, D-292): the four "How soon?" answers and the
/// emergency-or-regular choice.
#[test]
fn td_int_13_how_soon_and_the_choice_are_read() {
    let read = [
        ("1", Urgency::Today),
        ("Today, it's urgent", Urgency::Today),
        ("2", Urgency::FewDays),
        ("3", Urgency::NoRush),
        ("4", Urgency::ByDay),
    ];
    for (words, want) in read {
        assert_eq!(urgency(words), Some(want), "{words}");
    }
    assert_eq!(urgency("what?"), None);
    assert_eq!(choice("1"), Some(Choice::Emergency));
    assert_eq!(choice("Emergency"), Some(Choice::Emergency));
    assert_eq!(choice("2"), Some(Choice::Regular));
    assert_eq!(choice("hmm"), None);
}

fn az(time: &str) -> Timestamp {
    format!("2026-11-02T{time}:00-07:00").parse().expect("time")
}

/// TD-INT-15, TD-INT-13 (INT-033, INT-006, D-271, D-324, D-346): at 22:40 the count text comes
/// before the choice; with 0 the zero text goes alone and no choice is offered; at 14:00 only the
/// daytime choice goes, whatever the count.
#[test]
fn td_int_15_night_count_comes_before_the_choice() {
    assert!(is_night(az("22:40")) && is_night(az("07:59")) && is_night(az("20:00")));
    assert!(!is_night(az("14:00")) && !is_night(az("08:00")));
    let one = offer(az("22:40"), 1);
    assert_eq!(one.templates, [NIGHT_ONE_TEMPLATE, CHOICE_TEMPLATE]);
    assert!(one.offered);
    let many = offer(az("22:40"), 3);
    assert_eq!(many.templates, [NIGHT_MANY_TEMPLATE, CHOICE_TEMPLATE]);
    let none = offer(az("22:40"), 0);
    assert_eq!(
        (none.templates.as_slice(), none.offered),
        ([NIGHT_NONE_TEMPLATE].as_slice(), false)
    );
    for n in [0, 1, 5] {
        assert_eq!(offer(az("14:00"), n).templates, [CHOICE_TEMPLATE]);
    }
}

fn en(id: &str, values: &[(&str, &str)]) -> String {
    render(id, Language::En, false, values)
        .expect("render")
        .body
}

/// TD-INT-15 (INT-006, INT-033, D-324, D-346, D-436): the count texts are INT-033's words, "1 HVAC
/// business ... gets" for one; the zero text is INT-033's exactly; the daytime choice names the
/// email marked EMERGENCY and the 60 minutes, and never a phone call or "reach you tonight".
#[test]
fn td_int_15_count_and_choice_wording() {
    let hvac = [("count", "3"), ("trade", "HVAC")];
    assert_eq!(
        en(NIGHT_MANY_TEMPLATE, &hvac),
        "Ask Hava: 3 HVAC businesses on Ask Hava get emergency requests at night."
    );
    assert_eq!(
        en(NIGHT_ONE_TEMPLATE, &hvac),
        "Ask Hava: 1 HVAC business on Ask Hava gets emergency requests at night."
    );
    assert_eq!(
        en(NIGHT_NONE_TEMPLATE, &hvac),
        "Ask Hava: No HVAC business on Ask Hava gets emergency requests at night yet. Your request goes out at 8 a.m."
    );
    let day = en(CHOICE_TEMPLATE, &[]);
    assert!(day.contains(
        "Emergency: businesses get your request by email marked EMERGENCY, and the business you \
         pick has 60 minutes to confirm. Businesses that get emergency requests at night may be \
         able to come tonight. After-hours work usually costs more."
    ));
    let lower = day.to_lowercase();
    assert!(!lower.contains("call") && !lower.contains("reach you tonight"));
}

/// TD-INT-01 (INT-004, D-436): none of this task's texts gives a number, 211, a utility, a
/// shutoff, a fan, "drink water" or a symptom word, in either language.
#[test]
fn td_int_01_no_advice_in_any_emergency_template() {
    let reg = Registry::embedded().expect("registry");
    let banned = [
        "211",
        "utility",
        "shutoff",
        "shut off",
        "fan ",
        "drink water",
        "dizzy",
        "headache",
    ];
    let ids = [
        BEEP_TEMPLATE,
        CHOICE_TEMPLATE,
        NIGHT_MANY_TEMPLATE,
        NIGHT_ONE_TEMPLATE,
        NIGHT_NONE_TEMPLATE,
        SAFETY_TEMPLATE,
    ];
    for id in ids {
        for language in [Language::En, Language::Es] {
            let (raw, _) = reg.template(id, language).expect("template");
            let lower = raw.to_lowercase();
            for word in banned {
                assert!(!lower.contains(word), "{word} in {id}");
            }
        }
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn words(words: &str) -> Inbound<'_> {
    Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    }
}

fn text(contact: Uuid, minute: i64, inbound: Inbound<'_>) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x3900 + u128::from(minute.unsigned_abs())),
        contact,
        inbound,
        received_at: az("22:40") + SignedDuration::from_mins(minute),
    }
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

async fn send(db: &mut Db, contact: Uuid, minute: i64, said: &str) -> Handled {
    on_text(
        db,
        &secret(),
        &text(contact, minute, words(said)),
        Settings::default(),
    )
    .await
    .expect("handled")
}

/// TD-INT-01, TD-INT-16 (INT-002, INT-003, INT-005, INT-024, D-436): a match mid-conversation gets
/// the 911 text and holds the cards until SAFE; the request continues after SAFE; a new number's
/// "smell gas" gets the 911 text first, the consent text second.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_01_a_match_mid_conversation_holds_cards_until_safe(mut db: Db) {
    let first = Uuid::from_u128(0x391);
    let opened = send(&mut db, first, 0, "my sink is leaking").await;
    assert_eq!(templates(&opened), [CONSENT_TEMPLATE]);
    let id = opened.conversation.expect("request");
    assert!(!safe_hold(&mut db, id).await.expect("hold"));
    let smoke = send(&mut db, first, 2, "now there is smoke in the kitchen").await;
    assert_eq!(templates(&smoke), [SAFETY_TEMPLATE]);
    assert_eq!(smoke.screened, Screened::Matched(Hazard::SmokeFire));
    assert!(safe_hold(&mut db, id).await.expect("hold"));
    let still = send(&mut db, first, 3, "what do I do").await;
    assert_eq!((still.texts.len(), still.screened), (0, Screened::Clear));
    assert!(safe_hold(&mut db, id).await.expect("hold"));
    let safe = send(&mut db, first, 4, "ok").await;
    assert_eq!(safe.screened, Screened::Safe);
    assert!(
        safe.texts.is_empty(),
        "no gas-company or other text after SAFE"
    );
    assert!(!safe_hold(&mut db, id).await.expect("hold"));
    assert_eq!(
        safe.conversation,
        Some(id),
        "the request continues (INT-024)"
    );
    let events = emergency_events(&mut db, id).await.expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].matched, "smoke_fire");
    assert!(events[0].safe_at.is_some());
    let new = send(&mut db, Uuid::from_u128(0x392), 0, "smell gas").await;
    assert_eq!(templates(&new), [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
}

/// TD-INT-10 (INT-026, D-477): the caller's emergency flag (the AI reading) never sends the 911
/// text when no stem matches; a stem match sends it even when the flag is off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_10_the_ai_flag_never_stands_in_for_the_stem_match(mut db: Db) {
    let ai_says = Inbound {
        emergency: true,
        ..words("the breaker panel is buzzing and hot to the touch")
    };
    let contact = Uuid::from_u128(0x393);
    let handled = on_text(
        &mut db,
        &secret(),
        &text(contact, 0, ai_says),
        Settings::default(),
    )
    .await
    .expect("handled");
    assert_eq!(templates(&handled), [CONSENT_TEMPLATE]);
    assert_eq!(handled.screened, Screened::Clear);
    let gas = send(
        &mut db,
        Uuid::from_u128(0x394),
        0,
        "huele a gas en la cocina",
    )
    .await;
    assert_eq!(templates(&gas), [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
}

/// TD-INT-01 (INT-002, D-165): the name-and-address answer is never screened.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_01_the_address_answer_is_never_screened(mut db: Db) {
    let contact = Uuid::from_u128(0x395);
    send(&mut db, contact, 0, "my sink is leaking").await;
    let answer = text(contact, 5, words("Pat, smoke house on Main"));
    let handled = receive_answer(
        &mut db,
        &secret(),
        &answer,
        Settings::default(),
        Asked::NameAndAddress,
    )
    .await
    .expect("answer");
    assert_eq!(
        (handled.texts.len(), handled.screened),
        (0, Screened::Clear)
    );
}

/// TD-INT-01 (INT-037, D-361): "smoke detector beeping" gets the INT-037 question after the
/// consent text; answer 2 sends the 911 text and holds the cards; answer 1 continues as regular.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_01_a_beeping_alarm_gets_the_question(mut db: Db) {
    let alarm = Uuid::from_u128(0x396);
    let asked = send(&mut db, alarm, 0, "smoke detector beeping").await;
    assert_eq!(templates(&asked), [CONSENT_TEMPLATE, BEEP_TEMPLATE]);
    assert_eq!(asked.screened, Screened::BeepAsked);
    let id = asked.conversation.expect("request");
    assert!(!safe_hold(&mut db, id).await.expect("hold"));
    let two = send(&mut db, alarm, 1, "2").await;
    assert_eq!(templates(&two), [SAFETY_TEMPLATE]);
    assert!(safe_hold(&mut db, id).await.expect("hold"));
    let chirp = Uuid::from_u128(0x397);
    let asked = send(&mut db, chirp, 0, "smoke detector beeping").await;
    let id = asked.conversation.expect("request");
    let one = send(&mut db, chirp, 1, "1").await;
    assert!(one.texts.is_empty());
    assert!(!safe_hold(&mut db, id).await.expect("hold"));
    let later = send(&mut db, chirp, 2, "2").await;
    assert!(later.texts.is_empty(), "the question is answered once");
    let quiet = send(
        &mut db,
        Uuid::from_u128(0x398),
        0,
        "my smoke detector chirps",
    )
    .await;
    assert_eq!(templates(&quiet), [CONSENT_TEMPLATE]);
}

//! The one INT-004 911 text and its place first on every entry path (TD-INT-17, TD-CON-26:
//! INT-004, CON-001, CON-004, D-436).
//!
//! Text cases render the compiled-in templates and run the quiet-hours rule; database cases run
//! `on_text` on a fresh database with fake IDs and fake `555-01xx` numbers only. The stem matcher
//! is T039's (TD-INT-16): here the caller hands over the 911-list match as `emergency: true`.

use askhava_messaging::api::{
    AreaCodes, Decision, Encoding, Language, count_segments, decide, render,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{
    Handled, SAFETY_WITHIN, TextIn, conversation, on_text, safety_outgoing, safety_text,
    sent_in_time,
};
use crate::rules::{CONSENT_TEMPLATE, Inbound, SAFETY_TEMPLATE, Send, Settings, Then, plan};

const INT_004_EN: &str = "Ask Hava: Ask Hava isn't an emergency service. If you smell gas, see smoke or fire, a CO alarm is going off, or a power line is down, get everyone out and call 911 from outside. Reply SAFE when you're safe.";
const INT_004_ES: &str = "Ask Hava: Ask Hava no es un servicio de emergencia. Si huele a gas, ve humo o fuego, suena una alarma de CO o hay un cable de luz caído, saque a todos y llame al 911 desde afuera. Responda SEGURO cuando esté a salvo.";
/// Advice D-436 removed: no cause, symptom, health, heat, water, shutoff, utility or appliance line.
const NO_ADVICE: &str = "211|cooling|heat|water|shutoff|shut off|breaker|utility|unisource|epcor|\
    propane company|symptom|headache|dizzy|doctor|window|appliance|agua|calor|enfriamiento|llave";

fn emergency(words: &str) -> Inbound<'_> {
    Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: true,
        ai_spanish: None,
    }
}

/// TD-INT-17 (INT-004, D-436): the text equals its English and Spanish templates character for
/// character, the English is GSM-7 in 2 segments, its only number is 911 and it gives no advice.
#[test]
fn td_int_17_safety_text_equals_its_templates() {
    let en = safety_text(Language::En).expect("en");
    let es = safety_text(Language::Es).expect("es");
    assert_eq!((en.as_str(), es.as_str()), (INT_004_EN, INT_004_ES));
    let counted = count_segments(&en);
    assert_eq!((counted.encoding, counted.segments), (Encoding::Gsm7, 2));
    for text in [&en, &es] {
        let digits: Vec<&str> = text
            .split(|c: char| !c.is_ascii_digit())
            .filter(|d| !d.is_empty())
            .collect();
        assert_eq!(digits, ["911"], "{text}");
        let lower = text.to_lowercase();
        for advice in NO_ADVICE.split('|') {
            assert!(!lower.contains(advice), "{advice} in {text}");
        }
        assert!(!text.contains(['\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}']));
    }
}

/// TD-INT-17, TD-CON-26 (INT-004, TXT-039, D-492): the Spanish 911 text stays on while Spanish
/// conversations are off, and `safety_text` is the same words the template registry renders.
#[test]
fn td_int_17_spanish_safety_text_stays_on_while_spanish_is_off() {
    let off = render(SAFETY_TEMPLATE, Language::Es, false, &[]).expect("es off");
    assert_eq!(
        (off.language, off.body.as_str()),
        (Language::Es, INT_004_ES)
    );
    let on = render(SAFETY_TEMPLATE, Language::Es, true, &[]).expect("es on");
    assert_eq!(on.body, safety_text(Language::Es).expect("es"));
}

/// INT-004, QH-004: the 911 text is never held: it may go at once at any hour of the day and
/// night, in Arizona and in a stricter area-code zone (fake `555-01xx` numbers).
#[test]
fn int_004_safety_text_goes_at_any_hour() {
    let table = AreaCodes::embedded().expect("table");
    let start: Timestamp = "2026-07-15T00:00:00-07:00".parse().expect("midnight");
    for phone in ["+19285550142", "+12125550142"] {
        for minute in (0..24 * 60).step_by(15) {
            let now = start + SignedDuration::from_mins(minute);
            let decided = decide(&table, &safety_outgoing(phone), now).expect("decide");
            assert_eq!(decided, Decision::Send, "{phone} at {now}");
        }
    }
}

/// INT-004: "within 5 minutes" is 0 to 5:00 after the message; 5:01 or a time before it is not.
#[test]
fn int_004_sent_within_five_minutes() {
    let at: Timestamp = "2026-11-02T06:10:00Z".parse().expect("instant");
    assert_eq!(SAFETY_WITHIN, SignedDuration::from_mins(5));
    for (secs, ok) in [
        (0, true),
        (299, true),
        (300, true),
        (301, false),
        (-1, false),
    ] {
        let sent = at + SignedDuration::from_secs(secs);
        assert_eq!(sent_in_time(at, sent), ok, "{secs}s");
    }
}

/// TD-CON-26 (CON-001, CON-004, INT-004, D-008, D-419): a 911-list match gets the 911 text, then
/// the consent text, then the first question; with Spanish conversations off a Spanish match gets
/// the Spanish 911 text and the English consent text.
#[test]
fn td_con_26_safety_then_consent_then_question() {
    let gas = plan(false, &emergency("I smell gas"), Settings::default());
    let consent_en = Send::Consent {
        template: CONSENT_TEMPLATE,
        language: Language::En,
    };
    assert_eq!(gas.texts, [Send::Safety(Language::En), consent_en]);
    assert_eq!(gas.then, Then::FirstQuestion);
    let huele = plan(
        false,
        &emergency("huele a gas en la casa"),
        Settings::default(),
    );
    assert_eq!(huele.texts, [Send::Safety(Language::Es), consent_en]);
    assert_eq!(huele.then, Then::FirstQuestion);
    let consent = render(CONSENT_TEMPLATE, Language::En, false, &[]).expect("consent");
    assert!(consent.body.starts_with("Ask Hava: Got it."));
    assert_eq!(safety_text(Language::Es).expect("es"), INT_004_ES);
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn text_in(contact: Uuid, minute: i64, inbound: Inbound<'_>) -> TextIn<'_> {
    let start: Timestamp = "2026-11-03T05:30:00Z".parse().expect("instant");
    TextIn {
        inbox_id: Uuid::from_u128(0x5100 + u128::from(minute.unsigned_abs())),
        contact,
        inbound,
        received_at: start + SignedDuration::from_mins(minute),
    }
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

/// TD-CON-26 (CON-001, INT-004): on the text entry path, a new number's 911-list match at 22:30
/// Arizona queues the 911 text, then the consent text, and the first question follows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_26_text_path_queues_safety_then_consent(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0x5C0);
    let gas = text_in(contact, 0, emergency("smell gas by the water heater"));
    let handled = on_text(&mut db, &secret(), &gas, Settings::default())
        .await
        .expect("gas");
    assert_eq!(templates(&handled), [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
    assert_eq!(handled.then, Then::FirstQuestion);
}

/// TD-CON-26 (CON-001, INT-004, D-492): on the START-return path (the earlier request closed, the
/// START reply already sent by `messaging`), a Spanish 911-list match with Spanish conversations
/// off opens a new request with the 911 text, then the English consent text, then the question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_26_start_return_path_queues_safety_then_consent(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0x5C1);
    let mut first = emergency("my sink is leaking");
    first.emergency = false;
    let before = on_text(
        &mut db,
        &secret(),
        &text_in(contact, 0, first),
        Settings::default(),
    )
    .await
    .expect("first");
    let old = before.conversation.expect("request");
    sqlx::query!(
        "UPDATE intake.conversation SET closed_at = opened_at WHERE id = $1",
        old
    )
    .execute(&mut *db)
    .await
    .expect("close");
    let back = text_in(contact, 9, emergency("huele a gas en la cocina"));
    let handled = on_text(&mut db, &secret(), &back, Settings::default())
        .await
        .expect("back");
    assert_eq!(templates(&handled), [SAFETY_TEMPLATE, CONSENT_TEMPLATE]);
    assert_eq!(handled.then, Then::FirstQuestion);
    let id = handled.conversation.expect("new request");
    assert_ne!(id, old);
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert_eq!(row.language, "en");
}

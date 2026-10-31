//! The consent text and the first reply (TD-CON-01, TD-CON-11, TD-CON-14, TD-CON-18, TD-TXT-24:
//! CON-001 to CON-004, CON-026, CON-027, CON-031).
//!
//! Text cases render the compiled-in templates; database cases run `on_text` on a fresh database
//! with fake IDs only and read the queued outbox rows.

use askhava_messaging::api::{Encoding, Language, count_segments, render};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{Handled, TextIn, conversation, disclosure, on_text};
use crate::rules::{
    CONSENT_TEMPLATE, Inbound, Send, Settings, Then, consent_template, plan, wanted_language,
};

const CON_031_EN: &str = "Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: askhava.com/terms Privacy: askhava.com/privacy";
const CON_031_ES: &str = "Ask Hava: Recibido. Ahora, unas preguntas. Ask Hava no es contratista; usted contrata al negocio que elija. Textos automatizados sobre su solicitud; frecuencia variable. Pueden aplicar cargos de msj y datos. PARAR para salir, AYUDA para ayuda. Términos: askhava.com/terms Privacidad: askhava.com/privacy";
const CALL_022: &str = "If you report a danger like gas or smoke and we can't text, we may try one recorded call to this number. Don't wait for it: call 911.";
const CON_027: &str = "Ask Hava is a local service. Our texts are automated. The Ask Hava owner gets HELP messages right away and usually replies within 2 days, or within 24 hours if your job is urgent.";
const BANNED: &[&str] = &[
    "Reply OK",
    "Reply YES",
    "Responda OK",
    "small fee",
    "fee",
    "$",
    "Free for you",
];

fn consent(safety_call_on: bool, language: Language, spanish_on: bool) -> String {
    render(consent_template(safety_call_on), language, spanish_on, &[])
        .expect("consent")
        .body
}

fn msg(words: &str) -> Inbound<'_> {
    Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    }
}

/// TD-CON-18: the English text equals CON-031 character for character, GSM-7 in 2 segments, holds
/// every CON-002 line and both links, and no reply ask, fee line or "small fee" (CON-002, CON-003,
/// CON-004).
#[test]
fn td_con_18_english_consent_text_is_con_031_exactly() {
    let text = consent(false, Language::En, false);
    assert_eq!(text, CON_031_EN);
    assert_eq!(text.chars().count(), 295);
    let counted = count_segments(&text);
    assert_eq!((counted.encoding, counted.segments), (Encoding::Gsm7, 2));
    for line in [
        "Ask Hava is not a contractor; you hire the business you pick directly.",
        "Automated texts about your request",
        "Msg frequency varies",
        "Msg & data rates may apply",
        "Reply STOP to opt out, HELP for help",
        "Terms: askhava.com/terms",
        "Privacy: askhava.com/privacy",
    ] {
        assert!(text.contains(line), "missing {line}");
    }
    for banned in BANNED {
        assert!(!text.contains(banned), "holds {banned}");
    }
    assert!(!text.contains(['\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}']));
}

/// TD-CON-18, TD-TXT-24 (CON-026, CON-031): the Spanish text equals CON-031's Spanish, GSM-7 in 2
/// segments, and goes only while Spanish conversations are on.
#[test]
fn td_con_18_spanish_consent_text_only_while_spanish_is_on() {
    let es = consent(false, Language::Es, true);
    assert_eq!(es, CON_031_ES);
    assert_eq!(es.chars().count(), 303);
    let counted = count_segments(&es);
    assert_eq!((counted.encoding, counted.segments), (Encoding::Gsm7, 2));
    assert_eq!(consent(false, Language::Es, false), CON_031_EN);
}

/// TD-CON-18 (CALL-019, CALL-022, D-384): no safety-call sentence while the call is off; switched
/// on, the CALL-022 sentence is in the consent text, still with no reply ask or fee line.
#[test]
fn td_con_18_safety_call_sentence_only_while_the_call_is_on() {
    assert!(!consent(false, Language::En, false).contains("recorded call"));
    assert_eq!(consent_template(false), CONSENT_TEMPLATE);
    let on = consent(true, Language::En, false);
    assert!(on.contains(CALL_022));
    assert!(on.starts_with("Ask Hava: Got it. Next, a few quick questions."));
    assert!(on.ends_with("Terms: askhava.com/terms Privacy: askhava.com/privacy"));
    for banned in BANNED {
        assert!(!on.contains(banned), "holds {banned}");
    }
    let es = consent(true, Language::Es, true);
    assert!(es.contains("llamada grabada") && !es.contains("Responda"));
}

/// TD-TXT-24 (TXT-039, CON-026): "Hola, mi lavabo gotea" with Spanish off gets the English
/// consent text; with it on, the Spanish one; "I need help with my sink" reads as English.
#[test]
fn td_txt_24_spanish_first_message_gets_english_while_spanish_is_off() {
    let hola = msg("Hola, mi lavabo gotea");
    assert_eq!(wanted_language(hola.words, None), Language::Es);
    let off = plan(false, &hola, Settings::default());
    let on = plan(
        false,
        &hola,
        Settings {
            spanish_conversations_on: true,
            safety_call_on: false,
        },
    );
    let language = |p: &crate::rules::Plan| match p.texts.last() {
        Some(Send::Consent { language, .. }) => *language,
        other => panic!("no consent text: {other:?}"),
    };
    assert_eq!(language(&off), Language::En);
    assert_eq!(language(&on), Language::Es);
    assert_eq!(
        wanted_language("I need help with my sink", None),
        Language::En
    );
    assert_eq!(wanted_language("my sink", Some(true)), Language::Es);
}

/// TD-CON-11, TD-CON-14 (CON-027): after the consent text, "what?", "fine", "go ahead" and "de
/// acuerdo" are intake replies with no text; "is this a real person?" gets the disclosure line
/// then the next question; STOP and HELP are never answered with the consent text.
#[test]
fn td_con_11_replies_after_the_consent_text_are_conversation_steps() {
    for words in ["what?", "fine", "go ahead", "de acuerdo", "OK", "yes"] {
        let p = plan(true, &msg(words), Settings::default());
        assert_eq!((p.texts.len(), p.then), (0, Then::IntakeReply), "{words}");
    }
    for words in ["is this a real person?", "Who is this", "¿Quién es?"] {
        let p = plan(true, &msg(words), Settings::default());
        assert_eq!(
            (p.texts.len(), p.then),
            (0, Then::DiscloseThenAsk),
            "{words}"
        );
    }
    for consent_sent in [false, true] {
        let stop = Inbound {
            opt_out: true,
            ..msg("STOP")
        };
        let help = Inbound {
            help: true,
            ..msg("HELP")
        };
        for m in [stop, help] {
            assert_eq!(
                plan(consent_sent, &m, Settings::default()).then,
                Then::Nothing
            );
        }
    }
}

/// TD-CON-14 (CON-027): the disclosure line is the CON-027 sentence, in one text with no reply ask.
#[test]
fn td_con_14_disclosure_line_is_con_027() {
    let line = disclosure(Language::En, false).expect("disclosure");
    assert_eq!(line, format!("Ask Hava: {CON_027}"));
    assert!(!line.contains("Reply"));
    let es = disclosure(Language::Es, true).expect("disclosure es");
    assert!(es.starts_with("Ask Hava: Ask Hava es un servicio local."));
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + jiff::SignedDuration::from_mins(minute)
}

fn text_in(contact: Uuid, minute: i64, inbound: Inbound<'_>) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x100 + minute as u128),
        contact,
        inbound,
        received_at: at(minute),
    }
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

/// TD-CON-01 (CON-001, CON-027): a new number's request gets the consent text, once; a second
/// message and the same message again queue nothing more; the request holds its language and
/// consent template.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_01_first_message_gets_the_consent_text_once(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0xC0);
    let first = text_in(contact, 0, msg("I need help with my sink"));
    let handled = on_text(&mut db, &secret(), &first, Settings::default())
        .await
        .expect("first");
    assert_eq!(templates(&handled), ["consent"]);
    assert_eq!(handled.then, Then::FirstQuestion);
    let again = on_text(&mut db, &secret(), &first, Settings::default())
        .await
        .expect("again");
    assert_eq!((again.texts.len(), again.then), (0, Then::IntakeReply));
    let later = text_in(contact, 3, msg("what?"));
    let later = on_text(&mut db, &secret(), &later, Settings::default())
        .await
        .expect("later");
    assert_eq!(
        (later.texts.len(), later.conversation),
        (0, handled.conversation)
    );
    let id = handled.conversation.expect("request");
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert_eq!(
        (row.language.as_str(), row.consent_template.as_str()),
        ("en", "consent")
    );
}

/// TD-CON-01 (CON-001, D-131): HELP as a new number's first message gets no consent text and
/// opens nothing; the request that follows gets it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_01_help_first_then_the_consent_text_with_the_request(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0xC1);
    let help = text_in(
        contact,
        0,
        Inbound {
            help: true,
            ..msg("HELP")
        },
    );
    let handled = on_text(&mut db, &secret(), &help, Settings::default())
        .await
        .expect("help");
    assert_eq!((handled.conversation, handled.then), (None, Then::Nothing));
    let request = text_in(contact, 2, msg("my sink is leaking"));
    let request = on_text(&mut db, &secret(), &request, Settings::default())
        .await
        .expect("request");
    assert_eq!(templates(&request), ["consent"]);
}

/// TD-CON-01 (CON-001, D-008): on a 911-list match the 911 text and then the consent text are
/// queued, and `texts` gives them in that order, the order the caller sends them in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_01_911_text_first_then_the_consent_text(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0xC2);
    let gas = text_in(
        contact,
        0,
        Inbound {
            emergency: true,
            ..msg("I smell gas in the kitchen")
        },
    );
    let handled = on_text(&mut db, &secret(), &gas, Settings::default())
        .await
        .expect("gas");
    assert_eq!(templates(&handled), ["emergency_911", "consent"]);
}

/// CON-022 (postgres brain P5): the app role may add a consent record but never change one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn consent_records_are_append_only_for_the_app_role(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    let contact = Uuid::from_u128(0xC3);
    let first = text_in(contact, 0, msg("my sink is leaking"));
    let handled = on_text(&mut db, &secret(), &first, Settings::default())
        .await
        .expect("first");
    let id = handled.conversation.expect("request");
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    sqlx::query!(
        "INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, inbox_id, \
         consent_version, consented_at) SELECT homeowner_id, id, 'sms', first_inbox_id, 'consent:en', \
         opened_at FROM intake.conversation WHERE id = $1",
        id
    )
    .execute(&mut *db)
    .await
    .expect("insert");
    let update = sqlx::query!("UPDATE intake.consent_record SET channel = 'web_chat'")
        .execute(&mut *db)
        .await;
    assert!(update.is_err());
}

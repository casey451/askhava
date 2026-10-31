//! The opt-out detector and word lists (TD-CON-04, TD-CON-20 word-list case; CON-008, CON-010,
//! CON-011, CON-013, CON-030, D-493).
//!
//! Every sole opt-out word in any case and spacing opts out; the listed revocation phrases do, in
//! English and Spanish and with any punctuation; a word used in another sense does not. The app's
//! own opt-out and re-subscribe lists equal the provider's Advanced Opt-Out settings in
//! `stand_ins/provider_optout.json` (T167's), which has no YES; and the confirmation template is
//! the bare CON-009 text in both languages.

use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use sqlx::types::Uuid;

use crate::api::{
    APP_OPT_OUT_WORDS, CONFIRMATION_TEMPLATE, Classified, Detected, HELP_TEMPLATE, Language,
    Listing, OPT_OUT_PHRASES, PROVIDER_OPT_OUT_WORDS, ProviderKeyword, RESUBSCRIBE_WORDS,
    SPANISH_OPT_OUT_WORDS, classify, detect_opt_out, may_send_to, render,
};

const PROVIDER: &str = include_str!("../../../../stand_ins/provider_optout.json");
const CON_009_EN: &str = "Ask Hava: You're unsubscribed. You won't get more texts or automated calls from Ask Hava at this number. To use Ask Hava again, text START.";
const CON_009_ES: &str = "Ask Hava: Se canceló su suscripción. No recibirá más textos ni llamadas automáticas de Ask Hava en este número. Para volver a usar Ask Hava, envíe START.";

fn list(value: &Value) -> Vec<String> {
    let words = value.as_array().expect("a list of words");
    words
        .iter()
        .map(|w| w.as_str().expect("a word").to_lowercase())
        .collect()
}

/// TD-CON-04, CON-008: each sole word, in any case, with surrounding spaces, opts out.
#[test]
fn td_con_04_each_sole_word_in_any_case_opts_out() {
    let words = [
        "stop",
        "stopall",
        "quit",
        "end",
        "revoke",
        "opt out",
        "optout",
        "cancel",
        "unsubscribe",
        "parar",
        "alto",
        "cancelar",
        "baja",
    ];
    for word in words {
        for form in [
            word.to_owned(),
            word.to_uppercase(),
            format!("  {}  ", capitalize(word)),
            format!("{word}."),
        ] {
            assert!(
                matches!(detect_opt_out(&form), Some(Detected::Word(_))),
                "{form:?} opts out"
            );
        }
    }
    assert_eq!(detect_opt_out("Opt-Out"), Some(Detected::Word("opt out")));
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// TD-CON-04, CON-010: "please stop texting me" and its Spanish form opt out, as does every
/// listed phrase in any case and with any punctuation; Spanish phrases are marked Spanish.
#[test]
fn td_con_04_revocation_phrases_opt_out() {
    let english = detect_opt_out("Please STOP texting me!!");
    assert_eq!(english, Some(Detected::Phrase("en.stop_texting")));
    let spanish = detect_opt_out("Por favor, dejen de escribirme.");
    assert_eq!(spanish, Some(Detected::Phrase("es.dejen_de_escribirme")));
    assert!(spanish.is_some_and(Detected::spanish));
    assert_eq!(
        detect_opt_out("QUITENME de la lista"),
        Some(Detected::Phrase("es.quitenme_de_la_lista"))
    );
    assert_eq!(
        detect_opt_out("dont text me anymore"),
        Some(Detected::Phrase("en.dont_text_me"))
    );
    for (id, phrase) in OPT_OUT_PHRASES {
        let shouted = format!("ok, {}!", phrase.to_uppercase());
        assert_eq!(detect_opt_out(&shouted), Some(Detected::Phrase(id)), "{id}");
    }
}

/// TD-CON-04, CON-011: a word in another sense, or a normal request, is not an opt-out.
#[test]
fn td_con_04_a_word_in_another_sense_is_not_an_opt_out() {
    for text in [
        "can stop by at 3",
        "stop sign is down",
        "please cancel my request",
        "the leak won't stop",
        "yes",
        "",
    ] {
        assert_eq!(detect_opt_out(text), None, "{text:?}");
        assert_eq!(classify(text, ProviderKeyword::None), Classified::Other);
    }
}

/// TD-CON-20, CON-030, D-493: the app's opt-out and re-subscribe lists equal the provider's file,
/// which has exactly the twelve CON-030 words and START and UNSTOP only, no YES.
#[test]
fn td_con_20_word_lists_equal_the_provider_settings() {
    let file: Value = serde_json::from_str(PROVIDER).expect("provider settings");
    assert_eq!(list(&file["opt_out"]["keywords"]), PROVIDER_OPT_OUT_WORDS);
    assert_eq!(
        list(&file["opt_out"]["spanish_keywords"]),
        SPANISH_OPT_OUT_WORDS
    );
    assert_eq!(list(&file["opt_in"]["keywords"]), RESUBSCRIBE_WORDS);
    assert!(!list(&file["opt_in"]["keywords"]).contains(&"yes".to_owned()));
    assert_eq!(file["opt_out"]["reply"]["en"], CON_009_EN);
    assert_eq!(file["opt_out"]["reply"]["es"], CON_009_ES);
    for word in APP_OPT_OUT_WORDS {
        assert!(
            !PROVIDER_OPT_OUT_WORDS.contains(&word),
            "the app answers {word}"
        );
    }
}

/// TD-CON-20, CON-009: the app's confirmation is the bare CON-009 text in both languages.
#[test]
fn td_con_20_the_confirmation_is_the_bare_con_009_text() {
    let en = render(CONFIRMATION_TEMPLATE, Language::En, false, &[]).expect("en");
    assert_eq!(en.body, CON_009_EN);
    let es = render(CONFIRMATION_TEMPLATE, Language::Es, false, &[]).expect("es");
    assert_eq!(es.body, CON_009_ES, "the Spanish one stays on");
}

/// TD-CON-20, CON-015, CON-030: only the provider's START or UNSTOP match (`OptOutType=START`)
/// re-subscribes, so the app lifts a listing exactly when the provider lifts its block; a START
/// the provider did not match ("Start.", or no `OptOutType`) and "yes" do not (D-870).
#[test]
fn td_con_20_only_start_or_unstop_resubscribes() {
    for text in ["START", " unstop "] {
        assert_eq!(
            classify(text, ProviderKeyword::from_opt_out_type(Some("START"))),
            Classified::Resubscribe
        );
    }
    for text in ["Start.", "START", "unstop"] {
        assert_eq!(classify(text, ProviderKeyword::None), Classified::Other);
    }
    assert_eq!(classify("yes", ProviderKeyword::None), Classified::Other);
    assert_eq!(
        classify("YES", ProviderKeyword::Start),
        Classified::Resubscribe
    );
    let stop = classify("STOP", ProviderKeyword::from_opt_out_type(Some("STOP")));
    assert_eq!(
        stop,
        Classified::OptOut {
            detected: Some(Detected::Word("stop")),
            by_provider: true
        }
    );
    assert_eq!(
        ProviderKeyword::from_opt_out_type(Some("HELP")),
        ProviderKeyword::None
    );
}

/// TD-CON-05, CON-009, CON-013: a listed number gets only the confirmation of its current listing,
/// within 5 minutes, and the HELP reply.
#[test]
fn td_con_05_a_listed_number_gets_only_the_confirmation_and_help() {
    let since = Timestamp::from_second(1_793_000_000).expect("time");
    let at = |secs| since + SignedDuration::from_secs(secs);
    let event = Uuid::from_u128(0xe1);
    let listed = Some(Listing { since, event });
    let this = [event];
    assert!(may_send_to(None, "first_quote", &[], since));
    assert!(!may_send_to(listed, "first_quote", &this, at(1)));
    assert!(!may_send_to(listed, "consent", &this, at(1)));
    assert!(may_send_to(listed, CONFIRMATION_TEMPLATE, &this, at(300)));
    assert!(!may_send_to(listed, CONFIRMATION_TEMPLATE, &this, at(301)));
    assert!(!may_send_to(listed, CONFIRMATION_TEMPLATE, &this, at(-1)));
    let earlier = [Uuid::from_u128(0xe0)];
    assert!(
        !may_send_to(listed, CONFIRMATION_TEMPLATE, &earlier, at(1)),
        "an earlier listing's confirmation never goes"
    );
    assert!(may_send_to(listed, HELP_TEMPLATE, &[], at(86_400)));
}

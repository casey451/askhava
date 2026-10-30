//! The template registry (TD-TXT-01; TXT-001 to TXT-004, TXT-039, TXT-044, BIZ-018, WORD-008).
//! Fake data only.

use crate::api::{
    Audience, Encoding, Language, Registry, TemplateError, count_segments, render, template_ids,
};

const VALUES: &[(&str, &str)] = &[
    ("job", "1042"),
    ("count", "3"),
    ("trade", "plumbing"),
    ("link", "askhava.com/q/x"),
    ("quoted", "2"),
    ("business", "Test Plumbing"),
    ("number", "the masked number"),
    ("time", "5 p.m."),
    ("reply_time", "2 days"),
];

fn registry() -> &'static Registry {
    Registry::embedded().expect("the compiled-in templates are valid")
}

/// TD-TXT-01, TXT-001, TXT-003, WORD-008: every kind has both languages, Spanish is a draft, every
/// text begins "Ask Hava:" and renders with the sample values.
#[test]
fn every_kind_has_both_languages_and_begins_ask_hava() {
    let ids = template_ids().expect("ids");
    assert!(ids.iter().any(|id| id == "consent"));
    for id in &ids {
        for wanted in [Language::En, Language::Es] {
            let text = render(id, wanted, true, VALUES).expect("renders");
            assert!(text.body.starts_with("Ask Hava:"), "{id}");
            assert!(!text.body.contains('{'), "{id}");
            assert_eq!(text.draft, wanted == Language::Es, "{id}");
        }
    }
}

/// A planted registry of generic kinds, so the behaviour tests do not hang on later tasks' keys.
fn planted() -> Registry {
    let en = r#"{
      "plain": {"audience":"homeowner","segments":1,"text":"Ask Hava: Job {job} plain."},
      "stays": {"audience":"homeowner","segments":1,"text":"Ask Hava: Job {job} stays."},
      "notice": {"audience":"business","segments":1,"text":"Ask Hava: Job {job} notice."}
    }"#;
    let es = r#"{
      "plain": {"audience":"homeowner","segments":1,"draft":true,"text":"Ask Hava: Trabajo {job} normal."},
      "stays": {"audience":"homeowner","segments":1,"draft":true,"spanish_stays_on":true,"text":"Ask Hava: Trabajo {job} sigue."},
      "notice": {"audience":"business","segments":1,"draft":true,"text":"Ask Hava: Trabajo {job} aviso."}
    }"#;
    Registry::parse(en, es).expect("planted registry is valid")
}

const JOB: &[(&str, &str)] = &[("job", "1042")];

/// TD-TXT-01, TXT-039, TXT-002: Spanish conversations off gives English, except the stay-on texts.
#[test]
fn spanish_only_while_the_setting_is_on_except_the_stay_on_texts() {
    let reg = planted();
    let lang = |id: &str, on: bool| {
        reg.render(id, Language::Es, on, JOB)
            .expect("renders")
            .language
    };
    assert_eq!(lang("plain", false), Language::En);
    assert_eq!(lang("plain", true), Language::Es);
    assert_eq!(lang("stays", false), Language::Es);
    let english = reg
        .render("plain", Language::En, true, JOB)
        .expect("renders");
    assert_eq!(english.language, Language::En);
}

/// TXT-041, TXT-039: the HELP, 911, opt-out and "English only" texts stay Spanish with the setting off.
#[test]
fn the_required_stay_on_texts_are_flagged() {
    for stays in [
        "emergency_911",
        "help_reply",
        "opt_out_confirmation",
        "spanish_not_yet",
    ] {
        let text = render(stays, Language::Es, false, VALUES).expect("renders");
        assert_eq!(text.language, Language::Es, "{stays}");
    }
}

/// TD-TXT-01, BIZ-018: language is per person; the owner phone English and a team phone Spanish.
#[test]
fn business_language_is_per_phone_and_english_while_spanish_is_off() {
    let reg = planted();
    assert_eq!(reg.audience("notice"), Some(Audience::Business));
    let owner = reg
        .render("notice", Language::En, true, JOB)
        .expect("owner");
    let team = reg.render("notice", Language::Es, true, JOB).expect("team");
    assert_eq!(
        (owner.language, team.language),
        (Language::En, Language::Es)
    );
    let off = reg.render("notice", Language::Es, false, JOB).expect("off");
    assert_eq!(off.language, Language::En);
}

/// TXT-004: a missing value or unknown ID is an error, and a value holding braces is not expanded.
#[test]
fn rendering_is_fixed_and_strict() {
    let reg = planted();
    assert!(matches!(
        reg.render("nope", Language::En, true, JOB),
        Err(TemplateError::Unknown(_))
    ));
    assert!(matches!(
        reg.render("plain", Language::En, true, &[]),
        Err(TemplateError::MissingValue { .. })
    ));
    let text = reg
        .render("plain", Language::En, true, &[("job", "{job}")])
        .expect("renders");
    assert_eq!(text.body, "Ask Hava: Job {job} plain.");
}

fn pair(id: &str, es_extra: &str, es_draft: &str) -> (String, String) {
    let en =
        format!(r#"{{"{id}": {{"audience":"homeowner","segments":1,"text":"Ask Hava: Hi."}}}}"#);
    let es = format!(
        r#"{{"{id}": {{"audience":"homeowner","segments":1,"draft":{es_draft},"text":"{es_extra}"}}}}"#
    );
    (en, es)
}

/// TXT-001, TXT-003, WORD-008: a missing Spanish kind, a non-draft Spanish text or a text that does not
/// begin "Ask Hava:" is refused at load.
#[test]
fn a_registry_that_breaks_a_rule_is_refused() {
    let (en, es) = pair("a", "Ask Hava: Hola.", "true");
    assert!(Registry::parse(&en, &es).is_ok());
    assert!(Registry::parse(&en, "{}").is_err());
    let (_, not_draft) = pair("a", "Ask Hava: Hola.", "false");
    assert!(Registry::parse(&en, &not_draft).is_err());
    let (_, no_prefix) = pair("a", "Hola.", "true");
    assert!(Registry::parse(&en, &no_prefix).is_err());
}

/// TD-TXT-01 (TXT-044): the recorded `segments` is not below the counted one, the English consent text
/// is GSM-7 in at most 2 segments and so is the Spanish one.
#[test]
fn recorded_segments_hold_and_the_consent_texts_are_gsm7() {
    let reg = registry();
    for id in reg.ids() {
        for language in [Language::En, Language::Es] {
            let (raw, recorded) = reg.template(&id, language).expect("template");
            let counted = count_segments(raw);
            println!(
                "{id} {language:?} {:?} {} (recorded {recorded})",
                counted.encoding, counted.segments
            );
            assert!(
                counted.segments <= recorded,
                "{id} {language:?} grew past its recorded count"
            );
        }
    }
    for language in [Language::En, Language::Es] {
        let (raw, _) = reg.template("consent", language).expect("consent");
        let counted = count_segments(raw);
        assert_eq!((counted.encoding, counted.segments), (Encoding::Gsm7, 2));
    }
}

/// TXT-044: the counter knows the extension table and the Unicode limits.
#[test]
fn the_segment_counter_follows_gsm_rules() {
    assert_eq!(count_segments(&"a".repeat(160)).segments, 1);
    assert_eq!(count_segments(&"a".repeat(161)).segments, 2);
    assert_eq!(count_segments(&"[".repeat(81)).segments, 2);
    let ucs = count_segments(&format!("í{}", "a".repeat(69)));
    assert_eq!((ucs.encoding, ucs.segments), (Encoding::Ucs2, 1));
    assert_eq!(count_segments(&format!("í{}", "a".repeat(70))).segments, 2);
}

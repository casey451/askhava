//! The no-reply clock, the LIST words, the call-back deadline and the AI-off path, pure cases
//! (TD-INT-08, TD-INT-14, TD-PICK-33, TD-AI-12: INT-019, INT-021, INT-023, TXT-031). The database
//! cases are `tests/integration/tests/intake_timeouts.rs`.

use askhava_messaging::api::{Language, render, template_ids};
use jiff::{SignedDuration, Timestamp};

use crate::api::{
    CALL_BACK_DAYS, CLOSE_AFTER, CLOSED_TEMPLATE, ListAsk, REMIND_AFTER, REMINDER_TEMPLATE,
    call_back_due, counts_urgent, list_ask, texting_window,
};

fn az(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

/// TD-INT-08 (INT-019): the reminder is 24 hours after the last reply and the close 72; a
/// reminder that falls in quiet hours waits for 8 a.m. Arizona, one inside them goes at once.
#[test]
fn td_int_08_reminder_at_24_hours_held_to_8_am() {
    assert_eq!(REMIND_AFTER, SignedDuration::from_hours(24));
    assert_eq!(CLOSE_AFTER, SignedDuration::from_hours(72));
    assert_eq!(texting_window(az("03T08:00:00")), az("03T08:00:00"));
    assert_eq!(texting_window(az("03T19:59:59")), az("03T19:59:59"));
    assert_eq!(texting_window(az("03T20:00:00")), az("04T08:00:00"));
    assert_eq!(texting_window(az("03T23:30:00")), az("04T08:00:00"));
    assert_eq!(texting_window(az("03T07:59:59")), az("03T08:00:00"));
    assert_eq!(texting_window(az("03T00:00:00")), az("03T08:00:00"));
}

/// TD-PICK-33, TD-INT-08 (INT-019, D-406): the closing text equals INT-019's words, with a
/// Spanish template; the reminder is one segment in English and has a Spanish template; neither
/// holds a pressure word.
#[test]
fn td_pick_33_int_019_closing_text_equals_the_spec() {
    let job = [("job", "1234")];
    let en = render(CLOSED_TEMPLATE, Language::En, false, &job).expect("en");
    assert_eq!(
        en.body,
        "Ask Hava: We closed request 1234 because we didn't hear back. No problem. Text us any time and we'll start again."
    );
    for id in [CLOSED_TEMPLATE, REMINDER_TEMPLATE] {
        let es = render(id, Language::Es, true, &job).expect("es");
        assert!(es.body.starts_with("Ask Hava: ") && es.body.contains("1234"));
        let en = render(id, Language::En, false, &job).expect("en").body;
        for word in ["hurry", "last chance", "act now", "expire", "urgent"] {
            assert!(!en.to_lowercase().contains(word), "{id}: {word}");
        }
    }
}

/// The sample placeholder values every template renders with (the T026 registry test's list).
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

/// TD-INT-08, TD-AI-12 (INT-021, AI-008, D-492): no step holds the homeowner with a "we have your
/// message" text: no template, placeholders filled, in either language, is one.
#[test]
fn td_int_08_no_template_is_a_hold_text() {
    for id in template_ids().expect("ids") {
        for language in [Language::En, Language::Es] {
            let text = render(&id, language, true, VALUES).expect("renders");
            let body = text.body.to_lowercase();
            for hold in [
                "we have your message",
                "got your message",
                "tenemos su mensaje",
            ] {
                assert!(!body.contains(hold), "{id}: {hold}");
            }
        }
    }
}

/// TD-INT-08, TD-INT-14 (INT-023, D-226, D-277): a call-back is due within 24 hours for Today, an
/// emergency job, or an emergency keyword before "How soon?" is answered; else within the setting.
#[test]
fn td_int_08_call_back_due_by_urgency() {
    assert!(counts_urgent(Some("today"), false, false));
    assert!(counts_urgent(None, false, true));
    assert!(counts_urgent(None, true, false));
    assert!(!counts_urgent(Some("few_days"), true, false));
    assert!(!counts_urgent(None, false, false));
    let at = az("03T21:00:00");
    assert_eq!(call_back_due(at, true, CALL_BACK_DAYS), az("04T21:00:00"));
    assert_eq!(call_back_due(at, false, CALL_BACK_DAYS), az("07T21:00:00"));
    assert_eq!(call_back_due(at, false, 2), az("05T21:00:00"));
}

/// TD-INT-08 (TXT-031, D-491): LIST and LISTA in any case, alone or with one request number, are
/// LIST texts; anything longer is a normal message.
#[test]
fn td_int_08_list_and_lista_words() {
    let plain = ListAsk {
        spanish: false,
        job: None,
    };
    assert_eq!(list_ask("LIST"), Some(plain));
    assert_eq!(list_ask(" list "), Some(plain));
    assert_eq!(
        list_ask("Lista 1234"),
        Some(ListAsk {
            spanish: true,
            job: Some(1234)
        })
    );
    for words in [
        "list my sink",
        "LIST abc",
        "LIST 12 34",
        "LIST -5",
        "LIST +5",
        "LISTA 1.5",
        "listen",
        "",
        "my list",
    ] {
        assert_eq!(list_ask(words), None, "{words}");
    }
}

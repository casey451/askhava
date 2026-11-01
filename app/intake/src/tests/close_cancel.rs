//! Closing at the homeowner's word, pure cases (TD-JOB-05: JOB-018 to JOB-023). The database
//! cases, the close button and "Close which?", are `tests/integration/tests/close_which.rs`.

use askhava_messaging::api::{Language, render};

use crate::api::rules::{CloseReading, CloseTarget, close_reading};
use crate::api::{
    CLOSE_MENTION_TEMPLATE, CLOSE_WHICH_TEMPLATE, JOB_CLOSED_TEMPLATE, JOBS_CLOSED_TEMPLATE,
};

/// TD-JOB-05 (JOB-019): "CLOSE 1042", CERRAR, CLOSE ALL and its Spanish name what to close.
#[test]
fn td_job_05_close_and_its_number() {
    let number = CloseReading::Close(CloseTarget::Number(1042));
    for words in ["CLOSE 1042", "close #1042", "Cerrar 1042", " CLOSE  1042 "] {
        assert_eq!(close_reading(words, false), number, "{words}");
    }
    let all = CloseReading::Close(CloseTarget::All);
    for words in ["CLOSE ALL", "cerrar todo", "CERRAR TODAS"] {
        assert_eq!(close_reading(words, false), all, "{words}");
    }
    assert_eq!(close_reading("CLOSE 10 42", false), CloseReading::Other);
}

/// TD-JOB-05 (JOB-019, D-1200): longer cancel-intent messages close, in English and Spanish.
#[test]
fn td_job_05_cancel_intent_in_english_and_spanish() {
    for words in [
        "please cancel my request",
        "Never mind",
        "nevermind, we fixed it",
        "we no longer need this",
        "I don't need it anymore",
        "Ya no",
        "ya no lo necesito, gracias",
        "por favor cancele mi solicitud",
        "olvídalo",
        "CLOSE",
    ] {
        assert_eq!(close_reading(words, false), CloseReading::Intent, "{words}");
    }
}

/// TD-JOB-05 (JOB-020, JOB-023): "cancel" alone never closes, and a message that only mentions
/// cancelling gets the how-to-close line; "ya no" inside a request is a request.
#[test]
fn td_job_05_cancel_alone_or_mentioned_never_closes() {
    for words in [
        "cancel",
        "CANCEL",
        "cancelar",
        "I had to cancel the plumber",
        "the electrician cancelled on me",
        "cancel 1042",
    ] {
        assert_eq!(
            close_reading(words, false),
            CloseReading::Mention,
            "{words}"
        );
    }
    for words in [
        "el aire ya no funciona",
        "my sink is leaking",
        "close the gate",
        "1042",
    ] {
        assert_eq!(close_reading(words, false), CloseReading::Other, "{words}");
    }
}

/// TD-JOB-05 (JOB-021): while "Close which?" waits, a lone number or ALL answers it; otherwise
/// they close nothing.
#[test]
fn td_job_05_close_which_answers() {
    let number = CloseReading::Close(CloseTarget::Number(1043));
    assert_eq!(close_reading("1043", true), number);
    assert_eq!(close_reading("1043", false), CloseReading::Other);
    let all = CloseReading::Close(CloseTarget::All);
    for words in ["all", "ALL", "todas", "both"] {
        assert_eq!(close_reading(words, true), all, "{words}");
        assert_eq!(close_reading(words, false), CloseReading::Other, "{words}");
    }
    assert_eq!(close_reading("ok", true), CloseReading::Other);
}

/// TD-JOB-05 (JOB-021 to JOB-023): the JOB-023 line equals the spec; "Close which?" lists the
/// numbers with an ALL option; every text has a Spanish template.
#[test]
fn td_job_05_texts_equal_the_spec() {
    let mention = render(
        CLOSE_MENTION_TEMPLATE,
        Language::En,
        false,
        &[("job", "1042")],
    );
    assert_eq!(
        mention.expect("en").body,
        "Ask Hava: To close request 1042, reply CLOSE 1042."
    );
    let list = [("job", "1042 and 1043")];
    let which = render(CLOSE_WHICH_TEMPLATE, Language::En, false, &list).expect("en");
    assert!(which.body.starts_with("Ask Hava: Close which?"));
    assert!(which.body.contains("1042 and 1043") && which.body.contains("CLOSE ALL"));
    for id in [
        CLOSE_MENTION_TEMPLATE,
        CLOSE_WHICH_TEMPLATE,
        JOB_CLOSED_TEMPLATE,
        JOBS_CLOSED_TEMPLATE,
    ] {
        let es = render(id, Language::Es, true, &list).expect("es");
        assert_eq!(es.language, Language::Es, "{id}");
        assert!(
            es.body.contains("CERRAR") || es.body.contains("cerrada"),
            "{id}"
        );
    }
}

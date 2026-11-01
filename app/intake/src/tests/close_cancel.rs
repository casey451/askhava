//! Closing at the homeowner's word, pure cases (TD-JOB-05: JOB-018 to JOB-023). The database
//! cases, the close button and "Close which?", are `tests/integration/tests/close_which.rs`.

use askhava_messaging::api::{Language, render};

use crate::api::rules::{CloseReading, CloseTarget, close_reading, number_named};
use crate::api::{
    CLOSE_MENTION_TEMPLATE, CLOSE_WHICH_TEMPLATE, JOB_CLOSED_TEMPLATE, JOBS_CLOSED_TEMPLATE,
};

/// TD-JOB-05 (JOB-019): "CLOSE 1042", CERRAR, CLOSE ALL and its Spanish name what to close.
#[test]
fn td_job_05_close_and_its_number() {
    let number = CloseReading::Close(CloseTarget::Number(1042));
    for words in ["CLOSE 1042", "close #1042", "Cerrar 1042", " CLOSE  1042 "] {
        assert_eq!(close_reading(words, &[]), number, "{words}");
    }
    let all = CloseReading::Close(CloseTarget::All);
    for words in ["CLOSE ALL", "cerrar todo", "CERRAR TODAS"] {
        assert_eq!(close_reading(words, &[]), all, "{words}");
    }
    assert_eq!(close_reading("CLOSE 10 42", &[]), CloseReading::Other);
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
        assert_eq!(close_reading(words, &[]), CloseReading::Intent, "{words}");
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
        assert_eq!(close_reading(words, &[]), CloseReading::Mention, "{words}");
    }
    for words in [
        "el aire ya no funciona",
        "my sink is leaking",
        "close the gate",
        "1042",
    ] {
        assert_eq!(close_reading(words, &[]), CloseReading::Other, "{words}");
    }
}

/// TD-JOB-05 (JOB-021, D-1203): while "Close which?" waits, a lone open number or ALL answers it;
/// otherwise they close nothing.
#[test]
fn td_job_05_close_which_answers() {
    let asked = [1042, 1043];
    let number = CloseReading::Close(CloseTarget::Number(1043));
    assert_eq!(close_reading("1043", &asked), number);
    assert_eq!(close_reading("1043", &[]), CloseReading::Other);
    let all = CloseReading::Close(CloseTarget::All);
    for words in ["all", "ALL", "todas", "both"] {
        assert_eq!(close_reading(words, &asked), all, "{words}");
        assert_eq!(close_reading(words, &[]), CloseReading::Other, "{words}");
    }
    assert_eq!(close_reading("ok", &asked), CloseReading::Other);
}

/// TD-JOB-05 (JOB-021, D-1203): a waiting "Close which?" never takes a lone number that is not one
/// of the open requests (a ZIP code, a count, a day): it is left to the normal intake.
#[test]
fn td_job_05_close_which_ignores_numbers_not_open() {
    for words in ["86403", "2", "1044"] {
        assert_eq!(
            close_reading(words, &[1042, 1043]),
            CloseReading::Other,
            "{words}"
        );
    }
}

/// TD-JOB-05 (JOB-019, JOB-023, D-1203): someone else cancelling, an ambiguous phrase inside a
/// longer sentence, or "I don't need it today" is not cancel intent.
#[test]
fn td_job_05_mentions_and_need_sentences_are_not_intent() {
    for words in [
        "The last plumber had to cancel the job, can you find someone?",
        "my brother said he would cancel it if the part is late",
        "they had to cancel that appointment, the AC is still out",
        "the electrician wants to cancel this week, can someone else come",
    ] {
        assert_eq!(close_reading(words, &[]), CloseReading::Mention, "{words}");
    }
    for words in [
        "ya no quiero esperar, necesito alguien hoy",
        "no lo necesito hoy, puede ser mañana",
        "I don't need it today, next week is fine",
        "I don't need it now, maybe next month",
        "never mind the heater, the AC is out",
        "the old faucet, forget it fixed, the new one leaks",
        "we dont need it fixed until Monday",
    ] {
        assert_eq!(close_reading(words, &[]), CloseReading::Other, "{words}");
    }
}

/// TD-JOB-05 (JOB-019, D-1203): filler around an intent clause still reads as intent.
#[test]
fn td_job_05_intent_with_filler() {
    for words in [
        "ok never mind, my husband fixed it",
        "Actually forget it. Thanks!",
        "can you cancel my request please",
        "please cancel",
        "cancelar por favor",
        "ya no, gracias",
        "No longer needed, thank you",
    ] {
        assert_eq!(close_reading(words, &[]), CloseReading::Intent, "{words}");
    }
}

/// TD-JOB-05 (JOB-023, D-1203): the how-to line names the open number the message holds.
#[test]
fn td_job_05_number_named_picks_an_open_number() {
    let open = [1042, 1043];
    assert_eq!(number_named("cancel 1042", &open), Some(1042));
    assert_eq!(number_named("cancel #1043 please", &open), Some(1043));
    assert_eq!(number_named("cancel 86403", &open), None);
    assert_eq!(number_named("cancel", &open), None);
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

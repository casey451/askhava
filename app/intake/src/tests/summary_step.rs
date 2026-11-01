//! The card approval step's pure rules (TD-INT-23: INT-041, CARD-007; TD-AI-17). Fake card text
//! only.

use askhava_messaging::api::{Language, count_segments};
use jiff::SignedDuration;

use crate::api::{
    AUTO_SEND_AFTER, CARD_LINK_TEMPLATE, CARD_SHOWN_TEMPLATE, CardReply, card_digest, card_reply,
    preview_text,
};

const SHORT: &str = "Kitchen sink drips under the cabinet.\nHow soon: No rush";
const TAIL: &str = "It goes out in 30 minutes. Reply EDIT to change it, or SEND to send it now.";
const LINK: &str = "askhava.com/r/fake-token";

/// TD-INT-23 (INT-041): a card that fits 2 segments is texted in full in the spec's words, with
/// the 30-minute default read into the text.
#[test]
fn td_int_23_a_short_card_is_texted_in_full() {
    let (id, body) =
        preview_text(("1042", SHORT), LINK, AUTO_SEND_AFTER, Language::En).expect("text");
    assert_eq!(id, CARD_SHOWN_TEMPLATE);
    let want = format!("Ask Hava: Here's request 1042 as businesses will see it: {SHORT} {TAIL}");
    assert_eq!(body, want);
    assert!(count_segments(&body).segments <= 2);
}

/// TD-INT-23 (INT-041): a longer card sends the link text instead; neither text asks for "Reply OK".
#[test]
fn td_int_23_a_long_card_sends_the_link_text() {
    let long = "Water heater leaks at the base and the pilot light keeps going out. ".repeat(3);
    let (id, body) =
        preview_text(("1042", &long), LINK, AUTO_SEND_AFTER, Language::En).expect("text");
    assert_eq!(id, CARD_LINK_TEMPLATE);
    let want =
        format!("Ask Hava: Request 1042 is ready. Take a look before it goes out: {LINK} {TAIL}");
    assert_eq!(body, want);
    for text in [
        body,
        preview_text(("1042", SHORT), LINK, AUTO_SEND_AFTER, Language::En)
            .expect("text")
            .1,
    ] {
        assert!(!text.to_lowercase().contains("reply ok"), "{text}");
    }
}

/// TD-INT-23 (INT-041, D-460): the wait is a setting read into the text, in either language.
#[test]
fn td_int_23_the_wait_setting_is_read_into_the_text() {
    let wait = SignedDuration::from_mins(45);
    let (_, en) = preview_text(("7", SHORT), LINK, wait, Language::En).expect("text");
    assert!(en.contains("It goes out in 45 minutes."), "{en}");
    let (_, es) = preview_text(("7", SHORT), LINK, wait, Language::Es).expect("text");
    assert!(es.contains("Sale en 45 minutos."), "{es}");
    assert!(es.contains("CAMBIAR") && es.contains("ENVIAR"), "{es}");
}

/// TD-INT-23 (INT-041): SEND, OK, YES or ENVIAR send; EDIT or CAMBIAR edit; anything else, or a
/// reply word inside a longer message, is not a card reply.
#[test]
fn td_int_23_card_replies() {
    for word in [
        "SEND", "send", "OK", "ok.", "Yes", "YES!", "ENVIAR", " enviar ",
    ] {
        assert_eq!(card_reply(word), Some(CardReply::Send), "{word}");
    }
    for word in ["EDIT", "edit", "Cambiar", "CAMBIAR."] {
        assert_eq!(card_reply(word), Some(CardReply::Edit), "{word}");
    }
    for word in [
        "",
        "send it later",
        "1042 YES",
        "no",
        "STOP",
        "edit the photo please",
    ] {
        assert_eq!(card_reply(word), None, "{word}");
    }
}

/// TD-AI-17 (CARD-007): any change to the card, one character, gives another digest, so a card
/// the homeowner wasn't shown never matches the approved one.
#[test]
fn td_ai_17_one_changed_character_is_another_card() {
    let shown = card_digest(SHORT);
    assert_eq!(shown.len(), 32);
    assert_eq!(shown, card_digest(SHORT));
    assert_ne!(shown, card_digest(&SHORT.replace("drips", "drops")));
    assert_ne!(shown, card_digest(&format!("{SHORT} ")));
}

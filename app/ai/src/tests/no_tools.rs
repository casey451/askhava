//! No tools, nothing executed, no quote summaries, and the AI-021 output screen (TD-AI-02,
//! TD-AI-06, TD-AI-10; AI-006, AI-010, AI-021, CARD-010).

use askhava_catalog::api::{RESTRICTED_WORDS, restricted_match};
use proptest::prelude::*;
use serde_json::{Value, json};

use super::schemas::{open, reply_fields, scrubbed};
use crate::api::quote_summary;
use crate::models::AiConfig;
use crate::schemas::{ScreenHit, Screened, TextKind, screen_text};
use crate::service::{CallEnd, intake_request, reading_from_reply};

fn has_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map.contains_key(key) || map.values().any(|v| has_key(v, key)),
        Value::Array(items) => items.iter().any(|v| has_key(v, key)),
        _ => false,
    }
}

/// TD-AI-02 (AI-006): the request carries no tools, no tool choice and nothing but the five
/// Messages fields; a reply that stops for a tool is discarded, never acted on.
#[test]
fn requests_carry_no_tools_and_tool_use_is_discarded() {
    let request = intake_request(
        &AiConfig::from_pairs([]).unwrap(),
        &scrubbed("my sink leaks"),
    );
    for key in ["tools", "tool_choice", "mcp_servers"] {
        assert!(!has_key(&request, key), "{key}");
    }
    let mut keys: Vec<&str> = request
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["max_tokens", "messages", "model", "output_config", "system"]
    );
    let tool_use = json!({"stop_reason": "tool_use", "content": [
        {"type": "tool_use", "id": "toolu_fake", "name": "send_text", "input": {}}]});
    assert_eq!(
        reading_from_reply(&tool_use, &open()),
        Err(CallEnd::OtherStop)
    );
    let two_blocks = json!({"stop_reason": "end_turn", "content": [
        {"type": "text", "text": Value::Object(reply_fields()).to_string()},
        {"type": "tool_use", "id": "toolu_fake", "name": "charge", "input": {}}]});
    assert!(matches!(
        reading_from_reply(&two_blocks, &open()),
        Err(CallEnd::Discarded(_))
    ));
}

/// TD-AI-02 (AI-006, AI-010): `ai` cannot send, charge or release: it depends on no crate that
/// does, names no release capability, and its reading holds only the AI-010 fields.
#[test]
fn output_never_sends_charges_or_releases() {
    let manifest = include_str!("../../Cargo.toml");
    for crate_name in [
        "askhava-messaging",
        "askhava-payments",
        "askhava-picks",
        "askhava-calls",
    ] {
        assert!(!manifest.contains(crate_name), "{crate_name}");
    }
    let sources = [
        include_str!("../api.rs"),
        include_str!("../models.rs"),
        include_str!("../schemas.rs"),
        include_str!("../service.rs"),
    ];
    for banned in [
        "ReleaseProof",
        "release_mint",
        "SmsAdapter",
        "PaymentsAdapter",
        "read_contact",
    ] {
        assert!(sources.iter().all(|s| !s.contains(banned)), "{banned}");
    }
    let reply = json!({"stop_reason": "end_turn", "content": [
        {"type": "text", "text": Value::Object(reply_fields()).to_string()}]});
    let reading = reading_from_reply(&reply, &open()).unwrap();
    assert_eq!(reading.trade, "plumbing_repair");
}

proptest! {
    /// TD-AI-06 (AI-006, PICK-003, D-373): while AI quote summaries are off, no quote gets a
    /// summary; a mutant that returns one turns this red.
    #[test]
    fn no_quote_ever_gets_a_summary(raw in "[ -~]{0,200}") {
        prop_assert_eq!(quote_summary(&scrubbed(&raw)), None);
    }
}

/// TD-AI-10 (AI-021, CARD-010): the stand-in's planted outputs (`stand_ins/ai.py` `PLANTS`) are
/// rejected and the fixed template goes out instead.
#[test]
fn planted_restricted_word_url_and_long_text_fall_back_to_the_template() {
    let template = "Ask Hava: What is leaking?";
    let plants = [
        (
            "Ask Hava: Your plumber also sells cannabis and tobacco.",
            ScreenHit::Restricted("tobacco"),
        ),
        (
            "Ask Hava: See https://example.com/offer for details.",
            ScreenHit::Url,
        ),
        (
            &*format!("Ask Hava: {}", "x".repeat(590)),
            ScreenHit::TooLong,
        ),
    ];
    for (text, hit) in plants {
        assert_eq!(
            screen_text(TextKind::Question, text),
            Screened::Fallback(hit)
        );
        assert_eq!(
            screen_text(TextKind::Question, text).or_template(template),
            template
        );
    }
    for link in [
        "Visit example.com today",
        "go to www.fake-offer.net",
        "see Example.CO.",
    ] {
        assert_eq!(
            screen_text(TextKind::JobCard, link),
            Screened::Fallback(ScreenHit::Url),
            "{link}"
        );
    }
    let fine = "Ask Hava: Is the leak under the sink or behind the wall, e.g. at the pipe joint?";
    assert_eq!(
        screen_text(TextKind::Question, fine).or_template(template),
        fine
    );
    assert_eq!(
        screen_text(TextKind::Question, &"y".repeat(320)),
        Screened::Use("y".repeat(320))
    );
}

/// TD-AI-10 (CARD-010): the list matches whole words in any case, English and Spanish, and leaves
/// everyday trade words alone.
#[test]
fn restricted_list_matches_whole_words_only() {
    assert_eq!(
        restricted_match("Selling CIGARETTES here"),
        Some("cigarettes")
    );
    assert_eq!(restricted_match("una cerveza fría"), Some("cerveza"));
    assert_eq!(restricted_match("an arma de fuego"), Some("arma de fuego"));
    for clean in [
        "Essex street gunk",
        "pull the weeds",
        "pipe joint leaks",
        "nail gun",
        "a wine fridge",
    ] {
        assert_eq!(restricted_match(clean), None, "{clean}");
    }
    assert!(
        RESTRICTED_WORDS
            .iter()
            .all(|w| restricted_match(w).is_some())
    );
}

/// TD-AI-10 (AI-021, D-814): a link with a path, query, fragment or port is caught, and a
/// zero-width character, fullwidth letters or an ideographic full stop do not hide a restricted
/// word or a domain; the text that passes is the normalized copy that was checked.
#[test]
fn links_with_paths_and_disguised_text_are_caught() {
    for link in [
        "pay at evil.com/pay now",
        "bit.ly/abc",
        "evil.com?ref=1",
        "evil.com#top",
        "evil.com:8080",
        "(see evil.com/pay).",
        "evil\u{3002}com",
        "evil\u{FF0E}com/pay",
        "evil\u{FF61}com",
        "ev\u{200B}il.c\u{200B}om",
        "\u{FF45}\u{FF56}\u{FF49}\u{FF4C}.\u{FF43}\u{FF4F}\u{FF4D}",
    ] {
        assert_eq!(
            screen_text(TextKind::JobCard, link),
            Screened::Fallback(ScreenHit::Url),
            "{link:?}"
        );
    }
    for hidden in [
        "s\u{200B}ex",
        "s\u{00AD}ex",
        "\u{FF53}\u{FF45}\u{FF58}",
        "to\u{2060}bacco",
    ] {
        assert!(
            matches!(
                screen_text(TextKind::JobCard, hidden),
                Screened::Fallback(ScreenHit::Restricted(_))
            ),
            "{hidden:?}"
        );
    }
    for fine in [
        "the 1/2 inch pipe",
        "at 8:30 or 9:00",
        "and/or the valve",
        "1.5/hr",
    ] {
        assert_eq!(
            screen_text(TextKind::JobCard, fine),
            Screened::Use(fine.to_owned())
        );
    }
    assert_eq!(
        screen_text(TextKind::JobCard, "\u{FF2C}eak\u{200B}"),
        Screened::Use("Leak".to_owned())
    );
}

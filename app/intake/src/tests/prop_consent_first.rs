//! Properties of the first reply over generated conversations (TD-CON-01, TD-CON-11, TD-CON-24:
//! CON-001, CON-002, CON-004, CON-026, CON-027).
//!
//! A conversation is a generated run of messages (requests, Spanish requests, 911-list matches,
//! "what?", "is this a real person?", agreement words, STOP, HELP) fed through `rules::plan` with
//! the request's state carried along, and every planned text rendered from the templates.

use askhava_messaging::api::{Language, Registry, render};
use proptest::prelude::*;

use crate::rules::{Inbound, SAFETY_TEMPLATE, Send, Settings, Then, plan};

const NO_REPLY_ASKS: &[&str] = &["Reply OK", "Reply YES", "Responda OK"];

/// One generated message: its words and flags.
fn message() -> impl Strategy<Value = Inbound<'static>> {
    let words = prop::sample::select(vec![
        "I need help with my sink",
        "Hola, mi lavabo gotea",
        "what?",
        "is this a real person?",
        "fine",
        "go ahead",
        "de acuerdo",
        "I smell gas",
        "STOP",
        "HELP",
    ]);
    (words, any::<Option<bool>>()).prop_map(|(words, ai_spanish)| Inbound {
        words,
        opt_out: words == "STOP",
        help: words == "HELP",
        emergency: words == "I smell gas",
        ai_spanish,
    })
}

fn rendered(send: Send, settings: Settings) -> String {
    let (id, language) = match send {
        Send::Safety(language) => (SAFETY_TEMPLATE, language),
        Send::Consent { template, language } => (template, language),
    };
    render(id, language, settings.spanish_conversations_on, &[])
        .expect("render")
        .body
}

proptest! {
    /// TD-CON-01, TD-CON-11 (CON-001, CON-027): a property of `plan` with its `consent_sent` state
    /// carried along: the first planned text of a request is the consent text, or the 911 text with
    /// the consent text right after it; once `consent_sent` is true `plan` never plans it again;
    /// STOP and HELP never get it; nothing waits for a reply. That the database keeps the state, so
    /// the consent text is queued once per request even for repeated or concurrent messages, is
    /// checked by `tests::consent::td_con_01_first_message_gets_the_consent_text_once` and
    /// `td_con_01_concurrent_first_texts_queue_one_consent_text`.
    #[test]
    fn td_con_01_consent_text_first_and_once_per_request(
        messages in prop::collection::vec(message(), 1..12),
        spanish_conversations_on in any::<bool>(),
        safety_call_on in any::<bool>(),
    ) {
        let settings = Settings { spanish_conversations_on, safety_call_on };
        let mut consent_sent = false;
        let mut consents = 0;
        for msg in &messages {
            let p = plan(consent_sent, msg, settings);
            if msg.opt_out || msg.help {
                prop_assert!(p.texts.is_empty());
                prop_assert_eq!(p.then, Then::Nothing);
                continue;
            }
            if !consent_sent {
                let expected = usize::from(msg.emergency) + 1;
                prop_assert_eq!(p.texts.len(), expected);
                let last_is_consent = matches!(p.texts.last(), Some(Send::Consent { .. }));
                prop_assert!(last_is_consent);
                prop_assert_eq!(matches!(p.texts.first(), Some(Send::Safety(_))), msg.emergency);
                prop_assert_eq!(p.then, Then::FirstQuestion);
                consent_sent = true;
            } else {
                prop_assert!(p.texts.is_empty());
                prop_assert!(matches!(p.then, Then::IntakeReply | Then::DiscloseThenAsk));
            }
            consents += p.texts.iter().filter(|s| matches!(s, Send::Consent { .. })).count();
            for send in &p.texts {
                let body = rendered(*send, settings);
                for ask in NO_REPLY_ASKS {
                    prop_assert!(!body.contains(ask), "{} holds {}", body, ask);
                }
            }
        }
        prop_assert!(consents <= 1);
    }

    /// TD-CON-01 (CON-026): the consent text is Spanish only when the first message reads as
    /// Spanish and Spanish conversations are on; otherwise English.
    #[test]
    fn td_con_01_consent_language_follows_con_026(
        msg in message(),
        spanish_conversations_on in any::<bool>(),
    ) {
        let settings = Settings { spanish_conversations_on, safety_call_on: false };
        let p = plan(false, &msg, settings);
        if let Some(Send::Consent { language, .. }) = p.texts.last() {
            let reads_spanish = msg.words == "Hola, mi lavabo gotea" || msg.ai_spanish == Some(true);
            let spanish = reads_spanish && spanish_conversations_on;
            prop_assert_eq!(*language == Language::Es, spanish);
        }
    }
}

/// TD-CON-24 (CON-004, CON-027): no homeowner template in either language asks for "Reply OK",
/// "Reply YES" or "Responda OK" to go on.
#[test]
fn td_con_24_no_homeowner_text_asks_for_a_consent_reply() {
    let registry = Registry::embedded().expect("registry");
    for id in registry.ids() {
        for language in [Language::En, Language::Es] {
            let (text, _) = registry.template(&id, language).expect("template");
            for ask in NO_REPLY_ASKS {
                assert!(!text.contains(ask), "{id} holds {ask}");
            }
        }
    }
}

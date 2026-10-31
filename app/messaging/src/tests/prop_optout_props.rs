//! Property tests for opt-outs (TD-CON-04 property, TD-CON-05 property over all later events;
//! CON-008, CON-011, CON-013, CON-015, D-131, D-162).

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use sqlx::types::Uuid;

use crate::api::{
    CONFIRMATION_TEMPLATE, Classified, HELP_TEMPLATE, Listing, PROVIDER_OPT_OUT_WORDS,
    ProviderKeyword, RESUBSCRIBE_WORDS, classify, detect_opt_out, may_send_to,
};

/// The later inbound texts a listed number may send.
const INBOUND: [&str; 9] = [
    "yes",
    "HELP",
    "info",
    "ayuda",
    "STOP",
    "can stop by at 3",
    "my sink leaks",
    "start",
    "Unstop",
];
/// What the provider reports for `text`, as the fake Twilio matches it (T167): `START` only for a
/// bare START or UNSTOP once trimmed; the app never re-matches it (D-870).
fn provider(text: &str) -> ProviderKeyword {
    let word = text.trim().to_lowercase();
    if RESUBSCRIBE_WORDS.contains(&word.as_str()) {
        ProviderKeyword::Start
    } else {
        ProviderKeyword::None
    }
}

/// The texts the app may try to send.
const TEMPLATES: [&str; 5] = [
    "consent",
    "first_quote",
    "start_reply",
    CONFIRMATION_TEMPLATE,
    HELP_TEMPLATE,
];

proptest! {
    /// TD-CON-04, CON-008: a sole word in any mix of case with any surrounding spaces opts out.
    #[test]
    fn any_case_and_spacing_of_a_sole_word_opts_out(
        pick in 0usize..12, mask in any::<u16>(), left in 0usize..4, right in 0usize..4,
    ) {
        let word = PROVIDER_OPT_OUT_WORDS[pick];
        let mixed: String = word.chars().enumerate()
            .map(|(i, c)| if mask >> (i % 16) & 1 == 1 { c.to_ascii_uppercase() } else { c })
            .collect();
        let text = format!("{}{mixed}{}", " ".repeat(left), " ".repeat(right));
        prop_assert!(detect_opt_out(&text).is_some());
    }

    /// TD-CON-04, CON-011: an opt-out word followed by other words is not a sole-word opt-out.
    #[test]
    fn a_word_with_more_after_it_is_not_a_sole_word(pick in 0usize..12, tail in "[a-z]{3,8} [0-9]") {
        let text = format!("{} by at {tail}", PROVIDER_OPT_OUT_WORDS[pick]);
        prop_assert_eq!(detect_opt_out(&text), None);
    }

    /// TD-CON-05, CON-013, CON-015: after an opt-out, over any later sequence of inbound texts and
    /// send attempts, nothing but the confirmation within 5 minutes and the HELP reply goes until
    /// a START or UNSTOP. Each opt-out of a not-listed number is a new listing with its own event;
    /// a confirmation goes only for the current one, and never late.
    #[test]
    fn nothing_but_confirmation_and_help_until_start(
        events in proptest::collection::vec((0usize..9, 0usize..5, 0i64..900, 0u128..4), 1..40),
    ) {
        let opted = Timestamp::from_second(1_793_000_000).expect("time");
        let (mut listing, mut now, mut serial) = (Some(Listing { since: opted, event: Uuid::from_u128(0) }), opted, 0u128);
        for (text, template, step, about) in events {
            now += SignedDuration::from_secs(step);
            listing = match classify(INBOUND[text], provider(INBOUND[text])) {
                Classified::Resubscribe => None,
                Classified::OptOut { .. } => listing.or_else(|| {
                    serial += 1;
                    Some(Listing { since: now, event: Uuid::from_u128(serial) })
                }),
                Classified::Other => listing,
            };
            let template = TEMPLATES[template];
            let subject = [Uuid::from_u128(about)];
            if let Some(Listing { since, event }) = listing && may_send_to(listing, template, &subject, now) {
                let late = now.duration_since(since) > SignedDuration::from_mins(5);
                let current = subject.contains(&event);
                prop_assert!(template == HELP_TEMPLATE || (template == CONFIRMATION_TEMPLATE && current && !late));
            }
        }
    }
}

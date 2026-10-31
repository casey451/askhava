//! Property tests for opt-outs (TD-CON-04 property, TD-CON-05 property over all later events;
//! CON-008, CON-011, CON-013, CON-015, D-131, D-162).

use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;

use crate::api::{
    CONFIRMATION_TEMPLATE, Classified, HELP_TEMPLATE, PROVIDER_OPT_OUT_WORDS, ProviderKeyword,
    classify, detect_opt_out, may_send_to,
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
    /// a START or UNSTOP.
    #[test]
    fn nothing_but_confirmation_and_help_until_start(
        events in proptest::collection::vec((0usize..9, 0usize..5, 0i64..900), 1..40),
    ) {
        let opted = Timestamp::from_second(1_793_000_000).expect("time");
        let (mut since, mut now) = (Some(opted), opted);
        for (text, template, step) in events {
            now += SignedDuration::from_secs(step);
            since = match classify(INBOUND[text], ProviderKeyword::None) {
                Classified::Resubscribe => None,
                Classified::OptOut { .. } => since.or(Some(now)),
                Classified::Other => since,
            };
            let template = TEMPLATES[template];
            if let Some(at) = since && may_send_to(since, template, now) {
                let late = now.duration_since(at) > SignedDuration::from_mins(5);
                prop_assert!(template == HELP_TEMPLATE || (template == CONFIRMATION_TEMPLATE && !late));
            }
        }
    }
}

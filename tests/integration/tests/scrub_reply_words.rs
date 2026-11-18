//! TD-AI-31 (T658, D-4327; AI-002, AI-020, PRIV-005): the scrubber's all-caps rule never takes a
//! texting reply or command word as a street name, so `privacy::api::REPLY_WORDS` must hold every
//! opt-out and resubscribe word `messaging` reads.
//!
//! `privacy` may not depend on `messaging` (`kit/layers.json`), so `REPLY_WORDS` is a copy; this
//! test fails when `messaging::api::PROVIDER_OPT_OUT_WORDS` or `RESUBSCRIBE_WORDS` gains a word the
//! copy lacks. The other sources the copy names are private and are not checked by machine.

use askhava_messaging::api::{PROVIDER_OPT_OUT_WORDS, RESUBSCRIBE_WORDS};
use askhava_privacy::api::REPLY_WORDS;

/// TD-AI-31: every provider opt-out and resubscribe word is a reply word for the scrubber.
#[test]
fn reply_words_hold_every_opt_out_and_resubscribe_word() {
    for word in PROVIDER_OPT_OUT_WORDS.iter().chain(&RESUBSCRIBE_WORDS) {
        assert!(
            REPLY_WORDS.contains(word),
            "REPLY_WORDS lacks messaging's {word:?}"
        );
    }
}

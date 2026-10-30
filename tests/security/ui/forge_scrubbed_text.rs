//! TD-AI-01 (AI-004): code outside `privacy` cannot build `ScrubbedText` by struct literal; its one field is private.
//!
//! `ScrubbedText` derives no `Default` and implements no `From`, and its only constructor is
//! `privacy::api::scrub`. A literal is the one remaining way in, and the compiler refuses it.

use askhava_privacy::api::ScrubbedText;

fn main() {
    let _literal = ScrubbedText {
        text: String::from("call 928 555 0100"),
    };
}

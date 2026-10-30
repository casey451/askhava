//! No crate but `privacy` can build `ScrubbedText` (TD-AI-01 compile-fail case; AI-004, D-488).
//!
//! `ai` accepts only `ScrubbedText`, and only `privacy::api::scrub` makes one, so the `ai` module
//! never receives unscrubbed text. The type has no `Default`, no `From` and no public
//! constructor, so `ui/forge_scrubbed_text.rs` tries the one remaining way, a struct literal, from
//! outside `privacy`; it must fail to compile with the committed `.stderr`.
//! This crate has no features, so the proof holds for the build that ships.

/// Every forge attempt fails to compile (TD-AI-01, AI-004).
#[test]
fn scrubbed_text_cannot_be_forged_outside_privacy() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("ui/forge_scrubbed_text.rs");
}

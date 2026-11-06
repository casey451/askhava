//! Code outside `payments` cannot construct a release proof (TD-SEC-11; SEC-030, D-481, D-488).
//!
//! `privacy::api::read_for_release` hands a homeowner's contact details to a pick only for a
//! `ReleaseProof`, which only `ReleaseMint` makes, and only `payments` holds the mint. Each case in
//! `ui/forge_release_proof_*.rs` tries one way to forge a proof or a mint from outside `privacy`
//! (struct literal, `Default`, `Clone` of a borrowed one) and must fail to compile with the
//! committed `.stderr`. This crate has no features, so the proof holds for the build that ships.
//! The other TD-SEC-11 cases: `app/privacy/src/tests/release_mint_once.rs` (a second
//! `release_mint()` panics), `app/payments/src/release_proof.rs` and
//! `tests/integration/tests/confirm_and_pay.rs` (only a verified event or a no-fee decision mints).

/// Every forge attempt fails to compile (TD-SEC-11, SEC-030).
#[test]
fn release_proofs_cannot_be_forged_outside_payments() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("ui/forge_release_proof_*.rs");
}

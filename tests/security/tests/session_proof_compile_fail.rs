//! Business-realm code cannot construct a session proof (TD-SEC-25; SEC-046, D-480).
//!
//! `privacy::api` hands homeowner and owner plaintext only to a caller holding a
//! `HomeownerSessionProof` or `OwnerSessionProof`, which only `SessionMint` makes, in `portals`'
//! session extractors. Each case in `ui/forge_session_proof_*.rs` tries one way to forge one from
//! outside `privacy` (struct literal, `Default`, `Clone`) and must fail to compile with the
//! committed `.stderr`. This crate has no features, so the proof holds for the build that ships.
//! The other TD-SEC-25 cases: `vault_readers.rs` (no business route names a plaintext read) and
//! the in-crate test in `app/privacy/src/api.rs` (a homeowner session reads only its own record).

/// Every forge attempt fails to compile (TD-SEC-25, SEC-046).
#[test]
fn session_proofs_cannot_be_forged_outside_privacy() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("ui/forge_session_proof_*.rs");
}

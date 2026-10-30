//! The security test layer (`askhava-security`; SEC-029, D-488).
//!
//! This crate has no features and no code. Each task that proves a type
//! cannot be misused adds a driver in `tests/security/tests/` with one
//! `trybuild::TestCases` that globs only its own cases in `ui/` (for example
//! `ui/forge_release_proof_*.rs`), plus the expected `.stderr` files, which
//! are tied to the exact rustc version (`TRYBUILD=overwrite` after a toolchain
//! bump). The crate takes a path dependency on an app crate only to exercise
//! it, and reaches it only through its `api`.

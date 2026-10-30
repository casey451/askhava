//! The recovery test layer (`askhava-recovery-tests`; SEC-029, D-488).
//!
//! The package is not `askhava-recovery` because the recovery module holds
//! that name. This crate has no features and no code. Tests live in
//! `tests/recovery/tests/`, take a path dependency on each app crate they
//! exercise, and reach it only through its `api`.

//! Every adapter method that acts outside requires an idempotency key (TD-SEC-03; SEC-018, D-596).
//!
//! An architecture test over every source file of `app/adapters/src` (so a new vendor file is
//! covered without editing this test): each method of a `pub trait` takes
//! `idempotency: &IdempotencyKey` unless its name starts with a read-only prefix. The same scan
//! is run on a planted trait with a keyless `send_` method to prove a missing key fails. The key
//! itself is derived from what it identifies: stable for one record, different per record, action
//! and secret, and free of the input text. All values are fake.

use std::path::Path;

use askhava_adapters::api::IdempotencyKey;
use secrecy::SecretString;

/// Method-name prefixes that only read, so need no key (D-596).
const READ_ONLY: [&str; 6] = ["fetch_", "get_", "list_", "read_", "check_", "lookup_"];

/// The outside-acting trait methods in `source` whose signature has no idempotency key.
fn keyless_methods(source: &str) -> Vec<String> {
    let mut missing = Vec::new();
    for body in source.split("pub trait ").skip(1) {
        let body = body.split("\n}\n").next().unwrap_or_default();
        for method in body.split("fn ").skip(1) {
            let signature = method.split(')').next().unwrap_or_default();
            let exempt = READ_ONLY.iter().any(|p| method.starts_with(p));
            if !exempt && !signature.contains("idempotency: &IdempotencyKey") {
                missing.push(signature.to_owned());
            }
        }
    }
    missing
}

/// TD-SEC-03; SEC-018: every trait method that acts outside, in every adapter file, takes a key.
#[test]
fn every_outside_action_in_every_adapter_takes_a_key() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/adapters/src");
    let mut traits = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        traits += source.matches("pub trait ").count();
        let missing = keyless_methods(&source);
        assert!(missing.is_empty(), "{}: {missing:?}", path.display());
    }
    assert!(traits >= 9, "the scan found only {traits} adapter traits");
}

/// TD-SEC-03; SEC-018: a method that acts outside without a key is caught by the scan.
#[test]
fn a_missing_idempotency_key_fails_the_scan() {
    let planted = "pub trait Planted {\n    fn send_thing(&self, id: &str) -> u8;\n    \
                   fn fetch_thing(&self, id: &str) -> u8;\n    \
                   fn post_thing(&self, idempotency: &IdempotencyKey) -> u8;\n}\n";
    assert_eq!(keyless_methods(planted), ["send_thing(&self, id: &str"]);
}

fn key(secret: &str, action: &str, ids: &[&str]) -> IdempotencyKey {
    IdempotencyKey::derive(&SecretString::from(secret), action, ids)
}

/// TD-SEC-03; SEC-018: the key is reproducible from what it identifies and carries none of it.
#[test]
fn key_is_derived_from_what_it_identifies() {
    let base = key("fake-id-secret", "payments.refund", &["charge-1"]);
    assert_eq!(
        base,
        key("fake-id-secret", "payments.refund", &["charge-1"])
    );
    assert_ne!(
        base,
        key("fake-id-secret", "payments.refund", &["charge-2"])
    );
    assert_ne!(base, key("fake-id-secret", "letters.send", &["charge-1"]));
    assert_ne!(
        base,
        key("other-id-secret", "payments.refund", &["charge-1"])
    );
    assert_ne!(
        key("fake-id-secret", "a", &["b", "c"]),
        key("fake-id-secret", "a", &["bc"])
    );
    assert_eq!(base.as_str().len(), 64);
    assert!(base.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(!base.as_str().contains("charge"));
}

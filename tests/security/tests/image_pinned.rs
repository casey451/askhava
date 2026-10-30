//! The Dockerfile's base images are pinned by digest (SEC-021, SEC-045; TD-SEC-24).
//!
//! TD-SEC-24 (Dockerfile cases only; workflow `uses:` SHA cases are T169's and
//! T121's, the Dependabot case is a later task's): a docs test fails on any
//! image without a digest. The checks also hold the D-491 shape: no `# syntax=`
//! line, `RUSTUP_TOOLCHAIN` set, a non-root user 10001, `ca-certificates` and
//! `libssl3t64` in the runtime, and a separate worker binary.

use std::fs;
use std::path::PathBuf;

fn dockerfile() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Dockerfile");
    fs::read_to_string(path).expect("the Dockerfile exists at the repository root")
}

/// True for 64 lowercase hex characters.
fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Returns each `FROM` image reference that is not pinned by digest.
/// A stage name from an earlier `AS` is not an image and is skipped.
fn unpinned_images(text: &str) -> Vec<String> {
    let mut stages: Vec<String> = Vec::new();
    let mut bad = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if !words
            .first()
            .is_some_and(|w| w.eq_ignore_ascii_case("FROM"))
        {
            continue;
        }
        let image = words
            .iter()
            .skip(1)
            .find(|w| !w.starts_with("--"))
            .copied()
            .unwrap_or("");
        let pinned = image
            .split_once("@sha256:")
            .is_some_and(|(_, digest)| is_sha256_hex(digest));
        if !pinned && !stages.iter().any(|s| s == image) {
            bad.push(image.to_string());
        }
        let stage = words
            .iter()
            .position(|w| w.eq_ignore_ascii_case("AS"))
            .and_then(|pos| words.get(pos + 1));
        if let Some(name) = stage {
            stages.push((*name).to_string());
        }
    }
    bad
}

#[test]
fn td_sec_24_every_dockerfile_image_is_pinned_by_digest() {
    let text = dockerfile();
    assert!(text.lines().any(|l| l.starts_with("FROM ")), "no FROM line");
    assert_eq!(unpinned_images(&text), Vec::<String>::new());
}

#[test]
fn td_sec_24_checker_fails_on_an_unpinned_or_malformed_image() {
    let sample = "FROM rust:1.99.0-slim-trixie AS chef\n\
                  FROM chef AS builder\n\
                  FROM debian:trixie-slim@sha256:abc AS runtime\n\
                  FROM --platform=linux/amd64 alpine:3\n";
    assert_eq!(
        unpinned_images(sample),
        vec![
            "rust:1.99.0-slim-trixie",
            "debian:trixie-slim@sha256:abc",
            "alpine:3"
        ]
    );
}

#[test]
fn td_sec_24_image_follows_the_d491_shape() {
    let text = dockerfile();
    assert!(!text.lines().any(|l| l.trim_start().starts_with("# syntax")));
    assert!(text.contains("ENV RUSTUP_TOOLCHAIN="));
    assert!(text.contains("cargo chef prepare") && text.contains("cargo chef cook"));
    assert!(text.contains("debian:trixie-slim@sha256:"));
    assert!(text.contains("ca-certificates") && text.contains("libssl3t64"));
    assert!(text.lines().any(|l| l.trim() == "USER 10001"));
    assert!(text.contains("/usr/local/bin/worker"));
    let code = text.lines().filter(|l| !l.trim_start().starts_with('#'));
    assert!(
        !code.into_iter().any(|l| l.contains("test-control")),
        "SEC-035"
    );
}

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
        if !is_pinned(image) && !stages.iter().any(|s| s == image) {
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

/// True for a `FROM`-style image reference pinned by a 64-hex digest.
fn is_pinned(image: &str) -> bool {
    image
        .split_once("@sha256:")
        .is_some_and(|(_, digest)| is_sha256_hex(digest))
}

/// The value of a `from=` or `--from=` option in a line, if any.
fn from_option(line: &str) -> Option<&str> {
    line.split(|c: char| c.is_whitespace() || c == ',')
        .find_map(|w| {
            w.strip_prefix("--from=")
                .or_else(|| w.strip_prefix("from="))
        })
}

/// Returns each `COPY --from=` or `RUN --mount ... from=` source that is
/// neither an earlier stage name nor pinned by digest.
fn unpinned_sources(text: &str) -> Vec<String> {
    let mut stages: Vec<String> = Vec::new();
    let mut bad = Vec::new();
    for line in text.lines().map(str::trim_start) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if let Some(src) = from_option(line)
            && !is_pinned(src)
            && !stages.iter().any(|s| s == src)
        {
            bad.push(src.to_string());
        }
        let name = words
            .iter()
            .position(|w| w.eq_ignore_ascii_case("AS"))
            .and_then(|pos| words.get(pos + 1));
        if let (true, Some(name)) = (
            words
                .first()
                .is_some_and(|w| w.eq_ignore_ascii_case("FROM")),
            name,
        ) {
            stages.push((*name).to_string());
        }
    }
    bad
}

/// Returns each cargo line that turns on features (SEC-035).
fn feature_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter(|l| l.contains("cargo "))
        .filter(|l| l.contains("--features") || l.contains("--all-features"))
        .map(str::to_string)
        .collect()
}

/// True when `USER 10001` follows the last `FROM`.
fn runs_as_10001(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let last_from = lines
        .iter()
        .rposition(|l| l.to_ascii_uppercase().starts_with("FROM "));
    last_from.is_some_and(|i| lines[i..].contains(&"USER 10001"))
}

#[test]
fn td_sec_24_copy_and_mount_sources_are_stages_or_pinned() {
    assert_eq!(unpinned_sources(&dockerfile()), Vec::<String>::new());
}

#[test]
fn td_sec_24_source_checker_fails_on_an_unpinned_source() {
    let digest = "a".repeat(64);
    let sample = format!(
        "FROM rust@sha256:{digest} AS b\n\
         COPY --from=b /x /x\n\
         COPY --from=alpine:3 /y /y\n\
         RUN --mount=type=bind,from=busybox,target=/z true\n\
         COPY --from=ok@sha256:{digest} /w /w\n"
    );
    assert_eq!(unpinned_sources(&sample), vec!["alpine:3", "busybox"]);
}

#[test]
fn td_sec_24_no_cargo_features_in_the_image() {
    assert_eq!(feature_lines(&dockerfile()), Vec::<String>::new());
    let sample = "RUN cargo build --features test-control\n\
                  RUN cargo build --all-features\n\
                  RUN cargo build --locked\n# cargo --features\n";
    assert_eq!(feature_lines(sample).len(), 2);
}

#[test]
fn td_sec_24_image_runs_as_user_10001_in_the_last_stage() {
    assert!(runs_as_10001(&dockerfile()));
    assert!(!runs_as_10001("FROM a\nUSER 10001\nFROM b\n"));
    assert!(!runs_as_10001("FROM a\nUSER root\n"));
    assert!(runs_as_10001("FROM a\nFROM b\nUSER 10001\n"));
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

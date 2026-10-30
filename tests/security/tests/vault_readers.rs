//! Only the readers of `docs/architecture.md` section 8 name the vault's plaintext reads (TD-PRIV-01, TD-SEC-25; PRIV-004, SEC-046).
//!
//! Scans every Rust file under `app/` (comments and string literals removed) for the `privacy::api`
//! plaintext read functions and fails when one is named outside the crates D-093 lets read:
//! `read_contact` by `messaging` and `calls`, `read_for_release` by `picks` (with a `ReleaseProof`,
//! SEC-030), `read_own_contact` and `read_for_owner` by `portals` (with a session proof, SEC-046),
//! and no business-realm file of `portals` naming any of them. `quotes` names none: it reads card
//! photos only (PRIV-004). The compile-fail proof that business code cannot build a session proof
//! is T223's trybuild case; a homeowner session reading only its own record is the in-crate test
//! in `app/privacy/src/api.rs`. Each rule is proved on a planted source.

use std::fs;
use std::path::{Path, PathBuf};

/// Each read function and the crates that may name it besides `privacy` (D-093, D-491).
const READERS: [(&str, &[&str]); 4] = [
    ("read_contact", &["messaging", "calls"]),
    ("read_for_release", &["picks"]),
    ("read_own_contact", &["portals"]),
    ("read_for_owner", &["portals"]),
];

/// `src` with `//` comments and `"…"` string bodies blanked, so a mention in prose never counts.
fn code_only(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let (mut in_str, mut in_comment) = (false, false);
    while let Some(c) = chars.next() {
        match (in_str, in_comment, c) {
            (_, true, '\n') => in_comment = false,
            (_, true, _) => continue,
            (true, _, '\\') => {
                chars.next();
            }
            (true, _, '"') | (false, _, '"') => in_str = !in_str,
            (false, _, '/') if chars.peek() == Some(&'/') => in_comment = true,
            (true, _, _) => continue,
            _ => {}
        }
        out.push(c);
    }
    out
}

/// True when `word` appears in `code` as a whole identifier.
fn names(code: &str, word: &str) -> bool {
    code.match_indices(word).any(|(at, _)| {
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let before = code[..at].chars().next_back().is_some_and(ident);
        let after = code[at + word.len()..].chars().next().is_some_and(ident);
        !before && !after
    })
}

/// The findings for one file of crate `krate` at `rel` (PRIV-004, SEC-046).
fn check_file(krate: &str, rel: &str, src: &str) -> Vec<String> {
    let code = code_only(src);
    let business = krate == "portals" && rel.contains("business");
    READERS
        .iter()
        .filter(|(word, _)| names(&code, word))
        .filter(|(_, allowed)| krate != "privacy" && (business || !allowed.contains(&krate)))
        .map(|(word, _)| format!("{rel}: `{word}` named outside its readers"))
        .collect()
}

fn rs_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// TD-PRIV-01, TD-SEC-25: no crate outside D-093's readers, and no business-realm route, names a
/// `privacy::api` plaintext read.
#[test]
fn only_the_listed_readers_name_the_plaintext_reads() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rs_files(&root.join("app"), &mut files);
    let mut findings = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .into_owned();
        let krate = rel.split('/').nth(1).unwrap_or_default().to_owned();
        let src = fs::read_to_string(path).expect("readable source");
        findings.extend(check_file(&krate, &rel, &src));
    }
    assert!(
        files.iter().any(|p| p.ends_with("app/privacy/src/api.rs")),
        "the scan reached privacy"
    );
    assert_eq!(findings, Vec::<String>::new());
}

/// The scan itself: each planted violation gives exactly its finding; allowed and hidden mentions
/// give none.
#[test]
fn the_reader_scan_catches_each_planted_violation() {
    let call = "fn f() { askhava_privacy::api::read_contact(db, v, a, id, &[]); }";
    let cases: [(&str, &str, &str, usize); 9] = [
        ("messaging", "app/messaging/src/service.rs", call, 0),
        ("calls", "app/calls/src/service.rs", call, 0),
        ("jobs", "app/jobs/src/service.rs", call, 1),
        (
            "quotes",
            "app/quotes/src/card.rs",
            "use x::read_for_release;",
            1,
        ),
        (
            "picks",
            "app/picks/src/release.rs",
            "read_for_release(db, v, &p, id, &[])",
            0,
        ),
        (
            "portals",
            "app/portals/src/homeowner.rs",
            "read_own_contact(db, v, &p, &[])",
            0,
        ),
        (
            "portals",
            "app/portals/src/business_routes.rs",
            "read_for_owner(db, v, x, id, &[])",
            1,
        ),
        (
            "jobs",
            "app/jobs/src/a.rs",
            "// read_contact\nlet s = \"read_contact \\\" x\"; my_read_contact();",
            0,
        ),
        ("privacy", "app/privacy/src/api.rs", call, 0),
    ];
    for (krate, rel, src, want) in cases {
        assert_eq!(check_file(krate, rel, src).len(), want, "{rel}: {src}");
    }
}

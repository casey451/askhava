//! Only the readers of `docs/architecture.md` section 8 name the vault's plaintext reads (TD-PRIV-01, TD-SEC-25; PRIV-004, SEC-046).
//!
//! Scans every Rust file under `app/` (comments and string literals removed) for the `privacy::api`
//! plaintext read functions and fails when one is named outside the crates D-093 lets read:
//! `read_contact` by `messaging` and `calls`, `read_for_release` by `picks` (with a `ReleaseProof`,
//! SEC-030), `read_own_contact` and `read_for_owner` by `portals` (with a session proof, SEC-046),
//! and no business-realm file of `portals` (its path or a session type naming `business`) naming
//! any of them. `quotes` names none: it reads card
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

/// `src` with comments (`//` and nested `/* */`) dropped and the bodies of string literals (plain,
/// byte, C and raw `r#"…"#`) and character literals blanked, so a mention in prose or data never
/// counts and a `'"'` never opens a string (D-753).
fn code_only(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let skip = match (chars[i], chars.get(i + 1)) {
            ('/', Some('/')) => line_comment(&chars, i),
            ('/', Some('*')) => block_comment(&chars, i),
            ('"', _) => quoted(&chars, i, '"'),
            ('r', Some('"' | '#')) if raw_prefix(&chars, i) => raw_string(&chars, i),
            ('\'', _) => char_literal(&chars, i),
            _ => None,
        };
        match skip {
            Some(end) => {
                out.push(' ');
                i = end;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

fn ident_before(chars: &[char], at: usize) -> bool {
    at > 0 && (chars[at - 1].is_alphanumeric() || chars[at - 1] == '_')
}

/// Whether the `r` at `at` opens a raw string: alone, or after a lone `b` or `c` prefix.
fn raw_prefix(chars: &[char], at: usize) -> bool {
    !ident_before(chars, at) || (matches!(chars[at - 1], 'b' | 'c') && !ident_before(chars, at - 1))
}

/// The end of the `//` comment at `at` (its newline is kept).
fn line_comment(chars: &[char], at: usize) -> Option<usize> {
    Some(
        (at..chars.len())
            .find(|&j| chars[j] == '\n')
            .unwrap_or(chars.len()),
    )
}

/// The end of the block comment at `at`, nesting as Rust does.
fn block_comment(chars: &[char], at: usize) -> Option<usize> {
    let (mut depth, mut j) = (0_usize, at);
    while j + 1 < chars.len() {
        match (chars[j], chars[j + 1]) {
            ('/', '*') => (depth, j) = (depth + 1, j + 2),
            ('*', '/') if depth == 1 => return Some(j + 2),
            ('*', '/') => (depth, j) = (depth - 1, j + 2),
            _ => j += 1,
        }
    }
    Some(chars.len())
}

/// The end of the literal opened by `quote` at `at`, past escapes.
fn quoted(chars: &[char], at: usize, quote: char) -> Option<usize> {
    let mut j = at + 1;
    while j < chars.len() {
        match chars[j] {
            '\\' => j += 2,
            c if c == quote => return Some(j + 1),
            _ => j += 1,
        }
    }
    Some(chars.len())
}

/// The end of the raw string `r#…#"…"#…#` at `at`, or `None` when `r` starts an identifier.
fn raw_string(chars: &[char], at: usize) -> Option<usize> {
    let hashes = chars[at + 1..].iter().take_while(|&&c| c == '#').count();
    if chars.get(at + 1 + hashes) != Some(&'"') {
        return None;
    }
    let close: Vec<char> = std::iter::once('"')
        .chain(std::iter::repeat_n('#', hashes))
        .collect();
    let body = at + 2 + hashes;
    let found = (body..chars.len()).find(|&j| chars[j..].starts_with(&close));
    Some(found.map_or(chars.len(), |j| j + close.len()))
}

/// The end of the character literal at `at`, or `None` for a lifetime or label (`'a`).
fn char_literal(chars: &[char], at: usize) -> Option<usize> {
    match (chars.get(at + 1), chars.get(at + 2)) {
        (Some('\\'), _) => quoted(chars, at, '\''),
        (Some(_), Some('\'')) => Some(at + 3),
        _ => None,
    }
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

/// Whether `code` names an identifier holding both `business` and `session` in any case (a
/// business-realm session type or extractor), so a business route outside a `business` file is
/// still in the business realm (SEC-046, D-753).
fn business_session(code: &str) -> bool {
    code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(str::to_lowercase)
        .any(|w| w.contains("business") && w.contains("session"))
}

/// The findings for one file of crate `krate` at `rel` (PRIV-004, SEC-046).
fn check_file(krate: &str, rel: &str, src: &str) -> Vec<String> {
    let code = code_only(src);
    let business = krate == "portals" && (rel.contains("business") || business_session(&code));
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

/// D-753: the scan sees past character literals, nested block comments and raw strings, and treats
/// a `portals` file naming a business session as business realm whatever its path.
#[test]
fn the_reader_scan_reads_literals_comments_and_business_sessions() {
    let cases: [(&str, &str, &str, usize); 6] = [
        (
            "jobs",
            "app/jobs/src/b.rs",
            "let q = '\"'; read_contact(db, v, a, id, &[]);",
            1,
        ),
        (
            "jobs",
            "app/jobs/src/c.rs",
            "let e = '\\''; let l: &'static str = x; read_contact();",
            1,
        ),
        (
            "jobs",
            "app/jobs/src/d.rs",
            "/* read_contact /* nested */ read_contact */ let s = r#\"read_contact \" x\"#; br\"read_contact\"; br#\"read_contact\"#;",
            0,
        ),
        (
            "jobs",
            "app/jobs/src/e.rs",
            "/* a */ let s = r#\"\"#; read_contact();",
            1,
        ),
        (
            "portals",
            "app/portals/src/confirm_pay.rs",
            "async fn f(s: BusinessSession) { read_for_owner(db, v, x, id, &[]); }",
            1,
        ),
        (
            "portals",
            "app/portals/src/owner_portal.rs",
            "async fn f(s: OwnerSession) { read_for_owner(db, v, x, id, &[]); }",
            0,
        ),
    ];
    for (krate, rel, src, want) in cases {
        assert_eq!(check_file(krate, rel, src).len(), want, "{rel}: {src}");
    }
}

//! Only `privacy` and the `server` composition name the business-contact reads (TD-PRIV-28; PRIV-004, SEC-004, SEC-047; D-3262).
//!
//! Scans every Rust file under `app/` (comments and string literals removed) and fails when a
//! crate other than `privacy` or `server` names `read_business_mailbox` or `read_staff_phone`:
//! `portals` gets the values only through the composition's traits. A second scan, string literals
//! kept, proves `app/privacy/src/business_contacts.rs` never names the homeowner contact or stated
//! name tables and seals or opens only through the vault's `BusinessTable` wrappers
//! (`seal_business`, `open_business`). Each rule is proved on a planted source.

use std::fs;
use std::path::{Path, PathBuf};

const READS: [&str; 2] = ["read_business_mailbox", "read_staff_phone"];
const ALLOWED: [&str; 2] = ["privacy", "server"];
/// The vault's homeowner seal and open calls `business_contacts.rs` must never make.
const HOMEOWNER_CALLS: [&str; 6] = [
    ".seal(",
    ".open(",
    "seal_stated",
    "open_stated",
    "seal_at",
    "open_at",
];

/// `src` with comments dropped and, unless `keep_strings`, string and char literal bodies blanked.
fn strip(src: &str, keep_strings: bool) -> String {
    let chars: Vec<char> = src.chars().collect();
    let (mut out, mut i) = (String::with_capacity(src.len()), 0);
    while i < chars.len() {
        let next = chars.get(i + 1).copied();
        let end = match (chars[i], next) {
            ('/', Some('/')) => (i..chars.len())
                .find(|&j| chars[j] == '\n')
                .unwrap_or(chars.len()),
            ('/', Some('*')) => block_end(&chars, i),
            ('"', _) if !keep_strings => literal_end(&chars, i, '"'),
            ('\'', Some('\\')) if !keep_strings => literal_end(&chars, i, '\''),
            ('\'', _) if !keep_strings && chars.get(i + 2) == Some(&'\'') => i + 3,
            _ => {
                out.push(chars[i]);
                i += 1;
                continue;
            }
        };
        out.push(' ');
        i = end;
    }
    out
}

/// The index after the nested block comment opening at `at`.
fn block_end(chars: &[char], at: usize) -> usize {
    let (mut depth, mut i) = (0, at);
    while i + 1 < chars.len() {
        match (chars[i], chars[i + 1]) {
            ('/', '*') => (depth, i) = (depth + 1, i + 2),
            ('*', '/') if depth == 1 => return i + 2,
            ('*', '/') => (depth, i) = (depth - 1, i + 2),
            _ => i += 1,
        }
    }
    chars.len()
}

/// The index after the literal quoted by `quote` opening at `at`, escapes skipped.
fn literal_end(chars: &[char], at: usize, quote: char) -> usize {
    let mut i = at + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

/// True when `word` appears in `code` as a whole identifier.
fn names(code: &str, word: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(word).any(|(at, _)| {
        let before = code[..at].chars().next_back().is_some_and(ident);
        !before && !code[at + word.len()..].chars().next().is_some_and(ident)
    })
}

/// The findings for one file of crate `krate` at `rel`.
fn check_file(krate: &str, rel: &str, src: &str) -> Vec<String> {
    let code = strip(src, false);
    READS
        .iter()
        .filter(|w| names(&code, w) && !ALLOWED.contains(&krate))
        .map(|w| format!("{rel}: `{w}` named outside privacy and server"))
        .collect()
}

/// The findings for the store's own source: homeowner tables named or homeowner seals called.
fn check_store(src: &str) -> Vec<String> {
    let with_strings = strip(src, true);
    let code = strip(src, false);
    let tables = ["privacy.contact", "privacy.stated_name"].into_iter();
    let tables = tables
        .filter(|t| names(&with_strings, t))
        .map(str::to_owned);
    let calls = HOMEOWNER_CALLS.iter().filter(|c| code.contains(*c));
    tables
        .chain(calls.map(|c| format!("calls `{c}`")))
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

/// TD-PRIV-28: no crate but `privacy` and `server` names a business-contact read, and the store
/// names no homeowner table and calls only the `BusinessTable` wrappers.
#[test]
fn td_priv_28_only_privacy_and_server_name_the_business_contact_reads() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rs_files(&root.join("app"), &mut files);
    let mut findings = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&root).expect("under the root");
        let rel = rel.to_string_lossy().into_owned();
        let krate = rel.split('/').nth(1).unwrap_or_default().to_owned();
        let src = fs::read_to_string(path).expect("readable source");
        findings.extend(check_file(&krate, &rel, &src));
    }
    assert!(
        files
            .iter()
            .any(|p| p.ends_with("app/privacy/src/business_contacts.rs"))
    );
    assert_eq!(findings, Vec::<String>::new());
    let store = fs::read_to_string(root.join("app/privacy/src/business_contacts.rs"));
    let store = store.expect("the store");
    assert!(store.contains("seal_business") && store.contains("open_business"));
    assert_eq!(check_store(&store), Vec::<String>::new());
}

/// TD-PRIV-28: each planted violation is found; prose and literals elsewhere are not.
#[test]
fn td_priv_28_the_scan_catches_each_planted_violation() {
    let call = "fn f() { read_staff_phone(db, v, p, id); }";
    assert_eq!(check_file("portals", "app/portals/src/x.rs", call).len(), 1);
    assert!(check_file("server", "app/server/src/lib.rs", call).is_empty());
    assert!(check_file("privacy", "app/privacy/src/api.rs", call).is_empty());
    let hidden = "// read_business_mailbox\n/* read_staff_phone /* x */ */ fn f() { let c = '\"'; \
                  let s = \"read_business_mailbox\"; let _ = (c, s, my_read_staff_phone); }";
    assert!(check_file("portals", "app/portals/src/x.rs", hidden).is_empty());
    let table = "const T: &str = \"SELECT name FROM privacy.contact\";";
    assert_eq!(check_store(table), vec!["privacy.contact".to_owned()]);
    let stated = "fn f() { sqlx::query!(\"DELETE FROM privacy.stated_name\"); }";
    assert_eq!(check_store(stated), vec!["privacy.stated_name".to_owned()]);
    assert_eq!(check_store("fn f() { vault.seal(c, id, p); }").len(), 1);
    assert!(check_store("fn f() { vault.seal_business(t, id, p); }").is_empty());
    assert!(check_store("// privacy.contact\nfn f() {}").is_empty());
}

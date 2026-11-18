//! D-4052 (FEE-006): the old Confirm fee writer `record_fee_at_confirm` is test-only.
//!
//! Its ledger key (`CONFIRM_FEE_ACTION`, job and business) differs from the success path's
//! `charge_key(pick)`, so a production caller would book a second entry for one charge; the
//! production writers are `book_success` and `waive_at_confirm`. This scans every Rust file under
//! `app/` (comments and string literals removed, `src/tests/` and `#[cfg(test)]` items skipped)
//! and fails when one names it anywhere but its definition in `app/payments/src/service.rs` and
//! the `pub use` in `app/payments/src/api.rs`. The rule is proved on planted sources.

use std::fs;
use std::path::{Path, PathBuf};

/// The test-only writer.
const WRITER: &str = "record_fee_at_confirm";
const DEFINED_IN: &str = "app/payments/src/service.rs";
const EXPORTED_IN: &str = "app/payments/src/api.rs";

/// `src` with `//` and `/* */` comments dropped and string and `'"'` literal bodies blanked.
fn code_only(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        let skipped = if let Some(after) = rest.strip_prefix("'\"'") {
            after
        } else if rest.starts_with("//") {
            rest.find('\n').map_or("", |n| &rest[n..])
        } else if rest.starts_with("/*") {
            rest.find("*/").map_or("", |n| &rest[n + 2..])
        } else if let Some(hashes) = raw_open(rest) {
            let close = format!("\"{}", "#".repeat(hashes));
            let body = &rest[2 + hashes..];
            body.find(&close).map_or("", |n| &body[n + close.len()..])
        } else if c == '"' {
            string_end(&rest[1..])
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        };
        out.push(' ');
        rest = skipped;
    }
    out
}

/// The number of `#` of the raw string `r#…"` opening `rest`, if it opens one.
fn raw_open(rest: &str) -> Option<usize> {
    let hashes = rest
        .strip_prefix('r')?
        .chars()
        .take_while(|&c| c == '#')
        .count();
    rest[1 + hashes..].starts_with('"').then_some(hashes)
}

/// The text after the closing quote of a string whose body starts `body`.
fn string_end(body: &str) -> &str {
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match (escaped, c) {
            (false, '\\') => escaped = true,
            (false, '"') => return &body[i + 1..],
            _ => escaped = false,
        }
    }
    ""
}

/// `code` with each `#[cfg(test)]` item dropped: to its `;`, or its braced body.
fn without_test_items(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let item = &rest[at..];
        let end = match (item.find('{'), item.find(';')) {
            (Some(open), Some(semi)) if semi < open => semi + 1,
            (Some(open), _) => close_of(item, open),
            (None, Some(semi)) => semi + 1,
            (None, None) => item.len(),
        };
        rest = &item[end..];
    }
    out.push_str(rest);
    out
}

/// The index just past the brace that closes the one at `open`.
fn close_of(text: &str, open: usize) -> usize {
    let mut depth = 0_usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 1 => return open + i + 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    text.len()
}

/// Whether the mention at `at` is the definition or the re-export, the two allowed places.
fn allowed(rel: &str, code: &str, at: usize) -> bool {
    let before = &code[..at];
    match rel {
        DEFINED_IN => before.trim_end().ends_with("pub async fn"),
        EXPORTED_IN => {
            let statement = before.rfind(';').map_or(before, |s| &before[s + 1..]);
            statement
                .trim_start()
                .starts_with("pub use crate::service::")
        }
        _ => false,
    }
}

/// The findings for the file at `rel` with source `src`.
fn check_file(rel: &str, src: &str) -> Vec<String> {
    let code = without_test_items(&code_only(src));
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(WRITER)
        .filter(|&(at, _)| {
            let before = code[..at].chars().next_back().is_some_and(ident);
            let after = code[at + WRITER.len()..].chars().next().is_some_and(ident);
            !before && !after && !allowed(rel, &code, at)
        })
        .map(|_| format!("{rel}: `{WRITER}` named outside tests"))
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

/// D-4052: no non-test source under `app/` names the test-only fee writer.
#[test]
fn no_production_code_calls_the_test_only_fee_writer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rs_files(&root.join("app"), &mut files);
    let mut findings = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy();
        if rel.contains("/src/tests/") {
            continue;
        }
        let src = fs::read_to_string(path).expect("readable source");
        findings.extend(check_file(&rel, &src));
    }
    assert!(
        files.iter().any(|p| p.ends_with(DEFINED_IN)),
        "the scan reached payments"
    );
    assert_eq!(findings, Vec::<String>::new());
}

/// The scan itself: a planted call is flagged; the definition, the re-export, doc lines, strings
/// and `#[cfg(test)]` items are not.
#[test]
fn the_scan_flags_a_planted_call() {
    let call =
        "async fn confirm() { askhava_payments::api::record_fee_at_confirm(c, s, p, f).await; }";
    let cases: [(&str, &str, usize); 8] = [
        ("app/picks/src/confirm.rs", call, 1),
        (DEFINED_IN, call, 1),
        (EXPORTED_IN, "use crate::service::record_fee_at_confirm;", 1),
        (
            DEFINED_IN,
            "#[doc(hidden)]\npub async fn record_fee_at_confirm(\n c: u8) {}",
            0,
        ),
        (
            EXPORTED_IN,
            "pub use crate::service::{\n a, record_fee_at_confirm, b,\n};",
            0,
        ),
        (
            "app/picks/src/a.rs",
            "/// test-only `record_fee_at_confirm`\nfn f() {}",
            0,
        ),
        (
            "app/picks/src/b.rs",
            "const S: &str = \"record_fee_at_confirm\";",
            0,
        ),
        (
            "app/picks/src/c.rs",
            "#[cfg(test)]\nmod t { fn f() { record_fee_at_confirm(); } }",
            0,
        ),
    ];
    for (rel, src, expected) in cases {
        assert_eq!(check_file(rel, src).len(), expected, "{rel}: {src}");
    }
}

//! Every status type is an enum, matched only exhaustively (SEC-031; TD-SEC-12, D-453, D-488).
//!
//! Runs the source scan `scripts/check_rust_rules.py` over the repository and finds nothing; plants
//! each forbidden shape (a `_ =>` arm, `matches!` and `if let` on a status enum outside its module's
//! helpers) in a scratch tree under `CARGO_TARGET_TMPDIR` and sees the scan fail on each, while the
//! same shapes inside the enum's own `impl` pass; checks that every status module (`status.rs`)
//! sets `#![deny(clippy::wildcard_enum_match_arm)]`; and that no first-party struct holds a status
//! as text (vendor adapters keep their vendor's own strings). The scan's finer cases are
//! `tests/test_check_rust_rules.py`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root, two levels above this crate.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the scan over `tree`; returns whether it passed and what it printed.
fn scan(tree: &Path) -> (bool, String) {
    let script = root().join("scripts/check_rust_rules.py");
    let out = Command::new("python3")
        .arg(script)
        .arg("--root")
        .arg(tree)
        .output()
        .expect("python3 runs the scan");
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    (out.status.success(), text)
}

/// A status module whose helpers match freely inside the enum's own `impl`.
const STATUS_RS: &str = "//! Probe statuses.\n\
pub enum ProbeStatus { Open, Done }\n\
impl ProbeStatus {\n\
    pub fn is_done(self) -> bool { matches!(self, Self::Done) }\n\
    pub fn code(self) -> &'static str { match self { Self::Open => \"open\", _ => \"done\" } }\n\
}\n";

/// Each planted shape, laid out as `cargo fmt` writes it (the scan reads arms line by line, and
/// `cargo fmt --check` keeps every arm on its own line): (file, extra source, what it is).
const PLANTS: [(&str, &str, &str); 5] = [
    (
        "service.rs",
        "fn f(s: ProbeStatus) -> u8 {\n    match s {\n        ProbeStatus::Open => 1,\n        _ => 2,\n    }\n}\n",
        "`_ =>` arm outside the module",
    ),
    (
        "service.rs",
        "fn f(s: ProbeStatus) -> bool { matches!(s, ProbeStatus::Open) }\n",
        "`matches!` outside the module",
    ),
    (
        "service.rs",
        "fn f(s: ProbeStatus) -> u8 { if let ProbeStatus::Open = s { 1 } else { 2 } }\n",
        "`if let` outside the module",
    ),
    (
        "status.rs",
        "fn g(s: ProbeStatus) -> u8 { match s {\n ProbeStatus::Open => 1,\n _ => 2 } }\n",
        "`_ =>` arm outside the impl",
    ),
    (
        "status.rs",
        "fn g(s: ProbeStatus) -> bool { matches!(s, ProbeStatus::Open) }\n",
        "`matches!` outside the impl",
    ),
];

/// Writes a fresh probe crate named `case` with `extra` appended to `file`.
fn probe_tree(case: &str, plant: Option<(&str, &str)>) -> PathBuf {
    let tree = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("status_enums_{case}"));
    let src = tree.join("app/probe/src");
    let _ = fs::remove_dir_all(&tree);
    fs::create_dir_all(&src).expect("scratch tree");
    let mut status = STATUS_RS.to_owned();
    let mut service = "//! Probe service.\nuse crate::status::ProbeStatus;\n".to_owned();
    if let Some((file, extra)) = plant {
        let target = if file == "status.rs" {
            &mut status
        } else {
            &mut service
        };
        target.push_str(extra);
    }
    fs::write(src.join("status.rs"), status).expect("status.rs");
    fs::write(src.join("service.rs"), service).expect("service.rs");
    tree
}

/// TD-SEC-12: the repository's own sources pass the scan (SEC-031).
#[test]
fn td_sec_12_the_repository_has_no_status_shortcut() {
    let (ok, text) = scan(&root());
    assert!(ok, "check_rust_rules.py found problems:\n{text}");
}

/// TD-SEC-12: a clean probe passes and each planted shape fails the scan (SEC-031).
#[test]
fn td_sec_12_the_scan_fails_on_each_planted_shape() {
    let (ok, text) = scan(&probe_tree("clean", None));
    assert!(ok, "helpers inside the enum's impl are allowed:\n{text}");
    for (i, (file, extra, what)) in PLANTS.iter().enumerate() {
        let (ok, text) = scan(&probe_tree(&format!("plant{i}"), Some((file, extra))));
        assert!(!ok, "{what} in {file} was not caught");
        assert!(text.contains("SEC-031"), "{what}: {text}");
    }
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// TD-SEC-12: every status module denies wildcard arms, and no first-party struct keeps a status
/// as text (SEC-031, D-488).
#[test]
fn td_sec_12_status_modules_deny_wildcards_and_statuses_are_enums() {
    let mut files = Vec::new();
    rust_files(&root().join("app"), &mut files);
    let mut status_modules = 0;
    for path in &files {
        let text = fs::read_to_string(path).expect("read");
        let shown = path.display();
        if path.file_name().is_some_and(|n| n == "status.rs") {
            status_modules += 1;
            let head = text.split("\nuse ").next().unwrap_or_default();
            let denied =
                head.contains("#![deny(") && head.contains("clippy::wildcard_enum_match_arm");
            assert!(denied, "{shown} must deny clippy::wildcard_enum_match_arm");
        }
        if !path.components().any(|c| c.as_os_str() == "adapters") {
            for text_type in ["status: String", "status: &str", "status: &'static str"] {
                assert!(!text.contains(text_type), "{shown} holds a status as text");
            }
        }
    }
    assert!(status_modules > 0, "no status module found");
}

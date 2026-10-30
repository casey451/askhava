"""Tests for scripts/check_rust_rules.py (source rules, BUILD_BRIEF.md section 12, SEC-031, D-479)."""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_rust_rules as rr  # noqa: E402


def write(root: Path, rel: str, body: str) -> None:
    p = root / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(body, encoding="utf-8")


def test_real_repository_passes() -> None:
    assert rr.run(REPO_ROOT) == []


def test_clean_file_passes(tmp_path: Path) -> None:
    write(tmp_path, "app/jobs/src/rules.rs", "//! rules\n/// fee\npub fn fee(x: u32) -> u32 {\n    x\n}\n")
    assert rr.run(tmp_path) == []


def test_allow_without_reason_todo_dbg_unsafe_fail(tmp_path: Path) -> None:
    write(
        tmp_path,
        "app/jobs/src/service.rs",
        "#[allow(dead_code)]\nfn a() {}\n#[allow(dead_code, reason = \"used by tests\")]\nfn b() {}\n"
        "fn c() { todo!(); }\n// TODO later\nunsafe fn d() {}\nfn e() { dbg!(1); }\n",
    )
    problems = rr.run(tmp_path)
    kinds = [p.split(": ", 1)[1] for p in problems]
    assert "#[allow] without a reason" in kinds and kinds.count("#[allow] without a reason") == 1
    assert any("todo!" in k for k in kinds) and any("TODO" in k for k in kinds)
    assert any("unsafe" in k for k in kinds) and any("dbg" in k for k in kinds)


def test_wildcard_arm_in_status_module_fails_outside_its_impl(tmp_path: Path) -> None:
    body = (
        "pub enum JobStatus { Open, Done }\n"
        "impl JobStatus {\n    pub fn is_final(&self) -> bool {\n        matches!(self, JobStatus::Done)\n    }\n}\n"
        "pub fn next(s: JobStatus) -> u8 {\n    match s {\n        JobStatus::Open => 1,\n        _ => 0,\n    }\n}\n"
    )
    write(tmp_path, "app/jobs/src/status.rs", body)
    problems = rr.run(tmp_path)
    assert len(problems) == 1 and "wildcard arm" in problems[0] and ":10:" in problems[0]


def test_non_status_module_may_use_wildcards(tmp_path: Path) -> None:
    write(tmp_path, "app/site/src/routes.rs", "fn f(x: Option<u8>) -> u8 {\n    match x {\n        Some(v) => v,\n        _ => 0,\n    }\n}\n")
    assert rr.run(tmp_path) == []


def test_status_glob_import_and_comparison_outside_status_module_fail(tmp_path: Path) -> None:
    write(tmp_path, "app/jobs/src/service.rs", "use crate::status::JobStatus::*;\nfn f(s: JobStatus) -> bool { s == JobStatus::Open }\n")
    problems = rr.run(tmp_path)
    assert any("glob import" in p for p in problems) and any("== or !=" in p for p in problems)


def test_template_escaping_cannot_be_disabled(tmp_path: Path) -> None:
    write(tmp_path, "app/site/templates/page.html", "<p>{{ text|safe }}</p>\n")
    problems = rr.run(tmp_path)
    assert len(problems) == 1 and "disables escaping" in problems[0]


def test_multiline_allow_with_reason_and_masked_phone_text_pass(tmp_path: Path) -> None:
    write(tmp_path, "app/site/src/routes.rs", '#[allow(\n    clippy::too_many_arguments,\n    reason = "axum handler"\n)]\nfn f() {}\nconst MASK: &str = "XXX-XXX-1234";\n')
    assert rr.run(tmp_path) == []


def test_status_matches_if_let_and_arms_outside_status_module_fail(tmp_path: Path) -> None:
    write(tmp_path, "app/picks/src/service.rs", "fn f(s: PickStatus) {\n    if let PickStatus::Paid = s {}\n    let _ = matches!(s, PickStatus::Free);\n    match s {\n        PickStatus::Paid => {}\n    }\n}\n")
    problems = rr.run(tmp_path)
    assert len([p for p in problems if "outside its status module" in p]) == 3


STATUS_RS = "pub enum JobStatus { Open, Done }\n"


def messages(root: Path) -> list[str]:
    return [p.split(": ", 1)[1] for p in rr.run(root)]


def planted(tmp_path: Path, body: str, rel: str = "app/jobs/src/service.rs") -> list[str]:
    write(tmp_path, "app/jobs/src/status.rs", STATUS_RS)
    write(tmp_path, rel, body)
    return messages(tmp_path)


def test_alias_and_variant_imports_fail(tmp_path: Path) -> None:
    assert any("alias import" in m for m in planted(tmp_path, "use crate::status::JobStatus as S;\n"))
    assert any("alias import" in m for m in planted(tmp_path, "use crate::status::{PickStatus, JobStatus as S};\n"))
    assert any("variant import" in m for m in planted(tmp_path, "use crate::status::JobStatus::Open;\n"))
    assert any("variant import" in m for m in planted(tmp_path, "use crate::status::JobStatus::{Open, Done};\n"))
    assert planted(tmp_path, "use crate::status::JobStatus;\n") == []


def test_let_else_and_split_matches_fail(tmp_path: Path) -> None:
    assert planted(tmp_path, "fn f(s: JobStatus) {\n    let JobStatus::Open = s else { return };\n}\n")
    split = "fn f(s: JobStatus) -> bool {\n    matches!(\n        s,\n        JobStatus::Open\n    )\n}\n"
    assert planted(tmp_path, split)
    assert planted(tmp_path, "fn f(s: JobStatus) -> bool { matches!(s, Some(1)) }\n") == []


def test_reversed_comparison_and_assert_eq_fail(tmp_path: Path) -> None:
    assert any("== or !=" in m for m in planted(tmp_path, "fn f(s: JobStatus) -> bool { JobStatus::Open == s }\n"))
    assert any("== or !=" in m for m in planted(tmp_path, "fn f(s: JobStatus) -> bool { JobStatus::Open != s }\n"))
    assert planted(tmp_path, "fn f(s: JobStatus) {\n    assert_eq!(s, JobStatus::Open);\n}\n")
    assert planted(tmp_path, "fn f(s: JobStatus) {\n    assert_ne!(\n        s,\n        JobStatus::Open\n    );\n}\n")
    assert planted(tmp_path, "fn f(a: u8) {\n    assert_eq!(a, 1);\n}\n") == []


def test_catch_all_binding_arm_fails(tmp_path: Path) -> None:
    body = "pub enum JobStatus { Open, Done }\npub fn f(s: JobStatus) -> u8 {\n    match s {\n        JobStatus::Open => 1,\n        other => 2,\n    }\n}\n"
    write(tmp_path, "app/jobs/src/status.rs", body)
    assert any("wildcard arm" in m for m in messages(tmp_path))
    write(tmp_path, "app/jobs/src/status.rs", STATUS_RS)
    outside = "fn f(s: JobStatus) -> u8 {\n    match s {\n        JobStatus::Open => 1,\n        other => 2,\n    }\n}\n"
    write(tmp_path, "app/jobs/src/service.rs", outside)
    assert any("catch-all arm" in m for m in messages(tmp_path))
    write(tmp_path, "app/jobs/src/service.rs", "fn f(x: Option<u8>) -> u8 {\n    match x {\n        Some(1) => 1,\n        other => 2,\n    }\n}\n")
    assert messages(tmp_path) == []


def test_status_enum_not_named_status_defined_in_status_rs(tmp_path: Path) -> None:
    write(tmp_path, "app/picks/src/status.rs", "pub enum Stage { Open, Done }\n")
    write(tmp_path, "app/picks/src/service.rs", "fn f(s: Stage) -> bool { s == Stage::Open }\n")
    assert any("== or !=" in m for m in messages(tmp_path))
    write(tmp_path, "app/picks/src/status.rs", "pub enum Stage { Open, Done }\nfn f(s: Stage) -> u8 {\n    match s {\n        Stage::Open => 1,\n        _ => 0,\n    }\n}\n")
    write(tmp_path, "app/picks/src/service.rs", "")
    assert any("wildcard arm" in m for m in messages(tmp_path))


def test_vendor_status_enum_file_is_not_a_status_module(tmp_path: Path) -> None:
    body = "pub enum TransferStatus { Pending }\nfn f(x: Option<u8>) -> u8 {\n    match x {\n        Some(v) => v,\n        _ => 0,\n    }\n}\n"
    write(tmp_path, "app/adapters/src/stripe.rs", body)
    assert rr.run(tmp_path) == []


def test_ui_paths_are_scanned(tmp_path: Path) -> None:
    write(tmp_path, "tests/security/ui/release_proof.rs", "fn main() { unsafe { x() } }\n")
    assert any("unsafe" in m for m in messages(tmp_path))
    write(tmp_path, "tests/security/ui/release_proof.rs", "")
    write(tmp_path, "app/jobs/src/ui/page.rs", "use JobStatus::*;\n")
    assert any("glob import" in m for m in messages(tmp_path))


def test_block_comment_todo_fails_and_unsafe_in_comment_passes(tmp_path: Path) -> None:
    write(tmp_path, "app/jobs/src/a.rs", "/* TODO later */\nfn a() {}\n")
    assert any("TODO" in m for m in messages(tmp_path))
    ok = '// unsafe { x }\n/* unsafe { y }\n   unsafe fn z */\nconst S: &str = "unsafe { w }";\nfn a() {}\n'
    write(tmp_path, "app/jobs/src/a.rs", ok)
    assert messages(tmp_path) == []


def test_template_attribute_escape_ext_and_filter_safe_fail(tmp_path: Path) -> None:
    for attr in ('escape = "none"', 'ext = "txt"'):
        write(tmp_path, "app/site/src/page.rs", f'#[derive(Template)]\n#[template(\n    path = "p.html",\n    {attr}\n)]\nstruct P;\n')
        assert any("template attribute" in m for m in messages(tmp_path)), attr
    write(tmp_path, "app/site/src/page.rs", '#[template(path = "p.html")]\nstruct P;\n')
    assert messages(tmp_path) == []
    write(tmp_path, "app/site/templates/mail.txt", "{% filter safe %}x{% endfilter %}\n")
    assert any("disables escaping" in m for m in messages(tmp_path))

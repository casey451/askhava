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


def test_trybuild_fixtures_are_skipped(tmp_path: Path) -> None:
    write(tmp_path, "tests/security/ui/release_proof.rs", "fn main() { todo!(); }\n")
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

"""scripts/npm_audit.py excuses only GHSA-fc5q-39xc-vjpf on picomatch and fails closed (D-4223, SEC-021).

Every report here is planted JSON in `npm audit --json` shape (auditReportVersion 2); no test runs npm
or reaches the network. The first fixture is the real nine-package shape of November 17, 2026.
"""

from __future__ import annotations

import copy
import importlib.util
import json
import sys
from datetime import date
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parent.parent
_SPEC = importlib.util.spec_from_file_location("npm_audit", ROOT / "scripts" / "npm_audit.py")
assert _SPEC and _SPEC.loader
audit = importlib.util.module_from_spec(_SPEC)
sys.modules["npm_audit"] = audit
_SPEC.loader.exec_module(audit)

EXCUSED = "GHSA-fc5q-39xc-vjpf"
OTHER = "GHSA-2222-3333-4444"
TODAY = date(2026, 11, 17)  # every judgement is pinned, so the review date never ages these tests


def advisory(name: str, ghsa: str = EXCUSED, severity: str = "high") -> dict[str, Any]:
    """One advisory dict as npm prints it in a `via` list."""
    return {"source": 1, "name": name, "dependency": name, "title": f"planted advisory on {name}",
            "url": f"https://github.com/advisories/{ghsa}", "severity": severity, "range": "*"}


def vuln(name: str, via: list[Any]) -> dict[str, Any]:
    """One vulnerable-package entry."""
    return {"name": name, "severity": "high", "isDirect": False, "via": via, "effects": [], "range": "*",
            "nodes": [f"node_modules/{name}"], "fixAvailable": False}


def report(vulns: dict[str, Any]) -> str:
    """A whole report around a vulnerabilities map, with a matching count."""
    return json.dumps({"auditReportVersion": 2, "vulnerabilities": vulns,
                       "metadata": {"vulnerabilities": {"high": len(vulns), "total": len(vulns)}}})


def real_chain() -> dict[str, Any]:
    """The nine packages npm reported on main's package-lock.json on November 17, 2026."""
    tse = "@typescript-eslint/"
    return {
        "picomatch": vuln("picomatch", [advisory("picomatch")]),
        "fdir": vuln("fdir", ["picomatch"]),
        "tinyglobby": vuln("tinyglobby", ["fdir", "picomatch"]),
        f"{tse}typescript-estree": vuln(f"{tse}typescript-estree", ["tinyglobby"]),
        f"{tse}parser": vuln(f"{tse}parser", [f"{tse}typescript-estree"]),
        f"{tse}utils": vuln(f"{tse}utils", [f"{tse}typescript-estree"]),
        f"{tse}type-utils": vuln(f"{tse}type-utils", [f"{tse}typescript-estree", f"{tse}utils"]),
        f"{tse}eslint-plugin": vuln(f"{tse}eslint-plugin", [f"{tse}parser", f"{tse}type-utils", f"{tse}utils"]),
        "typescript-eslint": vuln("typescript-eslint", [f"{tse}eslint-plugin", f"{tse}parser",
                                                         f"{tse}typescript-estree", f"{tse}utils"]),
    }


def test_the_real_nine_package_chain_passes_and_names_the_excused_advisory(capsys: pytest.CaptureFixture[str]) -> None:
    assert audit.report(report(real_chain()), today=TODAY) == 0
    out = capsys.readouterr().out
    assert f"excused {EXCUSED} on picomatch (D-4223)" in out and out.rstrip().endswith("npm audit: ok")


def test_a_second_unrelated_advisory_fails(capsys: pytest.CaptureFixture[str]) -> None:
    vulns = real_chain()
    vulns["left-pad"] = vuln("left-pad", [advisory("left-pad", OTHER, "low")])
    assert audit.report(report(vulns), today=TODAY) == 1
    assert f"left-pad: {OTHER} on left-pad (low)" in capsys.readouterr().out


def test_a_package_whose_via_mixes_the_excused_advisory_and_another_fails(capsys: pytest.CaptureFixture[str]) -> None:
    vulns = real_chain()
    vulns["picomatch"]["via"].append(advisory("picomatch", OTHER))
    assert audit.report(report(vulns), today=TODAY) == 1
    out = capsys.readouterr().out
    assert f"picomatch: {OTHER} on picomatch" in out and f"typescript-eslint: {OTHER} on picomatch" in out


def test_a_string_via_that_reaches_another_advisory_fails() -> None:
    vulns = real_chain()
    vulns["evil"] = vuln("evil", [advisory("evil", OTHER)])
    vulns["fdir"]["via"].append("evil")
    problems, _, _ = audit.judge(vulns, audit.EXCEPTIONS, TODAY)
    assert "fdir: GHSA-2222-3333-4444 on evil (high): planted advisory on evil" in problems


def test_the_excused_advisory_on_a_different_package_fails() -> None:
    vulns = {"micromatch": vuln("micromatch", [advisory("micromatch")])}
    assert audit.report(report(vulns), today=TODAY) == 1
    renamed = {"picomatch": vuln("picomatch", [advisory("micromatch")])}
    assert audit.report(report(renamed), today=TODAY) == 1, "the advisory's own name must be the excepted package too"


def test_cycles_terminate_and_a_cycle_with_no_advisory_fails() -> None:
    vulns = real_chain()
    vulns["picomatch"]["via"].append("typescript-eslint")
    assert audit.judge(vulns, audit.EXCEPTIONS, TODAY)[0] == []
    loop = {"a": vuln("a", ["b"]), "b": vuln("b", ["a"])}
    problems, _, _ = audit.judge(loop, audit.EXCEPTIONS, TODAY)
    assert problems == ["a: reported vulnerable but traces to no advisory",
                        "b: reported vulnerable but traces to no advisory"]


@pytest.mark.parametrize("text", [
    "npm ERR! network request failed",
    "",
    "[]",
    json.dumps({"error": {"code": "ENOAUDIT", "summary": "audit endpoint unreachable"}}),
    json.dumps({"auditReportVersion": 1, "advisories": {}}),
    json.dumps({"auditReportVersion": 2, "vulnerabilities": {}}),
    json.dumps({"auditReportVersion": 2, "vulnerabilities": {},
                "metadata": {"vulnerabilities": {"total": 3}}}),
    json.dumps({"auditReportVersion": 2, "vulnerabilities": ["picomatch"],
                "metadata": {"vulnerabilities": {"total": 1}}}),
    report({"fdir": vuln("fdir", ["ghost"])}),
    report({"fdir": vuln("fdir", "picomatch")}),
    report({"fdir": vuln("fdir", [7])}),
    report({"x": vuln("x", [{"name": "x", "url": "https://example.invalid/no-id", "severity": "low"}])}),
])
def test_anything_it_cannot_judge_fails_closed(text: str) -> None:
    assert audit.report(text, today=TODAY) == 1


def test_an_empty_report_passes_and_notes_the_stale_exception(capsys: pytest.CaptureFixture[str]) -> None:
    assert audit.report(report({}), today=TODAY) == 0
    out = capsys.readouterr().out
    assert f"note: {EXCUSED} on picomatch (D-4223) is no longer reported" in out
    assert "npm audit: ok" in out


@pytest.mark.parametrize("field", audit.EXCEPTION_FIELDS)
def test_an_exception_missing_a_field_fails(field: str) -> None:
    broken = copy.deepcopy(audit.EXCEPTIONS[0])
    broken[field] = " "
    assert audit.report(report(real_chain()), (broken,), TODAY) == 1


def test_an_exception_with_a_malformed_id_or_decision_fails() -> None:
    for key, value in (("ghsa", "CVE-2026-0001"), ("decision", "soon"), ("review_by", "next month"),
                       ("dependents", ("fdir",)), ("dependents", "picomatch")):
        broken = {**audit.EXCEPTIONS[0], key: value}
        assert audit.report(report(real_chain()), (broken,), TODAY) == 1


def test_the_exception_list_is_exactly_the_one_d4223_names() -> None:
    assert [(e["ghsa"], e["package"], e["decision"], e["review_by"]) for e in audit.EXCEPTIONS] == [
        (EXCUSED, "picomatch", "D-4223", "2026-12-17")]
    assert sorted(audit.EXCEPTIONS[0]["dependents"]) == sorted(real_chain()), "exactly today's nine packages"


def test_main_reads_a_saved_report_and_fails_on_a_missing_file(tmp_path: Path) -> None:
    saved = tmp_path / "audit.json"
    saved.write_text(report(real_chain()), encoding="utf-8")
    assert audit.main(["--input", str(saved)], TODAY) == 0
    assert audit.main(["--input", str(tmp_path / "absent.json")], TODAY) == 1


def test_main_fails_when_npm_cannot_run(monkeypatch: pytest.MonkeyPatch) -> None:
    def no_npm(*_args: Any, **_kwargs: Any) -> Any:
        raise FileNotFoundError("npm")
    monkeypatch.setattr(audit.subprocess, "run", no_npm)
    assert audit.main([], TODAY) == 1


def test_the_advisory_must_sit_on_its_own_packages_entry() -> None:
    """The excused advisory held by fdir's entry (not picomatch's) is not excused, even reached from picomatch."""
    moved = {"picomatch": vuln("picomatch", ["fdir"]), "fdir": vuln("fdir", [advisory("picomatch")])}
    assert audit.report(report(moved), today=TODAY) == 1


def test_an_error_key_fails_even_on_an_otherwise_valid_report() -> None:
    valid = json.loads(report(real_chain()))
    assert audit.report(json.dumps(valid), today=TODAY) == 0
    assert audit.report(json.dumps({**valid, "error": {"code": "EAUDITPARTIAL"}}), today=TODAY) == 1


def test_a_version_one_report_fails_even_with_a_valid_map_and_count() -> None:
    old = {**json.loads(report(real_chain())), "auditReportVersion": 1}
    assert audit.report(json.dumps(old), today=TODAY) == 1


def test_a_new_package_reaching_the_excused_advisory_fails(capsys: pytest.CaptureFixture[str]) -> None:
    vulns = real_chain()
    vulns["globby-next"] = vuln("globby-next", ["picomatch"])
    assert audit.report(report(vulns), today=TODAY) == 1
    assert f"globby-next: {EXCUSED} on picomatch" in capsys.readouterr().out


def test_only_a_github_advisory_url_ending_in_the_id_counts() -> None:
    for url in (f"https://example.invalid/{EXCUSED}/{OTHER}", f"https://github.com/advisories/{EXCUSED}/{OTHER}",
                f"https://github.com/advisories/{OTHER}?see={EXCUSED}", f"https://github.com/advisories/{EXCUSED}x"):
        vulns = {"picomatch": vuln("picomatch", [{**advisory("picomatch"), "url": url}])}
        assert audit.report(report(vulns), today=TODAY) == 1, url


WARNING = ("npm audit: WARNING: exception GHSA-fc5q-39xc-vjpf (D-4223) passed its review date 2026-12-17: "
           "re-check for a patched picomatch and remove the exception or move the date by a plan change")


def test_after_the_review_date_every_run_warns_but_still_judges(capsys: pytest.CaptureFixture[str]) -> None:
    """A date never turns main or the nightly red (no commit to revert): it warns, and the report decides."""
    assert audit.report(report(real_chain()), today=date(2026, 12, 17)) == 0
    assert WARNING not in capsys.readouterr().out, "the review date itself is not yet past"
    assert audit.report(report(real_chain()), today=date(2026, 12, 18)) == 0
    out = capsys.readouterr().out
    assert WARNING in out and out.rstrip().endswith("npm audit: ok")
    vulns = real_chain()
    vulns["left-pad"] = vuln("left-pad", [advisory("left-pad", OTHER, "low")])
    assert audit.report(report(vulns), today=date(2026, 12, 18)) == 1, "past the date, other advisories still fail"


def test_main_passes_its_date_through_to_the_judgement(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    saved = tmp_path / "audit.json"
    saved.write_text(report(real_chain()), encoding="utf-8")
    assert audit.main(["--input", str(saved)], date(2026, 12, 18)) == 0
    assert WARNING in capsys.readouterr().out


def test_check_exceptions_refuses_dependents_that_omit_the_package_itself() -> None:
    broken = {**audit.EXCEPTIONS[0], "dependents": ("fdir", "tinyglobby")}
    with pytest.raises(audit.AuditError, match="dependents list naming its package"):
        audit.check_exceptions((broken,))
    audit.check_exceptions(audit.EXCEPTIONS)

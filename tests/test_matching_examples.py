"""Tests for the matching worked examples check in scripts/check_catalog.py (T003).

D-110: docs/catalog/matching_examples.yaml holds M01 to M16 (docs/test-design.md section 7.2),
the oracle for who receives a job card (CARD-001, CARD-002, CARD-003, LIC-012, LIC-013,
LIC-014, LIC-016, LIC-017, LIC-021, LIC-022, LIC-035). Each test plants one fault in a copy of
the real catalog and checks that `check_catalog.py` fails on it.
"""

from __future__ import annotations

import importlib.util
import re
import shutil
import sys
from pathlib import Path
from types import ModuleType

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = REPO_ROOT / "scripts"
CATALOG = Path("docs/catalog")
MATCHING = "matching_examples.yaml"


def load_script() -> ModuleType:
    sys.path.insert(0, str(SCRIPTS))
    spec = importlib.util.spec_from_file_location("check_catalog", SCRIPTS / "check_catalog.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_catalog"] = module
    spec.loader.exec_module(module)
    return module


check_catalog = load_script()


def errors_after(tmp_path: Path, old: str, new: str) -> list[str]:
    shutil.copytree(REPO_ROOT / CATALOG, tmp_path / CATALOG)
    path = tmp_path / CATALOG / MATCHING
    text = path.read_text(encoding="utf-8")
    assert text.count(old) == 1, old
    path.write_text(text.replace(old, new), encoding="utf-8")
    errors: list[str] = check_catalog.run(tmp_path).errors
    return errors


def test_real_examples_pass(capsys: pytest.CaptureFixture[str]) -> None:
    assert check_catalog.main(["--root", str(REPO_ROOT)]) == 0
    out = capsys.readouterr().out
    assert "Matching examples: 16" in out
    assert "ok   M15 matching: no_card" in out


def test_ids_match_the_test_design_table() -> None:
    section = (REPO_ROOT / "docs/test-design.md").read_text(encoding="utf-8")
    rows = re.findall(r"^\| (M\d{2}) \|", section, flags=re.MULTILINE)
    assert tuple(rows) == check_catalog.MATCHING_IDS


def test_missing_example_fails(tmp_path: Path) -> None:
    errors = errors_after(tmp_path, "id: M07", "id: M17")
    assert "M07: matching example missing (D-110)" in errors


def test_duplicated_example_fails(tmp_path: Path) -> None:
    errors = errors_after(tmp_path, "id: M08", "id: M07")
    assert "M07: duplicate matching example id" in errors


def test_missing_file_fails(tmp_path: Path) -> None:
    shutil.copytree(REPO_ROOT / CATALOG, tmp_path / CATALOG)
    (tmp_path / CATALOG / MATCHING).unlink()
    assert "missing docs/catalog/matching_examples.yaml" in check_catalog.run(tmp_path).errors


M12_EXPECTED = "expected: {outcome: card, rules: [LIC-016], map_covers: true}"
M09_BUSINESS = "handyman: true, city_list: true, ticked: [camera install]"


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        ("id: M09", "id: m9", "m9: matching example id must be M and two digits"),
        (M12_EXPECTED, M12_EXPECTED.replace("expected", "expect"), "M12: expected must be a mapping"),
        (M12_EXPECTED, "expected: {outcome: card, rules: [LIC-016]}", "M12: expected is missing map_covers"),
        ("outcome: refused_at_intake", "outcome: maybe", "M14: expected.outcome must be one of"),
        ("rules: [LIC-016]", "rules: [licensing]", "M12: expected.rules must be requirement IDs"),
        ("candidates: [tank pumping]", "candidates: []", "M12: job.candidates must not be empty"),
        ("candidates: [tank pumping]", "candidates: tank pumping", "M12: job.candidates must be a list"),
        (M09_BUSINESS, M09_BUSINESS.replace("true", "maybe", 1), "M09: business.handyman must be true or false"),
        ("rules: [LIC-016], map_covers: true", "rules: [LIC-016], map_covers: yes please", "M12: expected.map_covers"),
        ("table: septic pumping", "table: ''", "M12: job.table must be a name"),
        ("condition: quote must be under $1,000", "condition: ''", "M05: expected.condition must be text"),
    ],
)
def test_malformed_example_fails(tmp_path: Path, old: str, new: str, expected: str) -> None:
    errors = errors_after(tmp_path, old, new)
    assert any(e.startswith(expected) for e in errors), errors


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        ("candidates: [tank pumping]", "candidates: [tank cleaning]", "'tank cleaning' is not a septic_pumping job type"),
        ("ticked: [tank pumping]", "ticked: [drain clearing]", "'drain clearing' is not a septic_pumping job type"),
        ("roc: [R-6]", "roc: [R-99]", "business.roc 'R-99' is not in the license map"),
        ("agencies: [ADEQ septage-hauler vehicle license]", "agencies: [ADEQ]", "business.agencies 'ADEQ'"),
        ("category: local_moving, candidates: [in-state move]", "category: long_moving, candidates: [in-state move]",
         "job.category 'long_moving' is not a catalog category"),
    ],
)
def test_names_the_catalog_lacks_fail(tmp_path: Path, old: str, new: str, expected: str) -> None:
    errors = errors_after(tmp_path, old, new)
    assert any(expected in e for e in errors), errors


M02_EXPECTED = "expected: {outcome: no_card, rules: [LIC-035], map_covers: false}"


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        (M02_EXPECTED, M02_EXPECTED.replace("false", "true"), "M02: map_covers is True, the license map gives False"),
        (M02_EXPECTED, M02_EXPECTED.replace("no_card", "card"), "M02: a card the license map does not cover"),
        ("ticked: [pump repair]", "ticked: [filter repair]", "M08: a card to a business that ticked none"),
        ("expected: {outcome: no_card, rules: [CARD-001], map_covers: true}",
         "expected: {outcome: card, rules: [CARD-001], map_covers: true}", "M16: a card to a business that does not serve"),
        ("expected: {outcome: no_card, rules: [LIC-022], map_covers: null}",
         "expected: {outcome: card, rules: [LIC-022], map_covers: null, condition: under $1,000}",
         "M04: a card to a handyman for work handymen never get"),
        ("expected: {outcome: no_card, rules: [LIC-001], map_covers: null}",
         "expected: {outcome: card, rules: [LIC-001], map_covers: null, condition: under $1,000}",
         "M07: a card to a handyman for work handymen never get"),
        (", condition: quote must be under $1,000}", "}", "M05: a handyman card without its $1,000 condition"),
        ("expected: {outcome: no_card, rules: [LIC-013], map_covers: true}",
         "expected: {outcome: card, rules: [LIC-013], map_covers: true}", "M10: a card without the category's agency"),
        ("outcome: refused_at_intake", "outcome: no_card", "M14: refused_at_intake exactly when"),
        ("city_list: true, ticked: [in-town move, in-state move], towns: [Lake Havasu City]}\n    expected: {outcome: card",
         "city_list: false, ticked: [in-town move, in-state move], towns: [Lake Havasu City]}\n    expected: {outcome: card",
         "M13: a no-license card to a business not on the city list"),
    ],
)
def test_outcome_breaking_a_card_rule_fails(tmp_path: Path, old: str, new: str, expected: str) -> None:
    errors = errors_after(tmp_path, old, new)
    assert any(e.startswith(expected) for e in errors), errors


def test_non_list_and_non_mapping_examples_fail() -> None:
    assert check_catalog.check_matching({"examples": "M01"}, {}, {}).errors == [
        "matching_examples.yaml: examples must be a list (D-110)"
    ]
    report = check_catalog.check_matching({"examples": ["M01"]}, {}, {})
    assert "matching example is not a mapping" in report.errors

"""Tests for the per-job-type license scope in scripts/check_catalog.py (T002).

CARD-001, LIC-001, LIC-035 (D-058, D-392, D-504): every job type of a licensed category has a
license scope in docs/catalog/license_map.yaml, and a subclassification receives only job
types within its scope. Each test plants one fault in a copy of the real catalog.
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path
from types import ModuleType

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = REPO_ROOT / "scripts"
CATALOG = Path("docs/catalog")


def load_script() -> ModuleType:
    sys.path.insert(0, str(SCRIPTS))
    spec = importlib.util.spec_from_file_location("check_catalog", SCRIPTS / "check_catalog.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_catalog"] = module
    spec.loader.exec_module(module)
    return module


check_catalog = load_script()


def errors_after(tmp_path: Path, name: str, old: str, new: str) -> list[str]:
    shutil.copytree(REPO_ROOT / CATALOG, tmp_path / CATALOG)
    path = tmp_path / CATALOG / name
    text = path.read_text(encoding="utf-8")
    assert text.count(old) == 1, old
    path.write_text(text.replace(old, new), encoding="utf-8")
    errors: list[str] = check_catalog.run(tmp_path).errors
    return errors


def test_real_catalog_passes() -> None:
    assert check_catalog.run(REPO_ROOT).errors == []


def test_planted_job_type_with_no_license_scope_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "categories.yaml",
        "job_types: [leak repair, toilet repair, drain clearing, faucet replacement]",
        "job_types: [leak repair, toilet repair, drain clearing, faucet replacement, sump pump install]",
    )
    assert any("'sump pump install' has no license scope" in e for e in errors), errors


def test_planted_job_type_no_license_covers_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "categories.yaml",
        "job_types: [general pest service, scorpions, termites, pre-emergent weed control]",
        "job_types: [general pest service, scorpions, termites, pre-emergent weed control, bee removal]",
    )
    assert any("pest_control job type 'bee removal' has no license scope" in e for e in errors), errors


def test_drains_subclass_given_non_drain_job_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "plumbing_repair: {only: [drain clearing]}",
        "plumbing_repair: {only: [drain clearing, leak repair]}",
    )
    expected = (
        "subclassification R-37R Sewers, Drains and Pipe Laying gets plumbing_repair "
        "job type 'leak repair', outside its scope"
    )
    assert any(expected in e for e in errors), errors


def test_drains_subclass_given_every_plumbing_job_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "plumbing_repair: {only: [drain clearing]}",
        "plumbing_repair: {all: true}",
    )
    assert any("Drains and Pipe Laying gets plumbing_repair job type 'toilet repair'" in e for e in errors)


def test_cooling_subclass_given_heating_job_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "hvac_repair: {only: [AC not cooling, thermostat, duct repair]}",
        "hvac_repair: {only: [AC not cooling, thermostat, duct repair, furnace repair]}",
    )
    expected = (
        "subclassification R-39R Air Conditioning and Refrigeration gets hvac_repair "
        "job type 'furnace repair', outside its scope ['cooling']"
    )
    assert any(expected in e for e in errors), errors


def test_plumbing_subclass_given_gas_line_work_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "plumbing_repair: {all: true, also: [water heater replacement]}",
        "plumbing_repair: {all: true, also: [water heater replacement, gas line work]}",
    )
    assert any("R-37R Plumbing gets plumbing_repair job type 'gas line work'" in e for e in errors)


def test_subclass_without_scope_fails(tmp_path: Path) -> None:
    errors = errors_after(tmp_path, "license_map.yaml", "    scope: [drain]\n", "")
    assert any("subclassification R-37R Sewers, Drains and Pipe Laying has no scope" in e for e in errors)


def test_scope_tag_for_unknown_job_type_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "    furnace repair: [heating]\n",
        "    furnace repair: [heating]\n    boiler repair: [heating]\n",
    )
    assert any("scope_tags hvac_repair has no job type 'boiler repair'" in e for e in errors)


def test_subclassed_category_without_scope_tags_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "  hvac_replacement:\n    full system replacement: [cooling]\n    condenser replacement: [cooling]\n",
        "",
    )
    assert any("category hvac_replacement has no scope_tags" in e for e in errors), errors


def test_scope_tags_for_unknown_category_fails(tmp_path: Path) -> None:
    errors = errors_after(tmp_path, "license_map.yaml", "scope_tags:\n", "scope_tags:\n  boilers: {}\n")
    assert any("scope_tags names unknown category 'boilers'" in e for e in errors)


def test_main_exits_one_on_a_scope_fault(tmp_path: Path) -> None:
    errors_after(
        tmp_path,
        "license_map.yaml",
        "plumbing_repair: {only: [drain clearing]}",
        "plumbing_repair: {only: [drain clearing, faucet replacement]}",
    )
    assert check_catalog.main(["--root", str(tmp_path)]) == 1


def test_gas_piping_subclass_given_gas_pool_heater_fails(tmp_path: Path) -> None:
    errors = errors_after(
        tmp_path,
        "license_map.yaml",
        "plumbing_repair: {only: [gas line work]}",
        "plumbing_repair: {only: [gas line work]}\n      pool_repair: {only: [gas pool heater]}",
    )
    assert any("R-37R Gas Piping gets pool_repair job type 'gas pool heater'" in e for e in errors), errors

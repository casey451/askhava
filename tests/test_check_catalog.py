"""Tests for scripts/check_catalog.py (the real catalog plus fake broken copies)."""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path
from types import ModuleType

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = REPO_ROOT / "scripts"
CATALOG = Path("docs/catalog")


def load_script() -> ModuleType:
    sys.path.insert(0, str(SCRIPTS))
    spec = importlib.util.spec_from_file_location(
        "check_catalog", SCRIPTS / "check_catalog.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_catalog"] = module
    spec.loader.exec_module(module)
    return module


check_catalog = load_script()


def copy_catalog(tmp_path: Path) -> Path:
    shutil.copytree(REPO_ROOT / CATALOG, tmp_path / CATALOG)
    return tmp_path


def edit(root: Path, name: str, old: str, new: str) -> None:
    path = root / CATALOG / name
    text = path.read_text(encoding="utf-8")
    assert old in text, old
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def errors_after(tmp_path: Path, name: str, old: str, new: str) -> list[str]:
    root = copy_catalog(tmp_path)
    edit(root, name, old, new)
    errors: list[str] = check_catalog.run(root).errors
    return errors


def test_real_catalog_passes(capsys: pytest.CaptureFixture[str]) -> None:
    assert check_catalog.main(["--root", str(REPO_ROOT)]) == 0
    out = capsys.readouterr().out
    assert "Errors: 0" in out
    assert "ok   W11 pool_cleaning: basis 600.00 -> charged $75" in out


def test_pool_example_is_the_four_month_case() -> None:
    cat = {"recurring": True}
    basis = check_catalog.fee_basis(
        {"type": "fixed", "amount": "150.00", "period": "monthly"}, cat
    )
    assert basis == 600
    assert check_catalog.tier_fee(basis) == 75


@pytest.mark.parametrize(
    ("basis", "fee"),
    [("0.00", 25), ("499.99", 25), ("500.00", 75), ("1500.00", 75), ("1500.01", 199)],
)
def test_tier_edges(basis: str, fee: int) -> None:
    assert check_catalog.tier_fee(check_catalog.money(basis)) == fee


@pytest.mark.parametrize(
    ("name", "old", "new", "expected"),
    [
        (
            "worked_examples.yaml",
            'fee_basis: "600.00", fee: 75',
            'fee_basis: "300.00", fee: 25',
            "W11: expected",
        ),
        (
            "worked_examples.yaml",
            'fee_basis: "550.00", fee: 75',
            'fee_basis: "550.00", fee: 25',
            "W08: expected",
        ),
        (
            "worked_examples.yaml",
            "recurring_months: 4",
            "recurring_months: 2",
            "fee_rules.recurring_months",
        ),
        (
            "worked_examples.yaml",
            "outcome: refused, fee_basis: null, fee: 0, waived: 0}",
            'outcome: charged, fee_basis: "1000.00", fee: 75, waived: 0}',
            "W26: expected",
        ),
        (
            "worked_examples.yaml",
            'amount: "180.00"',
            "amount: 180.00",
            "W01: money must be",
        ),
        (
            "worked_examples.yaml",
            "category: hvac_repair",
            "category: garage_doors",
            "off at launch",
        ),
        (
            "worked_examples.yaml",
            "category: plumbing_repair",
            "category: no_such_trade",
            "unknown category",
        ),
        ("worked_examples.yaml", "id: W02", "id: W01", "duplicate example id"),
        (
            "worked_examples.yaml",
            "# Ask Hava worked",
            "# Recurring counts as 2 months. Ask Hava",
            "old recurring rule",
        ),
        (
            "worked_examples.yaml",
            "# Ask Hava worked",
            "# A $149 fee. Ask Hava",
            "old fee",
        ),
        (
            "categories.yaml",
            "# Ask Hava category",
            "# Havasu Home Quotes category",
            "old brand",
        ),
        (
            "categories.yaml",
            "# Ask Hava category",
            "# Our commission. Ask Hava category",
            "banned wording",
        ),
        (
            "categories.yaml",
            'enabled: false\n    launch_open: false\n    off_reason: "Off at launch (changes',
            'enabled: true\n    launch_open: false\n    off_reason: "Off at launch (changes',
            "garage_doors: must be listed and off",
        ),
        ("categories.yaml", "  - towing\n", "", "not_offered_at_launch"),
        (
            "categories.yaml",
            "    trip_fee_tier: 25\n    after_hours_emergency: false\n    job_types: [full system",
            "    trip_fee_tier: 199\n    after_hours_emergency: false\n    job_types: [full system",
            "trip_fee_tier must be one of (25, 75)",
        ),
        (
            "categories.yaml",
            "license: {kind: roc, classes: [R-42]}",
            "license: {kind: maybe}",
            "unknown license kind",
        ),
        (
            "categories.yaml",
            "    trip_fee_tier: 25\n    after_hours_emergency: true\n    job_types: [outlet repair",
            "    after_hours_emergency: true\n    job_types: [outlet repair",
            "missing trip_fee_tier",
        ),
    ],
)
def test_broken_catalog_fails(
    tmp_path: Path, name: str, old: str, new: str, expected: str
) -> None:
    errors = errors_after(tmp_path, name, old, new)
    assert any(expected in e for e in errors), errors


@pytest.mark.parametrize(
    ("example", "expected"),
    [
        (
            {"quote": {"type": "range", "low": "9.00", "high": "1.00"}},
            "range low is above",
        ),
        ({"quote": {"type": "per_day", "amount": "1.00"}}, "unknown quote type"),
        (
            {"quote": {"type": "hourly", "rate": "50.00", "hours_low": "5", "hours_high": "2"}},
            "hours_low is above",
        ),
        (
            {"quote": {"type": "fixed", "amount": "1.00", "period": "monthly"}},
            "has a period",
        ),
        ({"situation": "friend"}, "unknown situation"),
        (
            {"business": "handyman_exemption", "category": "hvac_repair"},
            "refuses handymen",
        ),
    ],
)
def test_bad_example_fields_fail(example: dict[str, object], expected: str) -> None:
    root_data = check_catalog.load(REPO_ROOT, check_catalog.CATEGORIES_FILE)
    cats, _ = check_catalog.check_categories(root_data)
    base: dict[str, object] = {
        "id": "X01",
        "category": "plumbing_repair",
        "business": "qualified",
        "situation": "new_customer",
        "quote": {"type": "fixed", "amount": "100.00"},
        "expected": {
            "outcome": "charged",
            "fee_basis": "100.00",
            "fee": 25,
            "waived": 0,
        },
    }
    assert check_catalog.check_example(base, cats) == ""
    assert expected in check_catalog.check_example({**base, **example}, cats)


def test_unknown_recurring_period_fails() -> None:
    with pytest.raises(ValueError, match="unknown period"):
        check_catalog.fee_basis(
            {"type": "fixed", "amount": "1.00", "period": "daily"}, {"recurring": True}
        )


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        (
            "    recurring: false\n    trip_fee_tier: 25\n    after_hours_emergency: false\n    job_types: [full system",
            "    recurring: no-ish\n    trip_fee_tier: 25\n    after_hours_emergency: false\n    job_types: [full system",
            "recurring must be true or false",
        ),
        (
            '    off_reason: "Off at launch (changes file 7.1 item 4)."\n',
            "",
            "off without",
        ),
        ("  - id: hvac_replacement", "  - id: hvac_repair", "duplicate id"),
    ],
)
def test_broken_category_fails(
    tmp_path: Path, old: str, new: str, expected: str
) -> None:
    errors = errors_after(tmp_path, "categories.yaml", old, new)
    assert any(expected in e for e in errors), errors


def test_non_mapping_file_fails(tmp_path: Path) -> None:
    root = copy_catalog(tmp_path)
    (root / check_catalog.CATEGORIES_FILE).write_text(
        "- just a list\n", encoding="utf-8"
    )
    with pytest.raises(TypeError, match="top level must be a mapping"):
        check_catalog.run(root)


def test_one_time_job_in_recurring_category_is_face_value() -> None:
    cats, _ = check_catalog.check_categories(
        check_catalog.load(REPO_ROOT, check_catalog.CATEGORIES_FILE)
    )
    example: dict[str, object] = {
        "id": "X02",
        "category": "house_cleaning",
        "job_type": "move-out cleaning",
        "business": "qualified",
        "situation": "new_customer",
        "quote": {"type": "fixed", "amount": "300.00"},
        "expected": {
            "outcome": "charged",
            "fee_basis": "300.00",
            "fee": 25,
            "waived": 0,
        },
    }
    assert check_catalog.check_example(example, cats) == ""
    for job in (None, "regular cleaning"):
        bad = {**example, "job_type": job}
        assert "must name a one-time job_type" in check_catalog.check_example(bad, cats)


@pytest.mark.parametrize(
    ("name", "old", "new", "expected"),
    [
        (
            "worked_examples.yaml",
            'fee_basis: "1500.00", fee: 75, waived: 0}\n\n  - id: W31',
            'fee_basis: "1500.00", fee: 199, waived: 0}\n\n  - id: W31',
            "W30: expected",
        ),
        (
            "worked_examples.yaml",
            "    job_type: move-out cleaning\n",
            "",
            "W32: recurring category quote with no period",
        ),
        (
            "worked_examples.yaml",
            'basis_rounding: "round down',
            'basis_rounding: "round up',
            "fee_rules.basis_rounding",
        ),
        (
            "worked_examples.yaml",
            'weekly: "x 52 / 12"',
            'weekly: "x 4"',
            "fee_rules.monthly_conversion",
        ),
        (
            "categories.yaml",
            "    advertised: false\n",
            "",
            "pest_control: must be listed",
        ),
        (
            "categories.yaml",
            "    one_time_job_types: [termites]\n",
            "    one_time_job_types: [bed bugs]\n",
            "one_time_job_types not in job_types",
        ),
        (
            "categories.yaml",
            "    job_types: [drywall patch,",
            "    one_time_job_types: [drywall patch]\n    job_types: [drywall patch,",
            "one_time_job_types in a non-recurring category",
        ),
    ],
)
def test_review_fixes_are_enforced(
    tmp_path: Path, name: str, old: str, new: str, expected: str
) -> None:
    errors = errors_after(tmp_path, name, old, new)
    assert any(expected in e for e in errors), errors


def test_missing_files_fail(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    assert check_catalog.main(["--root", str(tmp_path)]) == 1
    assert "missing docs/catalog/categories.yaml" in capsys.readouterr().err


@pytest.mark.parametrize(
    "phrase",
    [
        "Our pros fix leaks",
        "We'll send a plumber",
        "Book a repair",
        "Hire through Ask Hava",
        "Job assigned",
        "Ask Hava service provider",
        "nuestros profesionales",
    ],
)
def test_not_a_contractor_wording_fails(tmp_path: Path, phrase: str) -> None:
    """WORD-022 (D-360): the catalog check uses the shared not-a-contractor list."""
    errors = errors_after(
        tmp_path,
        "categories.yaml",
        '"What is leaking or not working?"',
        f'"What is leaking or not working? {phrase}"',
    )
    expected = "implies Ask Hava is a contractor (WORD-022)"
    assert any(expected in e for e in errors), errors


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        (
            "    licensed_only_decision: D-402\n",
            "",
            "pest_control: on without licensed_only_decision",
        ),
        (
            "    license: {kind: state_agency, agency: AZ Department of Agriculture (pest management)}",
            "    license: {kind: none}",
            "pest_control: must be for Department licensees only",
        ),
        (
            "    fee_free_job_types: [buyer inspection, ",
            "    fee_free_job_types: [",
            "home_inspection: charges a sale-related inspection",
        ),
        (
            "    job_types: [weekly yard pickup]",
            "    job_types: [weekly yard pickup, home warranty]",
            "warranty or protection plan (LIC-042)",
        ),
    ],
)
def test_legal_holds_are_enforced(tmp_path: Path, old: str, new: str, expected: str) -> None:
    """LIC-012, LIC-015, LIC-042 (D-402, D-403): pest licensed-only, sale inspections
    fee-free, no warranties."""
    errors = errors_after(tmp_path, "categories.yaml", old, new)
    assert any(expected in e for e in errors), errors


def test_pest_may_be_off_with_no_decision(tmp_path: Path) -> None:
    """LIC-012 (D-402): switching pest control off needs nothing; on needs the decision."""
    errors = errors_after(
        tmp_path,
        "categories.yaml",
        "    enabled: true\n    launch_open: true\n    licensed_only_decision: D-402\n",
        "    enabled: false\n    launch_open: false\n    off_reason: \"Department answer\"\n",
    )
    assert not any("pest_control" in e for e in errors), errors


@pytest.mark.parametrize(
    ("name", "old", "new", "expected"),
    [
        (
            "categories.yaml",
            "    enabled: false\n    launch_open: false\n    off_reason: \"Off at launch (changes",
            "    enabled: false\n    launch_open: true\n    off_reason: \"Off at launch (changes",
            "launch_open but not enabled (LIC-046)",
        ),
        (
            "license_map.yaml",
            "      plumbing_repair: {only: [drain clearing]}",
            "      plumbing_repair: {only: [drain jetting]}",
            "has no job type 'drain jetting'",
        ),
        (
            "license_map.yaml",
            "  - {class: CR-11, name: \"Electrical (dual)\"",
            "  - {class: C-11, name: \"Electrical (dual)\"",
            "commercial-only class C-11 maps job types (LIC-044)",
        ),
        (
            "license_map.yaml",
            "  - {class: CR-7, name:",
            "  - {class: CR-6, name:",
            "class CR-6 listed twice",
        ),
        (
            "license_map.yaml",
            "includes: [R-10]",
            "includes: [R-99]",
            "includes unknown class 'R-99'",
        ),
        (
            "license_map.yaml",
            "  - class: R-11\n    name: \"Electrical\"\n    source: aac_r4_9_103\n    job_types:\n      electrical: {all: true}\n",
            "",
            "includes unknown class 'R-11'",
        ),
        (
            "license_map.yaml",
            '    checked: "2026-10-28"\n  - id: aac_r4_9_104',
            '    checked: "soon"\n  - id: aac_r4_9_104',
            "needs a url and a checked date",
        ),
        (
            "question_bank.yaml",
            "  dryer_vent_cleaning:",
            "  dryer_vent_scrubbing:",
            "enabled category dryer_vent_cleaning has no question set",
        ),
        (
            "question_bank.yaml",
            '      - {id: details, card_field: false, en: "Describe the job',
            '      - {id: details, card_field: true, en: "Describe the job',
            "free-text question can't be a card field",
        ),
        (
            "question_bank.yaml",
            'es: "¿Qué hay que hacer?", ',
            "",
            "needs both en and es",
        ),
        (
            "question_bank.yaml",
            "options_es: [Bombeo de rutina, Se regresa o drena lento ahora, Inspección del tanque]",
            "options_es: [Bombeo de rutina, Inspección del tanque]",
            "options_en and options_es must match",
        ),
        (
            "question_bank.yaml",
            '"Ask Hava: Keep people and pets away',
            '"Keep people and pets away',
            "must start 'Ask Hava:'",
        ),
        (
            "question_bank.yaml",
            "ask_if: {question: work, answers: [1]}, en: \"What kind of engine?\"",
            "ask_if: {question: later_one, answers: [1]}, en: \"What kind of engine?\"",
            "ask_if names a question that isn't asked earlier",
        ),
        (
            "question_bank.yaml",
            "  epoxy_floors:\n    version: 1\n    confirmed_by: []\n    questions:\n",
            "  epoxy_floors:\n    version: 1\n    confirmed_by: []\n    questions:\n"
            "      - {id: a, en: x, es: y}\n      - {id: b, en: x, es: y}\n"
            "      - {id: c, en: x, es: y}\n      - {id: d, en: x, es: y}\n",
            "7 questions, need 3 to 6",
        ),
        (
            "question_bank.yaml",
            'band_floor: ["0", "1500.00", "5000.00"]',
            'band_floor: ["0"]',
            "needs one band_floor per option",
        ),
        (
            "question_bank.yaml",
            "es: \"¿Qué tipo de sistema es?\"",
            "es: \"¿Qué tipo de sistema es? Book a tech\"",
            "implies Ask Hava is a contractor (WORD-022)",
        ),
    ],
)
def test_round_13_checks_are_enforced(
    tmp_path: Path, name: str, old: str, new: str, expected: str
) -> None:
    """LIC-040, LIC-043, LIC-044, LIC-046, INT-042 (D-392, D-394, D-396)."""
    errors = errors_after(tmp_path, name, old, new)
    assert any(expected in e for e in errors), errors


def test_round_13_quote_types() -> None:
    """FEE-004, FEE-043 to FEE-045 (D-399, D-403): hourly, setup charges, fee-free jobs."""
    cats, _ = check_catalog.check_categories(
        check_catalog.load(REPO_ROOT, check_catalog.CATEGORIES_FILE)
    )
    base = {"id": "X", "business": "qualified", "situation": "new_customer"}
    hourly = check_catalog.compute(
        {**base, "quote": {"type": "hourly", "rate": "95.00", "hours_low": "3", "hours_high": "5"}},
        cats["handyman"],
    )
    assert (hourly.fee_basis, hourly.fee) == (check_catalog.Fraction(380), 25)
    no_hours = check_catalog.compute({**base, "quote": {"type": "hourly", "rate": "95.00"}}, cats["hvac_replacement"])
    assert (no_hours.fee_basis, no_hours.fee) == (None, 25)
    setup = check_catalog.compute(
        {**base, "quote": {"type": "fixed", "amount": "40.00", "period": "weekly", "setup": "900.00"}},
        cats["pool_cleaning"],
    )
    assert setup.fee == 75 and setup.fee_basis == check_catalog.Fraction("693.33")
    free = check_catalog.compute(
        {**base, "job_type": "refinance inspection", "quote": {"type": "fixed", "amount": "600.00"}},
        cats["home_inspection"],
    )
    assert (free.outcome, free.fee) == ("fee_free", 0)

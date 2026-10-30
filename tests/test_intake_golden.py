"""Loader test for the intake golden set, docs/catalog/intake_golden.yaml (T154).

INT-040: before any routing code exists, every golden row names a launch-open trade, one of that
trade's job types and the trade's first question, all checked against docs/catalog/categories.yaml
and docs/catalog/question_bank.yaml, so the OPS-040 golden run (T156) has a trusted answer key.
OPS-040: about 10 fake rows per launch-open trade, each with its expected trade, job type and
first question; every value is fake (scripts/pii_scan.py finds nothing, rows carry no other field).
OPS-053 (TD-OPS-28, D-492): at least a quarter of each trade's rows are in Spanish; the loader
fails a planted trade with 2 of its 10 rows in Spanish and passes it at 3 of 10.
"""

from __future__ import annotations

from collections import Counter
from pathlib import Path
from typing import Any

import pytest
import yaml
from pii_scan import scan_text

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "docs/catalog"
GOLDEN = CATALOG / "intake_golden.yaml"
FIELDS = {"id", "trade", "job_type", "question", "lang", "text"}
LANGS = {"en", "es"}
MIN_ROWS, MAX_ROWS = 8, 12  # "about 10" per launch-open trade (OPS-040)

Row = dict[str, Any]


def load_yaml(path: Path) -> Any:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def launch_open() -> dict[str, list[str]]:
    """Launch-open category id to its job types (categories.yaml, LIC-046)."""
    categories = load_yaml(CATALOG / "categories.yaml")["categories"]
    return {c["id"]: list(c["job_types"]) for c in categories if c.get("launch_open")}


def first_questions() -> dict[str, str]:
    """Each question set's first question with no ask_if: the first one intake asks."""
    sets = load_yaml(CATALOG / "question_bank.yaml")["sets"]
    return {k: next(q["id"] for q in s["questions"] if "ask_if" not in q) for k, s in sets.items()}


def row_errors(row: Row, trades: dict[str, list[str]], firsts: dict[str, str]) -> list[str]:
    """INT-040, OPS-040: one row's fields, trade, job type, question and language."""
    rid = row.get("id")
    if set(row) != FIELDS:
        return [f"{rid}: fields must be exactly {sorted(FIELDS)}"]
    trade = row["trade"]
    if trade not in trades:
        return [f"{rid}: {trade} is not a launch-open category"]
    errors = []
    if row["job_type"] not in trades[trade]:
        errors.append(f"{rid}: job type {row['job_type']!r} is not one of {trade}'s")
    if row["question"] != firsts.get(trade):
        errors.append(f"{rid}: question {row['question']!r} is not {trade}'s first question")
    if row["lang"] not in LANGS:
        errors.append(f"{rid}: lang {row['lang']!r} is not en or es")
    if not str(row["text"]).strip():
        errors.append(f"{rid}: text is empty")
    return errors


def trade_errors(rows: list[Row], trades: dict[str, list[str]]) -> list[str]:
    """OPS-040 about 10 rows per launch-open trade; OPS-053 at least a quarter in Spanish."""
    errors = []
    totals = Counter(r.get("trade") for r in rows)
    spanish = Counter(r.get("trade") for r in rows if r.get("lang") == "es")
    for trade in trades:
        if not MIN_ROWS <= totals[trade] <= MAX_ROWS:
            errors.append(f"{trade}: {totals[trade]} rows, want {MIN_ROWS} to {MAX_ROWS}")
        if spanish[trade] * 4 < totals[trade]:
            errors.append(f"{trade}: {spanish[trade]} of {totals[trade]} rows in Spanish, under a quarter")
    return errors


def check(rows: list[Row]) -> list[str]:
    """Every problem in a list of golden rows, checked against the real catalog."""
    trades, firsts = launch_open(), first_questions()
    errors = [e for row in rows for e in row_errors(row, trades, firsts)]
    ids = Counter(r.get("id") for r in rows)
    errors += [f"{rid}: id used {n} times" for rid, n in ids.items() if n > 1]
    return errors + trade_errors(rows, trades)


def golden_rows() -> list[Row]:
    rows: list[Row] = load_yaml(GOLDEN)["rows"]
    return rows


def with_spanish(rows: list[Row], trade: str, count: int) -> list[Row]:
    """A copy of the rows where exactly `count` of the trade's rows are in Spanish."""
    planted, seen = [], 0
    for row in rows:
        row = dict(row)
        if row["trade"] == trade:
            row["lang"] = "es" if seen < count else "en"
            seen += 1
        planted.append(row)
    return planted


def test_real_golden_set_passes() -> None:
    """INT-040, OPS-040, OPS-053: the committed golden set has no problem."""
    rows = golden_rows()
    assert check(rows) == []
    assert {r["trade"] for r in rows} == set(launch_open())


def test_every_value_is_fake() -> None:
    """OPS-040: the PII scanner finds no phone, address, name, key or email in the file."""
    assert scan_text(str(GOLDEN.relative_to(REPO_ROOT)), GOLDEN.read_text(encoding="utf-8")) == []


def test_ops_28_spanish_quarter() -> None:
    """TD-OPS-28 (OPS-053, D-492): 2 of 10 Spanish rows fails, 3 of 10 passes."""
    rows = golden_rows()
    trade = "plumbing_repair"
    assert sum(r["trade"] == trade for r in rows) == 10
    assert any("plumbing_repair: 2 of 10 rows in Spanish" in e for e in check(with_spanish(rows, trade, 2)))
    assert check(with_spanish(rows, trade, 3)) == []


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"job_type": "roof leak"}, "job type 'roof leak' is not one of plumbing_repair's"),
        ({"question": "shut_off"}, "is not plumbing_repair's first question"),
        ({"trade": "towing"}, "towing is not a launch-open category"),
        ({"lang": "fr"}, "lang 'fr' is not en or es"),
        ({"text": "  "}, "text is empty"),
        ({"phone": "555-0100"}, "fields must be exactly"),
    ],
)
def test_planted_row_fault_fails(change: Row, message: str) -> None:
    """INT-040: a row naming anything the catalog lacks, or carrying another field, fails."""
    rows = golden_rows()
    rows[0] = {**rows[0], **change}
    assert any(message in e for e in check(rows)), check(rows)


def test_missing_trade_and_duplicate_id_fail() -> None:
    """OPS-040: a launch-open trade with too few rows, and a repeated id, both fail."""
    rows = [r for r in golden_rows() if r["trade"] != "epoxy_floors"]
    rows.append(dict(rows[0]))
    errors = check(rows)
    assert "epoxy_floors: 0 rows, want 8 to 12" in errors
    assert "plumbing_repair-01: id used 2 times" in errors


def test_planted_pii_is_found() -> None:
    """OPS-040: the fake-data check is live: a real-looking phone in a row's text is flagged."""
    area = "928"  # kept apart from the number so the repo-wide scan of this file stays clean
    text = GOLDEN.read_text(encoding="utf-8").replace("kitchen sink", f"kitchen sink, call {area}-555-1234", 1)
    assert [rule for _, _, rule in scan_text("docs/catalog/intake_golden.yaml", text)] == ["phone"]

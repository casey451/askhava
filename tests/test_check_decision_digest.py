"""Tests for scripts/check_decision_digest.py (D-3856, D-4447).

Every case plants its own log, digest, task list and pair file in a temporary directory, never the
repository's own, so the suite never depends on the live decision log.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import check_decision_digest as cdd  # noqa: E402

# Out of numeric order on purpose (D-516 before D-515), with an unpadded `**D-` line like D-1552.
LOG = """# Decisions

- **D-001** Question: fake first. Choice: one.
- **D-516** Question: fake later number, earlier line. Choice: two.
  continuation of D-516
- **D-515** Question: fake. Choice: three.
**D-1552** Question: fake, no leading dash. Choice: four.
- **D-1600** Question: fake replacement. Choice: five.
"""

PAIRS = {"note": "fake", "superseded": {"D-515": "D-1600"}, "partial": {}}
TASKS = {"tasks": [{"id": "T500"}, {"id": "T553"}]}

GOOD = """# Decisions in force

Through D-1600

## Money

- D-001, D-0516 (money) The introduction fee by job size is fixed at the pick.

## Superseded

- D-515 -> D-1600

## Task-scoped

- T500: D-1552, D-1600
"""


def plant(tmp: Path, digest: str | None, log: str = LOG, pairs: dict | None = None) -> Path:
    (tmp / "docs").mkdir(exist_ok=True)
    (tmp / "kit").mkdir(exist_ok=True)
    (tmp / "docs" / "decisions.md").write_text(log, encoding="utf-8")
    (tmp / "kit" / "superseded_decisions.json").write_text(json.dumps(pairs or PAIRS))
    (tmp / "kit" / "tasks.json").write_text(json.dumps(TASKS))
    if digest is not None:
        (tmp / "docs" / "decisions-current.md").write_text(digest, encoding="utf-8")
    return tmp


def run(root: Path, *extra: str) -> int:
    return cdd.main(["--root", str(root), "--base-ref", "none-such-ref", *extra])


def output_of(capsys: pytest.CaptureFixture[str]) -> str:
    return capsys.readouterr().out


def test_good_digest_passes_with_padded_and_unpadded_numbers(tmp_path, capsys):
    assert run(plant(tmp_path, GOOD)) == 0
    assert "0 entries pending" in output_of(capsys)


def test_no_digest_passes_with_every_entry_pending(tmp_path, capsys):
    assert run(plant(tmp_path, None)) == 0
    assert "5 entries pending" in output_of(capsys)


def test_no_digest_fails_under_all(tmp_path, capsys):
    assert run(plant(tmp_path, None), "--all") == 1
    assert "does not exist" in output_of(capsys)


def test_digest_without_header_fails(tmp_path, capsys):
    assert run(plant(tmp_path, GOOD.replace("Through D-1600", "Covers everything"))) == 1
    assert "no `Through D-####` header" in output_of(capsys)


def test_header_lower_than_base_fails(tmp_path, capsys):
    base = tmp_path / "base.md"
    base.write_text(GOOD.replace("Through D-1600", "Through D-2000"))
    assert cdd.main(["--root", str(plant(tmp_path, GOOD)), "--base-file", str(base)]) == 1
    assert "lower than the base's D-2000" in output_of(capsys)


def test_header_equal_to_base_passes(tmp_path):
    base = tmp_path / "base.md"
    base.write_text(GOOD)
    assert cdd.main(["--root", str(plant(tmp_path, GOOD)), "--base-file", str(base)]) == 0


def test_unknown_number_fails(tmp_path, capsys):
    digest = GOOD.replace("- T500: D-1552, D-1600", "- T500: D-1552, D-1600, D-777")
    assert run(plant(tmp_path, digest)) == 1
    assert "D-777 is not an entry" in output_of(capsys)


def test_doubled_number_fails(tmp_path, capsys):
    digest = GOOD.replace("- T500: D-1552, D-1600", "- T500: D-1552, D-1600, D-01")
    assert run(plant(tmp_path, digest)) == 1
    assert "D-001 is counted 2 times" in output_of(capsys)


def test_missing_task_id_fails(tmp_path, capsys):
    assert run(plant(tmp_path, GOOD.replace("- T500:", "- T999:"))) == 1
    assert "task T999 is not in kit/tasks.json" in output_of(capsys)


def test_late_entry_at_or_below_bound_fails_by_name(tmp_path, capsys):
    log = LOG + "- **D-1200** Question: fake number reserved earlier, merged late. Choice: six.\n"
    assert run(plant(tmp_path, GOOD, log=log)) == 1
    assert "D-1200 is at or below the bound but not in the digest" in output_of(capsys)


def test_entries_above_bound_are_pending(tmp_path, capsys):
    log = LOG + "- **D-1700** Question: fake newer entry. Choice: seven.\n"
    assert run(plant(tmp_path, GOOD, log=log)) == 0
    assert "1 entries pending" in output_of(capsys)


def test_through_flag_bounds_by_number_not_file_position(tmp_path, capsys):
    digest = "Through D-516\n\n## Money\n\n- D-001, D-515, D-516 (money) Fake rule.\n"
    assert run(plant(tmp_path, digest), "--through", "D-0515") == 0
    assert "3 entries pending" in output_of(capsys)


def test_all_fails_on_entries_above_header(tmp_path, capsys):
    digest = "Through D-516\n\n## Money\n\n- D-001, D-516 (money) Fake rule.\n"
    assert run(plant(tmp_path, digest), "--all") == 1
    assert "D-1552 is at or below the bound" in output_of(capsys)


def test_pending_pair_may_stay_a_rule_line(tmp_path, capsys):
    digest = "Through D-1552\n\n## Money\n\n- D-001, D-515, D-516 (money) Fake rule.\n\n"
    digest += "## Task-scoped\n\n- T553: D-1552\n"
    assert run(plant(tmp_path, digest)) == 0
    assert "1 entries pending" in output_of(capsys)


def test_due_pair_left_as_rule_line_fails(tmp_path, capsys):
    digest = GOOD.replace("- D-515 -> D-1600\n", "").replace("D-001, D-0516", "D-001, D-515, D-516")
    assert run(plant(tmp_path, digest)) == 1
    assert "D-515 -> D-1600 is due but not under ## Superseded" in output_of(capsys)


def test_pair_not_in_pair_file_fails(tmp_path, capsys):
    digest = GOOD.replace("- D-515 -> D-1600", "- D-515 -> D-1600\n- D-001 -> D-1552")
    digest = digest.replace("D-001, D-0516", "D-0516")
    assert run(plant(tmp_path, digest)) == 1
    assert "D-001 -> D-1552 is not a pair" in output_of(capsys)


def test_rule_text_over_300_characters_fails(tmp_path, capsys):
    long_text = "Fake rule " + "x" * 300
    digest = GOOD.replace("The introduction fee by job size is fixed at the pick.", long_text)
    assert run(plant(tmp_path, digest)) == 1
    assert "over 300 characters" in output_of(capsys)


def test_malformed_lines_fail(tmp_path, capsys):
    digest = GOOD.replace("(money) The", "The").replace("- D-515 -> D-1600", "- D-515 to D-1600")
    assert run(plant(tmp_path, digest)) == 1
    out = output_of(capsys)
    assert "rule line" in out
    assert "not a `D-a -> D-b` pair" in out


def test_bad_through_value_is_rejected(tmp_path):
    with pytest.raises(SystemExit):
        run(plant(tmp_path, GOOD), "--through", "1600")

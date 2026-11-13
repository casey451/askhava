"""Tests for D-3474: a small crate CLAUDE.md plus a greppable NOTES.md.

Covers scripts/check_code_docs.py's byte budget and NOTES.md entry checks and scripts/split_crate_doc.py's
mechanical, lossless split.
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_code_docs as ccd  # noqa: E402
import split_crate_doc as scd  # noqa: E402

GOOD_DOC = "# x\n\n## Purpose\nFees.\n\n## Never break\n- Fees never fall: `t.rs`.\n\n## Detail\nSee NOTES.md.\n"
OLD_DOC = (
    "# askhava-payments: money\n\n## Purpose\nAll money moves here (FEE-001). T058 added fees.\n\n"
    "## What it owns\n- `src/rules.rs`: `fee_for` (FEE-001); " + "a long list of names, " * 40 + "the end.\n\n"
    "## Rules it enforces\n- FEE-039: `recurring_basis`.\n\n"
    "## Never break\n- The fee never falls (FEE-001): `prop.rs`. " + "Another guarded fact. " * 30 + "\n\n"
    "## How to change\n- Change a fee: edit the catalog.\n\n## Tests that guard it\n- `tests/fees.rs`.\n"
)


def crate(tmp_path: Path, doc: str = GOOD_DOC, notes: str | None = None) -> Path:
    (tmp_path / "docs/spec").mkdir(parents=True)
    (tmp_path / "docs/spec/fees.md").write_text("- **FEE-001** Tiers.\n- **FEE-039** Ongoing.\n", encoding="utf-8")
    (tmp_path / "docs/architecture.md").write_text("| `app/payments` | fees |\n", encoding="utf-8")
    base = tmp_path / "app/payments"
    base.mkdir(parents=True)
    (base / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
    (base / "CLAUDE.md").write_text(doc, encoding="utf-8")
    if notes is not None:
        (base / "NOTES.md").write_text(notes, encoding="utf-8")
    return tmp_path


def test_good_crate_with_notes_passes(tmp_path: Path) -> None:
    notes = "# payments notes\n- [FEE-001] (rules) `fee_for` applies FEE-001.\n- [-] (owns) `src/lib.rs`.\n- [FEE-039] (never 2) More.\n"
    assert ccd.run(crate(tmp_path, notes=notes)) == []


def test_crate_doc_over_its_byte_budget_fails(tmp_path: Path) -> None:
    doc = GOOD_DOC.replace("Fees.", "x" * ccd.MAX_CRATE_DOC_BYTES)
    assert any(f"limit {ccd.MAX_CRATE_DOC_BYTES}" in p for p in ccd.run(crate(tmp_path, doc)))


def test_notes_line_that_is_not_an_entry_fails(tmp_path: Path) -> None:
    root = crate(tmp_path, notes="# notes\n- [FEE-001] (rules) ok.\nloose paragraph\n- (rules) no ids\n")
    problems = ccd.run(root)
    assert sum("not an entry" in p for p in problems) == 2 and "app/payments/NOTES.md:3" in problems[0]


def test_notes_needs_a_title_and_short_lines(tmp_path: Path) -> None:
    root = crate(tmp_path, notes="- [-] (owns) " + "y" * ccd.MAX_NOTE_LINE_CHARS + "\n- [-] (owns) z\n")
    problems = ccd.run(root)
    assert any("line 1 must be" in p for p in problems)
    root2 = crate(tmp_path / "b", notes="# n\n- [-] (owns) " + "y" * ccd.MAX_NOTE_LINE_CHARS + "\n")
    assert any("make it two entries" in p for p in ccd.run(root2))


def test_unknown_requirement_id_in_notes_fails(tmp_path: Path) -> None:
    root = crate(tmp_path, notes="# n\n- [FEE-999] (rules) FEE-999 is not in the spec.\n")
    assert any("NOTES.md: names FEE-999" in p for p in ccd.run(root))


def test_split_keeps_every_word_and_passes_the_check(tmp_path: Path) -> None:
    guide, notes = scd.split("payments", OLD_DOC, {"FEE-001", "FEE-039"})
    assert scd.lost(OLD_DOC, guide, notes) == []
    assert "## Never break\n- The fee never falls (FEE-001): `prop.rs`." in guide
    assert "- [FEE-039] (rules) FEE-039: `recurring_basis`." in notes
    root = crate(tmp_path, guide, notes)
    assert ccd.run(root) == []


def test_split_cuts_long_items_into_entries_that_keep_their_ids(tmp_path: Path) -> None:
    _, notes = scd.split("payments", OLD_DOC, {"FEE-001", "FEE-039"})
    owns = [line for line in notes.splitlines() if "(owns)" in line]
    assert len(owns) > 1 and all(line.startswith("- [FEE-001] (owns) ") for line in owns)
    assert all(len(line) <= ccd.MAX_NOTE_LINE_CHARS for line in notes.splitlines())


def test_never_break_over_target_points_to_its_rest(monkeypatch) -> None:  # type: ignore[no-untyped-def]
    monkeypatch.setattr(scd, "TARGET_BYTES", 1000)
    guide, notes = scd.split("payments", OLD_DOC, {"FEE-001", "FEE-039"})
    assert "More: `(never 1)` in NOTES.md." in guide
    assert "(never 1) " in notes and scd.lost(OLD_DOC, guide, notes) == []


def test_lost_names_a_dropped_sentence() -> None:
    guide, notes = scd.split("payments", OLD_DOC, {"FEE-001", "FEE-039"})
    gone = scd.lost(OLD_DOC, guide, notes.replace("Change a fee: edit the catalog.", ""))
    assert any("Change a fee" in g for g in gone)


def test_real_crates_are_split_and_within_budget() -> None:
    for doc in sorted((REPO_ROOT / "app").glob("*/CLAUDE.md")):
        assert (doc.parent / "NOTES.md").is_file(), doc
        assert "\n## Detail\n" in doc.read_text(encoding="utf-8"), doc
        assert len(doc.read_bytes()) <= ccd.MAX_CRATE_DOC_BYTES, doc


def test_sweep_moves_detail_entries_to_notes(tmp_path: Path, monkeypatch) -> None:  # type: ignore[no-untyped-def]
    guide, notes = scd.split("payments", OLD_DOC, {"FEE-001", "FEE-039"})
    base = tmp_path / "app/payments"
    base.mkdir(parents=True)
    (base / "CLAUDE.md").write_text(guide + "- [FEE-001] (rules) A later task's entry.\n", encoding="utf-8")
    (base / "NOTES.md").write_text(notes, encoding="utf-8")
    monkeypatch.setattr(scd, "ROOT", tmp_path)
    assert scd.main(["--sweep", "app/payments"]) == 0
    assert (base / "CLAUDE.md").read_text(encoding="utf-8") == guide
    assert (base / "NOTES.md").read_text(encoding="utf-8").endswith("- [FEE-001] (rules) A later task's entry.\n")

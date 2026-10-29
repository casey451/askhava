"""Tests for scripts/check_copy.py (the copy panel's measurable bar, WORD-029, D-474)."""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_copy as cc  # noqa: E402


def site(tmp_path: Path, body: str) -> Path:
    (tmp_path / "docs/copy").mkdir(parents=True)
    (tmp_path / "docs/copy/site-copy.md").write_text(body, encoding="utf-8")
    return tmp_path


def test_real_repository_passes() -> None:
    assert cc.run(REPO_ROOT) == []


def test_plain_copy_passes(tmp_path: Path) -> None:
    root = site(tmp_path, "## Home · lede (homeowner)\n> Tell us what you need done. We'll send it to local businesses.\n")
    assert cc.run(root) == []


def test_long_sentence_fails(tmp_path: Path) -> None:
    long = " ".join(["word"] * 24) + "."
    root = site(tmp_path, f"## Home · lede (homeowner)\n> {long}\n")
    assert any("24-word sentence" in p for p in cc.run(root))


def test_superlative_fails(tmp_path: Path) -> None:
    root = site(tmp_path, "## Home · hero (homeowner)\n> The best quotes in Havasu.\n")
    assert any("'best'" in p for p in cc.run(root))


def test_hard_reading_level_fails_for_homeowners(tmp_path: Path) -> None:
    text = ("Comprehensive documentation regarding organizational responsibilities necessitates "
            "considerable administrative coordination. Additional individualized evaluation "
            "facilitates operational effectiveness throughout implementation. Representatives "
            "communicate institutional requirements comprehensively.")
    root = site(tmp_path, f"## Help · intro (homeowner)\n> {text}\n")
    assert any("reading grade" in p for p in cc.run(root))


def test_business_copy_gets_its_own_limit() -> None:
    block = cc.Block("x", "business", False, "word " * 5)
    assert cc.check_block(block) == []
    assert cc.MAX_GRADE["business"] > cc.MAX_GRADE["homeowner"]


def test_fixed_blocks_are_skipped(tmp_path: Path) -> None:
    long = " ".join(["word"] * 30) + "."
    root = site(tmp_path, f"## Footer · contractor line (homeowner, fixed)\n> {long}\n")
    assert cc.run(root) == []


def test_template_text_is_read_and_business_paths_use_business_limit(tmp_path: Path) -> None:
    tpl = tmp_path / "app/site/templates/business"
    tpl.mkdir(parents=True)
    (tpl / "page.html").write_text("<style>p{}</style><p>The best fee.</p><p>{{ name }}</p>", encoding="utf-8")
    problems = cc.run(tmp_path)
    assert problems == ["app/site/templates/business/page.html · text 1: claim word 'best' (say the fact instead)"]


def test_syllables_and_grade_are_sane() -> None:
    assert cc.syllables("quote") == 1
    assert cc.syllables("business") >= 2
    assert cc.grade("We send it. You pick one.") < 3


def test_main_reports(tmp_path: Path, capsys) -> None:  # type: ignore[no-untyped-def]
    root = site(tmp_path, "## Home · hero (homeowner)\n> The best quotes.\n")
    assert cc.main(["--root", str(root)]) == 1
    assert "1 problem(s)" in capsys.readouterr().out

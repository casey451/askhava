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


def test_a_price_starts_a_new_sentence() -> None:
    assert cc.sentences("Fees by job size. $25 under $500. $75 above.") == ["Fees by job size.", "$25 under $500.", "$75 above."]


def test_syllables_and_grade_are_sane() -> None:
    assert cc.syllables("quote") == 1
    assert cc.syllables("business") >= 2
    assert cc.grade("We send it. You pick one.") < 3


def test_main_reports(tmp_path: Path, capsys) -> None:  # type: ignore[no-untyped-def]
    root = site(tmp_path, "## Home · hero (homeowner)\n> The best quotes.\n")
    assert cc.main(["--root", str(root)]) == 1
    assert "1 problem(s)" in capsys.readouterr().out


# TD-WORD-32 (WORD-031, D-492, D-3889): the legal measure over the six public drafts.
HARD = ("Comprehensive documentation regarding organizational responsibilities necessitates considerable "
        "administrative coordination.")


def planted(n_words: int, n_syllables: int) -> str:
    """Two sentences of one- and two-syllable words with the given totals (fake text, no real data)."""
    tokens = ["ago"] * (n_syllables - n_words) + ["dog"] * (2 * n_words - n_syllables)
    half = n_words // 2
    return f"{' '.join(tokens[:half]).capitalize()}. {' '.join(tokens[half:]).capitalize()}."


def legal(tmp_path: Path, name: str, body: str) -> Path:
    (tmp_path / "docs/legal").mkdir(parents=True, exist_ok=True)
    (tmp_path / "docs/legal" / name).write_text(f"# Title\n\n## 1. Section\n\n{body}\n", encoding="utf-8")
    return tmp_path


def test_td_word_32_homeowner_section_fails_at_9_5_and_passes_at_9_0(tmp_path: Path) -> None:
    """TD-WORD-32: a homeowner-document section at grade 9.5 fails; one at 9.0 passes."""
    root = legal(tmp_path, "privacy-policy.md", planted(48, 64))
    assert [round(g, 1) for _, _, g in cc.legal_grades(root)] == [9.5]
    expected = "docs/legal/privacy-policy.md · 1. Section: reading grade 9.5 (legal limit 9 for homeowners)"
    assert cc.run(root) == [expected]
    root = legal(tmp_path, "privacy-policy.md", planted(26, 43))
    assert [round(g, 1) for _, _, g in cc.legal_grades(root)] == [9.0]
    assert cc.run(root) == []


def test_td_word_32_business_section_fails_at_10_5_and_passes_at_10_0(tmp_path: Path) -> None:
    """TD-WORD-32: a business-terms section at grade 10.5 fails; one at 10.0 passes."""
    root = legal(tmp_path, "business-terms.md", planted(56, 72))
    assert [round(g, 1) for _, _, g in cc.legal_grades(root)] == [10.5]
    expected = "docs/legal/business-terms.md · 1. Section: reading grade 10.5 (legal limit 10 for businesses)"
    assert cc.run(root) == [expected]
    root = legal(tmp_path, "business-terms.md", planted(48, 66))
    assert [round(g, 1) for _, _, g in cc.legal_grades(root)] == [10.0]
    assert cc.run(root) == []


def test_td_word_32_limits_are_separate_from_max_grade() -> None:
    """TD-WORD-32: the legal limits are 9 and 10; MAX_GRADE stays 8 and 9; the boundary is inclusive."""
    assert cc.LEGAL_MAX_GRADE == {"homeowner": 9.0, "business": 10.0}
    assert cc.MAX_GRADE == {"homeowner": 8.0, "business": 9.0}
    assert cc.legal_grade_problem("x", "homeowner", 9.0) is None
    assert cc.legal_grade_problem("x", "business", 10.0) is None
    assert cc.legal_grade_problem("x", "business", 10.01) is not None
    assert cc.LEGAL_DRAFTS["docs/legal/business-email-notices.md"] == "business"
    assert cc.LEGAL_DRAFTS["docs/legal/sms-terms.md"] == "homeowner"


def test_td_word_32_a_sentence_a_requirement_fixes_is_excepted(tmp_path: Path) -> None:
    """TD-WORD-32: an over-grade sentence a docs/spec/ requirement holds word for word is not measured,
    and a quoted span of five or more words is taken out of a longer sentence."""
    root = legal(tmp_path, "homeowner-terms.md", f"{HARD} {planted(26, 30)}")
    assert any("reading grade" in p for p in cc.run(root))
    (root / "docs/spec").mkdir(parents=True)
    (root / "docs/spec/wording.md").write_text(f"- **WORD-999** The terms say \"{HARD}\"\n", encoding="utf-8")
    assert cc.run(root) == []
    fixed = cc.fixed_text(root)
    assert cc.measured_text(f"Laftly says {HARD}", fixed) == "laftly says"
    assert cc.measured_text("Recibido, usted es el cliente.", fixed) == ""


def test_td_word_32_internal_documents_are_not_measured(tmp_path: Path) -> None:
    """TD-WORD-32, D-492: the security program, the incident plan and the README are not measured."""
    for name in ("information-security-program.md", "incident-response-plan.md", "README.md"):
        legal(tmp_path, name, " ".join([HARD] * 3))
    assert cc.legal_grades(tmp_path) == []
    assert cc.run(tmp_path) == []


def test_td_word_32_list_items_and_table_rows_are_sentences() -> None:
    """TD-WORD-32, D-3889: each list item and table row is its own unit; headings are labels only."""
    text = "Intro\n## A\n\n- one\n  more\n- **two** `x`\n\n| h1 | h2 |\n|---|---|\n| c1 | c2 |\n> quoted\n> line\n"
    units = ["one more", "two x", "h1 h2", "c1 c2", "quoted line"]
    assert cc.legal_sections(text) == [("opening", ["Intro"]), ("A", units)]


def test_td_word_32_repository_drafts_are_graded_and_pass() -> None:
    """TD-WORD-32: every one of the six drafts is graded, and the drafts in the repository pass."""
    graded = {where.split(" · ")[0] for where, _, _ in cc.legal_grades(REPO_ROOT)}
    drafts = ("homeowner-terms", "business-terms", "privacy-policy", "sms-terms", "business-email-notices",
              "accessibility-statement")
    assert graded == {f"docs/legal/{name}.md" for name in drafts}
    assert cc.legal_problems(REPO_ROOT) == []


def test_td_word_32_main_reports_each_legal_grade(capsys) -> None:  # type: ignore[no-untyped-def]
    """TD-WORD-32: --legal-grades prints the grade of each measured section."""
    assert cc.main(["--root", str(REPO_ROOT), "--legal-grades"]) == 0
    assert "homeowner docs/legal/privacy-policy.md · 2. What we collect" in capsys.readouterr().out

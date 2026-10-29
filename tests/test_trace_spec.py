"""Tests for scripts/trace_spec.py (fake spec files only)."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from types import ModuleType

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT = REPO_ROOT / "scripts" / "trace_spec.py"


def load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("trace_spec", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["trace_spec"] = module
    spec.loader.exec_module(module)
    return module


trace_spec = load_script()

SOURCE_TEXT = "# Fake source\n\nRule one.\nRule two.\n"
DECISIONS_TEXT = "# Decisions\n\n- **D-001** Fake decision.\n"


def make_repo(tmp_path: Path, spec_body: str, name: str = "area.md", prefix: str = "TST") -> Path:
    (tmp_path / "docs" / "spec").mkdir(parents=True)
    (tmp_path / "src.md").write_text(SOURCE_TEXT, encoding="utf-8")
    (tmp_path / "docs" / "decisions.md").write_text(DECISIONS_TEXT, encoding="utf-8")
    header = f"# Spec: test\n\nPrefix: `{prefix}`\n\n"
    (tmp_path / "docs" / "spec" / name).write_text(header + spec_body, encoding="utf-8")
    (tmp_path / "docs" / "spec" / "index.md").write_text("# Index\n", encoding="utf-8")
    return tmp_path


def errors_for(tmp_path: Path, spec_body: str) -> list[str]:
    report = trace_spec.run(make_repo(tmp_path, spec_body))
    errors: list[str] = report.errors
    return errors


def test_valid_spec_passes_and_counts(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    body = (
        "- **TST-001** A rule. Source: `src.md:3`.\n"
        "- **TST-002** Another rule (D-001). Source: `src.md:3`, `src.md:4`.\n"
    )
    root = make_repo(tmp_path, body)
    assert trace_spec.main(["--root", str(root)]) == 0
    out = capsys.readouterr().out
    assert "Requirements: 2" in out
    assert "Citations: 3 (2 distinct lines)" in out
    assert "Requirements resting on a logged decision: 1" in out
    assert "Errors: 0" in out


def test_duplicate_id_fails(tmp_path: Path) -> None:
    body = "- **TST-001** A. Source: `src.md:3`.\n- **TST-001** B. Source: `src.md:4`.\n"
    assert any("duplicate ID TST-001" in e for e in errors_for(tmp_path, body))


def test_duplicate_id_across_files_fails(tmp_path: Path) -> None:
    root = make_repo(tmp_path, "- **TST-001** A. Source: `src.md:3`.\n")
    other = "# Spec: other\n\nPrefix: `TST`\n\n- **TST-001** B. Source: `src.md:4`.\n"
    (root / "docs" / "spec" / "other.md").write_text(other, encoding="utf-8")
    errors = trace_spec.run(root).errors
    assert any("duplicate ID TST-001" in e for e in errors)
    assert any("prefix TST already used" in e for e in errors)


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        ("- **TST-001** A rule.\n", "no 'Source:' part"),
        ("- **TST-001** A rule. Source: none.\n", "has no citation"),
        ("- **TST-001** A rule. Source: `missing.md:1`.\n", "cited file not found"),
        ("- **TST-001** A rule. Source: `src.md:99`.\n", "past the end"),
        ("- **TST-001** A rule. Source: `src.md:0`.\n", "past the end"),
        ("- **TST-001** A rule. Source: `src.md:2`.\n", "blank line"),
        ("- **TST-001** A rule. Source: `../src.md:3`.\n", "cited file not found"),
        ("- **TST-001** A rule. Source: `docs/spec/area.md:1`.\n", "cites the spec itself"),
        ("- **TST-1** A rule. Source: `src.md:3`.\n", "malformed requirement line"),
        ("- **OTH-001** A rule. Source: `src.md:3`.\n", "does not use prefix TST"),
        ("- **TST-001** A rule (D-999). Source: `src.md:3`.\n", "names D-999"),
    ],
)
def test_bad_requirement_fails(tmp_path: Path, line: str, expected: str) -> None:
    errors = errors_for(tmp_path, line)
    assert any(expected in e for e in errors), errors


@pytest.mark.parametrize(
    "text",
    ["The brand is Havasu Home Quotes.", "The fee is $149.", "Recurring counts as 2 months."],
)
def test_superseded_value_fails(tmp_path: Path, text: str) -> None:
    errors = errors_for(tmp_path, f"- **TST-001** {text} Source: `src.md:3`.\n")
    assert any("superseded value" in e for e in errors), errors


def test_four_month_rule_and_new_fees_pass(tmp_path: Path) -> None:
    body = "- **TST-001** Recurring counts as 4 months; fees $25, $75, $199. Source: `src.md:3`.\n"
    assert errors_for(tmp_path, body) == []


def test_missing_prefix_and_empty_dir(tmp_path: Path) -> None:
    root = make_repo(tmp_path, "- **TST-001** A. Source: `src.md:3`.\n")
    (root / "docs" / "spec" / "area.md").write_text("# No prefix\n", encoding="utf-8")
    assert any("no 'Prefix:' line" in e for e in trace_spec.run(root).errors)
    empty = tmp_path / "empty"
    (empty / "docs" / "spec").mkdir(parents=True)
    assert trace_spec.run(empty).errors == ["no spec files in docs/spec"]


def test_main_returns_one_on_error(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    root = make_repo(tmp_path, "- **TST-001** A. Source: `src.md:99`.\n")
    assert trace_spec.main(["--root", str(root)]) == 1
    assert "ERROR" in capsys.readouterr().err


def test_repository_spec_is_traceable() -> None:
    report = trace_spec.run(REPO_ROOT)
    assert report.errors == []
    assert len(report.requirements) > 0


@pytest.mark.parametrize(
    "text",
    [
        "The page says \"Our pros are local.\"",
        "The text says \"We'll send a plumber tonight.\"",
        "The card says \"Book now.\"",
        "The portal shows \"Booked until Friday\".",
        "The page says \"Hire through us.\"",
        "The page says \"Businesses work through Ask Hava.\"",
        "The email says \"Job assigned to you.\"",
        "The page says \"Our network of service providers.\"",
        "The terms say \"We guarantee the work.\"",
        "The text says \"We found you a pro.\"",
        "The terms say \"Businesses are subcontractors.\"",
        "The quote page says \"Our price is fair.\"",
        "El texto dice \"nuestros profesionales\".",
        "El texto dice \"le enviaremos un plomero\".",
    ],
)
def test_not_a_contractor_wording_fails(tmp_path: Path, text: str) -> None:
    errors = errors_for(tmp_path, f"- **TST-001** {text} Source: `src.md:3`.\n")
    assert any("implies Ask Hava is a contractor (WORD-022)" in e for e in errors), errors


def test_quote_relay_wording_passes(tmp_path: Path) -> None:
    body = (
        "- **TST-001** The page says \"Ask Hava is not a contractor. We pass along your request "
        "and show you quotes from independent local businesses, as they wrote them. You choose, "
        "and you hire the business directly.\" and \"Ask Hava sends your request to the "
        "businesses.\" Source: `src.md:3`.\n"
    )
    assert errors_for(tmp_path, body) == []


def test_wording_rules_may_quote_banned_phrases(tmp_path: Path) -> None:
    """The rules that ban the phrases (D-360) quote them without failing the check."""
    body = '- **WORD-022** No page says "our pros" or "book". Source: `src.md:3`.\n'
    root = make_repo(tmp_path, body, prefix="WORD")
    assert trace_spec.run(root).errors == []
    assert trace_spec.WORDING_RULE_IDS == {
        "WORD-011",
        "WORD-020",
        "WORD-022",
        "WORD-023",
        "TERMS-012",
        "TERMS-026",
        "TERMS-027",
    }


@pytest.mark.parametrize(
    "text",
    [
        "The policy says \"We connect you with the business you choose.\"",
        "The site says \"Ask Hava connects homeowners with local pros.\"",
        "The trademark says \"consumer referral services\".",
        "The campaign says \"a referral network\".",
    ],
)
def test_banned_description_fails(tmp_path: Path, text: str) -> None:
    """WORD-023 (D-377): "connect you with" and referral-network wording fail the check."""
    errors = errors_for(tmp_path, f"- **TST-001** {text} Source: `src.md:3`.\n")
    assert any("WORD-022" in e for e in errors), errors


def test_honest_description_passes(tmp_path: Path) -> None:
    """WORD-023 (D-377): the one honest description passes."""
    body = (
        "- **TST-001** The site says \"Ask Hava is a quote-request service; the business the "
        "homeowner chooses pays Ask Hava a set introduction fee.\" and \"isn't connected to "
        "any business with Hava in its name\". Source: `src.md:3`.\n"
    )
    assert errors_for(tmp_path, body) == []

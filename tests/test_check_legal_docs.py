"""Tests for scripts/check_legal_docs.py (TERMS-028, D-389): the real drafts plus broken copies."""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path
from types import ModuleType

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = REPO_ROOT / "scripts"
LEGAL = Path("docs/legal")


def load_script() -> ModuleType:
    sys.path.insert(0, str(SCRIPTS))
    spec = importlib.util.spec_from_file_location(
        "check_legal_docs", SCRIPTS / "check_legal_docs.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_legal_docs"] = module
    spec.loader.exec_module(module)
    return module


check_legal_docs = load_script()


def copy_repo(tmp_path: Path) -> Path:
    """The parts the check reads: the drafts and the spec's requirement texts."""
    for rel in (LEGAL, Path("docs/spec")):
        shutil.copytree(REPO_ROOT / rel, tmp_path / rel)
    return tmp_path


def errors_after(tmp_path: Path, name: str, old: str, new: str) -> list[str]:
    root = copy_repo(tmp_path)
    path = root / LEGAL / name
    text = path.read_text(encoding="utf-8")
    assert old in text, old
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
    errors: list[str] = check_legal_docs.run(root).errors
    return errors


def test_real_drafts_pass(capsys: pytest.CaptureFixture[str]) -> None:
    assert check_legal_docs.main(["--root", str(REPO_ROOT)]) == 0
    out = capsys.readouterr().out
    assert "Legal documents checked: 9" in out
    assert "Errors: 0" in out


def test_every_draft_starts_with_the_note() -> None:
    for name in check_legal_docs.DOCUMENTS:
        first = (REPO_ROOT / LEGAL / name).read_text(encoding="utf-8").splitlines()[0]
        assert first == "Draft prepared with AI research; not reviewed by a lawyer."


@pytest.mark.parametrize(
    ("name", "old", "new", "expected"),
    [
        (
            "homeowner-terms.md",
            "Draft prepared with AI research; not reviewed by a lawyer.",
            "Draft.",
            "first line must be the draft note",
        ),
        (
            "sms-terms.md",
            "Version 0.3 (draft, consent by texting in) · October 28, 2026",
            "Version draft",
            "second line must be",
        ),
        (
            "homeowner-terms.md",
            "is limited to $100.",
            "is limited to $0.",
            "missing TERMS-004 wording",
        ),
        (
            "business-terms.md",
            "a fixed amount by job size ($25, $75 or $199)",
            "a fixed amount by job size ($25, $75 or $150)",
            "missing TERMS-026 wording",
        ),
        (
            "privacy-policy.md",
            "no AI reads your messages right now.",
            "The AI never writes anything.",
            "missing AI-018 wording",
        ),
        (
            "sms-terms.md",
            "a set fee of $25 to $199, by quote size",
            "a small fee",
            "missing CON-003 wording",
        ),
        (
            "privacy-policy.md",
            "pass your choice to the business you choose",
            "connect you with the business you choose",
            "WORD-022/WORD-023 wording",
        ),
        (
            "homeowner-terms.md",
            "We don't recommend, rank or rate any business",
            "We list only the best, top-rated, vetted pros",
            "banned word",
        ),
        (
            "sms-terms.md",
            "We send no marketing texts.",
            "We send no marketing texts. Ask about pest control.",
            "pest control is never advertised",
        ),
        (
            "accessibility-statement.md",
            "The Ask Hava owner usually replies",
            "Casey usually replies",
            "personal name (WORD-021)",
        ),
        (
            "business-terms.md",
            "## 13. Law and courts",
            "## 13. Law and courts\n\nThe fee is a license fee for data access to homeowners.",
            "fee framed as a license or data access (TERMS-027)",
        ),
        (
            "sms-terms.md",
            "Ask Hava: Got it. Next, a few quick questions.",
            "Ask Hava: Got your message.",
            "missing CON-031 wording",
        ),
        (
            "sms-terms.md",
            "You don't need to reply to agree.",
            "Reply OK to agree to the Terms.",
            "asks for a reply of OK or YES to agree (D-419)",
        ),
        (
            "homeowner-terms.md",
            "next to the Pick button the page says",
            "reply YES to our first text; next to the Pick button the page says",
            "asks for a reply of OK or YES to agree (D-419)",
        ),
        (
            "homeowner-terms.md",
            '"By picking, you agree to the Terms", with a link to them.',
            "a link to them.",
            "missing PICK-016 wording",
        ),
        (
            "homeowner-terms.md",
            "### OPTIONAL: Arbitration and class-action waiver (OFF by default; not in effect)",
            "### Arbitration and class-action waiver",
            "arbitration text outside an OPTIONAL (OFF) section",
        ),
        (
            "business-terms.md",
            "## 13. Law and courts",
            "## 13. Law and courts\n\nDisputes go to binding arbitration.",
            "arbitration text outside an OPTIONAL (OFF) section",
        ),
    ],
)
def test_broken_drafts_fail(
    tmp_path: Path, name: str, old: str, new: str, expected: str
) -> None:
    errors = errors_after(tmp_path, name, old, new)
    assert any(expected in e for e in errors), errors


def test_phrase_must_be_the_spec_wording(tmp_path: Path) -> None:
    """A listed phrase that drifts from its spec requirement fails even if the draft matches."""
    root = copy_repo(tmp_path)
    terms = root / "docs/spec/terms.md"
    text = terms.read_text(encoding="utf-8")
    old = "If a charge was our mistake, it is refunded in full."
    assert old in text
    terms.write_text(
        text.replace(old, "If a charge was our mistake, we may refund it."), encoding="utf-8"
    )
    errors = check_legal_docs.run(root).errors
    assert any("phrase not in TERMS-022's spec text" in e for e in errors), errors


def test_missing_and_unlisted_documents_fail(tmp_path: Path) -> None:
    root = copy_repo(tmp_path)
    (root / LEGAL / "sms-terms.md").unlink()
    (root / LEGAL / "extra.md").write_text("x\n", encoding="utf-8")
    errors = check_legal_docs.run(root).errors
    assert "missing docs/legal/sms-terms.md" in errors
    assert "docs/legal/extra.md: not listed in DOCUMENTS" in errors


def test_main_returns_one_on_error(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    root = copy_repo(tmp_path)
    (root / LEGAL / "README.md").write_text("no note\n", encoding="utf-8")
    assert check_legal_docs.main(["--root", str(root)]) == 1
    assert "ERROR README.md" in capsys.readouterr().err

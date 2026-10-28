"""Tests for scripts/check_ai_smell.py (the real repository plus planted AI smell, TD-WORD-11)."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_ai_smell as smell  # noqa: E402


def fake_repo(tmp_path: Path) -> Path:
    """A repository with only the AI-smell list, ready for planted files."""
    (tmp_path / "docs").mkdir()
    shutil.copy(REPO_ROOT / "docs/ai_smell.yaml", tmp_path / "docs/ai_smell.yaml")
    return tmp_path


def plant(root: Path, rel: str, text: str) -> list[smell.Hit]:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return smell.run(root, [])


def test_real_repository_passes(capsys: pytest.CaptureFixture[str]) -> None:
    assert smell.main(["--root", str(REPO_ROOT)]) == 0
    assert "AI smell: 0 hit(s)" in capsys.readouterr().out


@pytest.mark.parametrize(
    ("text", "rule"),
    [
        ("Ask Hava: A seamless way to get quotes.", "word 'seamless'"),
        ("Ask Hava: Get quotes — fast.", "em dash"),
        ("It's not just a quote, but a plan.", "not just X but Y"),
        ("Text us. They quote. You pick.", "three-beat slogan"),
        ("Ask Hava: Your quote is in!", "exclamation mark"),
        ("Ask Hava: Your quote is in ✨", "emoji"),
        ("Need a plumber fast?", "rhetorical opener"),
        ("The best part: it's free for you.", "word 'the best part'"),
        ("We'd love to help with your pool.", "word 'we'd love to'"),
        ("Their quotes line up side by side.", "word 'side by side'"),
    ],
)
def test_planted_copy_fails(tmp_path: Path, text: str, rule: str) -> None:
    root = fake_repo(tmp_path)
    hits = plant(root, "app/messaging/templates/first_quote.txt", text)
    assert rule in {hit.rule for hit in hits}


def test_spec_quotes_are_scanned_but_sources_are_not(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    row = '- **TXT-900** The text says "Ask Hava: A hassle-free way to compare." Source: Casey: "elevate it!"\n'
    hits = plant(root, "docs/spec/texts.md", row)
    assert [hit.rule for hit in hits] == ["word 'hassle-free'"]


def test_safety_text_is_exempt(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    row = '- **INT-004** The reply is "Call 911 now!" Source: x.\n'
    assert plant(root, "docs/spec/intake.md", row) == []


@pytest.mark.parametrize(
    ("rel", "text", "rule"),
    [
        ("app/portals/static/portal.css", ".hero{background:linear-gradient(90deg,#f0f,#0ff)}", "linear gradient"),
        ("app/portals/static/portal.css", ".note{border-left: 4px solid #b8935a}", "colored left border accent"),
        ("app/portals/templates/home.html", '<span class="badge">New</span>', "badge or pill"),
        ("app/site/templates/home.html", "<p>Effortless quotes for your home.</p>", "word 'effortless'"),
    ],
)
def test_planted_page_habits_fail(tmp_path: Path, rel: str, text: str, rule: str) -> None:
    root = fake_repo(tmp_path)
    assert rule in {hit.rule for hit in plant(root, rel, text)}


def test_markup_inside_style_is_not_read_as_copy(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    page = '<style>\n.x{content:"Amazing!"}\n</style>\n<p>Get local quotes by text.</p>\n'
    assert plant(root, "app/site/templates/home.html", page) == []


def test_template_tags_and_script_code_are_not_copy(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    page = "{% if !paid %}<p>Get local quotes by text.</p>{% endif %}{{ name|e }}\n"
    assert plant(root, "app/site/templates/home.html", page) == []
    assert plant(root, "app/site/static/site.ts", "if (x !== null && !done) { show(\"Sending\"); }\n") == []


def test_script_strings_are_scanned(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    hits = plant(root, "app/site/static/site.ts", 'const msg = "A seamless way to compare";\n')
    assert "word 'seamless'" in {hit.rule for hit in hits}


def test_plain_copy_passes(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    text = "Ask Hava: Request 1042 is out to 4 plumbing businesses. We'll text you when the first quote comes in.\n"
    assert plant(root, "app/messaging/templates/cards_sent.txt", text) == []


def test_cold_read_is_in_every_prompt_and_the_brief() -> None:
    template = (REPO_ROOT / "kit/prompts/TEMPLATE.md").read_text(encoding="utf-8")
    assert "scripts/check_ai_smell.py" in template and "cold read (WORD-028)" in template
    for prompt in sorted((REPO_ROOT / "kit/prompts").glob("T[0-9][0-9][0-9].md")):
        assert "cold read (WORD-028)" in prompt.read_text(encoding="utf-8"), prompt.name
    assert "## AI smell" in (REPO_ROOT / "docs/voice.md").read_text(encoding="utf-8")
    assert "AI smell (WORD-027, WORD-028" in (REPO_ROOT / "BUILD_BRIEF.md").read_text(encoding="utf-8")

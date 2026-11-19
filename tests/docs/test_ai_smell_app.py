"""Every customer template under app/ is in the AI-smell scan and passes it (T161).

TD-WORD-11 (WORD-027, WORD-028; D-452, D-474): `scripts/check_ai_smell.py` scans every page
template, text and email template file, message file and stylesheet under `app/` and finds nothing
in them; a copy of each real file kind with "seamless", an em dash, "not just a quote, but",
"Text us. They quote. You pick.", an exclamation mark or an emoji planted fails with the rule named;
the logged cold-read phrases that no customer file holds are on the list (the rest are
left out with their reasons in D-4654); `docs/voice.md` keeps its AI-smell section and `BUILD_BRIEF.md` section 7 step 4 names
AI smell among the reviewer's checks. The general scanner cases live in
`tests/test_check_ai_smell.py` (the page habits in a portal stylesheet and template, and the
skipped 911 row, INT-004); this file covers the app's real templates. Fake text only.

Run: python3 -m pytest tests/docs/test_ai_smell_app.py -q -p no:cacheprovider
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest
import yaml

import check_ai_smell

ROOT = Path(__file__).resolve().parents[2]

# What a customer reads under app/: page, text and email templates, message files, stylesheets.
CUSTOMER_GLOBS = ("app/**/templates/**/*", "app/**/messages/*.yaml", "app/**/*.css", "app/**/*.txt")

# The planted smells, each with the rule the scan must name.
PLANTED = (
    ("A seamless way to get quotes.", "word 'seamless'"),
    ("Quotes — fast.", "em dash"),
    ("It is not just a quote, but a plan.", "not just X but Y"),
    ("Text us. They quote. You pick.", "three-beat slogan"),
    ("Your quotes are in!", "exclamation mark"),
    ("Your quotes are in \U0001f389", "emoji"),
)

# The cold reads' logged phrases that D-4654 added to the list (the left-out ones are named there).
LOGGED = (
    "lo enviaremos",
    "contact wording",
    "your answers on this page stay",
    "the list we check",
    "goes by email to the email",
    "we can't read this text",
    "we read every message",
    "we still hold",
    "it reads as",
    "you accepted",
)


def customer_files() -> list[Path]:
    """Every customer template, message file and stylesheet under app/."""
    found = {p for pattern in CUSTOMER_GLOBS for p in ROOT.glob(pattern) if p.is_file()}
    return sorted(found)


def kinds() -> list[Path]:
    """One real customer file per suffix and crate, as plant targets."""
    seen: dict[tuple[str, str], Path] = {}
    for path in customer_files():
        crate = path.relative_to(ROOT).parts[1]
        seen.setdefault((crate, path.suffix), path)
    return sorted(seen.values())


def fake_repo(tmp_path: Path) -> Path:
    """A repository holding only the AI-smell list."""
    (tmp_path / "docs").mkdir()
    shutil.copy(ROOT / "docs" / "ai_smell.yaml", tmp_path / "docs" / "ai_smell.yaml")
    return tmp_path


def plant(root: Path, rel: str, text: str) -> set[str]:
    """The rules the scan names after writing `text` to `rel` under `root`."""
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return {hit.rule for hit in check_ai_smell.run(root, [])}


def test_td_word_11_every_customer_file_is_scanned_and_passes() -> None:
    files = customer_files()
    assert any(p.name == "en.json" for p in files)
    assert any(p.name == "es.json" for p in files)
    assert any(p.suffix == ".css" for p in files)
    assert len([p for p in files if p.suffix == ".html"]) >= 50
    scanned = {p.resolve() for p in check_ai_smell.default_paths(ROOT)}
    assert [p for p in files if p.resolve() not in scanned] == []
    hits = check_ai_smell.run(ROOT, files)
    assert [hit.show() for hit in hits] == []


@pytest.mark.parametrize("real", kinds(), ids=lambda p: p.relative_to(ROOT).as_posix())
@pytest.mark.parametrize(("smell", "rule"), PLANTED)
def test_td_word_11_a_planted_smell_in_a_real_file_fails(tmp_path: Path, real: Path, smell: str, rule: str) -> None:
    if real.suffix == ".css":
        pytest.skip("a stylesheet carries no copy; its habits are planted below")
    root, rel = fake_repo(tmp_path), real.relative_to(ROOT).as_posix()
    source = real.read_text(encoding="utf-8")
    line = f"<p>{smell}</p>" if real.suffix in check_ai_smell.MARKUP else smell
    planted = f"{source}\n{line}\n"
    assert planted != source
    assert rule in plant(root, rel, planted)


def test_word_028_logged_words_are_listed_and_fail() -> None:
    listed = yaml.safe_load((ROOT / "docs" / "ai_smell.yaml").read_text(encoding="utf-8"))["words"]
    rules = check_ai_smell.load_lists(ROOT)
    for word in LOGGED:
        assert word in listed, word
        hits = list(check_ai_smell.scan_text(rules, "planted", 1, f"Fake line: {word} here."))
        assert f"word '{word}'" in {hit.rule for hit in hits}, word


def test_td_word_11_voice_guide_and_brief_name_ai_smell() -> None:
    voice = (ROOT / "docs" / "voice.md").read_text(encoding="utf-8")
    assert "\n## AI smell" in voice
    brief = (ROOT / "BUILD_BRIEF.md").read_text(encoding="utf-8")
    section_7 = brief.split("\n## 7.", 1)[1].split("\n## 8.", 1)[0]
    assert "AI smell (WORD-027, WORD-028, D-452)" in section_7
    assert "scripts/check_ai_smell.py" in section_7

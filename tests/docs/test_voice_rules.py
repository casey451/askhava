"""The WORD-024 voice rule's pressure-word scan (T161).

TD-WORD-08 (WORD-024; D-406, D-447): the pressure words WORD-024 names (hurry, don't miss out, act
now, limited time, last chance, apúrese, no se lo pierda) are one list, the `words` of
`docs/ai_smell.yaml`, which `scripts/check_ai_smell.py` runs over every template, message file and
stylesheet under `app/` and the spec's quoted copy (WORD-003's scan list notes point here, D-4258).
This test checks that the list there is WORD-024's list, that each word planted in a homeowner or
business template fails with the word named, that every real template passes, and that the
safety (911) template is exempt from rewriting and unchanged: it still equals INT-004 word for
word. The list is read from the spec and the YAML, never retyped here.

Run: python3 -m pytest tests/docs/test_voice_rules.py -q -p no:cacheprovider
"""

from __future__ import annotations

import functools
import json
import re
from pathlib import Path

import pytest
import yaml

import brand
import check_ai_smell

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "messaging" / "templates"

# The safety texts WORD-024 exempts: the 911 text (INT-004), in English and Spanish.
SAFETY_KEYS = ("emergency_911",)


def spec_quotes(spec: str, req_id: str) -> list[str]:
    """The double-quoted strings of one requirement, before its "Source:" (docs/spec/)."""
    for line in (ROOT / "docs" / "spec" / spec).read_text(encoding="utf-8").splitlines():
        if line.startswith(f"- **{req_id}** "):
            return re.findall(r'"([^"]+)"', line.split(" Source:")[0])
    raise AssertionError(f"{req_id} not found in docs/spec/{spec}")


def pressure_words() -> list[str]:
    """WORD-024's pressure words, as the requirement quotes them."""
    return spec_quotes("wording.md", "WORD-024")


@functools.cache
def lists() -> check_ai_smell.Lists:
    """The AI-smell lists as the build's scan loads them."""
    return check_ai_smell.load_lists(ROOT)


def pressure_hits(text: str) -> list[str]:
    """The pressure-word rules `text` breaks, by rule name."""
    names = {f"word '{word}'" for word in pressure_words()}
    text = text.replace("\u2019", "'").replace("\u2018", "'")
    hits = check_ai_smell.scan_text(lists(), "planted", 1, text)
    return [hit.rule for hit in hits if hit.rule in names]


def templates() -> list[tuple[str, str, str, str]]:
    """Every text template as (language, key, audience, text), brand tokens filled."""
    found = []
    for lang in ("en", "es"):
        data = json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))
        for key, entry in data.items():
            text = brand.fill(entry["text"], spanish=lang == "es")
            found.append((lang, key, entry["audience"], text))
    return found


def test_td_word_08_the_pressure_list_is_one_list_in_ai_smell() -> None:
    words = pressure_words()
    assert len(words) == 7
    listed = yaml.safe_load((ROOT / "docs" / "ai_smell.yaml").read_text(encoding="utf-8"))
    assert set(words) <= set(listed["words"])
    assert "WORD-024" in listed["not_copy_requirements"]


@pytest.mark.parametrize("word", pressure_words())
@pytest.mark.parametrize("audience", ["homeowner", "business"])
def test_td_word_08_a_planted_pressure_word_fails(word: str, audience: str) -> None:
    good = next(t for _, k, a, t in templates() if a == audience and k not in SAFETY_KEYS)
    planted = f"{good} {word[0].upper()}{word[1:]}, reply today."
    assert pressure_hits(good) == []
    assert pressure_hits(planted) == [f"word '{word}'"]


def test_td_word_08_a_curly_apostrophe_does_not_hide_a_pressure_word() -> None:
    assert pressure_hits("Don\u2019t miss out on quotes.") == ["word 'don't miss out'"]
    assert pressure_hits("Don\u2018t miss out on quotes.") == ["word 'don't miss out'"]


@pytest.mark.parametrize(("lang", "key", "audience", "text"), templates(), ids=lambda v: str(v)[:40])
def test_td_word_08_every_real_template_passes(lang: str, key: str, audience: str, text: str) -> None:
    if key in SAFETY_KEYS:
        pytest.skip("WORD-024: the 911 text is exempt and unchanged (checked below)")
    assert pressure_hits(text) == [], f"{lang}.json {key} ({audience})"


def test_td_word_08_the_build_scan_finds_no_pressure_word() -> None:
    names = {f"word '{word}'" for word in pressure_words()}
    hits = [hit.show() for hit in check_ai_smell.run(ROOT, []) if hit.rule in names]
    assert hits == []


def test_td_word_08_the_911_text_is_exempt_and_unchanged() -> None:
    (int_004,) = spec_quotes("intake.md", "INT-004")
    english = {k: t for lang, k, _, t in templates() if lang == "en"}
    spanish = {k: t for lang, k, _, t in templates() if lang == "es"}
    assert english["emergency_911"] == int_004
    assert "911" in spanish["emergency_911"]
    assert "INT-004" in lists().exempt

"""The `ai` crate's WORD-022 phrase list equals `scripts/trace_spec.py`'s (AI-019, WORD-022, D-493).

`app/ai/data/word_022_phrases.json` holds, for each pattern of `trace_spec._NOT_A_CONTRACTOR_PATTERNS`,
the pattern itself and its phrases as slots (lists of alternatives the Rust check joins in order, a
trailing `*` meaning "any word letters follow"), because the app has no regex crate. This test fails
when the patterns differ in text or order, or when a pattern's slots do not spell exactly the phrases
the regex matches (both sides expanded to sets of lower-case phrases). Fake data only.

Run: python3 -m pytest tests/docs/test_word_022_phrases.py -q -p no:cacheprovider
"""

from __future__ import annotations

import itertools
import json
from pathlib import Path

import pytest

try:
    import re._constants as sre_constants
    import re._parser as sre_parse
except ImportError:  # Python before 3.11
    import sre_constants  # type: ignore[no-redef]
    import sre_parse  # type: ignore[no-redef]

import trace_spec

PHRASES_FILE = Path(__file__).resolve().parents[2] / "app" / "ai" / "data" / "word_022_phrases.json"
WILD = "*"


def _items(items: list) -> set[str]:
    out = {""}
    for op, av in items:
        out = {a + b for a in out for b in _item(op, av)}
    return out


def _item(op, av) -> set[str]:
    c = sre_constants
    if op is c.LITERAL:
        return {chr(av)}
    if op is c.AT:
        return {""}
    if op is c.IN:
        if any(kind is not c.LITERAL for kind, _ in av):
            raise ValueError("only literal character classes are expandable")
        return {chr(v) for _, v in av}
    if op is c.SUBPATTERN:
        return _items(list(av[-1]))
    if op is c.BRANCH:
        return set().union(*(_items(list(b)) for b in av[1]))
    if op in (c.MAX_REPEAT, c.MIN_REPEAT):
        low, high, sub = av
        if (low, high) == (0, 1):
            return {""} | _items(list(sub))
        if (low, high) == (0, c.MAXREPEAT) and list(sub) == [(c.IN, [(c.CATEGORY, c.CATEGORY_WORD)])]:
            return {WILD}
    raise ValueError(f"cannot expand {op}")


def regex_phrases(pattern: str) -> set[str]:
    """Every phrase `pattern` matches, lower-cased; `\\w*` becomes a trailing `*`."""
    return {p.lower() for p in _items(list(sre_parse.parse(pattern)))}


def slot_phrases(slots: list[list[str]]) -> set[str]:
    """Every phrase the JSON slots spell, as the Rust check joins them."""
    return {"".join(parts).lower() for parts in itertools.product(*slots)}


def _entries() -> list[dict]:
    return json.loads(PHRASES_FILE.read_text(encoding="utf-8"))["patterns"]


def test_patterns_equal_trace_spec_in_order():
    """TD-AI-08 (AI-019, WORD-022): one list, the spec check's, in the same order."""
    assert [e["regex"] for e in _entries()] == list(trace_spec._NOT_A_CONTRACTOR_PATTERNS)


@pytest.mark.parametrize("entry", _entries(), ids=lambda e: e["regex"][:40])
def test_slots_spell_exactly_the_regex_phrases(entry):
    """TD-AI-08 (AI-019, WORD-022): the Rust phrases match what the regex matches, no more, no less."""
    phrases = slot_phrases(entry["slots"])
    assert phrases == regex_phrases(entry["regex"])
    assert all(WILD not in p[:-1] for p in phrases), "a wildcard only ends a phrase"


def test_expander_catches_a_dropped_alternative():
    """The equality check is not vacuous: dropping one alternative is caught."""
    slots = [["our "], ["pros", "pro"]]
    assert slot_phrases(slots) == regex_phrases(r"\bour pros?\b")
    assert slot_phrases([["our "], ["pros"]]) != regex_phrases(r"\bour pros?\b")
    assert regex_phrases(r"\bsubcontract\w*") == {"subcontract*"}

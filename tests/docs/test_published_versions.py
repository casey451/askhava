"""Every published version of the four terms documents is kept once, with its SHA-256 (TD-TERMS-09;
TERMS-034, D-492, D-3144).

`docs/legal/published/versions.json` is a list, empty until the owner's publishing go (WEB-013,
WEB-017); the publishing change adds one entry per document version: `document` (one of the four),
`version`, `text` (the full text as published) and `sha256` (the hex SHA-256 of `text` in UTF-8). A
published entry never changes; a later edit is a new entry with a new version. This test checks the
shape, that each version appears once, and that every entry's text still matches its hash. The
planted cases prove that one changed character fails. Fake data only.

Run: python3 -m pytest tests/docs/test_published_versions.py -q -p no:cacheprovider
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

VERSIONS = Path(__file__).resolve().parents[2] / "docs" / "legal" / "published" / "versions.json"
DOCUMENTS = frozenset({"homeowner-terms", "business-terms", "privacy-policy", "sms-terms"})
FIELDS = frozenset({"document", "version", "text", "sha256"})


def problems(entries: object) -> list[str]:
    """Each way `entries` breaks TERMS-034: shape, a repeated version, or a text off its hash."""
    if not isinstance(entries, list):
        return ["versions.json must hold a list"]
    found: list[str] = []
    seen: set[tuple[str, str]] = set()
    for i, entry in enumerate(entries):
        if not isinstance(entry, dict) or set(entry) != FIELDS:
            found.append(f"entry {i}: fields must be exactly {sorted(FIELDS)}")
            continue
        if not all(isinstance(entry[f], str) and entry[f] for f in FIELDS):
            found.append(f"entry {i}: every field must be a non-empty string")
            continue
        if entry["document"] not in DOCUMENTS:
            found.append(f"entry {i}: unknown document {entry['document']!r}")
        key = (entry["document"], entry["version"])
        if key in seen:
            found.append(f"entry {i}: {key[0]} version {key[1]} is kept twice")
        seen.add(key)
        if hashlib.sha256(entry["text"].encode("utf-8")).hexdigest() != entry["sha256"]:
            found.append(f"entry {i}: {key[0]} version {key[1]} no longer matches its SHA-256")
    return found


def _entry(text: str, version: str = "1.0") -> dict[str, str]:
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    return {"document": "sms-terms", "version": version, "text": text, "sha256": digest}


def test_td_terms_09_published_versions_match_their_hashes() -> None:
    """TD-TERMS-09 (TERMS-034): the committed file is a list whose every entry matches its hash."""
    entries = json.loads(VERSIONS.read_text(encoding="utf-8"))
    assert problems(entries) == []


def test_td_terms_09_one_changed_character_fails() -> None:
    """TD-TERMS-09: changing one character of a published text fails."""
    entry = _entry("Fake terms text, version one.")
    assert problems([entry]) == []
    entry["text"] = entry["text"].replace("one", "onE")
    assert problems([entry]) == ["entry 0: sms-terms version 1.0 no longer matches its SHA-256"]


def test_td_terms_09_a_later_edit_is_a_new_version() -> None:
    """TD-TERMS-09: a later edit is a new entry beside the earlier one, which stays byte for byte."""
    first = _entry("Fake terms text, version one.")
    kept = dict(first)
    later = _entry("Fake terms text, version two.", "1.1")
    assert problems([first, later]) == []
    assert first == kept
    assert problems([first, _entry("Changed in place.")]) == [
        "entry 1: sms-terms version 1.0 is kept twice",
    ]


@pytest.mark.parametrize(
    ("entries", "problem"),
    [
        ({}, "versions.json must hold a list"),
        ([{"document": "sms-terms"}], "entry 0: fields must be exactly"),
        ([{**_entry("x"), "document": "other"}], "entry 0: unknown document 'other'"),
        ([{**_entry("x"), "version": ""}], "entry 0: every field must be a non-empty string"),
    ],
)
def test_td_terms_09_bad_shapes_fail(entries: object, problem: str) -> None:
    """TD-TERMS-09: a malformed file or entry fails, naming what is wrong."""
    found = problems(entries)
    assert found and found[0].startswith(problem)

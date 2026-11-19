"""Each audience's word for the request number, checked per template (T161).

TD-WORD-09 (WORD-025; D-407): every homeowner text template calls the number "request <n>"
("solicitud <n>") and every business one "job <n>" ("trabajo <n>"); a planted "job 1042" in a
homeowner template fails, as does a planted "request 1042" in a business one. The number is the
`{job}` placeholder (or a written number); a reply keyword before it (CLOSE, LIST, LINK, CERRAR,
LISTA) is not a name for it. Every template that shows the number names it at least once, in its
audience's word or a reply keyword.

Run: python3 -m pytest tests/docs/test_request_job_numbering.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "messaging" / "templates"

WORDS = {
    "homeowner": frozenset({"request", "requests", "solicitud", "solicitudes"}),
    "business": frozenset({"job", "jobs", "trabajo", "trabajos"}),
}
NOUNS = WORDS["homeowner"] | WORDS["business"]
NUMBERED = re.compile(r"([\wáéíóúñ]+) (?:\{job\}|#?\d+\b)")
NUMBER = re.compile(r"\{job\}")


def templates() -> list[tuple[str, str, str, str]]:
    """Every text template as (language, key, audience, text)."""
    found = []
    for lang in ("en", "es"):
        data = json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))
        found += [(lang, key, entry["audience"], entry["text"]) for key, entry in data.items()]
    return found


def wrong_words(audience: str, text: str) -> list[str]:
    """Each numbered noun in `text` that is not `audience`'s word ("job 1042" to a homeowner)."""
    return [
        match.group(0)
        for match in NUMBERED.finditer(text)
        if match.group(1).lower() in NOUNS and match.group(1).lower() not in WORDS[audience]
    ]


def names_the_number(audience: str, text: str) -> bool:
    """Whether a `{job}` in `text` is named by `audience`'s word or a reply keyword."""
    for match in NUMBERED.finditer(text):
        word = match.group(1)
        if word.lower() in WORDS[audience] or word.isupper():
            return True
    return False


def test_td_word_09_both_audiences_are_present() -> None:
    audiences = {audience for _, _, audience, _ in templates()}
    assert audiences == set(WORDS)


@pytest.mark.parametrize(("lang", "key", "audience", "text"), templates(), ids=lambda v: str(v)[:40])
def test_td_word_09_each_template_uses_its_audiences_word(
    lang: str, key: str, audience: str, text: str
) -> None:
    assert wrong_words(audience, text) == [], f"{lang}.json {key}"
    if NUMBER.search(text):
        assert names_the_number(audience, text), f"{lang}.json {key}: the number is unnamed"


@pytest.mark.parametrize(
    ("audience", "planted"),
    [
        ("homeowner", "{brand_short}: Quotes for job 1042 are in."),
        ("homeowner", "{brand_short}: Su trabajo {job} tiene cotizaciones."),
        ("business", "{brand_short}: Request 1042 picked you."),
        ("business", "{brand_short}: La solicitud {job} lo eligió."),
    ],
)
def test_td_word_09_planted_wrong_word_fails(audience: str, planted: str) -> None:
    assert wrong_words(audience, planted)


def test_td_word_09_right_words_and_keywords_pass() -> None:
    assert wrong_words("homeowner", "Request {job}: reply CLOSE {job} to close it.") == []
    assert wrong_words("business", "Job 1042 is yours. Trabajo {job}.") == []
    assert names_the_number("homeowner", "Reply LIST {job} for your quotes.")
    assert not names_the_number("homeowner", "Your quotes for {job} are in.")

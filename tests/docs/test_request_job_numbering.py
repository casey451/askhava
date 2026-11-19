"""Each audience's word for the request number, checked per template (T161).

TD-WORD-09 (WORD-025; D-407): every homeowner text template calls the number "request <n>"
("solicitud <n>") and every business one "job <n>" ("trabajo <n>"); a planted "job 1042" in a
homeowner template fails, as does a planted "request 1042" in a business one. The number is the
`{job}` placeholder (or a written number); a reply keyword before it (CLOSE, LIST, LINK, CERRAR,
LISTA) is not a name for it. Every template that shows the number names it at least once, in its
audience's word or a reply keyword. WORD-025 also covers pages: each portal and site page template
that numbers a request or job is classified by audience (`PORTAL_PAGES`, or a site page's name) and
held to the same word; the owner's pages are left out, as the owner reads both audiences' records.
The noun counts as the number's name in "job 1042", "job #{job}", "job number {job}", "Job: {job}",
"job ({job})" and "job {{ job_number }}".

Run: python3 -m pytest tests/docs/test_request_job_numbering.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "messaging" / "templates"
PORTAL = ROOT / "app" / "portals" / "templates"
SITE = ROOT / "app" / "site" / "templates"
PORTAL_PAGES = {
    "homeowner": ("checkin", "feedback", "homeowner_home", "photo_upload", "pick_code", "pick_review",
                  "quote_page"),
    "business": ("business_actions", "confirm_pay", "job_release", "not_a_fit", "notice_log",
                 "why_charged"),
}

WORDS = {
    "homeowner": frozenset({"request", "requests", "solicitud", "solicitudes"}),
    "business": frozenset({"job", "jobs", "trabajo", "trabajos"}),
}
NOUNS = WORDS["homeowner"] | WORDS["business"]
NUMBERED = re.compile(
    r"([\wáéíóúñ]+)(?: number| número| no\.)?:? [#(]?(?:\{job\}|\{\{ *[\w.]+ *\}\}|#?\d+\b)",
    re.IGNORECASE,
)
NUMBER = re.compile(r"\{job\}")


def templates() -> list[tuple[str, str, str, str]]:
    """Every text template as (language, key, audience, text)."""
    found = []
    for lang in ("en", "es"):
        data = json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))
        found += [(lang, key, entry["audience"], entry["text"]) for key, entry in data.items()]
    return found


def pages() -> list[tuple[str, str, str]]:
    """Every portal and site page template that numbers a request or job, as (name, audience, text).

    An unclassified page comes back with audience "unclassified", which the test fails.
    """
    audience_of = {name: who for who, names in PORTAL_PAGES.items() for name in names}
    found = []
    for path in sorted([*PORTAL.glob("*.html"), *SITE.glob("*.html")]):
        text = path.read_text(encoding="utf-8")
        if not any(m.group(1).lower() in NOUNS for m in NUMBERED.finditer(text)):
            continue
        if path.parent == SITE:
            audience = "business" if "business" in path.stem else "homeowner"
        elif path.stem.startswith("owner_"):
            continue
        else:
            audience = audience_of.get(path.stem, "unclassified")
        found.append((f"{path.parent.parent.name}/{path.name}", audience, text))
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


@pytest.mark.parametrize(("name", "audience", "text"), pages(), ids=lambda v: str(v)[:40])
def test_td_word_09_each_page_uses_its_audiences_word(name: str, audience: str, text: str) -> None:
    assert audience in WORDS, f"{name}: add it to PORTAL_PAGES"
    assert wrong_words(audience, text) == [], name


def test_td_word_09_pages_of_both_audiences_are_checked() -> None:
    assert {audience for _, audience, _ in pages()} == set(WORDS)


@pytest.mark.parametrize(
    ("audience", "planted"),
    [
        ("homeowner", "Request {job} is in. Your job #{job} has quotes."),
        ("homeowner", "Your job number {job} has quotes."),
        ("homeowner", "Job: {job}. Quotes are in."),
        ("homeowner", "Quotes for your job ({job}) are in."),
        ("homeowner", "<h1>Your quotes for job {{ request }}</h1>"),
        ("business", "<h1>Request {{ job_number }} picked you</h1>"),
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

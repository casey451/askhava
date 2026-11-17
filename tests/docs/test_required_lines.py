"""Each required line on each page template that needs it (TD-WORD-02; WORD-002, WORD-009, WORD-013,
WEB-002, WEB-019, D-485, D-493, D-3878, D-3880).

The templates are read as written: a required line is literal template text, never a Rust field
(D-3878), with only the brand tokens ({{ brand }} or a `brand()` name call) filled from
`app/core/data/brand.json` (D-1932). This file covers T202's cases (D-3880): the not-a-contractor
line, the automated-texts line, the Spanish key terms and the WORD-013 comparison. The quote-page,
insurance, privacy-copy and independent-business cases are T203's (D-493).

The Spanish pages are found by pattern, never by a fixed list: every `es_*.html` under
`app/site/templates/` and `app/portals/templates/` (TD-WORD-02, WORD-013). A Spanish page is
compared with its English counterpart, the same name without `es_` in the same folder, when it has
one. Fake data only.

Run: python3 -m pytest tests/docs/test_required_lines.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SITE = ROOT / "app" / "site" / "templates"
FOLDERS = (SITE, ROOT / "app" / "portals" / "templates")
BRAND = json.loads((ROOT / "app" / "core" / "data" / "brand.json").read_text(encoding="utf-8"))
NAME = BRAND["display_name"]
BRAND_TOKEN = re.compile(
    r"\{\{\s*(?:brand|askhava_core::api::brand\(\)(?:\.es\(\))?\.display_name\(\))\s*\}\}"
)

# WORD-002's long form, in English (the site footer) and in Spanish (each Spanish page's footer).
LONG_EN = (
    f"{NAME} is not a contractor and does not perform, bid on or supervise work. We pass along your"
    " request and show you quotes from independent local businesses, as they wrote them. You choose,"
    " and you hire the business directly."
)
LONG_ES = (
    f"{NAME} no es contratista y no realiza, cotiza ni supervisa trabajos. Pasamos su solicitud y le"
    " mostramos cotizaciones de negocios locales independientes, tal como las escribieron. Usted elige"
    " y contrata directamente al negocio."
)
# WORD-002's business statement (the business page, CARD-020, BIZ-030, TERMS-026).
BUSINESS_EN = (
    f"{NAME} is not a contractor. It passes along this request and the homeowner's choice. If the"
    f" homeowner picks you, they hire you directly; {NAME} doesn't direct, schedule, supervise or pay"
    " for the work."
)
BUSINESS_ES = f"{NAME} no es contratista. Hace llegar esta solicitud y la elección del propietario."
# WORD-009's automated-texts line on the help page.
AUTOMATED = f"{NAME} is a local service. Our texts are automated."

# WORD-013: what in an English page calls for which Spanish line on its twin.
FEE = ("introduction fee", "tarifa de presentación")
STOP = (re.compile(r"\bSTOP\b"), "PARAR")
HELP = (re.compile(r"\bHELP\b"), "AYUDA")
PRIVACY = (
    ("Your number is never shared", "Su número nunca se comparte"),
    (
        "Your first name and address go only to the business you pick",
        "Su nombre de pila y su dirección van solo al negocio que usted elija",
    ),
)


def text(path: Path) -> str:
    """The template with its brand tokens filled and whitespace collapsed."""
    return " ".join(BRAND_TOKEN.sub(NAME, path.read_text(encoding="utf-8")).split())


def spanish_pages() -> list[Path]:
    """Every Spanish page template, found by pattern (TD-WORD-02)."""
    return sorted(p for folder in FOLDERS for p in folder.glob("es_*.html"))


def site_pages() -> list[Path]:
    """Every site page that extends the site layout."""
    extends = '{% extends "site_layout.html" %}'
    return sorted(p for p in SITE.glob("*.html") if extends in p.read_text(encoding="utf-8"))


def word_013_gaps(english: str, spanish: str) -> list[str]:
    """The WORD-013 lines the English page carries and the Spanish page lacks."""
    gaps = []
    if FEE[0] in english.lower() and FEE[1] not in spanish.lower():
        gaps.append("fee line")
    for pattern, word in (STOP, HELP):
        if pattern.search(english) and word not in spanish:
            gaps.append(word)
    gaps += [f"privacy line ({es})" for en, es in PRIVACY if en in english and es not in spanish]
    return gaps


def test_td_word_02_the_site_layout_carries_the_long_statement() -> None:
    """WORD-002: the footer every English site page inherits holds the long statement."""
    assert LONG_EN in text(SITE / "site_layout.html")


@pytest.mark.parametrize("page", site_pages(), ids=lambda p: p.name)
def test_td_word_02_a_page_that_replaces_the_footer_keeps_the_statement(page: Path) -> None:
    """WORD-002: a site page that overrides the footer carries the long statement in its language."""
    body = text(page)
    if "{% block footer %}" in body:
        assert LONG_ES in body or LONG_EN in body, page.name


@pytest.mark.parametrize("name,line", [("business.html", BUSINESS_EN), ("es_business.html", BUSINESS_ES)])
def test_td_word_02_the_business_pages_carry_the_business_statement(name: str, line: str) -> None:
    """WORD-002: the business page carries the business statement, in English and Spanish."""
    assert line in text(SITE / name)


def test_td_word_02_the_help_page_says_texts_are_automated() -> None:
    """WORD-009: the help page holds the automated-texts line word for word."""
    assert AUTOMATED in text(SITE / "help.html")


def test_td_word_02_the_spanish_pages_are_found() -> None:
    """WEB-002, WEB-019: the Spanish home, how-it-works and business pages exist."""
    names = {p.name for p in spanish_pages()}
    assert {"es_home.html", "es_how_it_works.html", "es_business.html"} <= names


@pytest.mark.parametrize("page", spanish_pages(), ids=lambda p: p.name)
def test_td_word_02_spanish_key_terms(page: Path) -> None:
    """WEB-019, WORD-002: a Spanish page is in Spanish, says "no es contratista" and is marked for
    native-speaker review (D-180)."""
    body = text(page)
    assert re.search(r'\{% block lang %\}es\{% endblock %\}|<html lang="es"', body), page.name
    assert "no es contratista" in body, page.name
    assert "native-speaker review" in body, page.name


@pytest.mark.parametrize("page", spanish_pages(), ids=lambda p: p.name)
def test_td_word_02_word_013_spanish_page_matches_its_english_page(page: Path) -> None:
    """WORD-013: the Spanish page carries each of the fee line, PARAR and AYUDA and the privacy line
    its English counterpart carries."""
    english = page.with_name(page.name.removeprefix("es_"))
    if not english.exists():
        pytest.skip(f"{page.name} has no English counterpart in its folder")
    assert word_013_gaps(text(english), text(page)) == [], page.name


def test_td_word_02_the_comparison_catches_a_missing_line() -> None:
    """WORD-013: the comparison names each missing line (a fake pair of pages)."""
    english = "Reply STOP to opt out, HELP for help. A set introduction fee. Your number is never shared."
    missing = ["fee line", "PARAR", "AYUDA", "privacy line (Su número nunca se comparte)"]
    assert word_013_gaps(english, "Nada aquí.") == missing
    assert word_013_gaps(english, "PARAR, AYUDA, tarifa de presentación. Su número nunca se comparte.") == []

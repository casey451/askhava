"""No phone number but 911 in any template or page (TD-INT-17, INT-004, D-436).

The safety-contacts file of D-350 is withdrawn (D-436): no local, utility, 211 or other safety
number is ever given to a homeowner; the only number a text or page may show is 911. This scan
reads every text template (app/messaging/templates/en.json and es.json) and every file under a
`templates` folder in app/ (pages and emails), and fails on a written phone number other than 911
(10-digit and 7-digit forms, +1, tel: links), any other N11 code such as 211 or 311, a utility or
propane company name, or "cooling center". A `{placeholder}` filled at send time is not a written
number. It also checks that no safety-contacts file exists or is loaded. Planted cases use fake
555-01xx numbers only.

Run: python3 -m pytest tests/docs/test_no_phone_numbers_in_templates.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "app"
TEXTS = APP / "messaging" / "templates"

# A US number: optional +1 or 1, area code with or without brackets, then 3 and 4 digits.
TEN_DIGIT = re.compile(r"(?<![\w.])(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}(?![\w])")
SEVEN_DIGIT = re.compile(r"(?<![\w.-])\d{3}[\s.-]\d{4}(?![\w-])")
TEL_LINK = re.compile(r"\btel:\s*(?!911\b)[+\d(]", re.IGNORECASE)
N11 = re.compile(r"(?<![\w.$-])[2-8]11(?![\w-])")
UTILITIES = re.compile(
    r"\b(unisource|epcor|southwest gas|mohave electric|arizona public service|havasu heights|"
    r"amerigas|ferrellgas|suburban propane|poison control|city water|water company|"
    r"gas company|power company|electric company|cooling cent(?:er|re)s?|centros? de enfriamiento)\b"
    r"|\bAPS\b",
    re.IGNORECASE,
)


def numbers_in(text: str) -> list[str]:
    """Every phone number, N11 code, tel: link or utility name the text holds, other than 911."""
    found = [m.group(0) for m in TEN_DIGIT.finditer(text)]
    found += [m.group(0) for m in SEVEN_DIGIT.finditer(text)]
    found += [m.group(0) for m in TEL_LINK.finditer(text)]
    found += [m.group(0) for m in N11.finditer(text)]
    found += [m.group(0) for m in UTILITIES.finditer(text)]
    return found


def text_templates() -> list[tuple[str, str]]:
    out = []
    for lang in ("en", "es"):
        entries = json.loads((TEXTS / f"{lang}.json").read_text(encoding="utf-8"))
        out += [(f"{lang}:{key}", entry["text"]) for key, entry in entries.items()]
    return out


def page_files() -> list[Path]:
    """Every file in a `templates` folder under app/ other than the two text files."""
    files = {p for d in APP.rglob("templates") if d.is_dir() for p in d.rglob("*") if p.is_file()}
    return sorted(p for p in files if p.parent != TEXTS and "target" not in p.parts)


@pytest.mark.parametrize(("name", "text"), text_templates())
def test_no_text_template_holds_a_number_other_than_911(name: str, text: str) -> None:
    """TD-INT-17, INT-004: each English and Spanish text template's only number is 911."""
    assert numbers_in(text) == [], name


@pytest.mark.parametrize("path", page_files(), ids=lambda p: str(p.relative_to(ROOT)))
def test_no_page_holds_a_number_other_than_911(path: Path) -> None:
    """TD-INT-17, INT-004: no page or email template shows a phone number other than 911."""
    assert numbers_in(path.read_text(encoding="utf-8")) == [], path


def test_the_scan_covers_the_pages() -> None:
    """The page scan reads at least the shared site layout, so an empty glob cannot pass silently."""
    assert APP / "site" / "templates" / "base.html" in page_files()


def test_no_safety_contacts_file_exists_or_is_loaded() -> None:
    """TD-INT-17, D-436: the withdrawn safety-contacts file is absent and no app code names it."""
    assert not (APP / "messaging" / "safety_contacts.toml").exists()
    named = [
        p for p in APP.rglob("*")
        if p.is_file() and "target" not in p.parts and p.suffix in {".rs", ".toml", ".json", ".py"}
        and "safety_contacts" in p.read_text(encoding="utf-8", errors="ignore")
    ]
    assert named == []


@pytest.mark.parametrize(
    "planted",
    [
        "Ask Hava: Call 928-555-0142 for help.",
        "Ask Hava: Call (928) 555-0142 for help.",
        "Ask Hava: Call 928.555.0142 for help.",
        "Ask Hava: Call 9285550142 for help.",
        "Ask Hava: Call 1-928-555-0142 for help.",
        "Ask Hava: Call +1 928 555 0142 for help.",
        "Ask Hava: Call 555-0142 after hours.",
        '<a href="tel:+19285550199">Call us</a>',
        "Ask Hava: For help with bills, call 211.",
        "Ask Hava: Dial 311 for the city.",
        "Ask Hava: Call 811 before you dig.",
        "Ask Hava: Call UniSource about the gas.",
        "Ask Hava: Call EPCOR about the water.",
        "Ask Hava: Call APS if the power is out.",
        "Ask Hava: Find a cooling center near you.",
        "Ask Hava: Busque un centro de enfriamiento.",
        "Ask Hava: Call your water company.",
    ],
)
def test_the_scan_catches_a_planted_number(planted: str) -> None:
    """TD-INT-17, TD-OPS-17 (INT-004, D-436): a number other than 911, 211 or a utility fails."""
    assert numbers_in(planted) != [], planted


def test_the_scan_allows_911_and_ordinary_numbers() -> None:
    """911, a tel:911 link, fees, hours, years, times, ages and placeholders are not phone numbers."""
    clean = (
        "Ask Hava: Call 911 from outside. Llame al 911. <a href=\"tel:911\">911</a> "
        "$25, $75 or $199. Within 24 hours or 2 days, 8 a.m. to 8 p.m. In 2026, 18 or older. "
        "Request 1042. Call them back on {number}. Apps and maps."
    )
    assert numbers_in(clean) == []

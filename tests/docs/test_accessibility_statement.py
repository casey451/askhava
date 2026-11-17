"""The accessibility statement offers no phone line (T115; TD-WEB-08, A11Y-020, CALL-013, CALL-020).

D-379, D-3883, D-4240's closed lists: the statement document as written and the reader text of
the three templates `/accessibility` shows, whitespace squashed and List A (true wording) removed,
match no List B pattern nor the phone patterns imported from test_no_phone_numbers_in_templates.py;
the document holds A11Y-020's required lines. The brand is read from the brand file. Fake data only.

Run: python3 -m pytest tests/docs/test_accessibility_statement.py -q -p no:cacheprovider
"""

from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
STATEMENT = ROOT / "docs" / "legal" / "accessibility-statement.md"
TEMPLATES = ROOT / "app" / "site" / "templates"
BRAND = json.loads((ROOT / "app" / "core" / "data" / "brand.json").read_text(encoding="utf-8"))
NAME, DOMAIN = BRAND["display_name"], BRAND["domain"]

_SPEC = importlib.util.spec_from_file_location(
    "no_phone_numbers", ROOT / "tests" / "docs" / "test_no_phone_numbers_in_templates.py"
)
assert _SPEC is not None and _SPEC.loader is not None
_PHONES = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_PHONES)

LIST_A = [
    "at phone, tablet and desktop widths",
    "so no notice needs a phone call or audio",
    "Masked calls between a homeowner and the business they chose work with 711 relay services.",
    "A call to it hears a short recorded message asking you to text; there is no voicemail.",
    "If you use a relay service, you can reach us by text or email.",
]

LIST_B = [
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\bcall\w*", r"\b(?:tele)?phon(?:e|es|ed|ing)\b", r"\bdial(?:s|ed|ing)?\b", r"\bring\b",
        r"\bvoice(?:s|mail\w*)?\b", r"\b(?:talk|speak)\w*\s+(?:to|with)\b", r"\b711\b",
        r"\b(?:TTY|TDD|VRS)\b", r"\bvideo relay\b", r"\brelay\w*", r"\bhotline\b", r"\bhelp ?line\b",
        r"\btoll[- ]free\b", r"\bll[aá]m\w*", r"\btel[eé]fon\w*", r"\bvoz\b",
    )
] + [_PHONES.TEN_DIGIT, _PHONES.SEVEN_DIGIT, _PHONES.TEL_LINK]  # fmt: skip

REQUIRED = [
    "WCAG 2.2 Level AA", "Known limits", "by text", f"by email at help@{DOMAIN}", "the help page's form",
    "If you use a relay service, you can reach us by text or email.", "usually replies within 2 days",
]  # fmt: skip


def squash(text: str) -> str:
    """Whitespace runs as one space."""
    return re.sub(r"\s+", " ", text)


def offers(text: str) -> list[str]:
    """TD-WEB-08, A11Y-020, CALL-020: every List B match left once List A is removed."""
    left = squash(text)
    for line in LIST_A:
        left = left.replace(line, " ")
    return [m.group(0) for pattern in LIST_B for m in pattern.finditer(left)]


def reader_text(template: str) -> str:
    """A template's reader text: comments, statements, expressions and tags removed."""
    for pattern in (r"\{#.*?#\}", r"\{%.*?%\}", r"\{\{.*?\}\}", r"<[^>]*>"):
        template = re.sub(pattern, " ", template, flags=re.DOTALL)
    return template


def statement() -> str:
    """The statement document as written."""
    return STATEMENT.read_text(encoding="utf-8")


def scanned() -> list[tuple[str, str]]:
    names = ("accessibility.html", "site_layout.html", "base.html")
    pages = [(n, reader_text((TEMPLATES / n).read_text(encoding="utf-8"))) for n in names]
    return [("accessibility-statement.md", statement()), *pages]


@pytest.mark.parametrize(("name", "text"), scanned())
def test_td_web_08_no_phone_line_in_what_the_page_shows(name: str, text: str) -> None:
    """TD-WEB-08: the statement and the three templates offer no call and hold no number."""
    assert offers(text) == [], name


@pytest.mark.parametrize("line", REQUIRED)
def test_td_web_08_statement_holds_each_required_line(line: str) -> None:
    """TD-WEB-08, A11Y-020: text, email and form contact, the relay line and "usually"."""
    assert line in squash(statement())


MUST_FAIL = [
    "Call or text us.", "You can call anytime.", "Feel free to call.", "Calling us is fine.",
    "Request a callback.", "Caller ID shows our number; reply anytime.",
    f"A call to the {NAME} number reaches the owner.", "Reach us by phone.", f"Phone the {NAME} number.",
    "Telephoning us works too.", "Our telephone is answered on weekdays.", "Dial our number and wait.",
    "Give us a ring.", "Contact us by text, email or voice.", "Leave us a voice message.",
    "Leave a voicemail and we reply.", "Talking to us is easy.", "Speaking with us works.",
    "Talk to a person.", "Speak with the owner.", "Use 711 to reach us.", "Use TTY to reach us.",
    "TDD users can reach us.", "Use VRS to reach us.", "Video relay users can reach our number directly.",
    "Relay users can reach our number directly.", "Use a relay service to reach us.",
    "Our hotline is open.", "Our helpline is open.", "Our help line is open.",
    "Our toll-free number is open.", "It is toll free.", "Llámenos cuando quiera.",
    "Puede llamar a nuestro número.", "Escríbanos o use el teléfono.", "Deje un mensaje de voz.",
    "Text us at 928-555-0142.", "Our number is (928) 555-0142.", "Reach us at 555-0142.",
    "Reach us at +1 928 555 0142.", "tel:+19285550142",
    LIST_A[2].removesuffix(".") + ", and you can call us too.", LIST_A[3] + " Call us anyway.",
    LIST_A[1] + ", but you may phone us",
]  # fmt: skip


@pytest.mark.parametrize("text", MUST_FAIL)
def test_td_web_08_planted_offer_is_caught(text: str) -> None:
    """TD-WEB-08: each planted call offer or number is found."""
    assert offers(text) != [], text


def test_td_web_08_document_with_a_call_offer_is_caught() -> None:
    """TD-WEB-08: the document followed by a call offer is found."""
    assert offers(statement() + "\nYou can also call us.\n") != []


MUST_PASS = [
    *LIST_A,
    (
        f"Tell us about any barrier by text (text HELP to the {NAME} number), by email at "
        f"help@{DOMAIN}, or with the help page's form."
    ),
    f"The {NAME} owner usually replies within 2 days, the same as our HELP replies.",
    f"The {NAME} number is text-only.", "Every page is checked with VoiceOver on an iPhone.",
    "A screen reader speaks every label.", "Each dialog takes focus when it opens.",
    "We recall no page that fails a check.", "Pages are tested at 320, 768 and 1280 px.",
    "Version 0.2 (draft) · November 13, 2026 · Not yet in effect",
]  # fmt: skip


@pytest.mark.parametrize("text", MUST_PASS)
def test_td_web_08_true_wording_passes(text: str) -> None:
    """TD-WEB-08, D-3883: true wording that offers no call is not flagged."""
    assert offers(text) == [], text

"""Scan of every text template (TD-TXT-02; TXT-005 to TXT-008, TXT-010, WORD-008).

Reads app/messaging/templates/en.json and es.json and fails on: an emoji code point, a link that is
not askhava.com, a link shortener, a ROC link other than askhava.com/help/roc-complaint, promotion or
"save this number" wording, "reply CANCEL", or a text that does not begin "Ask Hava:". Fake data only.

Run: python3 -m pytest tests/docs/test_template_scan.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

DIR = Path(__file__).resolve().parents[2] / "app" / "messaging" / "templates"
ROC_LINK = "askhava.com/help/roc-complaint"
SHORTENERS = {
    "bit.ly", "t.co", "tinyurl.com", "goo.gl", "ow.ly", "is.gd", "buff.ly", "rebrand.ly", "cutt.ly",
    "tiny.cc", "shorturl.at", "rb.gy", "lnkd.in", "t.ly", "s.id",
}
PROMOTION = re.compile(
    r"\b(promo\w*|discount\w*|coupon\w*|special offer|limited time|save this number|save our number|"
    r"add us to your contacts|refer a friend|referral|on sale|a deal|great deals?|% off|descuento\w*|cup[oó]n\w*|oferta especial|"
    r"guarde este n[uú]mero)\b",
    re.IGNORECASE,
)
REPLY_CANCEL = re.compile(r"\b(reply|text|send|responda|env[ií]e)\s+(cancel|cancelar)\b", re.IGNORECASE)
LINK = re.compile(
    r"(?:https?://)?(?:(?:[a-z0-9-]+\.)+[a-z]{2,}|\d{1,3}(?:\.\d{1,3}){3})(?::\d+)?(?:/[^\s]*)?",
    re.IGNORECASE,
)
WARRANTY = re.compile(
    r"\b(home warranty|protection plan|service contract|extended warranty|guarantee\w*|warrant(?:y|ies)|"
    r"garant[ií]a\w*)\b",
    re.IGNORECASE,
)
EMOJI = re.compile(
    "[\u00a9\u00ae\u203c-\u3299\U0001f000-\U0001faff\ufe0f\u200d]"
)
# Characters in that wide range which are ordinary text: dashes, quotes, bullets and the euro sign.
PLAIN = set("–—‘’“”•…€")


def load(name: str) -> dict:
    return json.loads((DIR / f"{name}.json").read_text(encoding="utf-8"))


def all_templates():
    return [(f"{lang}:{key}", entry["text"]) for lang in ("en", "es") for key, entry in load(lang).items()]


def links(text: str) -> list[str]:
    return [m.group(0).rstrip(".,;:!?)") for m in LINK.finditer(text)]


def emoji_in(text: str) -> list[str]:
    return [c for c in text if EMOJI.match(c) and c not in PLAIN]


def problems(text: str) -> list[str]:
    """Every rule the text breaks, as short messages."""
    found = []
    if not text.startswith("Ask Hava:"):
        found.append('does not begin "Ask Hava:" (WORD-008)')
    if emoji_in(text):
        found.append("has an emoji (TXT-005)")
    for link in links(text):
        bare = re.sub(r"^https?://", "", link, flags=re.IGNORECASE)
        host = bare.split("/")[0].lower()
        if host in SHORTENERS:
            found.append(f"uses a link shortener {host} (TXT-006)")
        elif host != "askhava.com":
            found.append(f"links off askhava.com: {link} (TXT-006)")
        elif re.search(r"/roc\b", bare, re.IGNORECASE) and bare.lower() != ROC_LINK:
            found.append(f"ROC link must be {ROC_LINK} (TXT-007)")
    if re.search(r"\broc\b", text, re.IGNORECASE) and ROC_LINK not in text:
        found.append(f"mentions the ROC without {ROC_LINK} (TXT-007)")
    if PROMOTION.search(text):
        found.append("has promotion wording (TXT-008)")
    if WARRANTY.search(text):
        found.append("offers a warranty or protection plan (LIC-042)")
    if REPLY_CANCEL.search(text):
        found.append('says "reply CANCEL" (TXT-010)')
    return found


@pytest.mark.parametrize(("name", "text"), all_templates())
def test_every_template_passes_the_scan(name: str, text: str) -> None:
    """TD-TXT-02: each real template breaks no rule."""
    assert problems(text) == [], name


def test_both_files_hold_the_same_kinds_and_spanish_is_draft() -> None:
    """TXT-001, TXT-003: a kind without a Spanish template, or a Spanish text not marked draft, fails."""
    en, es = load("en"), load("es")
    assert set(en) == set(es)
    assert all(e["draft"] for e in es.values())
    assert not any(e.get("draft") for e in en.values())


def test_the_close_text_says_close_with_the_job_number() -> None:
    """TXT-010: the close instruction is "Reply CLOSE" plus the job number."""
    assert "Reply CLOSE {job}" in load("en")["close_howto"]["text"]


@pytest.mark.parametrize(
    ("planted", "expected"),
    [
        ("Ask Hava: Sorry \U0001f600 hi", "emoji"),
        ("Ask Hava: See bit.ly/abc", "shortener"),
        ("Ask Hava: See https://example.com/x", "off askhava.com"),
        ("Ask Hava: See askhava.com/roc", "ROC link must be"),
        ("Ask Hava: File with the ROC at roc.az.gov", "off askhava.com"),
        ("Ask Hava: Get a discount today", "promotion"),
        ("Ask Hava: Save this number for later", "promotion"),
        ("Ask Hava: Reply CANCEL to stop", "reply CANCEL"),
        ("Ask Hava: Ask about our home warranty", "warranty"),
        ("Ask Hava: Our guarantee covers it", "warranty"),
        ("Ask Hava: Tenemos una garantía extendida", "warranty"),
        ("Ask Hava: See http://192.168.1.5/x", "off askhava.com"),
        ("Hi there", "does not begin"),
    ],
)
def test_the_scan_catches_a_planted_violation(planted: str, expected: str) -> None:
    """TD-TXT-02, TD-LIC-18: each planted violation is caught."""
    assert any(expected in p for p in problems(planted)), problems(planted)


def test_the_scan_accepts_plain_links_and_times() -> None:
    """The scan does not flag askhava.com links, an optional scheme, or "p.m." and "a.m."."""
    assert problems("Ask Hava: Call 8 a.m. to 8 p.m. Arizona time. https://askhava.com/terms askhava.com/help/roc-complaint") == []

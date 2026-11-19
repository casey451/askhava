"""The consent text matches CON-031 character for character and is 2 GSM-7 segments (T161).

TD-CON-23 (CON-031, CON-018, CON-004; D-398, D-419, D-3890): the English template, its brand
tokens filled by `scripts/brand.py`'s `fill`, equals CON-031's English text, is GSM-7, 289
characters and 2 segments; the Spanish template equals CON-031's Spanish, is GSM-7 (no á, í, ó or
ú), 297 characters and 2 segments; neither holds the HELP reply-time promise, which the HELP reply
still holds (CON-018); and neither asks for a reply (CON-004). Each template's own `segments` field
agrees with the counter. The expected texts are read from the spec, never typed here.

Run: python3 -m pytest tests/docs/test_consent_segments.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

import brand

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "messaging" / "templates"
CONSENT_SPEC = ROOT / "docs" / "spec" / "consent.md"

# The GSM 03.38 default alphabet and its extension table (each extension character costs 2).
GSM7_BASIC = frozenset(
    "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?"
    "¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà"
)
GSM7_EXTENSION = frozenset("^{}\\[~]|€\f")
SINGLE, PART = 160, 153

# CON-018's reply-time promise, in each language's HELP reply.
PROMISE = {"en": "usually replies within", "es": "normalmente responde en"}


def septets(text: str) -> int:
    """The GSM-7 septets `text` takes; raises on a character outside GSM-7."""
    count = 0
    for char in text:
        if char in GSM7_BASIC:
            count += 1
        elif char in GSM7_EXTENSION:
            count += 2
        else:
            raise ValueError(f"not GSM-7: {char!r}")
    return count


def segments(text: str) -> int:
    """GSM-7 segments: one up to 160 septets, else 153 per part."""
    used = septets(text)
    return 1 if used <= SINGLE else -(-used // PART)


def con_031() -> tuple[str, str, int, int]:
    """CON-031's English and Spanish texts and the counts it states, from docs/spec/consent.md."""
    for line in CONSENT_SPEC.read_text(encoding="utf-8").splitlines():
        if line.startswith("- **CON-031** "):
            body = line.split(" Source:")[0]
            english, spanish = re.findall(r'"([^"]+)"', body)[:2]
            counts = [int(n) for n in re.findall(r"\((\d+) characters", body)]
            return english, spanish, counts[0], counts[1]
    raise AssertionError("CON-031 not found in docs/spec/consent.md")


def template(lang: str, key: str) -> dict:
    """One template entry of `en.json` or `es.json`."""
    return json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))[key]


def consent(lang: str) -> str:
    """The consent template with its brand tokens filled."""
    return brand.fill(template(lang, "consent")["text"], spanish=lang == "es")


def test_td_con_23_the_counter_counts_gsm7_segments() -> None:
    assert segments("a" * 160) == 1
    assert segments("a" * 161) == 2
    assert segments("a" * 306) == 2
    assert segments("a" * 307) == 3
    assert septets("€[") == 4
    for outside in ("á", "í", "ó", "ú", "“", "’"):
        with pytest.raises(ValueError):
            septets(outside)


def test_td_con_23_spec_counts_are_289_and_297() -> None:
    english, spanish, en_count, es_count = con_031()
    assert (en_count, es_count) == (289, 297)
    assert (len(english), len(spanish)) == (289, 297)


@pytest.mark.parametrize("lang", ["en", "es"])
def test_td_con_23_consent_equals_con_031_and_is_two_segments(lang: str) -> None:
    english, spanish, en_count, es_count = con_031()
    expected, count = (english, en_count) if lang == "en" else (spanish, es_count)
    text = consent(lang)
    assert text == expected
    assert len(text) == count
    assert segments(text) == 2
    assert template(lang, "consent")["segments"] == 2
    assert not set(text) & set("áíóú“”‘’")


@pytest.mark.parametrize("lang", ["en", "es"])
def test_td_con_23_reply_time_promise_only_in_help(lang: str) -> None:
    assert PROMISE[lang] not in consent(lang)
    assert PROMISE[lang] in template(lang, "help_reply")["text"]


def test_con_004_consent_asks_for_no_reply() -> None:
    for lang in ("en", "es"):
        text = consent(lang).lower()
        for ask in ("reply ok", "reply yes", "responda sí", "responda si"):
            assert ask not in text, f"{lang}: {ask}"
    assert "/terms" in consent("en")
    assert "/privacy" in consent("en")

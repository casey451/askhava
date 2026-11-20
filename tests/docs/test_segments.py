"""Every text template's encoding and segment count against its recorded count (T218).

TD-TXT-26 (TXT-043, TXT-044; D-490, D-491): reports each template's encoding (GSM-7 or UCS-2) and segment count
beside the `segments` count recorded in its `en.json` or `es.json` entry, and fails when the English
consent text (CON-031) needs UCS-2 or more than 2 segments, when the Spanish consent text is not
GSM-7 and 2 segments, or when any template needs more segments than recorded. A Spanish template's
UCS-2 encoding is reported, never failed. Brand tokens are filled by `scripts/brand.py` first. The
refusal to turn Spanish conversations on without the campaign note (TXT-043) is T480's, not tested here (D-3744).
The planted cases below prove each failure turns the check red. Fake data only.

Run: python3 -m pytest tests/docs/test_segments.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
from pathlib import Path

import brand
import pytest
from test_consent_segments import segments as gsm_segments
from test_consent_segments import septets

TEMPLATES = Path(__file__).resolve().parents[2] / "app" / "messaging" / "templates"
UCS2_SINGLE, UCS2_PART = 70, 67
CONSENT_MAX = 2


def measure(text: str) -> tuple[str, int]:
    """The encoding ("GSM-7" or "UCS-2") and segment count of `text`."""
    try:
        septets(text)
    except ValueError:
        units = sum(2 if ord(c) > 0xFFFF else 1 for c in text)
        return "UCS-2", 1 if units <= UCS2_SINGLE else -(-units // UCS2_PART)
    return "GSM-7", gsm_segments(text)


def load(lang: str) -> dict:
    return json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))


def report(lang: str) -> list[tuple[str, str, int, int]]:
    """(key, encoding, segments, recorded) for every template of `lang`, brand tokens filled."""
    rows = []
    for key, entry in load(lang).items():
        encoding, count = measure(brand.fill(entry["text"], lang == "es"))
        rows.append((key, encoding, count, entry["segments"]))
    return rows


def failures(lang: str, rows: list[tuple[str, str, int, int]]) -> list[str]:
    """Why `rows` fail TXT-044: the consent text's encoding or size, and any growth past the record."""
    bad = []
    for key, encoding, count, recorded in rows:
        if key == "consent" and (encoding != "GSM-7" or count > CONSENT_MAX):
            bad.append(f"{lang}:{key} is {encoding}, {count} segments")
        if count > recorded:
            bad.append(f"{lang}:{key} needs {count} segments, recorded {recorded}")
    return bad


@pytest.mark.parametrize("lang", ["en", "es"])
def test_td_txt_26_every_template_fits_its_recorded_segment_count(
    lang: str, capsys: pytest.CaptureFixture[str]
) -> None:
    rows = report(lang)
    assert rows
    with capsys.disabled():
        for key, encoding, count, recorded in rows:
            print(f"\nTXT-044 {lang}:{key} {encoding} {count} segments (recorded {recorded})", end="")
    assert not failures(lang, rows)


def test_td_txt_26_the_english_templates_are_all_gsm7_except_none_planted() -> None:
    assert [r for r in report("en") if r[0] == "consent"][0][1] == "GSM-7"


def test_td_txt_26_planted_ucs2_in_the_english_consent_fails() -> None:
    text = brand.fill(load("en")["consent"]["text"]) + " ’"
    encoding, count = measure(text)
    assert encoding == "UCS-2"
    assert failures("en", [("consent", encoding, count, 2)])


def test_td_txt_26_planted_third_segment_in_the_english_consent_fails() -> None:
    text = brand.fill(load("en")["consent"]["text"]) + " " + "x" * 160
    encoding, count = measure(text)
    assert (encoding, count) == ("GSM-7", 3)
    assert failures("en", [("consent", encoding, count, 3)])


def test_td_txt_26_planted_growth_past_the_recorded_count_fails() -> None:
    key, encoding, count, recorded = report("en")[0]
    assert failures("en", [(key, encoding, recorded + 1, recorded)])
    assert not failures("en", [("who_is_this", "GSM-7", 1, 2)])


def test_td_txt_26_spanish_ucs2_is_reported_not_failed_except_in_the_consent() -> None:
    encoding, count = measure("Laftly: ¿Cómo está? ’")
    assert encoding == "UCS-2"
    assert not failures("es", [("who_is_this", encoding, count, count)])
    assert failures("es", [("consent", encoding, count, count)])
    third = measure(brand.fill(load("es")["consent"]["text"], True) + " " + "x" * 160)
    assert third[1] == 3
    assert failures("es", [("consent", third[0], third[1], third[1])])


def test_td_txt_26_the_counter_counts_ucs2_segments() -> None:
    assert measure("’" * 70) == ("UCS-2", 1)
    assert measure("’" * 71) == ("UCS-2", 2)
    assert measure("a" * 160) == ("GSM-7", 1)

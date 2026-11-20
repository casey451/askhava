"""Sales phrases, price-lock phrases and non-transactional invitations (T218).

TD-WORD-30, TD-WORD-33, TD-TXT-27 (WORD-030, WORD-032, TXT-046; D-490, D-1494): the phrase scan fails a
lead-selling phrase ("ready to hire", "serious homeowners", "qualified", "pre-qualified", "high-intent",
a promise of jobs) and a win, pick or hire rate that does not cite the ledger with a date range and a
count; it fails "price lock" (and "price-lock"), "lock in your rate", "founding rate", "grandfathered
pricing" and "your rate never goes up" in any letter case. The scan set is `app/*/templates/`,
`app/messaging/templates/*.json`, `docs/copy/` and the public drafts in `docs/legal/` (the two
documents marked Internal in `docs/legal/README.md` are not business or homeowner text); internal
records such as `docs/decisions.md` are not scanned. TXT-046: no message template holds a recruiting,
referral, discount, survey, new-feature or social-media invitation from `docs/ai_smell.yaml`. The
lists are planted in the tests below to prove each turns the scan red. Fake data only.

Run: python3 -m pytest tests/docs/test_brain_phrases.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "messaging" / "templates"
INTERNAL_LEGAL = {"information-security-program.md", "incident-response-plan.md"}

SALES = ["ready to hire", "serious homeowners", "qualified", "pre-qualified", "high-intent", "high intent"]
PROMISES = ["we guarantee you jobs", "we promise you work", "guaranteed wins", "guaranteed jobs"]
PRICE_LOCK = [
    "price lock", "price-lock", "lock in your rate", "founding rate", "grandfathered pricing",
    "your rate never goes up",
]
SALES_RE = re.compile(
    r"\b(ready to hire|serious homeowners?|(?:pre-)?qualified|high[- ]intent|"
    r"guarantee[sd]? (?:you )?(?:jobs|work|wins)|promise[sd]? (?:you )?(?:jobs|work|wins))\b",
    re.IGNORECASE,
)
PRICE_LOCK_RE = re.compile(
    r"price[- ]lock|lock in your rate|founding rate|grandfathered pricing|your rate never goes up",
    re.IGNORECASE,
)
RATE_RE = re.compile(
    r"\b(?:win|pick|hire)(?:ing)? rate\b|\b\d+(?:\.\d+)?\s*%\s*(?:of\s+\w+\s+)?(?:win|won|pick|hire)",
    re.IGNORECASE,
)
DATE_RANGE = re.compile(r"\b\d{4}-\d{2}-\d{2}\b.*\b\d{4}-\d{2}-\d{2}\b|\b\w+ \d{1,2}\b.*\bto\b.*\b\w+ \d{1,2}\b")
COUNT = re.compile(r"\b\d+\s+(?:jobs|picks|quotes|requests)\b", re.IGNORECASE)


def scan_files() -> list[Path]:
    """Every file the phrase scan reads (WORD-030, WORD-032)."""
    files = [p for p in (ROOT / "app").glob("*/templates/**/*") if p.is_file()]
    files += TEMPLATES.glob("*.json")
    files += (ROOT / "docs" / "copy").glob("*.md")
    files += [p for p in (ROOT / "docs" / "legal").rglob("*.md") if p.name not in INTERNAL_LEGAL]
    return sorted(files)


def sales_hits(text: str) -> list[str]:
    """Lead-selling phrases, promises, price-lock phrases and unsourced rates in `text` (WORD-030, WORD-032)."""
    hits = [m.group(0) for m in SALES_RE.finditer(text)]
    hits += [m.group(0) for m in PRICE_LOCK_RE.finditer(text)]
    for line in text.splitlines():
        if RATE_RE.search(line) and not (
            "ledger" in line.lower() and DATE_RANGE.search(line) and COUNT.search(line)
        ):
            hits.append(line.strip())
    return hits


def invitations() -> dict[str, list[str]]:
    """The invitation lists in docs/ai_smell.yaml, by kind (TXT-046)."""
    data = yaml.safe_load((ROOT / "docs" / "ai_smell.yaml").read_text(encoding="utf-8"))
    return data["non_transactional_invitations"]


def invitation_hits(text: str) -> list[str]:
    """Invitation phrases from the list that `text` holds, case-insensitively."""
    found = []
    for phrases in invitations().values():
        for phrase in phrases:
            if re.search(rf"(?<!\w){re.escape(phrase)}(?!\w)", text, re.IGNORECASE):
                found.append(phrase)
    return found


def test_td_word_30_real_files_hold_no_sales_phrase_or_price_lock() -> None:
    files = scan_files()
    assert files, "the scan set is empty"
    bad = {
        str(p.relative_to(ROOT)): hits
        for p in files
        if (hits := sales_hits(p.read_text(encoding="utf-8", errors="ignore")))
    }
    assert not bad, bad


@pytest.mark.parametrize("phrase", SALES + PROMISES)
def test_td_word_30_planted_sales_phrase_fails(phrase: str) -> None:
    assert sales_hits(f"Meet {phrase.upper()} near you.")
    assert sales_hits(f"Meet {phrase} near you.")


def test_td_word_30_win_rate_needs_the_ledger_range_and_count() -> None:
    assert sales_hits("Our win rate is 62%.")
    assert sales_hits("A typical business has a 40% hire rate.")
    assert not sales_hits("Pick rate from the ledger, 2026-01-01 to 2026-03-31, 40 picks: 25%.")
    assert not sales_hits("A pick is confirmed by the homeowner.")


@pytest.mark.parametrize("phrase", PRICE_LOCK)
def test_td_word_33_planted_price_lock_phrase_fails(phrase: str) -> None:
    for variant in (phrase, phrase.upper(), phrase.title()):
        assert sales_hits(f"Join now: {variant}.")


def test_td_word_33_published_schedule_wording_passes() -> None:
    assert not sales_hits("Fees follow the published schedule.")
    assert not sales_hits("The introduction fee by job size in effect at the pick applies.")


def test_td_word_33_scan_set_leaves_internal_records_out() -> None:
    names = {str(p.relative_to(ROOT)) for p in scan_files()}
    for internal in ("docs/decisions.md", "docs/PREBUILD.md", "docs/for-casey-review.md", "docs/test-design.md"):
        assert internal not in names
    assert not any(n.startswith("docs/pilot/") for n in names)
    assert any(n.startswith("app/messaging/templates/") for n in names)
    assert any(n.startswith("docs/copy/") for n in names)
    assert any(n.startswith("docs/legal/") for n in names)


def test_td_txt_27_the_invitation_list_covers_all_six_kinds() -> None:
    assert set(invitations()) == {
        "recruiting", "referral", "discount", "survey", "new_feature", "social_media",
    }
    assert all(invitations().values())


@pytest.mark.parametrize("lang", ["en", "es"])
def test_td_txt_27_no_template_holds_an_invitation(lang: str) -> None:
    data = json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))
    bad = {key: hits for key, entry in data.items() if (hits := invitation_hits(entry["text"]))}
    assert not bad, bad


def test_td_txt_27_each_planted_invitation_turns_the_scan_red() -> None:
    for kind, phrases in invitations().items():
        for phrase in phrases:
            assert invitation_hits(f"Laftly: your job is posted. {phrase.upper()}"), (kind, phrase)
    assert not invitation_hits("Laftly: Your pick is confirmed. Reply STOP to opt out.")

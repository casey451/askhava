"""Pricing copy says ongoing services count as four months, capped at $75, with the pool example.

TD-FEE-19 (T114; REC-007, REC-008, REC-009, TERMS-005; D-196, D-3844, D-4258). REC-009: the
business page and its Spanish twin count ongoing services as four months (`RECURRING_MONTHS`,
`app/payments/src/rules.rs`), the cap line shows `RecurringCap::DEFAULT`'s fee, and REC-009's
`$150-a-month` pool example counts as 150 times `RECURRING_MONTHS` in the row `Tier::for_basis`
gives it ($75); the business terms say "4 months" and the cap. REC-007 (no early-cancel partial
refund) and the full number scan are `test_refund_terms_match.py`, whose helpers this file loads.
REC-008's mutation tests are the test design's section 4; here a changed month count or cap fails
the copy until it matches. Fake data only.

Run: python3 -m pytest tests/docs/test_recurring_copy.py -q -p no:cacheprovider
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

_SPEC = importlib.util.spec_from_file_location("terms_match", Path(__file__).with_name("test_refund_terms_match.py"))
assert _SPEC and _SPEC.loader
tm = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(tm)
FEE_FIELD = {"Small": "small", "Medium": "medium", "Large": "large"}


def raw_line(path: str, needle: str) -> str:
    lines = [line for line in tm.read(path).splitlines() if needle in line]
    assert len(lines) == 1, (path, needle, lines)
    return lines[0]


def cap_field() -> str:
    tier = next(t for t, cents in tm.tier_cents().items() if cents == tm.default_tier("RecurringCap"))
    assert tier == "Medium"
    return "${{ facts.fees." + FEE_FIELD[tier] + " }}"


@pytest.mark.parametrize(("path", "phrase", "cap"), [
    (tm.BH, "we count four months", "fee never goes over"), (tm.EB, "cuentan como cuatro meses", "tiene un tope de")])
def test_page_counts_ongoing_services_as_the_coded_months(path: str, phrase: str, cap: str) -> None:
    """TD-FEE-19, REC-009, REC-008: the page's month word is `RECURRING_MONTHS`; the cap is its fee."""
    assert tm.WORDS[phrase.split()[-2]] == tm.const(tm.PAY, "RECURRING_MONTHS")
    assert f"{cap} {cap_field()}" in raw_line(path, phrase)
    assert tm.partial_hits(tm.reader_text(path, raw_line(path, phrase))) == []


@pytest.mark.parametrize("path", [tm.BH, tm.EB])
def test_pool_example_sits_in_its_tier_row(path: str) -> None:
    """TD-FEE-19, REC-009, D-196: the $150-a-month pool example counts as four months in the $75 row."""
    monthly = tm.pool_month_dollars()
    basis = monthly * tm.const(tm.PAY, "RECURRING_MONTHS")
    row = raw_line(path, f"${monthly}")
    assert f"${basis}." in row
    assert tm.tier_for_basis(basis * 100) == "Medium"
    assert row.rstrip().endswith(cap_field() + "</td></tr>")


def test_business_terms_state_months_and_cap() -> None:
    """TD-FEE-19, REC-009, REC-007, TERMS-005: the terms count 4 months, capped at the cap's fee."""
    months, cap = tm.const(tm.PAY, "RECURRING_MONTHS"), f"${tm.default_tier('RecurringCap') // 100}"
    ongoing = raw_line(tm.BT, "**Ongoing services:**")
    assert f"counts as {months} months" in ongoing and ongoing.endswith(f"never more than {cap}.")
    assert f"ongoing services counted at {months} months and capped at {cap}" in tm.read(tm.BT)
    assert tm.partial_hits(ongoing) == [] and tm.partial_hits(ongoing + " Early-cancel refunds are prorated.")

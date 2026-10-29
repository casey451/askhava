"""Self-check for the K2b catalog in docs/catalog/.

Recomputes every worked example's introduction fee from the fee rules (FEE-001 to
FEE-013, FEE-039, FEE-043 to FEE-045, REC-002 to REC-005, LIC-019, LIC-020, D-009, D-010, D-033,
D-196, D-399, D-403) and fails on any mismatch. The rules are written here as constants, independent of the YAML, and the
YAML's own fee_rules block must match them. Also checks the category list's shape,
the categories that must be off or absent at launch, and that neither file contains a
superseded value or banned wording (including the WORD-022 not-a-contractor list).
Also checks the license map (LIC-043, LIC-044, D-392) and the intake question bank
(LIC-040, INT-042, D-394), and the launch flags (LIC-046, D-396).
Checks each category's job types (LIC-001) against the license map's per-job-type scope:
every job type of an enabled licensed category has a license that covers it (CARD-001), and
a subclassification receives only job types within its scope (LIC-035, D-058, D-504).
Loads the matching worked examples M01 to M16 (D-110) and fails on a missing, duplicated or
malformed one, on a name the catalog or license map lacks, and on an expected outcome that breaks
a necessary card condition (CARD-001, LIC-013, LIC-017, LIC-019, LIC-021, LIC-022, LIC-035,
LIC-038, LIC-043).
Prints one line per example; exits 1 on any error.

Usage: python3 scripts/check_catalog.py [--root REPO_ROOT]
"""
# LIC-041: the job-type `kind` field (repair, replace, install) and its check arrive with T233 (D-493).

from __future__ import annotations

import argparse
import math
import re
import sys
from collections.abc import Sequence
from dataclasses import dataclass, field
from fractions import Fraction
from pathlib import Path
from typing import Any

import yaml  # type: ignore[import-untyped]  # reason: PyYAML ships no stubs; only safe_load is used
from trace_spec import NOT_A_CONTRACTOR, SUPERSEDED

CATEGORIES_FILE = Path("docs/catalog/categories.yaml")
EXAMPLES_FILE = Path("docs/catalog/worked_examples.yaml")
LICENSE_MAP_FILE = Path("docs/catalog/license_map.yaml")
QUESTION_BANK_FILE = Path("docs/catalog/question_bank.yaml")
MATCHING_FILE = Path("docs/catalog/matching_examples.yaml")  # D-110
ALL_FILES = (CATEGORIES_FILE, EXAMPLES_FILE, LICENSE_MAP_FILE, QUESTION_BANK_FILE, MATCHING_FILE)
MATCHING_IDS = tuple(f"M{n:02d}" for n in range(1, 17))  # D-110: docs/test-design.md 7.2
MATCH_ID = re.compile(r"^M\d{2}$")
MATCH_OUTCOMES = frozenset({"card", "no_card", "refused_at_intake"})
MATCH_KEYS: dict[str, tuple[str, ...]] = {
    "job": ("category", "candidates", "town", "table"),
    "business": ("roc", "agencies", "handyman", "city_list", "ticked", "towns"),
    "expected": ("outcome", "rules", "map_covers"),
}
MATCH_LISTS = (("job", "candidates"), ("business", "roc"), ("business", "agencies"),
               ("business", "ticked"), ("business", "towns"), ("expected", "rules"))
MATCH_NON_EMPTY = frozenset({"candidates", "towns", "rules"})
REQUIREMENT_ID = re.compile(r"^[A-Z]+-\d{3}$")
TRIP_FEE_TIERS = (25, 75)  # FEE-004, D-399: $25 by default, never above $75
QUESTIONS_MIN, QUESTIONS_MAX = 3, 8  # LIC-040, D-394, D-431
QUOTE_FORMATS = frozenset({"range", "monthly", "tiers", "firm", "diagnostic_visit"})  # PICK-053
KB_TRADES_DIR = Path("docs/catalog/trade_kb/trades")  # D-431
SUBCLASS = re.compile(r"^R-\d+R ")  # LIC-035: an R-37R or R-39R subclassification row
MAX_PHOTOS = 3  # INT-044, D-432

TIER_LOW_FROM = Fraction(500)  # FEE-001: $500.00 and up is $75
TIER_HIGH_UP_TO = Fraction(1500)  # FEE-001: up to and including $1,500.00 is $75
FEE_SMALL, FEE_MEDIUM, FEE_LARGE = 25, 75, 199
TIER_FEES = (FEE_SMALL, FEE_MEDIUM, FEE_LARGE)
RECURRING_MONTHS = 4  # REC-002
RECURRING_FEE_CAP = 75  # FEE-039 / D-196: a quote with a period never pays more than this
HANDYMAN_LIMIT = Fraction(1000)  # LIC-019
MONTHLY_FACTOR: dict[str, Fraction] = {  # REC-004
    "weekly": Fraction(52, 12),
    "every_two_weeks": Fraction(26, 12),
    "monthly": Fraction(1),
    "quarterly": Fraction(1, 3),
    "yearly": Fraction(1, 12),
}
EXPECTED_FEE_RULES: dict[str, Any] = {
    "tiers": [
        {"fee": 25, "basis_below": "500.00"},
        {"fee": 75, "basis_from": "500.00", "basis_up_to_and_including": "1500.00"},
        {"fee": 199, "basis_above": "1500.00"},
    ],
    "recurring_months": RECURRING_MONTHS,
    "recurring_fee_cap": RECURRING_FEE_CAP,
    "monthly_conversion": {
        "weekly": "x 52 / 12",
        "every_two_weeks": "x 26 / 12",
        "monthly": "x 1",
        "quarterly": "/ 3",
        "yearly": "/ 12",
    },
    "range_basis": "midpoint",
    "basis_rounding": "round down to the whole cent before choosing the tier (D-033)",
    "handyman_limit": "1000.00",
}

MONEY = re.compile(r"^\d+\.\d{2}$")
CATEGORY_KEYS = (
    "id",
    "name",
    "group",
    "enabled",
    "launch_open",
    "license",
    "handyman_allowed",
    "construction_type",
    "recurring",
    "trip_fee_tier",
    "after_hours_emergency",
    "job_types",
    "refused",
    "quote_format",
    "keywords",
    "review",
)
LICENSE_KINDS = frozenset({"none", "roc", "state_agency", "roc_and_state_agency"})
MUST_BE_OFF = frozenset({"garage_doors", "junk_removal"})  # LIC-005
NOT_AT_LAUNCH = frozenset(
    {  # LIC-004
        "childcare",
        "home care",
        "massage",
        "mobile hair and nails",
        "mobile vets",
        "catering",
        "towing",
        "property management",
    }
)
SALE_INSPECTIONS = frozenset(  # LIC-015, D-376 (RESPA)
    {"buyer inspection", "pre-listing inspection", "refinance inspection"}
)
SALE_WORDS = ("buyer", "pre-listing", "listing", "refinanc", "purchase", "sale")
WARRANTY_WORDS = re.compile(  # LIC-042, D-376
    r"\b(?:warrant(?:y|ies)|protection plans?|service contracts?)\b", re.IGNORECASE
)
BANNED: tuple[tuple[re.Pattern[str], str], ...] = (
    *SUPERSEDED,
    (re.compile(r"\bcommission\b", re.IGNORECASE), "banned wording"),
    (re.compile(r"\bvetted\b", re.IGNORECASE), "banned wording"),
    (re.compile(r"\bbest\b", re.IGNORECASE), "banned wording"),
    (re.compile(r"\bconcierge\b", re.IGNORECASE), "banned wording"),  # WORD-019
    (re.compile(r"\bexclusive\b", re.IGNORECASE), "banned wording"),  # WORD-020, D-300
    (re.compile(r"\bpre-vetted\b", re.IGNORECASE), "banned wording"),  # WORD-020, D-300
    (  # WORD-020, D-300: "lead" in the sales sense; lead paint and lead pipe are allowed
        re.compile(r"\bleads?\b(?![- ](?:paint|pipes?|based)\b)", re.IGNORECASE),
        "banned wording",
    ),
    *NOT_A_CONTRACTOR,  # WORD-022, D-360
)


@dataclass(frozen=True)
class Outcome:
    outcome: str
    fee_basis: Fraction | None
    fee: int
    waived: int


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    lines: list[str] = field(default_factory=list)


def money(value: object) -> Fraction:
    """Parse a quoted dollar amount such as "150.00" exactly."""
    if not isinstance(value, str) or not MONEY.match(value):
        raise ValueError(f'money must be a quoted string like "150.00", got {value!r}')
    return Fraction(value)


def cents_down(amount: Fraction) -> Fraction:
    """Round down to the whole cent (D-033)."""
    return Fraction(math.floor(amount * 100), 100)


def show(amount: Fraction | None) -> str:
    return "null" if amount is None else f"{float(amount):.2f}"


def tier_fee(basis: Fraction) -> int:
    """FEE-001 / D-009."""
    if basis < TIER_LOW_FROM:
        return FEE_SMALL
    if basis <= TIER_HIGH_UP_TO:
        return FEE_MEDIUM
    return FEE_LARGE


def hourly_range(quote: dict[str, Any]) -> tuple[Fraction, Fraction]:
    """FEE-043: an hourly rate with estimated hours is the range rate x low to rate x high."""
    rate = money(quote["rate"])
    return rate * Fraction(str(quote["hours_low"])), rate * Fraction(str(quote["hours_high"]))


def is_site_visit(quote: dict[str, Any]) -> bool:
    """FEE-004, FEE-043: a trip-fee quote, or an hourly quote with no estimated hours."""
    return quote["type"] == "trip_fee" or (quote["type"] == "hourly" and "hours_low" not in quote)


def quote_top(quote: dict[str, Any]) -> Fraction:
    """The top of a quote, for the handyman limit (LIC-019)."""
    if quote["type"] == "range":
        return money(quote["high"])
    if quote["type"] == "hourly":
        return hourly_range(quote)[1]
    return money(quote["amount"])


def quote_amount(quote: dict[str, Any]) -> Fraction:
    """A fixed amount, or a range's midpoint (FEE-003, REC-005, FEE-043)."""
    if quote.get("setup") is not None:
        money(quote["setup"])  # FEE-044: shown as written, never in the basis
    if quote["type"] == "hourly":
        low, high = hourly_range(quote)
        if low > high:
            raise ValueError("hours_low is above hours_high")
        return (low + high) / 2
    if quote["type"] == "range":
        low, high = money(quote["low"]), money(quote["high"])
        if low > high:
            raise ValueError("range low is above range high")
        return (low + high) / 2
    if quote["type"] == "fixed":
        return money(quote["amount"])
    raise ValueError(f"unknown quote type {quote['type']!r}")


def fee_basis(quote: dict[str, Any], category: dict[str, Any]) -> Fraction:
    """The fee basis, with the 4-month rule for recurring services (REC-002, REC-004).

    A quote with no period is a one-time job priced at face value, even in a
    recurring category (for example a move-out clean from a house cleaner).
    """
    amount = quote_amount(quote)
    period = quote.get("period")
    if period is not None and not category["recurring"]:
        raise ValueError("a quote has a period but its category is not recurring")
    if period is None:
        return cents_down(amount)
    if period not in MONTHLY_FACTOR:
        raise ValueError(f"unknown period {period!r}")
    return cents_down(amount * MONTHLY_FACTOR[period] * RECURRING_MONTHS)


def over_handyman_limit(example: dict[str, Any], category: dict[str, Any]) -> bool:
    """LIC-019 / LIC-020: top of any range, construction-type unlicensed work only."""
    quote = example["quote"]
    if not category["construction_type"] or is_site_visit(quote):
        return False
    exempt = example["business"] == "handyman_exemption"
    if exempt and not category["handyman_allowed"]:
        raise ValueError(
            "handyman_exemption business in a category that refuses handymen"
        )
    if not exempt and category["license"]["kind"] != "none":
        return False
    return quote_top(quote) >= HANDYMAN_LIMIT


def check_one_time(example: dict[str, Any], category: dict[str, Any]) -> None:
    """A recurring category's quote with no period must name one of its one-time jobs."""
    quote = example["quote"]
    if not category["recurring"] or quote["type"] == "trip_fee" or "period" in quote:
        return
    job = example.get("job_type")
    if job not in category.get("one_time_job_types", []):
        raise ValueError(
            f"recurring category quote with no period must name a one-time job_type, got {job!r}"
        )


def compute(example: dict[str, Any], category: dict[str, Any]) -> Outcome:
    """The outcome the fee rules give for one worked example."""
    check_one_time(example, category)
    if example["quote"]["type"] not in {"fixed", "range", "trip_fee", "hourly"}:
        raise ValueError(f"unknown quote type {example['quote']['type']!r}")
    if over_handyman_limit(example, category):
        return Outcome("refused", None, 0, 0)
    quote = example["quote"]
    if is_site_visit(quote) and category.get("quote_format") != "diagnostic_visit":
        raise ValueError("a visit-fee quote outside a diagnostic_visit category (FEE-004, D-441)")
    if is_site_visit(quote):
        money(quote["amount"] if quote["type"] == "trip_fee" else quote["rate"])
        basis: Fraction | None = None
        fee = int(category["trip_fee_tier"])
    else:
        basis = fee_basis(quote, category)
        fee = tier_fee(basis)
        if quote.get("period") is not None:
            fee = min(fee, RECURRING_FEE_CAP)
    if example.get("job_type") in category.get("fee_free_job_types", []):
        return Outcome("fee_free", basis, 0, 0)  # FEE-045, D-403
    situation = example["situation"]
    if situation == "repeat_customer":
        return Outcome("charged", basis, 0, 0)
    if situation == "free_first_win":
        return Outcome("charged", basis, 0, fee)
    if situation != "new_customer":
        raise ValueError(f"unknown situation {situation!r}")
    return Outcome("charged", basis, fee, 0)


def expected_outcome(example: dict[str, Any]) -> Outcome:
    exp = example["expected"]
    basis = None if exp["fee_basis"] is None else money(exp["fee_basis"])
    return Outcome(str(exp["outcome"]), basis, int(exp["fee"]), int(exp["waived"]))


def check_example(
    example: dict[str, Any], categories: dict[str, dict[str, Any]]
) -> str:
    """Return "" when the example matches the rules, else an error message."""
    ex_id = example.get("id", "?")
    category = categories.get(example.get("category", ""))
    if category is None:
        return f"{ex_id}: unknown category {example.get('category')!r}"
    if not category["enabled"]:
        return f"{ex_id}: category {category['id']} is off at launch"
    try:
        got, want = compute(example, category), expected_outcome(example)
    except (KeyError, TypeError, ValueError) as exc:
        return f"{ex_id}: {exc}"
    if got != want:
        return (
            f"{ex_id}: expected {want.outcome} basis {show(want.fee_basis)} fee ${want.fee} "
            f"waived ${want.waived}, rules give {got.outcome} basis {show(got.fee_basis)} "
            f"fee ${got.fee} waived ${got.waived}"
        )
    return ""


def check_examples(
    data: dict[str, Any], categories: dict[str, dict[str, Any]]
) -> Report:
    report = Report()
    for key, want in EXPECTED_FEE_RULES.items():
        if data.get("fee_rules", {}).get(key) != want:
            report.errors.append(
                f"worked_examples.yaml fee_rules.{key} does not match FEE-001/REC-002"
            )
    seen: set[str] = set()
    for example in data.get("examples", []):
        ex_id = str(example.get("id"))
        if ex_id in seen:
            report.errors.append(f"{ex_id}: duplicate example id")
        seen.add(ex_id)
        problem = check_example(example, categories)
        status = "FAIL" if problem else "ok"
        exp = example.get("expected", {})
        report.lines.append(
            f"{status:4} {ex_id} {example.get('category')}: basis {exp.get('fee_basis') or '-'} "
            f"-> {exp.get('outcome')} ${exp.get('fee')} (waived ${exp.get('waived')})"
        )
        if problem:
            report.errors.append(problem)
    return report


def check_category(cat: dict[str, Any]) -> list[str]:
    cat_id = cat.get("id", "?")
    missing = [key for key in CATEGORY_KEYS if key not in cat]
    if missing:
        return [f"category {cat_id}: missing {', '.join(missing)}"]
    errors: list[str] = []
    if cat["license"].get("kind") not in LICENSE_KINDS:
        errors.append(
            f"category {cat_id}: unknown license kind {cat['license'].get('kind')!r}"
        )
    roc_kinds = {"roc", "roc_and_state_agency"}
    if (
        cat["license"]["kind"] in roc_kinds
        and cat["enabled"]
        and not cat["license"].get("classes")
    ):
        errors.append(f"category {cat_id}: enabled ROC category with no class")
    if cat["trip_fee_tier"] not in TRIP_FEE_TIERS:
        errors.append(
            f"category {cat_id}: trip_fee_tier must be one of {TRIP_FEE_TIERS} (FEE-004, D-399)"
        )
    if cat["launch_open"] is True and cat["enabled"] is not True:
        errors.append(f"category {cat_id}: launch_open but not enabled (LIC-046)")
    if not set(cat.get("fee_free_job_types", [])) <= set(cat["job_types"]):
        errors.append(f"category {cat_id}: fee_free_job_types not in job_types")
    for key in ("enabled", "launch_open", "handyman_allowed", "construction_type", "recurring"):
        if not isinstance(cat[key], bool):
            errors.append(f"category {cat_id}: {key} must be true or false")
    one_time = cat.get("one_time_job_types", [])
    if one_time and not cat["recurring"]:
        errors.append(
            f"category {cat_id}: one_time_job_types in a non-recurring category"
        )
    if not set(one_time) <= set(cat["job_types"]):
        errors.append(f"category {cat_id}: one_time_job_types not in job_types")
    errors += check_quote_format(cat)
    if not cat["enabled"] and not cat.get("off_reason"):
        errors.append(f"category {cat_id}: off without an off_reason")
    return errors


def check_quote_format(cat: dict[str, Any]) -> list[str]:
    """PICK-053, FEE-004 (D-441, D-433): one known format; monthly exactly when recurring."""
    cat_id = cat.get("id", "?")
    fmt = cat.get("quote_format")
    if fmt not in QUOTE_FORMATS:
        return [f"category {cat_id}: quote_format must be one of {sorted(QUOTE_FORMATS)} (PICK-053)"]
    errors: list[str] = []
    if (fmt == "monthly") != bool(cat.get("recurring")):
        errors.append(f"category {cat_id}: quote_format monthly exactly when recurring (PICK-053)")
    for job, job_fmt in (cat.get("job_type_formats") or {}).items():
        if job not in cat.get("job_types", []) or job_fmt not in QUOTE_FORMATS - {"diagnostic_visit"}:
            errors.append(f"category {cat_id}: job_type_formats {job!r} needs a job type and a non-visit format")
    if cat.get("design") and fmt != "range":
        errors.append(f"category {cat_id}: design is for range categories only (INT-045, D-433)")
    return errors


def check_pest(pest: dict[str, Any]) -> list[str]:
    """LIC-012 (D-402): pest control, when on, is for Department licensees only and never advertised."""
    if pest.get("enabled") is not True:
        return []
    errors: list[str] = []
    if not pest.get("licensed_only_decision"):
        errors.append(
            "category pest_control: on without licensed_only_decision (LIC-012, D-402)"
        )
    if pest.get("license", {}).get("kind") != "state_agency":
        errors.append("category pest_control: must be for Department licensees only (LIC-012)")
    return errors


def check_legal_holds(cats: dict[str, dict[str, Any]]) -> list[str]:
    """D-376, D-402, D-403: pest licensed-only, sale inspections fee-free, no warranties."""
    errors = check_pest(cats.get("pest_control", {}))
    inspection = cats.get("home_inspection")
    if inspection is not None:
        free = {str(j).lower() for j in inspection.get("fee_free_job_types", [])}
        # A job type's own words, without any "(no sale or refinance)" note.
        charged = [
            str(j).lower()
            for j in inspection.get("job_types", [])
            if str(j).lower() not in free
        ]
        if any(word in job.split("(")[0] for job in charged for word in SALE_WORDS):
            errors.append(
                "category home_inspection: charges a sale-related inspection (LIC-015, FEE-045)"
            )
        if not SALE_INSPECTIONS <= free:
            errors.append(
                "category home_inspection: buyer, pre-listing and refinance inspections must be "
                "fee_free_job_types (LIC-015, FEE-045)"
            )
    for cat in cats.values():
        text = " ".join([str(cat.get("name", "")), *map(str, cat.get("job_types", []))])
        if WARRANTY_WORDS.search(text):
            errors.append(f"category {cat.get('id')}: warranty or protection plan (LIC-042)")
    return errors


def check_categories(
    data: dict[str, Any],
) -> tuple[dict[str, dict[str, Any]], list[str]]:
    cats: dict[str, dict[str, Any]] = {}
    errors: list[str] = []
    for cat in data.get("categories", []):
        errors.extend(check_category(cat))
        if cat.get("id") in cats:
            errors.append(f"category {cat.get('id')}: duplicate id")
        cats[str(cat.get("id"))] = cat
    for cat_id in sorted(MUST_BE_OFF):
        if cat_id not in cats or cats[cat_id].get("enabled") is not False:
            errors.append(
                f"category {cat_id}: must be listed and off at launch (LIC-005)"
            )
    if (
        "pest_control" not in cats
        or cats["pest_control"].get("advertised") is not False
    ):
        errors.append(
            "category pest_control: must be listed with advertised: false (7.1 item 3)"
        )
    errors.extend(check_legal_holds(cats))
    if set(data.get("not_offered_at_launch", [])) != NOT_AT_LAUNCH:
        errors.append("not_offered_at_launch does not match LIC-004")
    names = {str(cat.get("name", "")).lower() for cat in cats.values()}
    errors.extend(
        f"category {n!r} is not offered at launch (LIC-004)"
        for n in NOT_AT_LAUNCH & names
    )
    return cats, errors


def check_text(path: Path, rel: str) -> list[str]:
    errors: list[str] = []
    for lineno, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        errors.extend(
            f"{rel}:{lineno}: {label} ({pat.pattern})"
            for pat, label in BANNED
            if pat.search(line)
        )
    return errors


def job_names(cat: dict[str, Any]) -> set[str]:
    """A category's job types plus its licensed-only refused jobs (license map "also")."""
    names = {str(j) for j in cat.get("job_types", [])}
    names |= {str(r.get("job")) for r in cat.get("refused", [])}
    names |= {
        str(r.get("job")).replace("(", "").split(")")[0]
        for r in cat.get("refused", [])
    }
    return names


def check_grants(
    where: str, grants: dict[str, Any], cats: dict[str, dict[str, Any]]
) -> tuple[list[str], set[str]]:
    """Check one class's job_types block; return errors and the categories it reaches."""
    errors: list[str] = []
    reached: set[str] = set()
    for cat_id, grant in (grants or {}).items():
        cat = cats.get(str(cat_id))
        if cat is None:
            errors.append(f"license map {where}: unknown category {cat_id!r}")
            continue
        listed = [*grant.get("only", []), *grant.get("also", [])]
        if not grant.get("all") and not grant.get("only"):
            errors.append(f"license map {where}: {cat_id} needs all: true or only: [...]")
        for job in listed:
            if str(job) not in job_names(cat):
                errors.append(f"license map {where}: {cat_id} has no job type {job!r}")
        reached.add(str(cat_id))
    return errors, reached


def is_commercial_only(cls: str, commercial: list[str]) -> bool:
    return cls in commercial or (cls.startswith("C-") and "C-" in commercial)


def check_license_classes(
    data: dict[str, Any], cats: dict[str, dict[str, Any]]
) -> tuple[list[str], set[str]]:
    errors: list[str] = []
    reached: set[str] = set()
    sources = {str(s.get("id")) for s in data.get("sources", [])}
    commercial = [str(c) for c in data.get("commercial_only", [])]
    names = [str(c.get("class")) for c in data.get("classes", [])]
    for cls in sorted({n for n in names if names.count(n) > 1}):
        errors.append(f"license map: class {cls} listed twice")
    for entry in data.get("classes", []):
        cls = str(entry.get("class"))
        if is_commercial_only(cls, commercial):
            errors.append(f"license map: commercial-only class {cls} maps job types (LIC-044)")
        if entry.get("source") not in sources:
            errors.append(f"license map: class {cls} cites unknown source {entry.get('source')!r}")
        for inc in entry.get("includes", []):
            if str(inc) not in names:
                errors.append(f"license map: class {cls} includes unknown class {inc!r}")
        if not entry.get("job_types") and not entry.get("includes"):
            errors.append(f"license map: class {cls} maps nothing")
        grant_errors, got = check_grants(f"class {cls}", entry.get("job_types", {}), cats)
        errors.extend(grant_errors)
        reached |= got
    return errors, reached


def granted_jobs(grant: dict[str, Any], cat: dict[str, Any]) -> list[str]:
    """The job types one grant gives: the category's (all) or those listed (only), plus also."""
    base = cat.get("job_types", []) if grant.get("all") else grant.get("only", [])
    return [str(job) for job in [*base, *grant.get("also", [])]]


def mapped_jobs(data: dict[str, Any], cats: dict[str, dict[str, Any]]) -> set[tuple[str, str]]:
    """CARD-001, LIC-043: every (category, job type) a class or agency in the map covers."""
    pairs: set[tuple[str, str]] = set()
    for row in [*data.get("classes", []), *data.get("agencies", [])]:
        for cat_id, grant in (row.get("job_types") or {}).items():
            cat = cats.get(str(cat_id))
            if cat is None:
                continue
            pairs |= {(str(cat_id), job) for job in granted_jobs(grant, cat)}
            needed = row.get("record_category_needed") or {}
            pairs |= {(str(cat_id), str(job)) for job in needed if str(job) in job_names(cat)}
    return pairs


def check_job_coverage(data: dict[str, Any], cats: dict[str, dict[str, Any]]) -> list[str]:
    """CARD-001, LIC-001, LIC-043: each job type of an enabled licensed category has a license."""
    pairs = mapped_jobs(data, cats)
    errors: list[str] = []
    for cat in cats.values():
        if cat.get("enabled") is not True or cat["license"]["kind"] == "none":
            continue
        errors.extend(
            f"license map: {cat['id']} job type {job!r} has no license scope (CARD-001, LIC-043)"
            for job in cat.get("job_types", [])
            if (str(cat["id"]), str(job)) not in pairs
        )
    return errors


def check_scope_tags(data: dict[str, Any], cats: dict[str, dict[str, Any]]) -> list[str]:
    """LIC-035 (D-504): every job type of a category a subclassification maps has a scope tag."""
    tags = data.get("scope_tags") or {}
    errors: list[str] = []
    for entry in data.get("classes", []):
        if SUBCLASS.match(str(entry.get("class"))):
            errors.extend(
                f"license map: category {cat_id} has no scope_tags (LIC-035)"
                for cat_id in entry.get("job_types") or {}
                if cat_id not in tags and cat_id in cats
            )
    for cat_id, jobs in tags.items():
        cat = cats.get(str(cat_id))
        if cat is None:
            errors.append(f"license map: scope_tags names unknown category {cat_id!r}")
            continue
        listed = {str(job) for job in jobs or {}}
        own = {str(j) for j in cat.get("job_types", [])}
        own |= {str(r.get("job")) for r in cat.get("refused", [])}
        errors.extend(
            f"license map: {cat_id} job type {job!r} has no license scope in scope_tags (LIC-035)"
            for job in sorted(own - listed)
        )
        errors.extend(
            f"license map: scope_tags {cat_id} has no job type {job!r}"
            for job in sorted(listed - job_names(cat))
        )
    return errors


def check_subclass(
    entry: dict[str, Any], tags: dict[str, Any], cats: dict[str, dict[str, Any]]
) -> list[str]:
    """LIC-035 (D-058, D-504): a subclassification receives only job types within its scope."""
    cls = str(entry.get("class"))
    scope = {str(s) for s in entry.get("scope") or []}
    if not scope:
        return [f"license map: subclassification {cls} has no scope (LIC-035)"]
    errors: list[str] = []
    for cat_id, grant in (entry.get("job_types") or {}).items():
        cat = cats.get(str(cat_id))
        if cat is None:
            continue
        for job in granted_jobs(grant, cat):
            job_tags = {str(s) for s in (tags.get(str(cat_id)) or {}).get(job) or []}
            if not job_tags & scope:
                errors.append(
                    f"license map: subclassification {cls} gets {cat_id} job type {job!r}, "
                    f"outside its scope {sorted(scope)} (LIC-035)"
                )
    return errors


def check_license_scope(data: dict[str, Any], cats: dict[str, dict[str, Any]]) -> list[str]:
    """CARD-001, LIC-035 (D-058, D-504): per-job-type license scope, before any routing code."""
    errors = check_job_coverage(data, cats) + check_scope_tags(data, cats)
    tags = data.get("scope_tags") or {}
    for entry in data.get("classes", []):
        if SUBCLASS.match(str(entry.get("class"))):
            errors.extend(check_subclass(entry, tags, cats))
    return errors


def check_license_map(data: dict[str, Any], cats: dict[str, dict[str, Any]]) -> list[str]:
    """LIC-043, LIC-044 (D-392): every licensed category reachable; no bad rows."""
    errors: list[str] = []
    for src in data.get("sources", []):
        if not src.get("url") or not re.match(r"^\d{4}-\d{2}-\d{2}$", str(src.get("checked", ""))):
            errors.append(f"license map: source {src.get('id')!r} needs a url and a checked date")
    class_errors, reached = check_license_classes(data, cats)
    errors.extend(class_errors)
    for agency in data.get("agencies", []):
        grant_errors, got = check_grants(
            f"agency {agency.get('license')!r}", agency.get("job_types", {}), cats
        )
        errors.extend(grant_errors)
        reached |= got
    for cat in cats.values():
        licensed = cat["license"]["kind"] != "none"
        if cat.get("enabled") is True and licensed and str(cat["id"]) not in reached:
            errors.append(f"license map: enabled category {cat['id']} has no mapped license (LIC-043)")
    return errors


def check_question(set_id: str, q: dict[str, Any], seen: set[str]) -> list[str]:
    qid = str(q.get("id"))
    where = f"question bank {set_id}.{qid}"
    errors: list[str] = []
    if qid in seen:
        errors.append(f"{where}: duplicate question id")
    if not q.get("en") or not q.get("es"):
        errors.append(f"{where}: needs both en and es")
    opts_en, opts_es = q.get("options_en"), q.get("options_es")
    if (opts_en is None) != (opts_es is None) or (
        opts_en is not None and len(opts_en) != len(opts_es or [])
    ):
        errors.append(f"{where}: options_en and options_es must match")
    if q.get("card_field") and not opts_en:
        errors.append(f"{where}: a free-text question can't be a card field (LIC-040)")
    floors = q.get("band_floor")
    if q.get("size_band") and (not opts_en or not floors or len(floors) != len(opts_en)):
        errors.append(f"{where}: a size_band question needs one band_floor per option")
    cond = q.get("ask_if")
    if cond is not None and str(cond.get("question")) not in seen:
        errors.append(f"{where}: ask_if names a question that isn't asked earlier")
    return errors


def check_kb_link(root: Path, set_id: str, qset: dict[str, Any]) -> list[str]:
    """LIC-040 (D-431, D-432): kb_trade names a knowledge-base file; photos are its ids."""
    slug = qset.get("kb_trade")
    photos = qset.get("photos", []) or []
    if slug is None:
        return [f"question bank {set_id}: photos without a kb_trade"] if photos else []
    path = root / KB_TRADES_DIR / f"{slug}.yaml"
    if not path.is_file():
        return [f"question bank {set_id}: kb_trade {slug} has no file (LIC-040)"]
    if len(photos) > MAX_PHOTOS:
        return [f"question bank {set_id}: more than {MAX_PHOTOS} photos (INT-044)"]
    known = {str(p.get("id")) for p in (yaml.safe_load(path.read_text(encoding="utf-8")) or {}).get("photos_to_request", [])}
    return [f"question bank {set_id}: photo {p} not in {slug}" for p in photos if str(p) not in known]


def check_question_bank(
    data: dict[str, Any], cats: dict[str, dict[str, Any]], root: Path | None = None
) -> list[str]:
    """LIC-040 (D-394, D-431): one 3-to-8-question set per enabled category."""
    errors: list[str] = []
    sets = data.get("sets", {}) or {}
    for set_id, qset in sets.items():
        if root is not None:
            errors.extend(check_kb_link(root, str(set_id), qset))
        if str(set_id) not in cats:
            errors.append(f"question bank: set {set_id} has no category")
        questions = qset.get("questions", [])
        if not QUESTIONS_MIN <= len(questions) <= QUESTIONS_MAX:
            errors.append(
                f"question bank {set_id}: {len(questions)} questions, need "
                f"{QUESTIONS_MIN} to {QUESTIONS_MAX} (LIC-040)"
            )
        seen: set[str] = set()
        for q in questions:
            errors.extend(check_question(str(set_id), q, seen))
            seen.add(str(q.get("id")))
    for cat in cats.values():
        if cat.get("enabled") is True and str(cat["id"]) not in sets:
            errors.append(f"question bank: enabled category {cat['id']} has no question set")
    for prompt in data.get("safety_prompts", []):
        if not str(prompt.get("en", "")).startswith("Ask Hava:") or not str(
            prompt.get("es", "")
        ).startswith("Ask Hava:"):
            errors.append(f"question bank: safety prompt {prompt.get('id')} must start 'Ask Hava:'")
        for set_id in prompt.get("sets", []):
            if set_id not in sets:
                errors.append(f"question bank: safety prompt {prompt.get('id')} names {set_id}")
    return errors

def match_shape(ex: dict[str, Any]) -> list[str]:
    """D-110: every block and field of a matching example is present and of the right type."""
    errors: list[str] = []
    for part, keys in MATCH_KEYS.items():
        block = ex.get(part)
        if not isinstance(block, dict):
            errors.append(f"{part} must be a mapping")
        elif missing := [key for key in keys if key not in block]:
            errors.append(f"{part} is missing {', '.join(missing)}")
    if errors:
        return errors
    for part, key in MATCH_LISTS:
        value = ex[part][key]
        if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
            errors.append(f"{part}.{key} must be a list of names")
        elif key in MATCH_NON_EMPTY and not value:
            errors.append(f"{part}.{key} must not be empty")
    for key in ("category", "town", "table"):
        if not isinstance(ex["job"][key], str) or not ex["job"][key]:
            errors.append(f"job.{key} must be a name")
    for key in ("handyman", "city_list"):
        if not isinstance(ex["business"][key], bool):
            errors.append(f"business.{key} must be true or false")
    exp = ex["expected"]
    if exp["outcome"] not in MATCH_OUTCOMES:
        errors.append(f"expected.outcome must be one of {sorted(MATCH_OUTCOMES)}")
    if exp["map_covers"] not in (True, False, None):
        errors.append("expected.map_covers must be true, false or null")
    if "condition" in exp and (not isinstance(exp["condition"], str) or not exp["condition"]):
        errors.append("expected.condition must be text")
    if not errors and not all(REQUIREMENT_ID.match(r) for r in exp["rules"]):
        errors.append("expected.rules must be requirement IDs such as LIC-035")
    return errors


def license_rows(lmap: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """LIC-043: the license map's ROC class rows by class and agency rows by license name."""
    rows = {str(row.get("class")): row for row in lmap.get("classes", [])}
    rows.update({str(row.get("license")): row for row in lmap.get("agencies", [])})
    return rows


def row_grants(
    name: str, rows: dict[str, dict[str, Any]], cats: dict[str, dict[str, Any]], seen: frozenset[str]
) -> set[tuple[str, str]]:
    """LIC-043: every (category, job type) a class or agency gets, directly or through includes.

    An agency's record-category jobs (LIC-012: termites, pre-emergent weed control) count as
    granted, as in `mapped_jobs` (D-504); the examples do not model the record category (D-506).
    """
    row = rows.get(name) or {}
    needed = [str(job) for job in row.get("record_category_needed") or {}]
    pairs = {
        (str(cat_id), job)
        for cat_id, grant in (row.get("job_types") or {}).items()
        if str(cat_id) in cats
        for job in [*granted_jobs(grant, cats[str(cat_id)]), *needed]
        if job in job_names(cats[str(cat_id)])
    }
    for inc in row.get("includes", []):
        if str(inc) not in seen:
            pairs |= row_grants(str(inc), rows, cats, seen | {str(inc)})
    return pairs


def covers(names: list[str], ex: dict[str, Any], rows: dict[str, Any], cats: dict[str, Any]) -> bool:
    """CARD-001, LIC-035: the named licenses together cover every candidate job type."""
    pairs: set[tuple[str, str]] = set()
    for name in names:
        pairs |= row_grants(name, rows, cats, frozenset({name}))
    cat_id = ex["job"]["category"]
    return all((cat_id, job) in pairs for job in ex["job"]["candidates"])


def match_refs(ex: dict[str, Any], cat: dict[str, Any], rows: dict[str, Any]) -> list[str]:
    """D-110: job types, ROC classes and agencies use the catalog's and license map's names."""
    known = job_names(cat)
    errors = [
        f"{part}.{key} {name!r} is not a {cat['id']} job type"
        for part, key in (("job", "candidates"), ("business", "ticked"))
        for name in ex[part][key]
        if name not in known
    ]
    errors.extend(
        f"business.{key} {name!r} is not in the license map"
        for key, field_ in (("roc", "class"), ("agencies", "license"))
        for name in ex["business"][key]
        if field_ not in rows.get(name, {})
    )
    return errors


def card_problems(ex: dict[str, Any], cat: dict[str, Any], covered: bool | None, rows: dict[str, Any],
                  cats: dict[str, Any]) -> list[str]:
    """CARD-001, LIC-013, LIC-019, LIC-021, LIC-022, LIC-038: conditions every card needs."""
    job, biz = ex["job"], ex["business"]
    problems: list[str] = []
    if job["town"] not in biz["towns"]:
        problems.append("a card to a business that does not serve the job's town (CARD-001)")
    if not set(biz["ticked"]) & set(job["candidates"]):
        problems.append("a card to a business that ticked none of the candidates (CARD-001)")
    refused = {str(r.get("job")) for r in cat.get("refused", [])}
    if biz["handyman"]:
        if not cat["handyman_allowed"] or refused & set(job["candidates"]):
            problems.append("a card to a handyman for work handymen never get (LIC-021, LIC-022)")
        if not ex["expected"].get("condition"):
            problems.append("a handyman card without its $1,000 condition (LIC-019)")
        if not biz["city_list"]:
            problems.append("a handyman card to a business not on the city list (LIC-038)")
    elif cat["license"]["kind"] == "none":
        if not biz["city_list"]:
            problems.append("a no-license card to a business not on the city list")
    elif covered is not True:
        problems.append("a card the license map does not cover for every candidate (CARD-001)")
    dual = cat["license"]["kind"] == "roc_and_state_agency" and not biz["handyman"]
    if dual and not covers(biz["agencies"], ex, rows, cats):
        problems.append("a card without the category's agency license too (LIC-013)")
    return problems


def match_logic(
    ex: dict[str, Any], cat: dict[str, Any], rows: dict[str, Any], cats: dict[str, Any]
) -> list[str]:
    """D-110: the expected outcome agrees with the license map and the necessary card rules."""
    biz, exp = ex["business"], ex["expected"]
    names = [*biz["roc"], *biz["agencies"]]
    covered = covers(names, ex, rows, cats) if names else None
    problems: list[str] = []
    if covered != exp["map_covers"]:
        problems.append(f"map_covers is {exp['map_covers']}, the license map gives {covered}")
    at_intake = {str(r.get("job")) for r in cat.get("refused", []) if r.get("routed_to") == "none"}
    if bool(at_intake & set(ex["job"]["candidates"])) != (exp["outcome"] == "refused_at_intake"):
        problems.append("refused_at_intake exactly when a candidate is refused for everyone (LIC-017)")
    if exp["outcome"] == "card":
        problems.extend(card_problems(ex, cat, covered, rows, cats))
    return problems


def check_match(ex: object, cats: dict[str, dict[str, Any]], rows: dict[str, Any]) -> list[str]:
    """D-110: one matching example's shape, names and expected outcome."""
    if not isinstance(ex, dict):
        return ["matching example is not a mapping"]
    ex_id = str(ex.get("id"))
    if not MATCH_ID.match(ex_id):
        return [f"{ex_id}: matching example id must be M and two digits"]
    problems = match_shape(ex)
    if not problems:
        cat = cats.get(ex["job"]["category"])
        if cat is None:  # an off category keeps its examples: switching it off needs nothing
            problems = [f"job.category {ex['job']['category']!r} is not a catalog category"]
        else:
            problems = match_refs(ex, cat, rows) or match_logic(ex, cat, rows, cats)
    return [f"{ex_id}: {p}" for p in problems]


def check_matching(
    data: dict[str, Any], cats: dict[str, dict[str, Any]], lmap: dict[str, Any]
) -> Report:
    """D-110: M01 to M16 are each present once and well formed (TD-LIC-02's oracle)."""
    report = Report()
    examples = data.get("examples")
    if not isinstance(examples, list):
        report.errors.append("matching_examples.yaml: examples must be a list (D-110)")
        return report
    ids = [str(ex.get("id")) if isinstance(ex, dict) else "?" for ex in examples]
    report.errors.extend(f"{i}: duplicate matching example id" for i in sorted(set(ids)) if ids.count(i) > 1)
    report.errors.extend(f"{i}: matching example missing (D-110)" for i in MATCHING_IDS if i not in ids)
    rows = license_rows(lmap)
    for ex in examples:
        problems = check_match(ex, cats, rows)
        report.errors.extend(problems)
        if isinstance(ex, dict):
            outcome = (ex.get("expected") or {}).get("outcome")
            report.lines.append(f"{'FAIL' if problems else 'ok':4} {ex.get('id')} matching: {outcome}")
    return report


def load(root: Path, rel: Path) -> dict[str, Any]:
    data = yaml.safe_load((root / rel).read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise TypeError(f"{rel.as_posix()}: top level must be a mapping")
    return data


def run(root: Path) -> Report:
    report = Report()
    for rel in ALL_FILES:
        if not (root / rel).is_file():
            report.errors.append(f"missing {rel.as_posix()}")
    if report.errors:
        return report
    for rel in ALL_FILES:
        report.errors.extend(check_text(root / rel, rel.as_posix()))
    cats, cat_errors = check_categories(load(root, CATEGORIES_FILE))
    report.errors.extend(cat_errors)
    examples = check_examples(load(root, EXAMPLES_FILE), cats)
    report.errors.extend(examples.errors)
    report.lines.extend(examples.lines)
    lmap = load(root, LICENSE_MAP_FILE)
    report.errors.extend(check_license_map(lmap, cats))
    report.errors.extend(check_license_scope(lmap, cats))
    bank = load(root, QUESTION_BANK_FILE)
    report.errors.extend(check_question_bank(bank, cats, root))
    matching = check_matching(load(root, MATCHING_FILE), cats, lmap)
    report.errors.extend(matching.errors)
    report.lines.extend(matching.lines)
    report.lines.append(
        f"License map: {len(lmap.get('classes', []))} classes; question sets: "
        f"{len(bank.get('sets', {}))}"
    )
    enabled = sum(1 for cat in cats.values() if cat.get("enabled") is True)
    report.lines.append(
        f"Categories: {len(cats)} ({enabled} on, {len(cats) - enabled} off)"
    )
    report.lines.append(f"Worked examples: {len(examples.lines)}")
    report.lines.append(f"Matching examples: {len(matching.lines)}")
    return report


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0] if __doc__ else None
    )
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    args = parser.parse_args(argv)
    report = run(args.root)
    for line in report.lines:
        print(line)
    for error in report.errors:
        print(f"ERROR {error}", file=sys.stderr)
    print(f"Errors: {len(report.errors)}")
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())

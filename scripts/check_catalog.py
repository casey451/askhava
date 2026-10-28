"""Self-check for the K2b catalog in docs/catalog/.

Recomputes every worked example's introduction fee from the fee rules (FEE-001 to
FEE-013, FEE-039, REC-002 to REC-005, LIC-019, LIC-020, D-009, D-010, D-033, D-196) and fails on any
mismatch. The rules are written here as constants, independent of the YAML, and the
YAML's own fee_rules block must match them. Also checks the category list's shape,
the categories that must be off or absent at launch, and that neither file contains a
superseded value or banned wording (including the WORD-022 not-a-contractor list).
Prints one line per example; exits 1 on any error.

Usage: python3 scripts/check_catalog.py [--root REPO_ROOT]
"""

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
    "license",
    "handyman_allowed",
    "construction_type",
    "recurring",
    "trip_fee_tier",
    "after_hours_emergency",
    "job_types",
    "refused",
    "intake_questions",
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


def quote_amount(quote: dict[str, Any]) -> Fraction:
    """A fixed amount, or a range's midpoint (FEE-003, REC-005)."""
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
    if not category["construction_type"] or quote["type"] == "trip_fee":
        return False
    exempt = example["business"] == "handyman_exemption"
    if exempt and not category["handyman_allowed"]:
        raise ValueError(
            "handyman_exemption business in a category that refuses handymen"
        )
    if not exempt and category["license"]["kind"] != "none":
        return False
    top = money(quote["high"]) if quote["type"] == "range" else money(quote["amount"])
    return top >= HANDYMAN_LIMIT


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
    if over_handyman_limit(example, category):
        return Outcome("refused", None, 0, 0)
    quote = example["quote"]
    if quote["type"] == "trip_fee":
        money(quote["amount"])
        basis: Fraction | None = None
        fee = int(category["trip_fee_tier"])
    else:
        basis = fee_basis(quote, category)
        fee = tier_fee(basis)
        if quote.get("period") is not None:
            fee = min(fee, RECURRING_FEE_CAP)
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
    if cat["trip_fee_tier"] not in TIER_FEES:
        errors.append(f"category {cat_id}: trip_fee_tier must be one of {TIER_FEES}")
    for key in ("enabled", "handyman_allowed", "construction_type", "recurring"):
        if not isinstance(cat[key], bool):
            errors.append(f"category {cat_id}: {key} must be true or false")
    one_time = cat.get("one_time_job_types", [])
    if one_time and not cat["recurring"]:
        errors.append(
            f"category {cat_id}: one_time_job_types in a non-recurring category"
        )
    if not set(one_time) <= set(cat["job_types"]):
        errors.append(f"category {cat_id}: one_time_job_types not in job_types")
    if not cat["enabled"] and not cat.get("off_reason"):
        errors.append(f"category {cat_id}: off without an off_reason")
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


def load(root: Path, rel: Path) -> dict[str, Any]:
    data = yaml.safe_load((root / rel).read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise TypeError(f"{rel.as_posix()}: top level must be a mapping")
    return data


def run(root: Path) -> Report:
    report = Report()
    for rel in (CATEGORIES_FILE, EXAMPLES_FILE):
        if not (root / rel).is_file():
            report.errors.append(f"missing {rel.as_posix()}")
    if report.errors:
        return report
    for rel in (CATEGORIES_FILE, EXAMPLES_FILE):
        report.errors.extend(check_text(root / rel, rel.as_posix()))
    cats, cat_errors = check_categories(load(root, CATEGORIES_FILE))
    report.errors.extend(cat_errors)
    examples = check_examples(load(root, EXAMPLES_FILE), cats)
    report.errors.extend(examples.errors)
    report.lines.extend(examples.lines)
    enabled = sum(1 for cat in cats.values() if cat.get("enabled") is True)
    report.lines.append(
        f"Categories: {len(cats)} ({enabled} on, {len(cats) - enabled} off)"
    )
    report.lines.append(f"Worked examples: {len(examples.lines)}")
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

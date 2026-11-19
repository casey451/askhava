"""The business terms, the BIZ-030 summaries and the privacy policy say what the code says.

TD-FEE-19 (T114; REC-007, REC-009, TERMS-005, TERMS-019; D-196, D-492, D-3844, D-4258, D-4349).
Scope (D-4258): the business terms, their Spanish summary and the privacy policy as written, and
the reader text of `business.html` and `es_business.html` (blocks and tags removed, lines kept).
Numbers: every match of D-4258's closed written forms must be a `PAIRS` entry (its value read from
the named code constant or spec line, never typed here) or a `LIST_A` entry (named, not compared),
keyed by file, written form and value with its count, never by line: a number changed on one side
alone, or added without its pair, fails. No `catalog.setting` store exists (D-3844): the catalog
settings are the code's constants and `docs/catalog/categories.yaml`'s `trip_fee_tier` values.
No partial refund (REC-007): no List B pattern in a document, no partial or pro-rata rule in the
payments and picks code, and each Stripe refund form sends only the payment intent (a full
refund). Triggers (TERMS-005, TERMS-019): the terms name each `RefundReason` and `Unmet`
condition and the no-refund line. Each planted case is its own parametrized case. Fake data only.

Run: python3 -m pytest tests/docs/test_refund_terms_match.py -q -p no:cacheprovider
"""

from __future__ import annotations

import math
import re
from collections import Counter
from collections.abc import Callable
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
BT, SUM, PP = "docs/legal/business-terms.md", "docs/legal/business-terms-summary-es.md", "docs/legal/privacy-policy.md"
BH, EB = "app/site/templates/business.html", "app/site/templates/es_business.html"
PAY, JOBS = "app/payments/src/rules.rs", "app/jobs/src/rules.rs"
REFUND = "app/payments/src/refund_rules.rs"
DOCS = (BT, SUM, PP, BH, EB)
HOUR, DAY = 60, 24 * 60

N = r"(?<![\w$.,-])(\d+|an?|one|two|three|four|five|seven|twelve|thirty|cuatro)"
WORDS = {"a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "seven": 7,
         "twelve": 12, "thirty": 30, "cuatro": 4}
UNITS = {"business hour": ("business hour", HOUR), "minute": ("minute", 1), "hour": ("hour", HOUR),
         "day": ("day", DAY), "week": ("week", 7 * DAY), "month": ("month", 1),
         "year": ("year", 1), "minutos": ("minute", 1), "horas hábiles": ("business hour", HOUR),
         "horas": ("hour", HOUR), "días": ("day", DAY), "meses": ("month", 1), "años": ("year", 1),
         "more hours": ("more hours", HOUR), "call attempts": ("call attempts", 1),
         "complaints": ("complaints", 1), "tiers": ("tiers", 1), "characters": ("characters", 1)}
UNIT_RE = r"more hours?|call attempts|complaints|tiers|characters|business hours?|minutes?|hours?|days?|weeks?|months?|years?|minutos|horas hábiles|horas|días|meses|años"
FORMS = [
    ("money", re.compile(r"\$\s?\d{1,3}(?:,\d{3})*(?:\.\d{2})?")),
    ("span", re.compile(N + r"(\s+|-)(" + UNIT_RE + r")\b", re.I)),
    ("day-N", re.compile(r"\bday-(\d+)\b", re.I)),
    ("clock", re.compile(r"\b(\d{1,2})\s?([ap])\.m\.", re.I)),
    ("twice", re.compile(r"\btwice\b|\bdos veces\b", re.I)),
]
LIST_A_PHRASES = {"once or twice a day": (BT, BH), "una o dos veces al día": (SUM, EB),
                  "is never part of the fee basis": (BT,)}  # removed only in the files D-4258 names


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def _blank(m: re.Match[str]) -> str:
    return " " + "\n" * m.group(0).count("\n")


def reader_text(path: str, text: str | None = None) -> str:
    """The scope text: a template loses its blocks, scripts, styles and tags; lines are kept."""
    text = read(path) if text is None else text
    if path.endswith(".html"):
        for pat in (r"\{#.*?#\}", r"\{%.*?%\}", r"\{\{.*?\}\}", r"<script.*?</script>",
                    r"<style.*?</style>", r"<[^>]*>"):
            text = re.sub(pat, _blank, text, flags=re.S | re.I)
    for phrase in (p for p, files in LIST_A_PHRASES.items() if path in files):
        text = re.sub(re.escape(phrase), _blank, text, flags=re.I)
    return text


def _num(word: str) -> int:
    return int(word) if word.isdigit() else WORDS[word.lower()]


def _read_match(form: str, m: re.Match[str]) -> tuple[str, int]:
    if form == "money":
        dollars, _, cents = m.group(0).lstrip("$ ").replace(",", "").partition(".")
        return "money", int(dollars) * 100 + int(cents or 0)
    if form == "span":
        word = m.group(3).lower()
        unit, factor = UNITS.get(re.sub(r"\s+", " ", word)) or UNITS[word.removesuffix("s")]
        return unit + ("-" if m.group(2) == "-" else ""), _num(m.group(1)) * factor
    if form == "day-N":
        return "day-N", int(m.group(1))
    if form == "clock":
        return "clock", int(m.group(1)) % 12 + (12 if m.group(2).lower() == "p" else 0)
    return "twice", 2


def numbers(text: str) -> Counter[tuple[str, int]]:
    """Every number in D-4258's closed written forms, as (form, value) with its count."""
    found: Counter[tuple[str, int]] = Counter()
    for form, pattern in FORMS:
        for m in pattern.finditer(text):
            found[_read_match(form, m)] += 1
    return found


# --- Values from the code and the spec (D-3844): each names its file and constant. ---

def _eval(expr: str) -> int | list[int]:
    expr = expr.strip()
    if expr.startswith("["):
        return [int(x.replace("_", "")) for x in expr.strip("[]").split(",")]
    dur = re.fullmatch(r"SignedDuration::from_(hours|mins)\((.+)\)", expr)
    if dur:
        return int(_eval(dur.group(2))) * (HOUR if dur.group(1) == "hours" else 1)
    return math.prod(int(p.strip().replace("_", "")) for p in expr.split("*"))


def find(pattern: str, text: str, flags: int = 0) -> re.Match[str]:
    m = re.search(pattern, text, flags)
    assert m, f"not found: {pattern}"
    return m


def const(path: str, name: str) -> int:
    """`const NAME: T = expr;` in `path`; a duration is read in minutes."""
    return _eval(find(rf"\bconst {name}: [^=]+= (.+?);", read(path)).group(1))  # type: ignore[return-value]


def field(path: str, name: str) -> int:
    """A struct literal's `name: expr,` field in `path`; a duration is read in minutes."""
    return _eval(find(rf"\b{name}: (SignedDuration::from_\w+\([^)]*\)|[\d_ *]+),", read(path)).group(1))  # type: ignore[return-value]


def tier_cents() -> dict[str, int]:
    """`Tier::cents`'s arms (FEE-001), by variant name."""
    body = find(r"fn cents\(self\).*?\n    \}", read(PAY), re.S).group(0)
    return {name: int(v.replace("_", "")) for name, v in re.findall(r"Self::(\w+) => ([\d_]+),", body)}


def default_tier(type_name: str) -> int:
    """The fee of `VisitTier::DEFAULT` or `RecurringCap::DEFAULT`."""
    return tier_cents()[find(rf"impl {type_name} \{{.*?DEFAULT: Self = Self\(Tier::(\w+)\)", read(PAY), re.S).group(1)]


def visit_dollars(accepted: bool) -> list[int]:
    """The dollar arms `VisitTier::from_dollars` accepts (`Ok`) or refuses (`Err`)."""
    body = find(r"fn from_dollars\(.*?\n    \}", read(PAY), re.S).group(0)
    return [int(d) for d in re.findall(r"(\d+) => " + ("Ok" if accepted else "Err"), body)]


def tier_for_basis(cents: int) -> str:
    """`Tier::for_basis` (FEE-001): under the $75 edge Small, up to and including its top Medium."""
    if cents < const(PAY, "MEDIUM_FROM_CENTS"):
        return "Small"
    return "Medium" if cents <= const(PAY, "MEDIUM_UP_TO_CENTS") else "Large"


def spec_span(path: str, req: str, unit: str) -> int:
    """The first `N unit` span on requirement `req`'s spec line."""
    return _num(find(rf"^- \*\*{req}\*\*.*?" + N + rf"\s+{unit}s?\b", read(path), re.M).group(1))


def attempts_needed() -> int:
    return int(find(r"attempts\.len\(\) >= (\d+)", read(REFUND)).group(1))


def pool_month_dollars() -> int:
    """REC-009's example monthly price (`docs/spec/recurring.md`), `$150-a-month`."""
    return int(find(r"^- \*\*REC-009\*\*.*?\$(\d+)-a-month", read("docs/spec/recurring.md"), re.M).group(1))


Pair = tuple[str, str, Callable[[], int], int]


def _money(path: str, fees: int, edges: int, pool: bool) -> list[Pair]:
    """The tier fees (`Tier::cents`), the tier edges and REC-009's pool example and its basis."""
    pairs: list[Pair] = [(path, "money", (lambda t=t: tier_cents()[t]), fees) for t in ("Small", "Medium", "Large")]
    pairs += [(path, "money", (lambda e=e: const(PAY, e)), edges) for e in ("MEDIUM_FROM_CENTS", "MEDIUM_UP_TO_CENTS")]
    return pairs + ([(path, "money", lambda: pool_month_dollars() * 100, 1),
                     (path, "money", lambda: pool_month_dollars() * 100 * const(PAY, "RECURRING_MONTHS"), 1)]
                    if pool else [])


def _summary(path: str, confirm_unit: str, recurring: bool) -> list[Pair]:
    """The BIZ-030 summary's shared numbers: confirm window, emergency window, hours, twice."""
    pairs: list[Pair] = [
        (path, confirm_unit, lambda: const(JOBS, "CONFIRM_BUSINESS_MINUTES"), 1),
        (path, "minute", lambda: const(JOBS, "EMERGENCY_MINUTES"), 1),
        (path, "clock", lambda: const("app/core/src/clock.rs", "BUSINESS_OPEN_HOUR"), 1),
        (path, "clock", lambda: const("app/core/src/clock.rs", "BUSINESS_CLOSE_HOUR"), 1),
        (path, "twice", attempts_needed, 1),
        (path, "day", lambda: const("app/businesses/src/signup.rs", "REPEAT_FREE_DAYS") * DAY, 1)]
    if recurring:
        pairs += [(path, "month", lambda: const(PAY, "RECURRING_MONTHS"), 1),
                  (path, "day", lambda: const("app/payments/src/free_wins.rs", "FREE_WIN_WINDOW_DAYS") * DAY, 2)]
    return pairs


PAIRS: list[Pair] = [
    *_money(BT, 5, 2, pool=False),
    (BT, "money", lambda: default_tier("VisitTier"), 1),
    (BT, "money", lambda: max(visit_dollars(True)) * 100, 1),
    (BT, "money", lambda: default_tier("RecurringCap"), 2),
    (BT, "money", lambda: const("app/licensing/src/handyman.rs", "HANDYMAN_LIMIT_CENTS"), 1),
    (BT, "month", lambda: const(PAY, "RECURRING_MONTHS"), 2),
    (BT, "business hour", lambda: const(JOBS, "CONFIRM_BUSINESS_MINUTES"), 3),
    (BT, "minute", lambda: const(JOBS, "EMERGENCY_MINUTES"), 4),
    (BT, "business hour", lambda: const(JOBS, "PAYMENT_BUSINESS_MINUTES"), 1),
    (BT, "clock", lambda: const("app/core/src/clock.rs", "BUSINESS_OPEN_HOUR"), 4),
    (BT, "clock", lambda: const("app/core/src/clock.rs", "BUSINESS_CLOSE_HOUR"), 4),
    (BT, "more hours", lambda: field("app/picks/src/confirm.rs", "hold_minutes"), 1),
    (BT, "hour", lambda: const("app/quotes/src/intake.rs", "QUOTE_WINDOW"), 1),
    (BT, "characters", lambda: const("app/quotes/src/notes.rs", "NOTE_LIMIT"), 1),
    (BT, "day", lambda: const("app/payments/src/free_wins.rs", "FREE_WIN_WINDOW_DAYS") * DAY, 4),
    (BT, "day", lambda: const("app/businesses/src/signup.rs", "REPEAT_FREE_DAYS") * DAY, 2),
    (BT, "call attempts", attempts_needed, 2), (BT, "twice", attempts_needed, 1),
    (BT, "hour", lambda: const(REFUND, "ATTEMPT_GAP"), 2),
    (BT, "hour", lambda: const(REFUND, "ATTEMPT_WINDOW"), 3),
    (BT, "hour", lambda: const(REFUND, "DECIDE_AFTER"), 1),
    (BT, "hour-", lambda: int(re.findall(r"\(Step::SeventyTwo, (\d+)\)", read("app/picks/src/unreachable_steps.rs"))[0]) * HOUR, 1),
    (BT, "day-N", lambda: const("app/messaging/src/checkin_email.rs", "CHECK_IN_DAYS")[0], 1),  # type: ignore[index]
    (BT, "day", lambda: const("app/businesses/src/complaints.rs", "REPLY_BOX"), 1),
    (BT, "complaints", lambda: const("app/businesses/src/complaints.rs", "PAUSE_COUNT"), 1),
    (BT, "day", lambda: const("app/businesses/src/complaints.rs", "PAUSE_WINDOW_DAYS") * DAY, 1),
    (BT, "hour", lambda: field("app/licensing/src/handlers.rs", "pause_after"), 1),
    (BT, "year", lambda: spec_span("docs/spec/privacy.md", "PRIV-010", "year"), 1),
    *_money(SUM, 4, 0, pool=False), *_summary(SUM, "hour", recurring=False),
    *_money(BH, 0, 2, pool=True), *_summary(BH, "business hour", recurring=True),
    *_money(EB, 0, 2, pool=True), *_summary(EB, "business hour", recurring=True),
    (PP, "day", lambda: spec_span("docs/spec/privacy.md", "PRIV-007", "day") * DAY, 1),
    (PP, "day", lambda: spec_span("docs/spec/privacy.md", "PRIV-009", "day") * DAY, 1),
    (PP, "year", lambda: spec_span("docs/spec/privacy.md", "PRIV-008", "year"), 1),
    (PP, "year", lambda: spec_span("docs/spec/consent.md", "CON-022", "year"), 1),
    (PP, "year", lambda: const("app/messaging/src/consent_alerts.rs", "CONSENT_KEEP_YEARS"), 1),
    (PP, "year", lambda: spec_span("docs/spec/privacy.md", "PRIV-010", "year"), 2),
    (PP, "day", lambda: const("app/server/src/offdb_stream.rs", "REQUEST_LOG_DAYS") * DAY, 1),
    (PP, "day-", lambda: const("app/server/src/offdb_stream.rs", "REQUEST_LOG_DAYS") * DAY, 1),
    (PP, "day", lambda: const("app/portals/src/data_download.rs", "ANSWER_WITHIN"), 1),
]
# List A (D-4258): numbers no code or spec line holds, named and not compared.
LIST_A = [(BT, "12 months", "month", 12, 1), (BT, "30 days", "day", 30 * DAY, 1),
          (BT, "14 days", "day", 14 * DAY, 1), (BT, "2 days", "day", 2 * DAY, 2),
          (BT, "3 tiers", "tiers", 3, 1), (SUM, "2 días", "day", 2 * DAY, 1),
          (BH, "2 days", "day", 2 * DAY, 1), (BH, "4 days", "day", 4 * DAY, 1),
          (EB, "2 días", "day", 2 * DAY, 1), (EB, "4 días", "day", 4 * DAY, 1)]


def expected(path: str, drop: tuple | None = None) -> Counter[tuple[str, int]]:
    want: Counter[tuple[str, int]] = Counter()
    entries = [(e[0], e[2], (lambda n=e[3]: n), e[4]) for e in LIST_A if e != drop]
    for file, form, value, count in [*PAIRS, *entries]:
        want[(form, value())] += count if file == path else 0
    return +want


def mismatch(path: str, text: str | None = None, drop: tuple | None = None) -> dict[tuple[str, int], tuple[int, int]]:
    found, want = numbers(reader_text(path, text)), expected(path, drop)
    return {k: (found[k], want[k]) for k in found.keys() | want.keys() if found[k] != want[k]}


@pytest.mark.parametrize("entry", LIST_A, ids=lambda e: f"{Path(e[0]).name}:{e[1]}x{e[4]}")
def test_list_a_entry_passes_in_its_file(entry: tuple[str, str, str, int, int]) -> None:
    """D-4258 (4), (6): each List A entry reads as its key, stands in its file, and is what lets it pass."""
    path, written, form, value, count = entry
    assert numbers(written) == Counter({(form, value): 1})
    found, want = numbers(reader_text(path))[(form, value)], expected(path)[(form, value)]
    assert found == want >= count
    assert mismatch(path, drop=entry)[(form, value)] == (found, want - count)


@pytest.mark.parametrize(("phrase", "path"), [(p, f) for p, files in LIST_A_PHRASES.items() for f in files])
def test_list_a_phrase_passes_only_in_its_named_files(phrase: str, path: str) -> None:
    """D-4258 (4), (5), (6): each List A phrase stands in its named file and reads as no number and no
    partial refund there; the same phrase planted in the privacy policy (a file it is not named for) fails."""
    assert phrase.lower() in read(path).lower()
    assert numbers(reader_text(path, phrase)) == Counter() and partial_hits(reader_text(path, phrase)) == []
    planted = read(PP) + "\n" + phrase + "\n"
    assert mismatch(PP, planted) != {} or partial_hits(reader_text(PP, planted)) != []


@pytest.mark.parametrize("path", DOCS)
def test_every_number_matches_its_code_or_spec_value(path: str) -> None:
    """TD-FEE-19, TERMS-005, D-4258: each file as it stands passes; every number and List A entry counted."""
    assert mismatch(path) == {}


def _nth_line(text: str, anchor: str, nth: int) -> int:
    return [i for i, line in enumerate(text.splitlines()) if anchor in line][nth]


PLANTED_NUMBERS = [  # (file, line anchor, nth line with it, old, new), each from D-4258's list
    (BT, "**$199** over", 0, "over $1,500.00", "over $1,600.00"),
    (BT, "On a regular job", 0, "4 business hours", "3 business hours"),
    (BT, "Refund if you can't reach them: 2", 0, "72 hours", "48 hours"),
    (BT, "**Ongoing services:**", 0, "more than $75", "more than $199"),
    (BT, "Repeat picks from the same homeowner", 1, "closes.", "closes for 6 weeks."),
    (SUM, "Si el mismo propietario", 0, "180 días", "90 días"),
]


@pytest.mark.parametrize("case", PLANTED_NUMBERS, ids=lambda c: f"{Path(c[0]).name}:{c[4]}")
def test_planted_number_change_fails(case: tuple[str, str, int, str, str]) -> None:
    """TD-FEE-19, TERMS-005: a number changed on the document side alone fails."""
    path, anchor, nth, old, new = case
    lines = read(path).splitlines()
    index = _nth_line(read(path), anchor, nth)
    assert old in lines[index]
    lines[index] = lines[index].replace(old, new)
    assert mismatch(path, "\n".join(lines)) != {}


@pytest.mark.parametrize(("text", "want"), [
    ("$1,500", ("money", 150_000)), ("$1,500.00", ("money", 150_000)),
    ("a 72-hour question", ("hour-", 72 * HOUR)), ("within 72 hours", ("hour", 72 * HOUR)),
    ("count four months", ("month", 4)), ("cuatro meses", ("month", 4)),
    ("8 p.m.", ("clock", 20)), ("4 horas hábiles", ("business hour", 4 * HOUR)),
])
def test_written_forms_read_as_one_value(text: str, want: tuple[str, int]) -> None:
    """TD-FEE-19, D-4258: each written form is read to its value."""
    assert numbers(text) == Counter({want: 1})


def test_visit_tier_and_catalog_trip_fees() -> None:
    """TD-FEE-19, TERMS-005, D-3844: $25 default, $25 and $75 allowed, $199 refused; the catalog agrees."""
    cents = tier_cents()
    assert default_tier("VisitTier") == cents["Small"]
    assert sorted(visit_dollars(True)) == [cents["Small"] // 100, cents["Medium"] // 100]
    assert visit_dollars(False) == [cents["Large"] // 100]
    trips = re.findall(r"trip_fee_tier: (\d+)", read("docs/catalog/categories.yaml"))
    assert trips and all(int(t) in visit_dollars(True) for t in trips)
    assert tier_for_basis(pool_month_dollars() * 100 * const(PAY, "RECURRING_MONTHS")) == "Medium"


# --- No partial refund (REC-007, D-4258 List B), documents and code. ---

LIST_B = [re.compile(p, re.I) for p in (
    r"\bpartial(?:ly)?\b", r"\bpro[- ]?rat\w*", r"\bpro rata\b", r"\bin part\b",
    r"\b(?:half|portion|part|some|\d+\s?%|percent(?:age)?)\s+of\s+(?:the|your|its|that)\s+(?:introduction\s+)?(?:fee|charge|refund)\b",
    r"\brefund\w*\s+(?:\w+\s+){0,3}?(?:less|minus|reduced|smaller)\b",
    r"\b(?:reduced|smaller|lower)\s+(?:refund|fee)\b", r"\bearly[- ]cancel\w*",
    r"\bparcial(?:es|mente)?\b", r"\bprorrat\w*", r"\ben parte\b",
    r"\b(?:mitad|parte|porci[oó]n|\d+\s?%|porcentaje)\s+de\s+(?:la|su|esa)\s+(?:tarifa|cobro|devoluci[oó]n|reembolso)\b",
    r"\b(?:reembolso|devoluci[oó]n)\s+(?:\w+\s+){0,2}?(?:menor|reducid[oa])\b")]
CODE_PARTIAL = re.compile(r"(?i)\bpartial(?:ly)?\b|partial_?refund|prorat|pro_?rata")


def partial_hits(text: str) -> list[str]:
    return [m.group(0) for p in LIST_B for m in p.finditer(text)]


@pytest.mark.parametrize("path", DOCS)
def test_no_partial_refund_wording(path: str) -> None:
    """TD-FEE-19, REC-007, D-4258: no List B pattern in a scope document."""
    assert partial_hits(reader_text(path)) == []


@pytest.mark.parametrize(("sentence", "caught"), [(x, True) for x in (
    "You get a partial refund if the homeowner cancels early.", "The refund is prorated.",
    "Half of the fee is refunded.", "50% of your fee comes back.", "A portion of the fee is returned.",
    "Refunded in part.", "The refund is less the processing cost.", "A reduced refund applies after Confirm.",
    "Early-cancel refunds are smaller.", "Reembolso parcial si cancela.", "Se prorratea la tarifa.",
    "La mitad de la tarifa se devuelve.", "Se devuelve en parte.", "Una devolución reducida.")] + [(x, False) for x in (
    "It's not a share of the job.", "never a percentage or share of the job's price",
    "No es un porcentaje del trabajo.", "A one-time setup charge is never part of the fee basis.")])
def test_planted_partial_wording(sentence: str, caught: bool) -> None:
    """TD-FEE-19, REC-007, D-4258: each must-fail sentence is caught; must-pass and List A ones are not."""
    assert bool(partial_hits(reader_text(BT, sentence))) is caught


def code_lines(text: str) -> list[str]:
    """Non-test lines: each `#[cfg(test)]` item (a `;` line or a braced block) is skipped."""
    kept, skip, opened, depth = [], False, False, 0
    for line in text.splitlines():
        if line.strip() == "#[cfg(test)]":
            skip, opened, depth = True, False, 0
        elif skip:
            opened = opened or "{" in line
            depth += line.count("{") - line.count("}")
            skip = depth > 0 if opened else not line.rstrip().endswith(";")
        else:
            kept.append(line)
    return kept


def code_files() -> list[Path]:
    files = [p for crate in ("payments", "picks") for p in (ROOT / "app" / crate / "src").rglob("*.rs")
             if "tests" not in p.relative_to(ROOT / "app" / crate / "src").parts]
    return [*files, ROOT / "app/adapters/src/payments.rs"]


def code_partial_hits(text: str) -> list[str]:
    return [line for line in code_lines(text) if CODE_PARTIAL.search(line)]


def test_no_partial_refund_rule_in_code() -> None:
    """TD-FEE-19, REC-007, D-4258: no partial or pro-rata rule in the payments and picks code."""
    hits = {str(p.relative_to(ROOT)): code_partial_hits(p.read_text(encoding="utf-8")) for p in code_files()}
    assert len(hits) > 10 and {k: v for k, v in hits.items() if v} == {}


@pytest.mark.parametrize(("line", "caught"), [
    ("fn partial_refund(", True), ("let prorated = fee / 2;", True),
    ("#[derive(PartialEq)]", False), ("a.partial_cmp(&b)", False)])
def test_planted_code_lines(line: str, caught: bool) -> None:
    """TD-FEE-19, REC-007, D-4258: the planted code lines fail and `PartialEq` passes."""
    assert bool(code_partial_hits(line)) is caught


FULL_FORM = '[("payment_intent", payment_intent)]'


def _first_arg(text: str) -> str:
    """The text up to the first top-level `,` or the call's closing bracket."""
    depth = 0
    for i, ch in enumerate(text):
        if ch in "([{":
            depth += 1
        elif (ch in ")]}" and depth == 0) or (ch == "," and depth == 0):
            return text[:i].strip()
        elif ch in ")]}":
            depth -= 1
    return text.strip()


def refund_forms(text: str) -> list[str]:
    """The form every `.post("refunds", form, ...)` call sends, whatever the form's shape: an inline
    array as written, or a named binding resolved to its nearest `let NAME = ...;` before the call."""
    forms = []
    for post in re.finditer(r'\.post(?:::<[^>]*>)?\(\s*"refunds"\s*,', text):
        arg = _first_arg(text[post.end():]).lstrip("&").strip()
        if re.fullmatch(r"\w+", arg):
            lets = re.findall(rf"let (?:mut )?{arg}\b[^=]*= (.+?);", text[: post.start()], re.S)
            arg = lets[-1] if lets else ""
        forms.append(re.sub(r"\s+", " ", arg))
    return forms


AMOUNT = '[("payment_intent", payment_intent), ("amount", amount)]'


@pytest.mark.parametrize(("text", "full"), [
    (read("app/adapters/src/payments.rs"), True),
    (f'let form = {AMOUNT};\nself.post("refunds", &form, k)', False),
    (f'self.post("refunds", &{AMOUNT}, idempotency).await', False),
    (f'let params = {AMOUNT};\nlet created: Created = self.post("refunds", &params, k).await?;', False),
    (read("app/adapters/src/payments.rs") + f'\nself.post("refunds", &{AMOUNT}, k).await', False)],
    ids=["adapter-as-is", "named-form-amount", "inline-amount", "params-amount", "third-post-amount"])
def test_refund_posts_send_only_the_payment_intent(text: str, full: bool) -> None:
    """TD-FEE-19, REC-007, FEE-026, D-4258 (5): every refund post, however its form is written, sends
    only the payment intent (a full refund); an amount in any one of them, inline or named, fails."""
    forms = refund_forms(text)
    assert forms and len(forms) == len(re.findall(r'\(\s*"refunds"\s*,', text))
    assert all(f == FULL_FORM for f in forms) is full


# --- Refund triggers, timing and method (TERMS-005, TERMS-019). ---

TRIGGERS = {"Unreachable": "**Couldn't reach the homeowner.**",
            "CancelledBeforeTalk": "**Homeowner cancelled first.**",
            "ChargedAfterCancel": "**Charged after a cancel or after the job moved on.**"}
UNMET = {"AlreadyRefunded": "each happens once", "NothingCharged": "to the card that paid",
         "Talked": "no call in either direction was answered by a person",
         "TooFewAttempts": "call attempts", "AttemptsTooClose": "apart",
         "NoMissedCallNotice": "sent or attempted the missed-call text",
         "DoneOrProblemReply": "has not replied DONE or PROBLEM",
         "EmailedAndAgreed": "did not reply YES to the 72-hour"}


def variants(enum: str) -> list[str]:
    return re.findall(r"^    (\w+)", find(rf"pub enum {enum} \{{(.*?)\n\}}", read(REFUND), re.S).group(1), re.M)


def refunds_section() -> str:
    return read(BT).split("## 7. Refunds", 1)[1].split("\n## ", 1)[0]


def test_each_refund_trigger_and_condition_is_in_the_terms() -> None:
    """TD-FEE-19, TERMS-005: each `RefundReason` and `Unmet` condition is stated in section 7."""
    section = refunds_section()
    assert sorted(variants("RefundReason")) == sorted(TRIGGERS)
    assert sorted(variants("Unmet")) == sorted(UNMET)
    for phrase in [*TRIGGERS.values(), *UNMET.values()]:
        assert phrase in section, phrase
    items = re.findall(r"^\d+\. (\*\*.+?\*\*)", section, re.M)
    assert items == [*TRIGGERS.values(), "**Our mistake.**"]


def test_no_refund_when_the_business_never_calls() -> None:
    """TD-FEE-19, TERMS-019, D-047: the terms say a business that never calls in the window is not refunded."""
    hours = const(REFUND, "ATTEMPT_WINDOW") // HOUR
    assert f"**No refund** when you never call within {hours} hours of the release" in refunds_section()

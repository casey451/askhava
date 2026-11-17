"""The homeowner and business terms hold every required clause, and their wording scans pass.

TD-TERMS-01, TD-TERMS-03, TD-TERMS-05, TD-TERMS-06 (T111; TERMS-001 to TERMS-004, TERMS-006 to
TERMS-009, TERMS-011, TERMS-013 to TERMS-016, TERMS-018, TERMS-022, TERMS-024, TERMS-026 to
TERMS-028, TERMS-035, TERMS-036, FEE-002, FEE-006, BIZ-030, BIZ-057, WORD-003, WORD-004, WORD-020,
WEB-010; D-357, D-378, D-492, D-3877, D-3968).

The clause tests read `docs/legal/homeowner-terms.md`, `business-terms.md` (its "Summary: how
you're charged" section is BIZ-030's English summary) and `business-terms-summary-es.md` (the
Spanish summary, TD-TERMS-03). Every number they check comes from the code that sets it: the three
introduction fees from `app/payments/src/rules.rs`, never typed here, so changing a fee fails the
fee lists until the drafts match.

The wording scans are D-4038's closed lists, each pattern and removal case-insensitive, each
pattern with its own planted sentence that must fail and each removal a planted negated sentence
that must pass:
- List 1, money: every amount is read with its cents in the forms `$N`, `$N.NN`, `$N,NNN`,
  `USD N`, `N dollars`, `N dólares`, `N dolares`, `N USD`; a comma inside an amount is only a
  thousands separator before exactly three digits; each amount must be a figure the code sets.
- List 2, FEE-002: no percent form in a business document but "never a percentage".
- List 3, WORD-004: no "commission"; no "share of" in a business document but the negated ones.
- List 4, the Spanish summary outside its two English paragraphs: the Spanish ranking, commission,
  prospect, pest, fee-framing and arbitration words.
- List 5, WORD-020: no "partner" or "affiliate" but the TERMS-009 and WORD-014 sentences; no
  "socio" or "afiliado" in the Spanish summary.
The reviewer's reading, the cold read and the copy panel cover any phrasing off these lists.
Fake data only.

Run: python3 -m pytest tests/docs/test_terms_clauses.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

import brand
import check_legal_docs

ROOT = Path(__file__).resolve().parents[2]
LEGAL = ROOT / "docs" / "legal"
HOMEOWNER = "homeowner-terms.md"
BUSINESS = "business-terms.md"
SUMMARY_ES = "business-terms-summary-es.md"
MONEY_DOCUMENTS = (HOMEOWNER, BUSINESS, SUMMARY_ES)
BUSINESS_DOCUMENTS = tuple(sorted(check_legal_docs.BUSINESS_DOCUMENTS))
# TD-TERMS-05, TERMS-004: the homeowner liability cap the spec fixes, in cents.
LIABILITY_CAP_CENTS = 10_000


def _const(path: str, name: str) -> int:
    """The value of a Rust `const NAME: T = <int>;` or `= <int> * <int>;` (underscores allowed)."""
    text = (ROOT / path).read_text(encoding="utf-8")
    match = re.search(rf"\b{name}\s*:\s*\w+\s*=\s*([\d_]+(?:\s*\*\s*[\d_]+)*)\s*;", text)
    assert match, f"{path}: no const {name}"
    product = 1
    for factor in match.group(1).split("*"):
        product *= int(factor.strip().replace("_", ""))
    return product


def fee_cents() -> tuple[int, ...]:
    """FEE-001: the three introduction fees in cents, as `Tier::cents` sets them, small first."""
    text = (ROOT / "app/payments/src/rules.rs").read_text(encoding="utf-8")
    fees = [
        int(re.search(rf"Self::{tier}\s*=>\s*([\d_]+)", text).group(1).replace("_", ""))  # type: ignore[union-attr]
        for tier in ("Small", "Medium", "Large")
    ]
    return tuple(fees)


def allowed_cents() -> frozenset[int]:
    """List 1: every amount the drafts may name, each a figure the code or the spec sets."""
    return frozenset(
        {
            *fee_cents(),
            _const("app/payments/src/rules.rs", "MEDIUM_FROM_CENTS"),  # FEE-001 tier edge, $500.00
            _const("app/payments/src/rules.rs", "MEDIUM_UP_TO_CENTS"),  # FEE-001 tier edge, $1,500.00
            _const("app/licensing/src/handyman.rs", "HANDYMAN_LIMIT_CENTS"),  # LIC-024, $1,000
            LIABILITY_CAP_CENTS,  # TERMS-004, $100
        }
    )


def read(name: str) -> str:
    return check_legal_docs.normalize((LEGAL / name).read_text(encoding="utf-8"))


def brand_data() -> dict:
    return brand.load(brand.BRAND_FILE)


def dollars(cents: int) -> str:
    return f"${cents // 100}"


# ---------------------------------------------------------------- List 1: money

NUMBER = r"\d(?:[\d,.]*\d)?"
MONEY_FORMS = (
    re.compile(rf"\$\s*({NUMBER})"),
    re.compile(rf"\bUSD\s*({NUMBER})", re.IGNORECASE),
    re.compile(rf"(?<![\d,.$])({NUMBER})\s*(?:USD\b|dollars?\b|d[oó]lares\b)", re.IGNORECASE),
)
AMOUNT = re.compile(r"(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d{2})?")
FEE_LIST = re.compile(rf"\$({NUMBER}), \$({NUMBER}) (?:or|o) \$({NUMBER})", re.IGNORECASE)


def to_cents(token: str) -> int | None:
    """An amount in cents, or None when its commas or cents are malformed ("25,50", "25.5")."""
    if not AMOUNT.fullmatch(token):
        return None
    whole, _, cents = token.replace(",", "").partition(".")
    return int(whole) * 100 + int(cents or 0)


def amounts(text: str) -> list[tuple[str, int | None]]:
    """List 1: every amount in the text, with its cents (None when malformed)."""
    found = []
    for form in MONEY_FORMS:
        found += [(m.group(0), to_cents(m.group(1))) for m in form.finditer(text)]
    return found


def money_errors(text: str, allowed: frozenset[int], fees: tuple[int, ...]) -> list[str]:
    """List 1 and TD-TERMS-03: amounts exist, each is a code figure, each fee list is the fees."""
    found = amounts(text)
    errors = [] if found else ["no amount found"]
    errors += [f"amount not set by the code: {raw!r}" for raw, cents in found if cents not in allowed]
    lists = [tuple(to_cents(t) for t in m.groups()) for m in FEE_LIST.finditer(text)]
    errors += [] if lists else ["no fee list found"]
    errors += [f"fee list {got} is not the fee settings {fees}" for got in lists if got != fees]
    return errors


def test_fee_settings_are_read_from_the_code() -> None:
    """TD-TERMS-03: three increasing fees, read from `Tier::cents`."""
    fees = fee_cents()
    assert len(fees) == 3 and fees == tuple(sorted(fees)) and all(fees)


@pytest.mark.parametrize("name", MONEY_DOCUMENTS)
def test_every_amount_matches_a_code_figure(name: str) -> None:
    """TD-TERMS-03, TD-TERMS-05, List 1: each draft's amounts and fee lists match the settings."""
    assert money_errors(read(name), allowed_cents(), fee_cents()) == []


def test_changing_a_fee_setting_fails_the_drafts() -> None:
    """TD-TERMS-03: a fee setting the drafts don't show fails until they match."""
    small, medium, large = fee_cents()
    changed = (small, medium, large + 5_000)
    allowed = allowed_cents() | {large + 5_000}
    for name in MONEY_DOCUMENTS:
        assert money_errors(read(name), allowed, changed), name


def test_fee_list_with_commas_reads_three_amounts() -> None:
    """List 1: "$25, $75 or $199" is three amounts; the comma and space are punctuation."""
    fees = fee_cents()
    text = f"A fixed amount ({dollars(fees[0])}, {dollars(fees[1])} or {dollars(fees[2])})."
    assert [cents for _, cents in amounts(text)] == list(fees)
    assert money_errors(text, allowed_cents(), fees) == []


def test_no_amount_fails() -> None:
    """List 1: the fee-list assertion fails when no amount is found."""
    assert "no amount found" in money_errors("Joining is free.", allowed_cents(), fee_cents())


@pytest.mark.parametrize(
    "sentence",
    (
        "A {small}.50 setup charge applies.",
        "Cargo extra de {small},50.",
        "A {small}.5 charge applies.",
        "A $1,23 charge applies.",
        "A $59 charge applies.",
        "A $2,345 charge applies.",
        "A charge of USD 59 applies.",
        "A charge of 59 USD applies.",
        "A charge of 59 dollars applies.",
        "Un cargo de 59 dólares.",
        "Un cargo de 59 dolares.",
    ),
)
def test_planted_amount_fails(sentence: str) -> None:
    """List 1: cents count, a non-thousands comma fails, and every listed form is read."""
    fees = fee_cents()
    text = sentence.format(small=dollars(fees[0])) + f" ({dollars(fees[0])}, {dollars(fees[1])} or {dollars(fees[2])})"
    assert any(e.startswith("amount not set") for e in money_errors(text, allowed_cents(), fees))


@pytest.mark.parametrize("sentence", ("Fee basis $1,500.00 and $500.00.", "Under $1,000 total.", "Limited to $100."))
def test_planted_code_figure_passes(sentence: str) -> None:
    """List 1: the tier edges, the handyman limit and the liability cap are code or spec figures."""
    assert all(cents in allowed_cents() for _, cents in amounts(sentence))
    assert amounts(sentence)


# ---------------------------------------------------------------- Lists 2, 3 and 5: patterns


def scrub(text: str, removals: tuple[str, ...]) -> str:
    """The text with each removal phrase taken out, case-insensitively."""
    for phrase in removals:
        text = re.sub(re.escape(phrase), " ", text, flags=re.IGNORECASE)
    return text


def hits(text: str, patterns: tuple[str, ...], removals: tuple[str, ...] = ()) -> list[str]:
    rest = scrub(text, removals)
    return [m.group(0) for p in patterns for m in [re.search(p, rest, re.IGNORECASE)] if m]


PERCENT = (r"%", r"\bpercent\w*", r"\bpct\b", r"\bporcentaj\w*", r"\bporcentual\w*", r"\bpor\s*ciento\b")
PERCENT_REMOVALS = ("never a percentage", "nunca un porcentaje")
COMMISSION = (r"\bcommission\w*",)
SHARE = (r"\bshares?\s+of\b",)
SHARE_REMOVALS = ("never a percentage or share of", "never a share of", "not a share of")
PARTNER = (r"\bpartners?\b", r"\baffiliat\w*")
PARTNER_ES = (r"\bsocios?\b", r"\bafiliad\w*")


def partner_removals() -> tuple[str, ...]:
    """List 5: WORD-020's two exempt sentences (TERMS-009; WORD-014 with the brand filled)."""
    word_014 = brand.fill("Independent business, not affiliated with {brand}.", data=brand_data())
    return (check_legal_docs.TWILIO_LINE[1], word_014)


SCANS = (
    ("FEE-002", BUSINESS_DOCUMENTS, PERCENT, PERCENT_REMOVALS),
    ("WORD-004", (HOMEOWNER, *BUSINESS_DOCUMENTS), COMMISSION, ()),
    ("WORD-004", BUSINESS_DOCUMENTS, SHARE, SHARE_REMOVALS),
)


@pytest.mark.parametrize(("req", "names", "patterns", "removals"), SCANS)
def test_documents_pass_the_scans(req: str, names: tuple[str, ...], patterns: tuple[str, ...], removals: tuple[str, ...]) -> None:
    """TD-TERMS-03, Lists 2 and 3: FEE-002 and WORD-004 wording in the drafts."""
    for name in names:
        assert hits(read(name), patterns, removals) == [], f"{req} {name}"


def test_documents_pass_the_partner_scan() -> None:
    """List 5, WORD-020: no partner or affiliate wording but the two exempt sentences."""
    for name in (HOMEOWNER, *BUSINESS_DOCUMENTS):
        assert hits(read(name), PARTNER, partner_removals()) == [], name
    assert hits(spanish_rest(), PARTNER_ES) == []


PLANTED = (
    (PERCENT, PERCENT_REMOVALS, "The fee is 10% of the job."),
    (PERCENT, PERCENT_REMOVALS, "The fee is 10 percent of the job."),
    (PERCENT, PERCENT_REMOVALS, "Fee: 10 pct."),
    (PERCENT, PERCENT_REMOVALS, "Un porcentaje del trabajo."),
    (PERCENT, PERCENT_REMOVALS, "Una tarifa porcentual."),
    (PERCENT, PERCENT_REMOVALS, "El 10 por ciento del trabajo."),
    (COMMISSION, (), "A small Commission applies."),
    (SHARE, SHARE_REMOVALS, "The fee is a share of the job."),
    (PARTNER, (), "Our Partners do the work."),
    (PARTNER, (), "An affiliated business."),
    (PARTNER_ES, (), "Nuestros socios hacen el trabajo."),
    (PARTNER_ES, (), "Un negocio afiliado."),
)


@pytest.mark.parametrize(("patterns", "removals", "sentence"), PLANTED)
def test_planted_wording_fails(patterns: tuple[str, ...], removals: tuple[str, ...], sentence: str) -> None:
    """Lists 2, 3 and 5: each pattern catches its planted sentence, in any case."""
    assert hits(sentence, patterns, removals)
    assert hits(sentence.upper(), patterns, removals)


@pytest.mark.parametrize(
    ("patterns", "removals", "sentence"),
    (
        (PERCENT, PERCENT_REMOVALS, "A fixed amount, NEVER A PERCENTAGE of the price."),
        (PERCENT, PERCENT_REMOVALS, "Una cantidad fija, nunca un porcentaje del precio."),
        (SHARE, SHARE_REMOVALS, "A fixed amount, never a percentage or share of the job's price."),
        (SHARE, SHARE_REMOVALS, "It's a fixed introduction fee, never a share of the job."),
        (SHARE, SHARE_REMOVALS, "It's Not A Share Of the job."),
        (PARTNER, None, "No mobile information will be shared with third parties or affiliates for marketing or promotional purposes."),
        (PARTNER, None, "independent business, not affiliated with {brand}."),
    ),
)
def test_planted_negation_passes(patterns: tuple[str, ...], removals: tuple[str, ...] | None, sentence: str) -> None:
    """Lists 2, 3 and 5: each removal lets its negated or exempt sentence pass, in any case."""
    text = brand.fill(sentence, data=brand_data())
    assert hits(text, patterns, partner_removals() if removals is None else removals) == []


# ---------------------------------------------------------------- List 4: the Spanish summary

SPANISH = (
    r"\bcomisi\w*",
    r"\bclientes?\s+potencial(?:es)?\b",
    r"\bprospect\w*",
    r"\bmejor(?:es)?\b",
    r"\bverificad[oa]s?\b",
    r"\bexaminad[oa]s?\b",
    r"\bconfiables?\b",
    r"\bexclusiv[oa]s?\b",
    r"\bm[aá]s\s+(?:valorad|calificad)[oa]s?\b",
    r"\bn[uú]mero\s+(?:1|uno)\b",
    r"\bplagas?\b",
    r"\b(?:cuota|tarifa|cargo|pago|costo|precio)\s+(?:de|por)\s+(?:la\s+|una\s+)?licencia\b",
    r"\blicencia\s+de\s+uso\b",
    r"\bacceso\s+a\s+(?:(?:los|las|sus|la|el)\s+)?(?:datos|informaci[oó]n|propietari[oa]s?|clientes?)\b",
    r"\bderecho\s+(?:de|a)\s+(?:uso|usar|utilizar)\b",
    r"\bcompra\s+de\s+(?:(?:la\s+)?informaci[oó]n\s+de\s+|(?:los\s+)?datos\s+de\s+)?contacto\w*",
    r"\bventa\s+de\s+(?:la\s+|los\s+|sus\s+)?(?:informaci[oó]n|datos)\b",
    r"\barbitraje\b",
)
SPANISH_PLANTED = (
    "Sin comisión.",
    "Le damos clientes potenciales.",
    "Prospectos nuevos.",
    "Los mejores negocios.",
    "Negocios verificados.",
    "Negocios examinados.",
    "Negocios confiables.",
    "Un trato exclusivo.",
    "Los más valorados.",
    "El número 1 de la ciudad.",
    "Control de plagas.",
    "Una cuota de licencia.",
    "Una licencia de uso.",
    "Acceso a los datos.",
    "El derecho de uso.",
    "La compra de información de contacto.",
    "La venta de datos.",
    "Arbitraje obligatorio.",
)


def spanish_rest() -> str:
    """The Spanish summary with its two exempt English paragraphs removed (D-3968)."""
    text = read(SUMMARY_ES)
    for _, template in check_legal_docs.REQUIRED[SUMMARY_ES]:
        text = text.replace(check_legal_docs.normalize(brand.fill(template, data=brand_data())), " ")
    return text


def test_spanish_summary_passes_list_4() -> None:
    """TD-TERMS-03, List 4, D-3877: no listed Spanish wording outside the English paragraphs."""
    assert hits(spanish_rest(), SPANISH) == []


@pytest.mark.parametrize(("pattern", "sentence"), tuple(zip(SPANISH, SPANISH_PLANTED, strict=True)))
def test_planted_spanish_wording_fails(pattern: str, sentence: str) -> None:
    """List 4: each pattern catches its own planted sentence, in any case."""
    assert hits(sentence, (pattern,))
    assert hits(sentence.upper(), (pattern,))


# ---------------------------------------------------------------- the clauses


def settings() -> dict[str, int]:
    """The windows and days the drafts state, each read from the code that sets it."""
    help_times = (ROOT / "app/server/src/inbound_text.rs").read_text(encoding="utf-8")
    reply = re.search(r"\bHELP_TIMES\s*:\s*\([^)]*\)\s*=\s*\((\d+)\s*,", help_times)
    assert reply, "no HELP_TIMES"
    return {
        "hours": _const("app/jobs/src/rules.rs", "CONFIRM_BUSINESS_MINUTES") // 60,  # BIZ-030, PICK-019
        "minutes": _const("app/jobs/src/rules.rs", "EMERGENCY_MINUTES"),  # BIZ-030, PICK-044
        "repeat_days": _const("app/businesses/src/signup.rs", "REPEAT_FREE_DAYS"),  # BIZ-030, FEE-013
        "reply_days": int(reply.group(1)),  # TERMS-022, CON-018
    }


def fill(template: str) -> str:
    """A clause template with the brand from the brand file and the numbers from the code."""
    small, medium, large = (dollars(c) for c in fee_cents())
    numbers = {**settings(), "small": small, "medium": medium, "large": large}
    return check_legal_docs.normalize(brand.fill(template, data=brand_data()).format(**numbers))


CLAUSES: tuple[tuple[str, str, str], ...] = (
    (HOMEOWNER, "TERMS-001", "You accept these terms only when you pick a quote, on your quote page or by text."),
    (HOMEOWNER, "TERMS-001", 'next to the Pick button the page says "By picking, you agree to the Terms"'),
    (HOMEOWNER, "TERMS-001", "ends with the same line and a link to {domain}/terms"),
    (HOMEOWNER, "TERMS-001", "you pick by texting back the quote's number and YES, for example 2 YES"),
    (HOMEOWNER, "TERMS-001", "Your first text, our consent text and any other reply never accept these terms."),
    (HOMEOWNER, "TERMS-001", "is an electronic signature with the same effect as signing on paper"),
    (HOMEOWNER, "TERMS-001", "(A.R.S. 44-7007) and the federal E-SIGN Act"),
    (HOMEOWNER, "TERMS-002", "You must be 18 or older."),
    (HOMEOWNER, "TERMS-002", "Arizona law governs these terms."),
    (HOMEOWNER, "TERMS-003", "{brand} is not an emergency service"),
    (BUSINESS, "TERMS-003", "{brand} is not an emergency service."),
    (BUSINESS, "BIZ-030", "you have {minutes} minutes to confirm the pick instead of {hours} business hours"),
    (HOMEOWNER, "TERMS-004", check_legal_docs.LONG_STATEMENT[1]),
    (HOMEOWNER, "TERMS-004", "You and the business make your own agreement; {brand} is not part of it and doesn't guarantee the work."),
    (HOMEOWNER, "TERMS-004", "**Limit of liability.** To the extent the law allows, {brand}'s total liability to you for any claim about the service is limited to $100. This limit does not apply to gross negligence, intentional misconduct, or anything the law does not allow us to limit."),
    (HOMEOWNER, "TERMS-004", "Nothing in these terms releases or waives any claim for personal injury."),
    (HOMEOWNER, "TERMS-004", "{brand} gives no warranty of any kind."),
    (HOMEOWNER, "WEB-026", "The business you choose pays {brand} a set introduction fee ({small}, {medium} or {large}, by quote size). You pay {brand} nothing."),
    (BUSINESS, "TERMS-006", "**Non-discrimination.** You may not refuse, price or treat a job differently because of a homeowner's race"),
    (BUSINESS, "TERMS-007", "I authorize {brand_legal} to save this card with Stripe for introduction fees."),
    (BUSINESS, "TERMS-007", "**Handymen** (not licensed contractors) certify on each quote that it is a full-job price under $1,000"),
    (BUSINESS, "TERMS-011", "Job requests come by email"),
    (BUSINESS, "TERMS-011", "{brand} never places recorded or automated calls to you."),
    (HOMEOWNER, "TERMS-016", "We keep a record of the terms version you accepted"),
    (BUSINESS, "TERMS-016", "We keep a record of each version you accepted and when"),
    (BUSINESS, "TERMS-022", "If a charge was our mistake, it is refunded in full."),
    (BUSINESS, "TERMS-022", '**Questions about a charge:** use "Ask about a charge" in your portal or reply to any {brand} email. The {brand} owner usually replies within {reply_days} days.'),
    (BUSINESS, "TERMS-024", "As part of the introduction, {brand} gives you the homeowner's first name and the job address so you can contact them about this job. Use them only for this job and follow-up about it. No marketing lists, no resale."),
    (BUSINESS, "TERMS-024", "A homeowner can report a misuse of their details by replying PROBLEM."),
    (BUSINESS, "FEE-006", "charged when you confirm, and never changed by the final bill"),
    (BUSINESS, "BIZ-057", "{brand} never buys, imports, rents or resells requests from another site or list."),
    (BUSINESS, "BIZ-030", "Free to join. Pay only when you confirm a pick: {small} / {medium} / {large} by job size. Refund if you can't reach them."),
    (BUSINESS, "BIZ-030", "Picks must be confirmed within {hours} business hours (8 a.m. to 8 p.m., every day), or within {minutes} minutes for an emergency job."),
    (BUSINESS, "BIZ-030", "It's an introduction fee for a homeowner who chose you."),
    (BUSINESS, "BIZ-030", "You pay one fixed fee when you confirm a pick: {small}, {medium} or {large} by job size. It's not a share of the job."),
    (BUSINESS, "BIZ-030", "It's a fixed introduction fee, never a share of the job, and it doesn't depend on the work being done or paid for."),
    (BUSINESS, "BIZ-030", "The homeowner sees every quote and picks one business: you. It isn't shared with anyone else."),
    (BUSINESS, "BIZ-030", "Repeat picks from the same homeowner are free for {repeat_days} days after the job closes."),
    (BUSINESS, "BIZ-030", "Fees follow the published schedule, and a change needs your OK first."),
    (BUSINESS, "BIZ-030", "Statements download as CSV."),
    (BUSINESS, "BIZ-030", "If many picks in your trade end without a hire, the {brand} owner reviews it and may add a credit."),
    (SUMMARY_ES, "TERMS-018", "The paid professional review of this Spanish comes before the Spanish conversations setting is turned on"),
    (SUMMARY_ES, "BIZ-030", "Paga solo cuando confirma que un propietario lo eligió: {small} / {medium} / {large} según el tamaño del trabajo."),
    (SUMMARY_ES, "BIZ-030", "dentro de {hours} horas del horario de 8 a.m. a 8 p.m., cualquier día, o dentro de {minutes} minutos si es un trabajo de emergencia"),
    (SUMMARY_ES, "BIZ-030", "gratis durante {repeat_days} días después de que se cierre el trabajo"),
    (SUMMARY_ES, "TERMS-022", "Si un cobro fue nuestro error, se reembolsa completo."),
    (SUMMARY_ES, "TERMS-022", "El dueño de {brand} normalmente responde en {reply_days} días."),
    (SUMMARY_ES, "TERMS-026", "({small}, {medium} o {large}), nunca un porcentaje ni una parte del precio del trabajo"),
    (SUMMARY_ES, "TERMS-036", "{brand} no promete ningún número de solicitudes, cotizaciones, elecciones ni trabajos."),
)
GENERAL: tuple[tuple[str, str, str], ...] = (
    (HOMEOWNER, "TERMS-035", "These terms, the {brand} Privacy Policy and the {brand} SMS Terms are the whole agreement"),
    (HOMEOWNER, "TERMS-035", "If we don't enforce a part of these terms right away, we can still enforce it later."),
    (HOMEOWNER, "TERMS-035", "{brand} may transfer these terms to a business that takes over the service, and will tell you first."),
    (HOMEOWNER, "TERMS-035", "We send notices about these terms by email"),
    (HOMEOWNER, "TERMS-035", "A change applies only to requests you send after that date. The version in effect when you pick governs that request."),
    (BUSINESS, "TERMS-035", "These terms, the {brand} Privacy Policy and the {brand} Business Email Notice Terms are the whole agreement"),
    (BUSINESS, "TERMS-035", "If we don't enforce a part of these terms right away, we can still enforce it later."),
    (BUSINESS, "TERMS-035", "Your business may not transfer them without {brand}'s written OK."),
    (BUSINESS, "TERMS-035", "We send notices to the email address on your account."),
    (BUSINESS, "TERMS-035", "write to us at help@{domain} and give us 30 days to try to fix it"),
    (BUSINESS, "TERMS-035", "Either of us may use small claims court at any time."),
    (BUSINESS, "TERMS-035", "We'll email you any change at least 14 days before it applies"),
    (BUSINESS, "TERMS-036", "{brand} may end your business's account for a false certification or statement, misuse of homeowner details, discrimination, unpaid charges, or a confirmed breach of the handyman certification. We email you the reason."),
    (BUSINESS, "TERMS-036", "fees you owe, the limits on using homeowner details (section 8) and the indemnity (section 10)"),
    (BUSINESS, "TERMS-036", "{brand} doesn't promise any number of job cards, quotes, picks or jobs."),
)


@pytest.mark.parametrize(("name", "req", "template"), CLAUSES + GENERAL)
def test_clause_is_in_its_draft(name: str, req: str, template: str) -> None:
    """TD-TERMS-01, TD-TERMS-03, TD-TERMS-05: each clause, with code-set numbers, is in its draft."""
    assert fill(template) in read(name), f"{req} missing from {name}"


@pytest.mark.parametrize("name", (HOMEOWNER, BUSINESS))
def test_general_section_ends_the_terms(name: str) -> None:
    """TERMS-035: the last section is General, opening with the severability sentence."""
    text = (LEGAL / name).read_text(encoding="utf-8")
    last = text.rsplit("\n## ", 1)[1]
    assert re.match(r"\d+\. General\n", last)
    assert last.split("\n- ", 2)[1].startswith(check_legal_docs.SEVERABILITY[1])


@pytest.mark.parametrize("name", (BUSINESS, SUMMARY_ES))
def test_business_statement_and_paragraph_open_the_document(name: str) -> None:
    """TERMS-026, D-3877: the WORD-002 business statement comes before the first section, and the
    TERMS-026 paragraph is there word for word."""
    statement, paragraph = (
        fill(template) for req, template in check_legal_docs.REQUIRED[BUSINESS] if req in ("WORD-002", "TERMS-026")
    )
    text = read(name)
    assert statement in text and paragraph in text
    assert text.index(statement) < text.index(" ## ")


def test_class_waiver_and_arbitration_only_in_the_optional_section() -> None:
    """TERMS-002: no arbitration clause or class waiver in effect; only the OPTIONAL, OFF section."""
    lines = (LEGAL / HOMEOWNER).read_text(encoding="utf-8").splitlines()
    assert check_legal_docs.check_arbitration(HOMEOWNER, lines) == []
    text = "\n".join(lines)
    start = text.index("### OPTIONAL: Arbitration")
    end = text.index("\n## ", start)
    outside = text[:start] + text[end:]
    assert not re.search(r"\bclass\b|arbitrat|waiver", outside, re.IGNORECASE)


def test_spanish_summary_is_listed_as_a_business_document() -> None:
    """TD-TERMS-06, TERMS-015, TERMS-028: the summary is a checked business document."""
    assert SUMMARY_ES in check_legal_docs.DOCUMENTS
    assert SUMMARY_ES in check_legal_docs.BUSINESS_DOCUMENTS
    report = check_legal_docs.run(ROOT)
    assert report.errors == [] and report.checked == len(check_legal_docs.DOCUMENTS) == 10


def test_privacy_policy_holds_the_no_sharing_sentence() -> None:
    """TD-TERMS-01, TERMS-008, TERMS-009: the privacy policy keeps the Twilio sentence and Do Not
    Track (the policy is T112's draft; this task reads it)."""
    text = read("privacy-policy.md")
    assert check_legal_docs.TWILIO_LINE[1] in text
    assert "Do Not Track" in text

"""The fee is only ever an introduction fee (T203; WORD-026, TD-WORD-10; D-443, D-444, D-4258).

D-4258's rule (2), one definition for worker and reviewer, run on `test_wording_scan.reader(rel)[1]`
of every `test_wording_scan.scanned_files()` file:
1. T104's List A (`test_wording_scan.LIST_A`) is removed word for word first.
2. Imported List B, never allowed: `test_wording_scan.FEE` (commission, comisión, share, cut) and
   `test_wording_scan.LEADS[0]` ("lead"), applied as T104 applies them.
3. Own List B, allowed only when negated (`test_wording_scan.negated`): fee names other than the
   introduction fee, buying or unlocking a job or a homeowner, customer data, paid placement,
   subscriptions and software access, in English and Spanish.
The business's own charges ("trip fee"), the carrier's "roaming fees", the opt-out "Se canceló su
suscripción." and "customer information" (the Safeguards Rule term) are not on List B. Phrasing off
these lists is a scan list note, logged as "statements scan list: add <pattern>". The receipt line
"Laftly introduction service, job <n>" stays TD-WORD-10's existing case. Fake data only.

Run: python3 -m pytest tests/docs/test_introduction_fee_wording.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_wording_scan as scan  # noqa: E402  T104's scope, reader, lists and negation (D-4258)

NAME = scan.NAME
IMPORTED = (*scan.FEE, scan.LEADS[0])
WHO = r"(?:introductions?|jobs?|requests?|homeowners?|contacts?|customers?|leads?)"
OWN = tuple(
    re.compile(p, re.IGNORECASE)
    for p in (
        r"\b(?:referral|finder'?s|lead|booking|service|platform|listing|placement|membership|subscription|success"
        r"|match(?:ing)?|connection|transaction|advertising|marketing|access|contact|unlock|dispatch|broker(?:age)?)"
        r"\s+fees?\b",
        r"\blead\s+(?:price|cost|charge)s?\b",
        r"\b(?:pay|price|cost)[- ]per[- ](?:lead|click|contact|request|job)\b",
        r"\bper[- ]lead\b",
        rf"\bpurchas\w*\s+(?:[\w']+\s+){{0,2}}?{WHO}\b",
        rf"\b(?:buy|buys|buying|bought)\s+(?:[\w']+\s+){{0,2}}?{WHO}\b",
        r"\bbuy this\b",
        r"\bunlock\w*",
        r"\bcustomer (?:data|lists?)\b",
        r"\b(?:sell|sells|sold|selling|pay(?:s|ing)? for|paid for|purchase[sd]?)\s+(?:[\w']+\s+){0,2}?"
        r"contact (?:info\w*|details)\b",
        r"\bpaid placement\b",
        r"\b(?:featured|sponsored|premium|promoted|boosted)\s+"
        r"(?:listings?|placements?|spots?|business(?:es)?|profiles?)\b",
        r"\b(?:monthly|annual|yearly|paid)\s+(?:subscriptions?|memberships?|plans?)\b",
        r"\bsoftware access\b",
        r"\b(?:tarifa|cuota|cargo|costo|precio)s?\s+(?:de|por)\s+(?:referencia|referido|servicio|plataforma"
        r"|suscripci[oó]n|membres[ií]a|reserva|conexi[oó]n|anuncio|publicidad|cliente|contacto|lead|prospecto"
        r"|acceso)\w*",
        r"\bcompr\w*\s+(?:[\w']+\s+){0,2}?"
        r"(?:presentaci\w+|trabajos?|solicitud\w*|clientes?|contactos?|propietarios?)\b",
        r"\bdesbloque\w*",
        r"\bdatos de(?:l)? clientes?\b",
        r"\bvend\w*\s+(?:[\w']+\s+){0,2}?(?:datos|informaci[oó]n) de contacto\b",
        r"\b(?:suscripci[oó]n|membres[ií]a)(?:es|s)?\s+(?:mensual|anual|pagad)\w*",
        r"\b(?:anuncios?|lugares?|puestos?) (?:destacad|patrocinad|pagad)\w*",
    )
)
MUST_FAIL = (
    "We charge a commission.", "A small cut of the job.", "Buy leads here.", "A referral fee per job.",
    "The finder's fee is $25.", "A lead fee per job.", "The lead price is $25.", "Pay per lead.", "Per-lead pricing.",
    "A booking fee per job.", "Our service fee.", "A platform fee.", "A listing fee.", "A placement fee.",
    "A membership fee.", "A subscription fee.", "A success fee.", "A matching fee.", "A connection fee.",
    "A transaction fee.", "An advertising fee.", "A marketing fee.", "An access fee.", "A contact fee.",
    "An unlock fee.", "Purchase this job.", "Buy the homeowner's request.", "Buy this.", "Unlock the homeowner.",
    "Customer data for sale.", "We sell the homeowner's contact info.", "Paid placement for $50.",
    "Featured listings for businesses.", "A monthly subscription.", "Software access included.",
    "Una tarifa de referencia.", "Cuota de servicio.", "Cargo por contacto.", "Compre este trabajo.",
    "Desbloquee al cliente.", "Datos de clientes.", "Vendemos la información de contacto.", "Suscripción mensual.",
    "Anuncios destacados.",
)
MUST_PASS = (
    "It's a fixed introduction fee, never a share of the job.", "Accept the introduction",
    "You were picked by the homeowner", "You pay one fixed fee when you confirm a pick.",
    "Trip fee counts toward the repair", "Your phone company may charge roaming fees.", "Se canceló su suscripción.",
    f"Before a provider handles customer information, {NAME} checks it.", "There is no subscription fee.",
    f"{NAME} charges no listing fee and no paid placement.", "Es una tarifa de presentación fija.",
    "Pueden aplicar cargos de msj y datos.", "a home sale, purchase or refinance",
)


def fee_hits(text: str, own: tuple[re.Pattern[str], ...] = OWN) -> list[str]:
    """WORD-026: T104's List A removed, the imported list never allowed, the own list unless negated."""
    for phrase in scan.LIST_A:
        text = re.sub(re.escape(phrase), " ", text, flags=re.IGNORECASE)
    found = [m.group(0) for p in IMPORTED for m in p.finditer(text)]
    return found + [m.group(0) for p in own for m in p.finditer(text) if not scan.negated(text, m.start())]


@pytest.mark.parametrize("sentence", MUST_FAIL)
def test_td_word_10_word_026_a_fee_named_otherwise_fails(sentence: str) -> None:
    assert fee_hits(sentence)


@pytest.mark.parametrize("sentence", MUST_PASS)
def test_td_word_10_word_026_the_introduction_fee_passes(sentence: str) -> None:
    assert fee_hits(sentence) == []


def test_td_word_10_every_own_pattern_has_a_planted_line() -> None:
    """D-2506: each own pattern catches a planted sentence, so removing one is seen."""
    for pattern in OWN:
        assert any(pattern.search(s) for s in MUST_FAIL), pattern.pattern


def test_td_word_10_a_negation_is_lifted_by_but() -> None:
    assert fee_hits("There is no setup cost, but a listing fee applies.")


@pytest.mark.parametrize("rel", scan.scanned_files())
def test_td_word_10_every_scanned_file_calls_it_an_introduction_fee(rel: str) -> None:
    assert fee_hits(scan.reader(rel)[1]) == [], rel

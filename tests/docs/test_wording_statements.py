"""The required statements and lines are where the spec puts them, and four closed scans hold (T203).

TD-WORD-05, TD-WORD-02 (the insurance, privacy and independent-business cases, D-493), TD-WORD-04,
TD-WEB-03 (WEB-010, WEB-021), TD-LIC-18 (WEB-010, WEB-015, WORD-002, WORD-006, WORD-007, WORD-014,
WORD-017, WORD-021, WEB-018, WEB-021, WEB-025, BIZ-030, CARD-020, PICK-017, TERMS-004, LIC-042;
D-356, D-3155, D-3879, D-3889, D-4258, D-4596).

Every expected line is read from `docs/spec/` (`test_wording_scan.spec_quotes`) and the brand from
the brand file; Rust-built text is matched in its placeholder form (`{name}`, `{brand}`, D-3889).
The scans are D-4258's closed lists, one definition for worker and reviewer, run on
`test_wording_scan.reader(rel)[1]` of every `test_wording_scan.scanned_files()` file:
1. WEB-010: List A removed, then PEST with no negation allowance; no pest page or pest link.
2. WEB-021, D-183: a sentence naming the owner never names the owner's employer or occupation.
3. LIC-042, TERMS-004: List A removed, then WARRANTY, allowed only when negated.
The no-owner's-name scan (WORD-021) skips Askama comments and the copy file's `**Status:` note.
Phrasing off these lists is a scan list note, logged as "statements scan list: add <pattern>".
The fee rule (WORD-026) is `test_introduction_fee_wording.py`. Fake data only.

Run: python3 -m pytest tests/docs/test_wording_statements.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

import pytest
import yaml

import check_legal_docs

sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_wording_scan as scan  # noqa: E402  T104's scope, reader and negation, imported (D-4258)

ROOT, NAME, I = scan.ROOT, scan.NAME, re.IGNORECASE
SITE, PORTALS = "app/site/templates/", "app/portals/templates/"
LONG_EN, SHORT_EN, BUSINESS_EN, LONG_ES = scan.WORD_002
SHORT_ES = f"{NAME} no es contratista; usted contrata al negocio que elija."
BUSINESS_ES = (
    f"{NAME} no es contratista. Hace llegar esta solicitud y la elección del propietario. Si el propietario"
    f" lo elige a usted, lo contrata directamente; {NAME} no dirige, programa, supervisa ni paga el trabajo."
)
PICK = scan.spec_quotes("quotes-pick.md", "PICK-017")[1:3]
AGREEMENT, CAP, CAP_HEADING = scan.spec_quotes("terms.md", "TERMS-004")
HAVA_Q, HAVA_A, HAVA_END = scan.spec_quotes("website.md", "WEB-025")
OLD = str(scan.DATA["forbidden_alone"][0])
BIZ_030 = [scan.spec_quotes("businesses.md", "BIZ-030")[i] for i in (2, 4, 6)]  # the lines with no fee amount
WORD_017 = scan.spec_quotes("wording.md", "WORD-017")[:2]
PRIVACY = ". ".join(scan.spec_quotes("wording.md", "WORD-007"))  # WORD-007: the two lines together

# TD-WORD-05: (file, line) for each surface; a `.rs` file is read in its placeholder form.
SURFACES = (
    (SITE + "site_layout.html", LONG_EN), ("docs/legal/homeowner-terms.md", LONG_EN),
    ("docs/legal/privacy-policy.md", LONG_EN), (PORTALS + "quote_page.html", LONG_EN),
    (SITE + "es_home.html", LONG_ES), (SITE + "business.html", BUSINESS_EN),
    (SITE + "es_business.html", BUSINESS_ES), ("docs/legal/business-terms.md", BUSINESS_EN),
    ("docs/legal/business-terms-summary-es.md", BUSINESS_EN), ("app/businesses/src/signup.rs", BUSINESS_EN),
    (PORTALS + "pick_review.html", PICK[0]), (PORTALS + "pick_review.html", PICK[1]),
    ("docs/legal/homeowner-terms.md", AGREEMENT), ("docs/legal/homeowner-terms.md", CAP),
    ("docs/legal/homeowner-terms.md", f"**{CAP_HEADING}.**"),
    ("app/quotes/src/quote_card.rs", scan.WORD_014),
    (SITE + "home.html", PRIVACY), ("docs/legal/privacy-policy.md", PRIVACY),
    (SITE + "business.html", f"{HAVA_A}\"{OLD}\"{HAVA_END}"), (SITE + "help.html", f"{HAVA_A}\"{OLD}\"{HAVA_END}"),
    ("app/messaging/src/business_notices.rs", scan.spec_quotes("job-cards.md", "CARD-020")[1]),
    *((SITE + "business.html", line) for line in WORD_017),
    *((rel, line) for rel in (SITE + "business.html", "docs/legal/business-terms.md", "app/businesses/src/signup.rs")
      for line in BIZ_030),
)
# CARD-020, BIZ-030, WORD-002: the business statement reaches the card email, the portal card and the sign-up.
WIRING = (
    ("app/quotes/src/card_notices.rs", '"statement": business_statement()'),
    ("app/messaging/src/business_notices.rs", '&p["statement"]'),
    ("app/portals/src/business_actions.rs", "statement: business_statement()"),
    (PORTALS + "business_actions.html", "{{ p.statement }}"),
    ("app/businesses/src/signup.rs", "business_statement(),\n"),
    (PORTALS + "business_signup.html", "{% for line in fee_lines %}<li>{{ line }}</li>"),
    (SITE + "help.html", '{% extends "site_layout.html" %}'),
)


def flat(text: str) -> str:
    return " ".join(text.split())


def surface_text(rel: str, source: str) -> str:
    """What a surface says: reader text, old-name tokens filled; Rust in its placeholder form."""
    if rel.endswith(".rs"):
        return flat(re.sub(r"\\\n\s*", "", source))
    source = re.sub(r"\{\{\s*(?:facts\.)?old_name\s*\}\}", OLD, source)
    path = ROOT / rel
    if rel.endswith(".html"):
        return flat(scan.html_text(source))
    return flat(source if path.suffix == ".md" else scan.json_text(source, rel.endswith("es.json")))


def form(rel: str, line: str) -> str:
    """The line as its surface holds it: Rust-built text in its placeholder form (D-3889)."""
    if not rel.endswith(".rs"):
        return flat(line)
    token = "{brand}" if "quote_card" in rel else "{short}" if "notices" in rel else "{name}"
    for spec, code in (("<category>", "{trade}"), ("<town>", "{town}"), ("<time>", "{time}"), (NAME, token)):
        line = line.replace(spec, code)
    return flat(line)


def holds(rel: str, line: str, source: str) -> bool:
    """WORD-002 and the other required lines stand on the surface."""
    return form(rel, line) in surface_text(rel, source)


def source(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


@pytest.mark.parametrize("rel,line", SURFACES, ids=lambda v: str(v)[:40])
def test_td_word_05_td_word_02_each_line_stands_and_its_removal_fails(rel: str, line: str) -> None:
    text = source(rel)
    assert holds(rel, line, text), f"{rel}: missing {line!r}"
    removed = surface_text(rel, text).replace(form(rel, line), " ")
    assert form(rel, line) not in removed, f"{rel}: removing the line must fail the check"


@pytest.mark.parametrize("rel,needle", WIRING, ids=lambda v: str(v)[:40])
def test_td_word_05_card_020_the_business_statement_is_wired_to_each_surface(rel: str, needle: str) -> None:
    assert needle in source(rel), f"{rel}: missing {needle!r}"


def top_line(text: str) -> str:
    """The quote page's line right after its `<h1>` (WORD-002's "top of the quote page")."""
    lines = scan.html_text(text).splitlines()
    return next(lines[i + 1] for i, line in enumerate(lines) if line.startswith("Your quotes for request"))


def test_td_word_05_word_002_the_quote_page_opens_with_the_long_statement() -> None:
    text = source(PORTALS + "quote_page.html")
    assert top_line(text) == LONG_EN
    statement = next(line for line in text.splitlines() if 'data-fixed="WORD-002"' in line)
    moved = text.replace(statement + "\n", "").replace("{% endblock %}", statement + "\n{% endblock %}")
    assert top_line(moved) != LONG_EN


@pytest.mark.parametrize("key", ["consent", "consent_safety_call"])
def test_td_word_05_the_consent_text_carries_the_short_form_in_both_languages(key: str) -> None:
    for lang, short in (("en", SHORT_EN), ("es", SHORT_ES)):
        entry = json.loads(source(f"app/messaging/templates/{lang}.json"))[key]
        filled = flat(scan.brand.fill(entry["text"], lang == "es", scan.DATA))
        assert short in filled and short not in filled.replace(short, " ")
        assert lang == "en" or entry.get("draft") is True, f"{key}: the Spanish text is marked for review"


def test_td_word_02_the_insurance_lines_are_word_006s() -> None:
    lines = json.loads(source("app/licensing/templates/license_lines.json"))["en"]
    expected = scan.spec_quotes("wording.md", "WORD-006")
    assert [lines["insurance_yes"], lines["insurance_no"]] == expected


def licensed_trades() -> list[dict[str, object]]:
    """WEB-015: every licensed category of the catalog, with its `plural` (T521, D-3879)."""
    data = yaml.safe_load(source("docs/catalog/categories.yaml"))
    return [c for c in data["categories"] if c["license"]["kind"] != "none"]


def opening_holds(template: str, plural: str) -> bool:
    lines = scan.html_text(template.replace("{{ c.plural }}", plural)).splitlines()
    opening = f"Get quotes from licensed {plural}."
    return opening in lines and lines[lines.index(opening) + 1] == LONG_EN


@pytest.mark.parametrize("trade", licensed_trades(), ids=lambda c: str(c["id"]))
def test_td_word_05_web_015_each_licensed_trade_opens_with_its_plural(trade: dict[str, object]) -> None:
    template = source(SITE + "services.html")
    plural = str(trade.get("plural") or "")
    assert plural and opening_holds(template, plural)
    assert not opening_holds(template.replace("{{ brand }} is not a contractor", "We"), plural)


def compiled(*patterns: str) -> tuple[re.Pattern[str], ...]:
    return tuple(re.compile(p, I) for p in patterns)


def without(text: str, list_a: tuple[str, ...]) -> str:
    """List A is removed word for word before its rule's List B (D-4258 (0))."""
    for phrase in list_a:
        text = re.sub(re.escape(phrase), " ", text, flags=I)
    return text


def files() -> list[str]:
    return scan.scanned_files()


def text_of(rel: str) -> str:
    return scan.reader(rel)[1]


# (1) WEB-010: pest control never advertised; no negation allowance.
PEST_A = (
    "Pest control license", "My license includes the wood-destroying organism (termite) category",
    "My license includes the weed control category", "never names pest control on the home page",
    "for a home inspection, or a termite report, for a home sale, purchase or refinance",
    "Licencia de control de plagas",
    "Mi licencia incluye la categoría de organismos que destruyen la madera (termitas)",
)
PEST = compiled(
    r"\bpests?\b", r"\bexterminat\w*", r"\btermites?\b", r"\bfumigat\w*", r"\bscorpions?\b", r"\binsects?\b",
    r"\bbugs?\b", r"\brodents?\b", r"\b(?:mice|rats?|roach(?:es)?|cockroach(?:es)?)\b", r"\bweed control\b",
    r"\bpre-?emergents?\b", r"\bplagas?\b", r"\bexterminad\w*", r"\bfumigaci\w*", r"\btermitas?\b",
    r"\balacran(?:es)?\b", r"\bescorpi[oó]n(?:es)?\b", r"\binsectos?\b", r"\broedores?\b", r"\bcucarachas?\b",
    r"\bratas?\b", r"\bratones?\b", r"\bcontrol de malezas\b", r"\bpre-?emergentes?\b",
)
PEST_PAGE = re.compile(r"pest|plaga|termit", I)
PEST_LINK = re.compile(r"\b(?:href|action|src)\s*=\s*[\"'][^\"']*(?:pest|plaga|termit|exterminat)", I)
PEST_FAIL = (
    "Pest control season is here.", "Book pest control today.", "Get rid of pests.", "Call an exterminator.",
    "Termite inspections this spring.", "Fumigation for your home.", "Scorpion season is coming.",
    "Keep insects out.", "No more bugs.", "Rodent removal.", "Mice in the garage?", "Roaches in the kitchen?",
    "Weed control for your yard.", "Pre-emergent treatments now.", "Control de plagas en Havasu.",
    "Exterminador local.", "Fumigación para su casa.", "Inspección de termitas.", "Temporada de alacranes.",
    "Temporada de escorpiones.", "Insectos fuera.", "Control de roedores.", "Cucarachas en la cocina.",
    "Ratas en el garaje.", "Ratones en la casa.", "Control de malezas.", "Tratamiento pre-emergente.",
)
PEST_PASS = (
    *PEST_A,
    "No introduction fee for a home inspection, or a termite report, for a home sale, purchase or refinance.",
)


def pest_hits(text: str, patterns: tuple[re.Pattern[str], ...] = PEST) -> list[str]:
    """WEB-010: List B on the text with List A removed."""
    text = without(text, PEST_A)
    return [m.group(0) for p in patterns for m in p.finditer(text)]


def pest_pages(names: list[str], sources: list[str]) -> list[str]:
    """WEB-010: a template named for pests, or a link, form or source pointing at a pest page."""
    pages = [n for n in names if PEST_PAGE.search(Path(n).name)]
    return pages + [m.group(0) for s in sources for m in PEST_LINK.finditer(s)]


@pytest.mark.parametrize("sentence", PEST_FAIL)
def test_td_web_03_web_010_a_pest_line_fails(sentence: str) -> None:
    assert pest_hits(sentence)


@pytest.mark.parametrize("sentence", PEST_PASS)
def test_td_web_03_web_010_list_a_passes(sentence: str) -> None:
    assert pest_hits(sentence) == []


def test_td_web_03_web_010_a_pest_page_or_link_fails() -> None:
    assert pest_pages(["app/site/templates/pest_control.html"], [])
    assert pest_pages([], ['<a href="/pest-control">Our services</a>'])
    templates = sorted(str(p) for p in ROOT.glob("app/*/templates/**/*") if p.is_file())
    assert pest_pages(templates, [Path(t).read_text(encoding="utf-8") for t in templates if t.endswith(".html")]) == []


def test_td_web_03_web_010_every_pest_pattern_has_a_planted_line() -> None:
    for pattern in PEST:
        assert any(pattern.search(s) for s in PEST_FAIL), pattern.pattern


# (2) WEB-021, D-183: a sentence naming the owner never names the owner's employer or occupation.
OWNER = re.compile(r"\bowner(?:'s)?\b|\bdue[ñn][oa]\b|\bfounder\b|\bfundador(?:a)?\b", I)
OCCUPATION = compiled(
    r"\bemploy(?:er|ers|ed|ee|ees|ment|s)?\b", r"\boccupation\w*", r"\bocupaci[oó]n\b", r"\bprofession\w*",
    r"\bprofesi[oó]n\b", r"\bcareers?\b", r"\bcarrera\b", r"\bjob\s+(?:at|with|for|as|in)\b",
    r"\bworks?\s+(?:at|for|as|in)\b", r"\bworked\b", r"\bworking\s+(?:at|for|as|in)\b", r"\bby day\b",
    r"\btrabaj\w*\s+(?:en|para|como|con)\b", r"\bemple(?:o|os|ad[oa]s?|ador(?:a|es)?)\b",
    r"\b(?:owner|due[ñn][oa])\s+(?:is|was|es|era)\s+(?:an?|un|una)\b",
)
OWNER_FAIL = (
    "The owner works at the hospital.", "The owner is a firefighter.", "The owner's employer is the city.",
    "Its owner is employed full time.", "The owner's occupation is nursing.", "The owner has a job with the county.",
    "The owner worked in construction.", "The owner's profession is teaching.", "The owner's career is in law.",
    "The owner is working as a teacher.", "By day, the owner teaches.", "El dueño trabaja en el hospital.",
    "El dueño es un bombero.", "El empleador del dueño es la ciudad.", "La ocupación del dueño.",
    "La profesión del dueño.", "La carrera del dueño.",
)
OWNER_PASS = (
    "The owner has a full-time day job.", "El dueño tiene un trabajo de tiempo completo.",
    f"The {NAME} owner has a full-time day job; help and charge questions are usually answered within 2 days.",
    "Its owner lives in Havasu.", f"{NAME}'s owner never uses {NAME}'s data for any other work or business.",
    f"El dueño de {NAME} nunca usa los datos de {NAME} para ningún otro trabajo o negocio.",
    f"This section is a draft for the {NAME} owner's decision.", f"The {NAME} owner is away until Friday.",
)


def owner_hits(text: str, patterns: tuple[re.Pattern[str], ...] = OCCUPATION) -> list[str]:
    """WEB-021: each sentence holding an owner word and an employer or occupation pattern."""
    sentences = [s for s in re.split(r"[.!?;:\n]", text) if OWNER.search(s)]
    return [s.strip() for s in sentences if any(p.search(s) for p in patterns)]


@pytest.mark.parametrize("sentence", OWNER_FAIL)
def test_td_web_03_web_021_the_owners_occupation_fails(sentence: str) -> None:
    assert owner_hits(sentence)


@pytest.mark.parametrize("sentence", OWNER_PASS)
def test_td_web_03_web_021_the_day_job_lines_pass(sentence: str) -> None:
    assert owner_hits(sentence) == []


def test_td_web_03_web_021_every_occupation_pattern_has_a_planted_line() -> None:
    for pattern in OCCUPATION:
        assert any(pattern.search(s) and OWNER.search(s) for s in OWNER_FAIL), pattern.pattern


# (3) LIC-042, TERMS-004: no warranty or guarantee by Laftly; allowed only when negated.
WARRANTY_A = ("Nothing in these terms is a home warranty, protection plan or service contract.",)
WARRANTY = compiled(
    r"\bwarrant(?:y|ies|ied|s|ing)\b", r"\bguarant(?:ee|ees|eed|eeing|y|ies)\b", r"\bgarant[ií]\w*",
    r"\bprotection plans?\b", r"\bservice contracts?\b", r"\bservice plans?\b", r"\bextended (?:coverage|protection)\b",
    r"\bsatisfaction\b", r"\bplan(?:es)? de (?:protecci[oó]n|servicio)\b", r"\bcontratos? de servicio\b",
    r"\bsatisfacci[oó]n\b", r"\bdevoluci[oó]n de (?:su )?dinero\b",
)
WARRANTY_FAIL = (
    f"{NAME} gives a warranty on every job.", "All work is guaranteed.", "Satisfaction guaranteed.",
    f"{NAME} warrants the work.", "Get a home protection plan.", "Buy a service contract.", "Ask about service plans.",
    "Extended coverage available.", "Garantía en cada trabajo.", "Trabajo garantizado.",
    "Plan de protección para su casa.", "Contrato de servicio.", "Satisfacción total.", "Devolución de su dinero.",
    f"{NAME} is not a contractor, but we guarantee the work.",
)
WARRANTY_PASS = (
    f"{NAME} gives no warranty of any kind.", "Examples are a subpoena or a warrant.",
    "Only what a law requires, such as a subpoena or warrant",
    f"{NAME} never directs, controls, schedules, supervises, inspects, warrants or pays for your work.",
    f"{NAME} nunca dirige, controla, programa, supervisa, inspecciona, garantiza ni paga su trabajo.",
    *WARRANTY_A, f"{NAME} doesn't guarantee the work.", "A minus sign means money back to you.",
)


def warranty_hits(text: str, patterns: tuple[re.Pattern[str], ...] = WARRANTY) -> list[str]:
    """LIC-042, TERMS-004: List B on the text with List A removed, unless negated."""
    text = without(text, WARRANTY_A)
    return [m.group(0) for p in patterns for m in p.finditer(text) if not scan.negated(text, m.start())]


@pytest.mark.parametrize("sentence", WARRANTY_FAIL)
def test_td_lic_18_a_warranty_by_the_service_fails(sentence: str) -> None:
    assert warranty_hits(sentence)


@pytest.mark.parametrize("sentence", WARRANTY_PASS)
def test_td_lic_18_a_negated_warranty_passes(sentence: str) -> None:
    assert warranty_hits(sentence) == []


def test_td_lic_18_every_warranty_pattern_has_a_planted_line() -> None:
    for pattern in WARRANTY:
        assert any(pattern.search(s) for s in WARRANTY_FAIL), pattern.pattern


# WORD-021: no personal name (the legal-docs check's list, imported) and the old name never alone.
NAMES = tuple(p for p, why in check_legal_docs.BANNED if why.startswith("personal name"))
ALONE = tuple(p for p, _ in check_legal_docs.alone_patterns(scan.DATA))
HAVA_PAIR = (HAVA_Q, f"{HAVA_A}\"{OLD}\"{HAVA_END}")


def name_hits(text: str) -> list[str]:
    """WORD-021: names and the old name alone, the WEB-025 question and answer and status notes aside."""
    text = "\n".join(line for line in text.splitlines() if not line.startswith("**Status:"))
    text = without(text, HAVA_PAIR)
    return [m.group(0) for p in NAMES + ALONE for m in p.finditer(text)]


def test_td_word_04_a_planted_name_or_the_old_name_alone_fails() -> None:
    planted = re.sub(r"\\b", "", NAMES[0].pattern)
    page = f"{{# {planted} keeps this #}}<p>Ask {planted} about it.</p>"
    assert name_hits(scan.html_text(page)) == [planted]
    assert name_hits(f"Ask {OLD} about it.") == [OLD]


@pytest.mark.parametrize(
    "line", [scan.DATA["domain"], "Havasu", f"the {NAME} owner", f"el dueño de {NAME}", *HAVA_PAIR]
)
def test_td_word_04_the_brand_the_town_and_the_web_025_answer_pass(line: str) -> None:
    assert name_hits(line) == []


@pytest.mark.parametrize("rel", files())
def test_td_web_03_td_lic_18_td_word_04_scanned_files_pass_every_statements_scan(rel: str) -> None:
    """WEB-010, WEB-021, LIC-042, WORD-021 on every scanned file as it stands."""
    text = text_of(rel)
    assert pest_hits(text) == [], rel
    assert owner_hits(text) == [], rel
    assert warranty_hits(text) == [], rel
    assert name_hits(text) == [], rel

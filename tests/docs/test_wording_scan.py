"""The wording scan: every template, page, text and drafted document reads true (T104).

TD-WORD-01, TD-WORD-06, TD-WEB-05, TD-WORD-03, TD-WEB-04, TD-WORD-07 (TERMS-010, TERMS-012, WORD-001,
WORD-003, WORD-004, WORD-010, WORD-011, WORD-012, WORD-015, WORD-019, WORD-020, WORD-022; D-029, D-245,
D-300, D-360, D-377, D-3155, D-3883, D-4119, D-4229).

The rules are D-4229's closed lists, the same definition for worker and reviewer:
1. Reader text. `.html`: comments, `<script>` and `<style>` are deleted, a `{{ }}` naming `brand`
   becomes the brand file's legal name, domain or display name, every other `{{ }}` and `{% %}` is
   deleted, block tags become line breaks and inline tags spaces, and the `alt`, `title`,
   `placeholder`, `aria-label`, `content` and `value` attributes are kept. `en.json` and `es.json`:
   each entry's `text`, one line each, brand tokens filled. Markdown as written; in `docs/legal/`
   the word lists read what `check_legal_docs.check_phrases` leaves (D-3883).
2. List A, true wording, is removed word for word before the word lists.
3. List B is never allowed; it imports `trace_spec.NOT_A_CONTRACTOR` and `trace_spec.SUPERSEDED`,
   and `check_legal_docs.BUSINESS_BANNED` for a business file, never copying them.
4. List C, rankings, is allowed only when negated in the same sentence.
5. List S, TERMS-010: D-4119's grammar (`test_privacy_policy.sharing_claims`, imported) per line,
   the "text ... to" rule and the Spanish clause rule; a portal page's own "shared inbox" is
   removed first.
6. WORD-015, the Stripe description, and WORD-012, cost comparisons only with a cited source.
Phrasing off these lists is a scan list note, logged as "wording scan list: add <pattern>".
The brand is read from the brand file, never a literal. Fake data only.

Run: python3 -m pytest tests/docs/test_wording_scan.py -q -p no:cacheprovider
"""

from __future__ import annotations

import functools
import html
import json
import re
import sys
from pathlib import Path

import pytest

import brand
import check_legal_docs
import trace_spec

sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_privacy_policy  # noqa: E402  D-4119's sharing grammar, imported, never copied (D-4229)

ROOT = Path(__file__).resolve().parents[2]
I = re.IGNORECASE
DATA = brand.load(brand.BRAND_FILE)
NAME = DATA["display_name"]
B = re.escape(NAME)
PORTALS = "app/portals/templates/"


def spec_quotes(spec: str, req_id: str) -> list[str]:
    """The double-quoted strings of one requirement, before its "Source:" (docs/spec/)."""
    for line in (ROOT / "docs" / "spec" / spec).read_text(encoding="utf-8").splitlines():
        if line.startswith(f"- **{req_id}** "):
            return re.findall(r'"([^"]+)"', line.split(" Source: ")[0])
    raise AssertionError(f"{req_id} not found in docs/spec/{spec}")


TERMS_009 = spec_quotes("terms.md", "TERMS-009")[0]
WORD_014 = spec_quotes("wording.md", "WORD-014")[0]
WORD_015 = spec_quotes("wording.md", "WORD-015")[0]
WORD_002 = spec_quotes("wording.md", "WORD-002")
WORD_018 = spec_quotes("wording.md", "WORD-018")

# (2) List A: true wording, removed word for word before the word lists.
LIST_A = (
    TERMS_009, WORD_014, "#1 means first received, not first choice.", "Which fits best?",
    "Arizona Corporation Commission", "never a percentage or share of", "never a share of",
    "not a share of", "lead paint", "lead pipe", "lead pipes", "lead-based",
)


def compiled(*patterns: str) -> tuple[re.Pattern[str], ...]:
    return tuple(re.compile(p.replace("<brand>", B), I) for p in patterns)


# (3) List B: never allowed.
CLAIMS = compiled(  # WORD-003, WORD-020: vetted, best, top
    r"\b(?:pre-?)?vet(?:s|ted|ting)?\b",
    r"\bbackground[- ]check\w*",
    r"\b(?:verified|screened|approved|certified|trusted|recommended|preferred|hand-?picked)\s+(?:local\s+)?"
    r"(?:business(?:es)?|pros?|professionals?|contractors?|plumbers?|electricians?)\b",
    r"\b(?:negocios?|empresas?|contratistas?|profesionales)\s+(?:\w+\s+){0,2}?"
    r"(?:verificad|investigad|aprobad|certificad|recomendad|confiable)\w*",
    r"\bbest\b", r"\bmejor(?:es)?\b", r"#1\b", r"\bn[uú]mero (?:1|uno)\b", r"\b(?:five|5)[- ]stars?\b",
    r"\btop[- ](?:rated|ranked|choice|picks?|business(?:es)?|pros?|contractors?|quotes?)\b",
)
ORDER = compiled(  # WORD-003: order by merit
    r"\bsort(?:s|ed|ing)?\s+(?:\w+\s+){0,3}?by\b",
    r"\b(?:list(?:s|ed)?|show[sn]?|order(?:s|ed)?|display(?:s|ed)?|arranged?)\s+(?:\w+\s+){0,3}?by\s+"
    r"(?:price|cost|quality|ratings?|reviews?|score)\b",
    r"\b(?:cheapest|lowest|highest)\b[^.!?;\n]*\bfirst\b",
    r"\b(?:shown|listed|displayed|appears?|goes?|comes?)\s+first\b",
    r"\b(?:orden|muestr|mostr|list|present|aparec)\w*\s+(?:\w+\s+){0,3}?por\s+"
    r"(?:precio|calidad|calificaci|rese|puntuaci)\w*",
    r"\b(?:barat[oa]s?|bajos?|bajas?|econ[óo]mic[oa]s?)\s+(?:\w+\s+){0,3}?primer[oa]s?\b",
)
FEE = compiled(  # WORD-004
    r"\bcommission\w*", r"\bcomisi[oó]n(?:es)?\b", r"\bshares?\s+of\b", r"\b(?:revenue|fee|profit)[- ]shar\w+",
    r"\b(?:our|its|<brand>'s)\s+(?:\w+\s+)?(?:share|cut)\b",
    r"\b(?:takes?|took|taking|gets?|got|getting|earns?|earned|earning|keeps?|kept|keeping)\s+(?:\w+\s+){0,3}?"
    r"(?:cut|share|slice|percentage)\b",
    r"\b(?:a|the|small|\d+\s?%)\s+(?:\w+\s+)?cut\s+(?:of|from)\b",
)
LEADS = compiled(  # WORD-020
    r"\bleads?\b", r"\bexclusive\b", r"\bexclusiv[oa]s?\b", r"\baffiliat\w*", r"\bafiliad\w*", r"\bpartners?\b",
    r"\bsocios?\b", r"\bclientes? potencial(?:es)?\b", r"\bprospectos?\b",
)
OTHER = compiled(
    r"\bconcierge\b", r"\bread their reviews\b", r"\ba new <brand> feature\b",  # WORD-019
    r"\blead[- ]generat\w*", r"\bmarketplace of providers\b", r"\bconnects? (?:you|homeowners) (?:with|to)\b",
    r"\breferral (?:network|service)s?\b",  # TERMS-012, WORD-023
)
JUDGING = compiled(  # WORD-010
    r"\b(?:good|skilled|experienced|reliable|professional|qualified) enough\b",
    r"\b(?:poor|low|bad|shoddy|substandard)[- ](?:quality|work|workmanship|reviews?|ratings?)\b",
    r"\b(?:did not|didn't|does not|doesn't|failed to) (?:meet|pass) (?:our|<brand>'s) "
    r"(?:standards?|review|check|screening|quality bar)\b",
    r"\bfailed (?:our|<brand>'s) (?:standards?|review|check|screening)\b",
)
PRICING = compiled(  # WORD-011
    r"\b(?:we|<brand>)(?:'ll| will| can| may)? (?:price|bid on|negotiate|haggle|round|convert|adjust|edit)s?\b",
    r"\b(?:we|<brand>)(?:'ll| will| can| may)? (?:suggest|recommend|set)s? (?:a|the|your) (?:price|quote|amount)\b",
)
NAMES = compiled(  # WORD-001
    r"\bask ?hava\b", r"\bhavasu home quotes\b",
    r"<brand>,? (?:Inc|Corp|Co|Ltd|LLP|PLLC|L\.L\.C|Incorporated|Corporation|Company|Limited)\b",
)
OWN_B = CLAIMS + ORDER + FEE + LEADS + OTHER + JUDGING + PRICING + NAMES
IMPORTED_B = tuple(p for p, _ in trace_spec.NOT_A_CONTRACTOR + trace_spec.SUPERSEDED)
BUSINESS_B = tuple(p for p, _ in check_legal_docs.BUSINESS_BANNED)

# (4) List C: rankings, allowed only when negated.
LIST_C = compiled(
    r"\brank\w*", r"\brated\b", r"\bratings?\b",
    r"\brat(?:e|es|ing)\s+(?:\w+\s+){0,2}?(?:business(?:es)?|contractors?|pros?|quotes?|anyone|them)\b",
    r"\bclasific\w*\s+(?:\w+\s+){0,3}?(?:negocios?|empresas?|contratistas?|cotizaci\w+)", r"\bcalific\w*",
)
NEGATORS = frozenset("not never no nor neither without cannot nunca ni sin jamás".split())
CONJUNCTIONS = frozenset("and but so yet y pero".split())
LIMITERS = frozenset("but only just pero sino solo sólo".split())
WORD = re.compile(r"[\w']+")

# (6) List S, TERMS-010.
MARK_SHARED = re.compile(r"\bmark(?: it| my inbox)(?: not)? shared\b", I)
INBOX_SHARED = re.compile(r"\binbox (?:is|isn't)(?: not)?(?: marked)? shared\b|\bshared inbox\b", I)
OBJECT = test_privacy_policy.OBJECT
TEXT_TO = re.compile(
    r"\btext(?:s|ed|ing)?\s+(?:[\w'-]+\s+){0,3}?(?:numbers?|phones?)\s+to\s+(?:[\w'-]+\s+){0,2}?"
    + test_privacy_policy.THIRD_PARTY.pattern,
    I,
)
SENTENCE = re.compile(r"(?<=[.!?])\s+")
ES_OBJECT = re.compile(
    r"\b(?:su|sus|tu|tus)\s+(?:n[uú]meros?|tel[eé]fonos?|celular(?:es)?|m[oó]vil(?:es)?)\b"
    r"|\bn[uú]meros? de (?:tel[eé]fono|celular|m[oó]vil)\b|\bconsentimiento\b"
    r"|\bpermiso para (?:enviar(?:le)? )?(?:mensajes|textos)\b",
    I,
)
ES_CLAUSE = re.compile(r"[;:()]|,\s*(?=(?:y|pero|aunque)\b)|\b(?:pero|aunque|mientras)\b", I)
ES_NEGATION = re.compile(r"\b(?:no|nunca|jam[aá]s|ni|ning[uú]n|ninguna|ninguno|nadie|sin|tampoco)\b", I)
ES_LIFT = re.compile(r"\b(?:hasta|antes|salvo|excepto|a menos que|a no ser que|sino)\b", I)
ES_VERB = re.compile(r"\b(?:compart|vend|alquil|transfer|transfier|revel|divulg)\w*|\bced(?:e|emos|en|er|ido|erá)\b", I)
ES_PARTY = re.compile(
    r"\b(?:negocios?|empresas?|contratistas?|profesional(?:es)?|terceros?|socios?|afiliad\w*|anunciantes?)\b", I
)
ES_PARTY_VERB = re.compile(
    r"\b(?:da|damos|dan|dar|dará|daremos|darán|dio|dimos|dado|entreg\w+|pas(?:a|amos|an|ar|ará|aremos|ó|ado)"
    r"|env[ií]\w+|mostr\w+|muestr\w+|ve|ven|ver|verá|verán|recib\w+|tien\w+|tendr\w+|obt\w+|va|van|irá)\b",
    I,
)

# (7) WORD-015 and WORD-012.
STRIPE = re.compile(r"stripe", I)
DESCRIPTION = re.compile(r"\bdescri\w+", I)
SOURCE = re.compile(r"\b(?:Source|Fuente):\s*\S", I)
COMPETITOR = re.compile(
    r"\b(?:Angi|Angie's List|HomeAdvisor|Thumbtack|Houzz|Porch|Networx|Nextdoor|TaskRabbit|Yelp"
    r"|Google Local Services|other (?:sites?|services?|platforms?|apps?|companies)|competitors?"
    r"|otr[oa]s (?:sitios|servicios|plataformas|empresas))\b",
    I,
)
COST = re.compile(r"\$\s?\d|\d\s?%|\bpercent\b|\bpor ?ciento\b", I)
LIST_C_END = re.compile(r"[.!?;:\n]")

# (1) Reader text of a template.
HIDDEN = re.compile(r"\{#.*?#\}|<!--.*?-->|<(script|style)\b.*?</\1\s*>", re.S | I)
EXPRESSION = re.compile(r"\{\{(.*?)\}\}", re.S)
STATEMENT = re.compile(r"\{%.*?%\}", re.S)
TAG = re.compile(r"<(/?)(!?[A-Za-z][\w-]*)((?:\"[^\"]*\"|'[^']*'|[^'\">])*)>")
KEPT = re.compile(
    r"(?<![\w-])(?:alt|title|placeholder|aria-label|content|value)\s*=\s*(?:\"([^\"]*)\"|'([^']*)')", I
)
INLINE = frozenset("a abbr b cite code em i mark small span strong sub sup u".split())


def brand_value(match: re.Match[str]) -> str:
    """A `{{ }}` naming the brand reads as the brand file's value; any other is deleted."""
    expr = match.group(1)
    if not re.search(r"\bbrand\b", expr):
        return ""
    if "legal_name" in expr:
        return DATA["legal_name"]
    return DATA["domain"] if "domain" in expr else NAME


def tag_text(match: re.Match[str]) -> str:
    """A tag becomes a line break (a space if inline) plus its kept attribute values, one per line."""
    kept = [a or b for a, b in KEPT.findall(match.group(3))]
    if kept:
        return "\n" + "\n".join(kept) + "\n"
    return " " if match.group(2).lower() in INLINE else "\n"


def lines_of(text: str) -> str:
    """One space for each whitespace run, straight apostrophes, no empty lines."""
    text = text.replace("’", "'").replace("‘", "'")
    return "\n".join(" ".join(line.split()) for line in text.splitlines() if line.strip())


def html_text(source: str) -> str:
    """What a reader sees on a page (rule 1)."""
    text = HIDDEN.sub(" ", source)
    text = EXPRESSION.sub(brand_value, text)
    text = STATEMENT.sub(" ", text)
    text = text.replace("\r", " ").replace("\n", " ")
    return lines_of(html.unescape(TAG.sub(tag_text, text)))


def json_text(source: str, spanish: bool) -> str:
    """Each message's text, one line each, brand tokens filled (rule 1)."""
    entries = json.loads(source).values()
    return lines_of("\n".join(" ".join(brand.fill(e["text"], spanish, DATA).split()) for e in entries))


@functools.cache
def specs() -> dict[str, str]:
    return check_legal_docs.spec_texts(ROOT)


def reader(rel: str) -> tuple[str, str]:
    """(text the word lists read, text List S, WORD-015 and WORD-012 read) of one scanned file."""
    source = (ROOT / rel).read_text(encoding="utf-8")
    if rel.endswith(".html"):
        text = html_text(source)
        return text, text
    if rel.endswith(".json"):
        text = json_text(source, rel.endswith("es.json"))
        return text, text
    raw = lines_of(source)
    if rel.startswith("docs/legal/"):
        body = check_legal_docs.normalize(source)
        return lines_of(check_legal_docs.check_phrases(Path(rel).name, body, specs(), DATA)[1]), raw
    return raw, raw


def scanned_files() -> list[str]:
    """D-3883 (2): templates, legal drafts but the README, and two copy documents."""
    found = [
        p.relative_to(ROOT).as_posix()
        for p in sorted((ROOT / "app").glob("*/templates/**/*"))
        if p.is_file() and (p.suffix == ".html" or p.name in ("en.json", "es.json"))
    ]
    found += [f"docs/legal/{p.name}" for p in sorted((ROOT / "docs" / "legal").glob("*.md")) if p.name != "README.md"]
    return found + ["docs/copy/site-copy.md", "docs/copy/mail-app-steps.md"]


def negated(text: str, start: int) -> bool:
    """List C (4): a negation earlier in the sentence, with no conjunction or limiter after it."""
    begin = max(text.rfind(c, 0, start) for c in ".!?;:\n") + 1
    tokens = [t.lower() for t in WORD.findall(text[begin:start])]
    marks = [i for i, t in enumerate(tokens) if t in NEGATORS or t.endswith("n't")]
    return bool(marks) and not set(tokens[marks[-1] + 1 :]) & (CONJUNCTIONS | LIMITERS)


def is_business(rel: str) -> bool:
    name = Path(rel).name
    return name in check_legal_docs.BUSINESS_DOCUMENTS or "business" in name


def word_findings(rel: str, text: str) -> list[str]:
    """WORD-001, WORD-003, WORD-004, WORD-010, WORD-011, WORD-019, WORD-020, WORD-022, TERMS-012, TERMS-027:
    Lists B and C on the text with List A removed."""
    for phrase in LIST_A:
        text = re.sub(re.escape(phrase), " ", text, flags=I)
    found = [
        f"{rel}: {m.group(0)!r} ({p.pattern})"
        for p in OWN_B + IMPORTED_B + (BUSINESS_B if is_business(rel) else ())
        for m in p.finditer(text)
    ]
    return found + [
        f"{rel}: ranking {m.group(0)!r} ({p.pattern})"
        for p in LIST_C
        for m in p.finditer(text)
        if not negated(text, m.start())
    ]


def portal_line(line: str) -> str:
    """A portal page's own shared inbox is not a sharing claim (`business_home.html`)."""
    out = []
    for sentence in SENTENCE.split(line):
        sentence = MARK_SHARED.sub(" ", sentence)
        out.append(sentence if OBJECT.search(sentence) else INBOX_SHARED.sub(" ", sentence))
    return " ".join(out)


def spanish_claim(sentence: str) -> bool:
    """TERMS-010 (c): a Spanish clause says a number or consent goes to someone."""
    if not ES_OBJECT.search(sentence):
        return False
    for clause in ES_CLAUSE.split(sentence):
        if not clause or (ES_NEGATION.search(clause) and not ES_LIFT.search(clause)):
            continue
        if ES_VERB.search(clause) or (ES_PARTY.search(clause) and ES_PARTY_VERB.search(clause)):
            return True
    return False


def sharing_findings(rel: str, text: str) -> list[str]:
    """TERMS-010 (List S): no line says a phone number, opt-in or consent is shared, sold or passed on."""
    found = []
    for line in text.splitlines():
        line = portal_line(line) if rel.startswith(PORTALS) else line
        found += test_privacy_policy.sharing_claims(line)
        found += [
            s
            for s in SENTENCE.split(line)
            if (OBJECT.search(s) and TEXT_TO.search(s)) or spanish_claim(s)
        ]
    return [f"{rel}: sharing claim {s!r}" for s in found]


def stripe_findings(rel: str, text: str) -> list[str]:
    """WORD-015: a line describing the Stripe account holds the exact description."""
    return [
        f"{rel}: Stripe description not WORD-015's: {line!r}"
        for line in text.splitlines()
        if STRIPE.search(line) and DESCRIPTION.search(line) and WORD_015 not in line
    ]


def cost_findings(rel: str, text: str) -> list[str]:
    """WORD-012: a cost comparison with another service only in a file citing its source."""
    if SOURCE.search(text):
        return []
    return [
        f"{rel}: uncited cost comparison {s.strip()!r}"
        for s in LIST_C_END.split(text)
        if COMPETITOR.search(s) and COST.search(s)
    ]


def findings(rel: str, words: str, raw: str) -> list[str]:
    """Every rule on one file: the word lists on words, the others on raw (rule 7)."""
    return (
        word_findings(rel, words)
        + sharing_findings(rel, raw)
        + stripe_findings(rel, raw)
        + cost_findings(rel, raw)
    )


def scan(sentence: str, rel: str = "docs/copy/planted.md") -> list[str]:
    text = lines_of(brand.fill(sentence, data=DATA))
    return findings(rel, text, text)


# (5) Lists B and C: must fail. "{brand}" is filled from the brand file.
WORD_FAILS = (
    "Every business is vetted.", "We vet every business.", "Pre-vetted local pros.",
    "Background-checked businesses.", "Trusted local pros.", "Negocios verificados cerca de usted.",
    "The best plumbers in town.", "Los mejores negocios de la ciudad.", "We're #1 in Havasu.",
    "Somos el número uno.", "Five-star businesses.", "{brand} is the top choice.", "Top-rated businesses.",
    "We rank businesses by reviews.", "Quotes are ranked.", "We don't just rank businesses, we rank them well.",
    "{brand} is not a contractor, and we rank every quote.", "{brand} is not a contractor and we rank every quote.",
    "{brand} no es contratista y clasifica a los negocios.", "We rate every business.",
    "Highly rated local businesses.", "See each business's rating.", "Clasificamos a los negocios.",
    "Negocios calificados.", "Quotes are sorted by price, lowest first.", "The cheapest quote is shown first.",
    "Quotes are listed by price.", "Quotes are shown by price.", "Las cotizaciones se muestran por precio.",
    "Mostramos las cotizaciones por precio.", "Ordenamos las cotizaciones por precio.",
    "Las más baratas aparecen primero.", "Los precios más bajos van primero.", "We charge a commission.",
    "Cobramos una comisión.", "We take a small share of each job.", "Our share is 10% of the job.",
    "We earn a cut on every job.", "We take a cut.", "We get a cut of every job.", "A 10% cut of the job.",
    "Revenue share with businesses.", "Buy leads here.", "Exclusive requests.", "Solicitudes exclusivas.",
    "Our partner businesses.", "An affiliate of {brand}.", "Un afiliado de {brand}.", "Nuestros socios.",
    "Clientes potenciales para su negocio.", "Nuevos prospectos cada semana.", "Your home concierge.",
    "Read their reviews.", "A new {brand} feature.", "Lead generation for contractors.",
    "A marketplace of providers.", "We connect you with local pros.", "{brand} connects homeowners with businesses.",
    "A referral network.", "A referral service.", "This business isn't good enough for this job.",
    "This business did not meet our standards.", "This business failed our review.",
    "Poor-quality work is not allowed.", "We negotiate the price for you.", "{brand} will suggest a price.",
    "We round the quote.", "Ask Hava sends your request.", "AskHava sends your request.",
    "Havasu Home Quotes sends your request.", "{brand} Inc. runs the service.", "{brand}, Inc. runs the service.",
    "The fee is $45.", "Recurring jobs count as 2 months.",
)
# TD-WORD-06's thirteen phrases, then one sentence for each other NOT_A_CONTRACTOR pattern (WORD-022).
NOT_A_CONTRACTOR_FAILS = (
    "Meet our pros.", "We'll send a plumber.", "Booked until Friday.", "Hire through us.",
    "Pay through {brand}.", "A pro is assigned to you.", "A service provider will call.", "We found you a pro.",
    "We subcontract the job.", "Work is guaranteed.", "See our price.", "Nuestros profesionales.",
    "Le enviaremos un plomero.",
    "Call your contractor.", "We'll schedule a visit.", "We dispatch a plumber.", "We match you with a pro.",
    "Our provider partners.", "Join our network.", "We guarantee the work.", "The {brand} guarantee.",
    "Pros work for us.", "Hire us today.", "Subcontratamos el trabajo.", "We connect you with pros.",
    "A referral network of pros.",
)
BUSINESS_FAILS = (
    "A license fee for each job.", "The licence covers each job.", "Data access for each job.",
    "Access to homeowners in your area.", "The right to use the request.", "A purchase of contact details.",
    "The sale of information.",
)
WORD_PASSES = (
    "{brand} doesn't rank businesses or show ratings.", "The number is arrival order, not a ranking.",
    "We don't recommend, rank or rate any business.",
    "{brand} doesn't rank or rate businesses, and no business can pay for a better spot.",
    "{brand} no clasifica ni califica a los negocios.",
    "Nobody pays to be first. Quotes show in the order they came in.",
    "Every eligible quote shown, in the order received.",
    "To sort your request and help word our questions, an AI service reads it.",
    "Para clasificar su solicitud y ayudar a redactar nuestras preguntas.", "Msg & data rates may apply.",
    "The midpoint of the hourly rate times your estimated hours.",
    "Le escribiremos cuando llegue la primera cotización.", "It's not a share of the job.",
    "It's a fixed introduction fee, never a share of the job.",
    "Check it on the Arizona Corporation Commission site.",
    "Please share sensitive details on {domain} rather than by text.", "Test for lead paint before sanding.",
    "Only the business you pick gets your contact info.", "This business isn't eligible for this job type.",
    "We never change a quote's amount or words.", "Busy until Friday.", "{brand} sends your request.",
    "The business you pick.", "{brand_legal} runs the service.", "{brand_business}.",
    "Check their license and reviews, then choose.", "Tap our name at the top and tap Add to VIP.",
    "The picked quote was lower than yours.", "Recurring jobs count as 4 months of that price.",
    *LIST_A, *WORD_002,
)

# (6) List S: must fail and must pass.
PORTAL_FAILS = (
    "Your phone number is marked shared with the business you pick.",
    "Your phone number lands in the shared inbox the business reads.",
    "Your inbox is shared with the business, and so is your phone number.",
    "We have your phone number; the inbox is shared with businesses.",
)
SHARING_FAILS = PORTAL_FAILS + (
    "We share your mobile number with the businesses.", "We share your number with businesses you pick.",
    "We never share your phone number except with the business you pick.",
    "We won't share your phone number unless a business pays for it.",
    "We disclose your phone number to the business.", "We provide your phone number to the business you pick.",
    "We pass your phone number to the business.", "We hand your phone number to the pro.",
    "We forward your phone number to the contractor.", "We give your phone to the business.",
    "We share your name, address and phone number with the business you pick.",
    "We share your info, including your phone number, with the business.",
    "We share your details, including your phone number, with the business.",
    "Your phone number goes to the business you pick.", "Your number goes to the business you pick.",
    "We'll text your number to the pro.", "Phone number? We share it with the pro you pick.",
    "We keep your phone number. Then we pass it to the business.",
    "We have your phone number and give it to the business you pick.",
    "We collect your phone number and let the business see it.", "We sell opt-ins to marketers.",
    "Su número de teléfono lo recibe el negocio que elija.", "Le damos su número de teléfono al negocio que elija.",
    "Su número de teléfono se le da al negocio que elija.", "Compartimos su número con los negocios.",
    "Vendemos su consentimiento a terceros.", "No compartimos su número hasta que usted elija un negocio.",
    # The scan fails closed: these are reworded in copy, never exempted (D-4229).
    "After you give consent, we share this request with up to five businesses.",
    "Add your phone number, then we send this request to local businesses.",
    "Verify your mobile number so we can send this request to pros.",
    "We never text your number to businesses.",
)
PORTAL_INBOX = (
    "To mark it shared, first add a mobile number that gets texts. "
    "A code sent by email would land in the shared inbox, where others could see it."
)
SHARING_PASSES = (
    "Your number is never shared.", "Your first name and address go only to the business you pick.",
    *WORD_018, "Only the business you pick gets your contact info.",
    "Once you confirm your phone number, we send them a description of the job.",
    "Enter your phone number and we will send this code.", "We text your phone number a code; type it in.",
    "We'll send this to the 3 plumbing businesses.", "Su número nunca se comparte.",
    "{brand} nunca le da su número a ningún negocio.",
    "Solo el negocio que usted elija recibe su nombre y dirección.",
    "Le enviamos un código a su número de teléfono.",
)

# (7) WORD-015 and WORD-012.
STRIPE_FAILS = ("Stripe Description: A home services marketplace.", "Description shown on Stripe: Home services.")
STRIPE_PASSES = (f"Stripe description: {WORD_015}", "Card details are entered in Stripe's card form.")
COST_FAILS = (
    "Thumbtack charges $80 for the same job.", "Other sites charge $60 or more per request.",
    "Angi takes 20% of the job.", "Otros sitios cobran $60 por solicitud.",
)
CITED = "Thumbtack charges $80 for the same job.\nSource: Thumbtack price list, checked May 2026."
COST_PASSES = (CITED, "Check its reviews on Google and Yelp.", "The fee is never more than $75.")

# (1) Reader text: planted templates.
HTML_PASSES = (
    "<p>Quotes {# the best #} in order.</p>", "<p>Quotes {{ best }} in order.</p>",
    "<p>{% if best %}Quotes in order.{% endif %}</p>", '<p class="best">Quotes in order.</p>',
    '<a href="/best">Quotes in order.</a>', "<p>We never <strong>rank</strong> businesses.</p>",
    "<p>Your number is never shared.</p>\n<p>Only the business you pick gets your first name and address, "
    "after you pick it.</p>",
)
HTML_FAILS = (
    '<meta name="description" content="The best plumbers.">', '<input type="submit" value="Best quotes">',
    '<img src="/a.png" alt="Best plumbers">', "<p>Hire through {{ brand }}.</p>",
)


@pytest.mark.parametrize("rel", scanned_files())
def test_scanned_file_reads_true(rel: str) -> None:
    """TD-WORD-01, TD-WORD-06, TD-WORD-03, TD-WEB-04, TD-WEB-05, TD-WORD-07: every scanned file is clean."""
    assert findings(rel, *reader(rel)) == []


def test_scope_holds_every_kind_of_file() -> None:
    """D-3883 (2): pages, both message files, the legal drafts and the copy are all scanned."""
    files = scanned_files()
    assert "app/messaging/templates/es.json" in files and "docs/legal/privacy-policy.md" in files
    assert "docs/legal/README.md" not in files and "docs/copy/copy-panel.md" not in files
    assert all((ROOT / f).is_file() for f in files)


@pytest.mark.parametrize("sentence", WORD_FAILS + NOT_A_CONTRACTOR_FAILS)
def test_word_lists_fail(sentence: str) -> None:
    """TD-WORD-01, TD-WORD-03, TD-WORD-06, TD-WEB-04, TD-WEB-05, TD-WORD-07: List B and List C."""
    assert scan(sentence)


@pytest.mark.parametrize("sentence", BUSINESS_FAILS)
def test_business_file_fails(sentence: str) -> None:
    """TD-WORD-07, TERMS-027: BUSINESS_BANNED holds in a business file only."""
    assert scan(sentence, "docs/copy/business-planted.md")
    assert not scan(sentence)


@pytest.mark.parametrize("sentence", WORD_PASSES)
def test_word_lists_pass(sentence: str) -> None:
    """TD-WORD-01, TD-WORD-03, TD-WORD-06: true wording, List A and the WORD-002 statements pass."""
    assert scan(sentence) == []


def test_every_pattern_has_a_planted_sentence() -> None:
    """D-2506: each listed pattern is caught by a planted sentence, so removing one is seen."""
    planted = [lines_of(brand.fill(s, data=DATA)) for s in WORD_FAILS + NOT_A_CONTRACTOR_FAILS]
    missing = [p.pattern for p in OWN_B + IMPORTED_B + LIST_C if not any(p.search(s) for s in planted)]
    missing += [p.pattern for p in BUSINESS_B if not any(p.search(s) for s in BUSINESS_FAILS)]
    assert missing == []


@pytest.mark.parametrize("sentence", SHARING_FAILS)
def test_sharing_claims_fail(sentence: str) -> None:
    """TD-WORD-01, TERMS-010: List S, in any file and in a portal page."""
    assert scan(sentence)
    assert scan(sentence, PORTALS + "planted.html")


@pytest.mark.parametrize("sentence", SHARING_PASSES)
def test_sharing_truths_pass(sentence: str) -> None:
    """TD-WORD-01, TERMS-010: true sentences about numbers pass."""
    assert scan(sentence) == []


def test_portal_shared_inbox() -> None:
    """TERMS-010: a portal page's own shared inbox passes there and fails anywhere else."""
    assert scan(PORTAL_INBOX, PORTALS + "planted.html") == []
    assert scan(PORTAL_INBOX)


@pytest.mark.parametrize("sentence", STRIPE_FAILS + COST_FAILS)
def test_stripe_and_cost_fail(sentence: str) -> None:
    """TD-WORD-01, TD-WORD-07: WORD-015 and WORD-012."""
    assert scan(sentence)


@pytest.mark.parametrize("sentence", STRIPE_PASSES + COST_PASSES)
def test_stripe_and_cost_pass(sentence: str) -> None:
    """TD-WORD-01, TD-WORD-07: WORD-015 and WORD-012."""
    assert scan(sentence) == []


@pytest.mark.parametrize("source", HTML_PASSES)
def test_reader_text_passes(source: str) -> None:
    """D-3155: comments, expressions, statements, class and href are not reader text."""
    text = html_text(source)
    assert findings("app/site/templates/planted.html", text, text) == []


@pytest.mark.parametrize("source", HTML_FAILS)
def test_reader_text_fails(source: str) -> None:
    """D-3155: meta content, input value and alt are reader text; a brand expression reads as the brand."""
    text = html_text(source)
    assert findings("app/site/templates/planted.html", text, text)


def test_message_files_fill_the_brand() -> None:
    """Rule 1: each message's text is one line with the brand filled."""
    text = json_text('{"a": {"text": "{brand} sends\\nyour request."}, "b": {"text": "Hi."}}', False)
    assert text == f"{NAME} sends your request.\nHi."

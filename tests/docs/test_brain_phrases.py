"""Sales phrases, price-lock phrases and non-transactional invitations (T218).

TD-WORD-30, TD-WORD-33, TD-TXT-27 (WORD-030, WORD-032, TXT-046; D-490, D-1494, D-4258): one phrase scan
over the files `test_wording_scan.scanned_files()` lists (minus the two internal legal documents, D-3890)
plus `docs/copy/copy-panel.md` and `docs/copy/voice-of-customer.md`, with WORD-030's and WORD-032's closed
List B, allowed only when `test_wording_scan.negated` says so, after List A is removed. TXT-046: the six
invitation lists of `docs/ai_smell.yaml` (`notice_invitations`, no negation allowance) over every notice
source: message templates (brand filled), the email layout and the pages naming it, and the string
literals of `app/messaging/src/*.rs` before `#[cfg(test)]`. Every pattern is case-insensitive; `W` below
stands for one word and its space. Every planted case is its own parametrized case; the brand is read from
the brand file. Fake data only.

Run: python3 -m pytest tests/docs/test_brain_phrases.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

import pytest
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_wording_scan as scan  # noqa: E402  T104's reader, scanned_files and negated, imported, never copied

ROOT = scan.ROOT
NAME = scan.NAME
HANDLE = NAME.lower()
TEMPLATES = ROOT / "app" / "messaging" / "templates"
INTERNAL_LEGAL = ("docs/legal/information-security-program.md", "docs/legal/incident-response-plan.md")
EXTRA_COPY = ("docs/copy/copy-panel.md", "docs/copy/voice-of-customer.md")
LIST_A = ("{brand} doesn't promise any number of job cards, quotes, picks or jobs.".replace("{brand}", NAME),)
W = r"(?:[\w']+\s+)"

WORD_030 = (
    r"\bready to hire\b",
    r"\bserious homeowners?\b",
    r"\bpre-?qualified\b",
    r"\bqualified\b",
    r"\bhigh[- ]intent\b",
    r"\blistos? para contratar\b",
    r"\bpropietarios serios\b",
    r"\bprecalificad\w*",
    r"\balta intenci[oó]n\b",
    r"\bpromis\w*\s+W{0,4}?(?:jobs?|job cards?|picks?|wins?|work|customers?|clients?|quotes?|hires?|income|business)\b",
    r"\bguarantee[sd]?\s+W{0,3}?(?:jobs?|job cards?|picks?|wins?|work|customers?|clients?|quotes?|hires?|income)\b",
    r"\b(?:you'll|you will|will|guaranteed to)\s+(?:get|win|land|receive|book)\s+W{0,2}?(?:jobs?|customers?|clients?|picks?|wins?|work)\b",
    r"\bmore (?:jobs|customers|clients|wins|picks|hires)\b",
    r"\b(?:win|pick|hire|hiring|close|closing|success|conversion|booking)\s+rates?\b",
    r"\b(?:prometemos|promete|garantizamos|garantiza)\s+(?:\w+\s+){0,3}?(?:trabajos?|clientes?|elecciones|contrataciones)\b",
    r"\bm[aá]s (?:trabajos|clientes)\b",
    r"\btasas? de (?:[eé]xito|selecci[oó]n|contrataci[oó]n|elecci[oó]n|conversi[oó]n)\b",
)
WORD_032 = (
    r"\bprice[- ]?locks?\w*",
    r"\block(?:s|ed|ing)?\s+in\s+(?:your|the|a|our|their|today's|current|this)?\s*(?:rates?|prices?|fees?|pricing|deal)\b",
    r"\block(?:s|ed|ing)?\s+(?:it\s+)?in\s+(?:now|today|early)\b",
    r"\block(?:s|ed|ing)?\s+W{0,2}?(?:rates?|prices?|fees?|pricing)\b",
    r"\bfounding\s+(?:rates?|prices?|pricing|members?|business(?:es)?|partners?|fees?)\b",
    r"\bgrandfather\w*",
    r"\b(?:rates?|prices?|fees?|pricing)\s+W{0,2}?(?:never|won't|will not|doesn't|does not|can't|cannot)\s+(?:go(?:es)? up|rises?|increases?|changes?)\b",
    r"\b(?:rates?|prices?|fees?|pricing|tarifas?|precios?)\s+W{0,3}?(?:frozen|locked|freez\w*)\b",
    r"\bfrozen\s+W{0,2}?(?:rates?|prices?|fees?|pricing)\b",
    r"\bfreez\w*\s+W{0,2}?(?:rates?|prices?|fees?|pricing)\b",
    r"\b(?:early|first)[- ](?:bird|adopter|sign[- ]?up|members?)s?\b",
    r"\b(?:introductory|launch|founder'?s?)\s+(?:rates?|prices?|pricing|fees?|specials?)\b",
    r"\b(?:sign up|join)\s+(?:now|early|today|before)\b",
    r"\bguaranteed?\s+W{0,2}?(?:rates?|prices?|fees?|pricing)\b",
    r"\b(?:rates?|prices?|fees?|pricing)\b[^.!?;\n]*\b(?:for life|forever)\b",
    r"\b(?:for life|forever)\b[^.!?;\n]*\b(?:rates?|prices?|fees?|pricing)\b",
    r"\bcongel\w*\s+(?:\w+\s+){0,2}?(?:precios?|tarifas?)\b",
    r"\b(?:precios?|tarifas?)\s+(?:\w+\s+){0,2}?congelad\w*",
    r"\b(?:precios?|tarifas?)\s+(?:\w+\s+){0,2}?(?:de por vida|para siempre)\b",
    r"\bbloque\w*\s+(?:su |el |la )?(?:precio|tarifa)\w*",
    r"\b(?:precios?|tarifas?) (?:de )?fundador\w*",
    r"\bmantenga su (?:precio|tarifa)\b",
    r"\b(?:precio|tarifa)s?\s+(?:\w+\s+){0,2}?nunca\s+(?:sube|suben|cambia|cambian|aumenta|aumentan)\b",
    r"\bnunca sube\b",
)
SALES_LISTS = [re.compile(p.replace("W", W), re.IGNORECASE) for p in WORD_030 + WORD_032]

MUST_FAIL = [
    'Homeowners ready to hire.',
    'Serious homeowners only.',
    'Qualified requests.',
    'Pre-qualified jobs.',
    'High-intent homeowners.',
    'Propietarios listos para contratar.',
    'Propietarios serios.',
    'Solicitudes precalificadas.',
    'Clientes de alta intención.',
    'We promise you jobs.',
    '{brand} promises more work.',
    'Guaranteed jobs every week.',
    "You'll get more customers.",
    'You will win jobs.',
    'More jobs for your business.',
    'Our win rate is 40%.',
    'A 60% pick rate.',
    'Hire rate: 30%.',
    'Success rate of 90%.',
    'Prometemos trabajos.',
    'Garantizamos clientes.',
    'Más trabajos para usted.',
    'Tasa de éxito del 90%.',
    'Price lock for early businesses.',
    'Price-lock guarantee.',
    'Lock in your rate.',
    'Lock your price today.',
    'Founding rate.',
    'Founding member pricing.',
    'Grandfathered pricing.',
    'Your rate never goes up.',
    "Fees won't increase for you.",
    'Your fee is frozen.',
    'We freeze your fee.',
    'Early-bird pricing.',
    'Early adopters pay less.',
    'Launch pricing.',
    'Introductory rate.',
    'Sign up now.',
    'Join early.',
    'Guaranteed pricing.',
    'Your fee, for life.',
    'This rate is yours forever.',
    'Congelamos su tarifa.',
    'Precio de por vida.',
    'Bloquee su precio.',
    'Tarifa de fundador.',
    'Mantenga su tarifa.',
    'Su tarifa nunca sube.',
    'Frozen rates.',
    'Your rate is locked.',
    'Lock in now.',
    'Forever the same rate.',
    'Tarifa congelada.',
    '{brand} is not a contractor, but we promise jobs.',
]
MUST_PASS = [
    "{brand} doesn't promise any number of job cards, quotes, picks or jobs.",
    'kept locked for 5 years',
    'Your phone number, and your yes or stop to our texts: kept locked for 5 years after our last text.',
    'The midpoint of the hourly rate times your estimated hours.',
    'Fees follow the published schedule.',
    'A new fee never applies until you accept it.',
    'The fee never goes over $199.',
    'Una tarifa nueva nunca se aplica hasta que usted la acepte.',
    "Joining early doesn't lock in your rate.",
    "We don't promise jobs.",
    'The fee fixed at the pick is charged.',
    'Your details are locked in our vault.',
    'The pipe was frozen.',
    'Keep this code forever? No, it ends in 10 minutes.',
]
INVITATION_FAIL = [
    'Join {brand} today.',
    'Sign up your business.',
    'List your business with us.',
    'Become a partner.',
    "We're hiring.",
    'Apply to join.',
    'Know a plumber who needs work?',
    'Invite other businesses.',
    'Únase a {brand}.',
    'Registre su negocio.',
    '¿Conoce un negocio?',
    'Refer a friend.',
    'Tell your friends about us.',
    'Share {brand} with a neighbor.',
    'Spread the word.',
    'Pass it on.',
    'Recomiéndenos.',
    'Invite a sus amigos.',
    'Comparta este enlace.',
    'Corra la voz.',
    'Programa de referidos.',
    'Get a discount.',
    'Use this coupon.',
    'Promo code inside.',
    '20% off your next request.',
    '$10 off.',
    'Free month for new businesses.',
    'Special offer.',
    'Limited-time pricing.',
    'Save $20.',
    'Un descuento para usted.',
    'Use este cupón.',
    'Promoción de verano.',
    'Oferta especial.',
    'Código promocional.',
    'Take our survey.',
    'A short questionnaire.',
    'Answer our poll.',
    'Rate us.',
    'How did we do?',
    'How likely are you to recommend us?',
    'Leave us a review.',
    'Review us on Google.',
    'Responda la encuesta.',
    'Un cuestionario.',
    'Califíquenos.',
    'Déjenos una reseña.',
    'A new feature is here.',
    'Now available: photo quotes.',
    'Introducing quote alerts.',
    'Coming soon: business texting.',
    "What's new at {brand}.",
    'Check out our new page.',
    "We've added quote alerts.",
    'Nueva función disponible.',
    'Ya disponible.',
    'Presentamos alertas.',
    'Próximamente.',
    'Novedades de {brand}.',
    'Find us on Facebook.',
    'See us on Instagram.',
    'On Twitter.',
    'On TikTok.',
    'On Nextdoor.',
    'On YouTube.',
    'On LinkedIn.',
    'x.com/{handle}',
    'Follow us.',
    'Like us.',
    'Social media.',
    'Síganos.',
    'Redes sociales.',
    'Refer someone today.',
    'Refer them to us.',
    'Referred by a friend?',
    'Our referral program.',
]
INVITATION_PASS = [
    'Reply STOP to opt out.',
    '{brand}: Job 1042: no call yet.',
    'Responda PARAR para salir.',
    'Es gratis para usted.',
    'Thanks. The {brand} owner reads every problem report, usually within 7 days.',
    'Reply HELP for help.',
    'This refers to job 1042.',
]
LITERAL = re.compile(r'r#"(.*?)"#|"((?:\\.|[^"\\])*)"', re.S)


def filled(text: str) -> str:
    """A planted case with the brand name and handle read from the brand file."""
    return text.replace("{brand}", NAME).replace("{handle}", HANDLE)


def sales_files() -> list[str]:
    """Every file the phrase scan reads (WORD-030, WORD-032, D-3890)."""
    files = [f for f in scan.scanned_files() if f not in INTERNAL_LEGAL]
    return files + [f for f in EXTRA_COPY if f not in files]


def sales_hits(text: str) -> list[str]:
    """List B matches in `text` after List A is removed, none of them negated (WORD-030, WORD-032)."""
    for phrase in LIST_A:
        text = re.sub(re.escape(phrase), " ", text, flags=re.IGNORECASE)
    return [m.group(0) for p in SALES_LISTS for m in p.finditer(text) if not scan.negated(text, m.start())]


def invitation_lists() -> dict[str, list[re.Pattern[str]]]:
    """The six `notice_invitations` lists of docs/ai_smell.yaml, compiled case-insensitively (TXT-046)."""
    data = yaml.safe_load((ROOT / "docs" / "ai_smell.yaml").read_text(encoding="utf-8"))["notice_invitations"]
    return {kind: [re.compile(p, re.IGNORECASE) for p in patterns] for kind, patterns in data.items()}


def invitation_hits(text: str) -> list[str]:
    """Invitation matches in `text`; no negation allowance (TXT-046)."""
    return [m.group(0) for lists in invitation_lists().values() for p in lists for m in p.finditer(text)]


def template_texts(data: dict, lang: str) -> list[str]:
    """Each template entry's text with the brand tokens filled (TXT-046)."""
    return [scan.brand.fill(entry["text"], lang == "es") for entry in data.values()]


def literals(source: str) -> list[str]:
    """The string literals of a Rust file up to its first `#[cfg(test)]` line (email bodies built in code)."""
    head = source.split("#[cfg(test)]")[0]
    return [a or b for a, b in LITERAL.findall(head)]


def notice_texts() -> dict[str, list[str]]:
    """Every notice source by path: message templates, the email layout and pages naming it, Rust literals."""
    found = {}
    for lang in ("en", "es"):
        path = TEMPLATES / f"{lang}.json"
        found[path.relative_to(ROOT).as_posix()] = template_texts(json.loads(path.read_text(encoding="utf-8")), lang)
    pages = [(ROOT / "app/messaging/templates/email_layout.html").relative_to(ROOT).as_posix()]
    for path in sorted((ROOT / "app").glob("*/templates/*.html")):
        if "email_layout" in path.read_text(encoding="utf-8"):
            pages.append(path.relative_to(ROOT).as_posix())
    for rel in pages:
        found[rel] = [scan.reader(rel)[1]]
    for path in sorted((ROOT / "app" / "messaging" / "src").glob("*.rs")):
        found[path.relative_to(ROOT).as_posix()] = literals(path.read_text(encoding="utf-8"))
    return found


def test_td_word_30_the_scan_set_is_the_named_one() -> None:
    files = sales_files()
    assert "docs/legal/business-terms.md" in files
    assert "docs/copy/copy-panel.md" in files and "docs/copy/voice-of-customer.md" in files
    assert not set(files) & set(INTERNAL_LEGAL)
    assert any(f.startswith("app/") and f.endswith("en.json") for f in files)
    for internal in ("docs/decisions.md", "docs/PREBUILD.md", "docs/for-casey-review.md", "docs/test-design.md"):
        assert internal not in files


def test_td_word_30_every_scanned_file_as_it_stands_is_clean() -> None:
    bad = {rel: hits for rel in sales_files() if (hits := sales_hits(scan.reader(rel)[1]))}
    assert not bad, bad


@pytest.mark.parametrize("case", MUST_FAIL)
def test_td_word_30_td_word_33_planted_phrase_fails(case: str) -> None:
    text = filled(case)
    assert sales_hits(text), text
    assert sales_hits(text.upper()), text.upper()


@pytest.mark.parametrize("case", MUST_PASS)
def test_td_word_30_td_word_33_must_pass_case_is_clean(case: str) -> None:
    assert not sales_hits(filled(case))


def test_td_word_33_hyphen_and_case_variants_of_price_lock_fail() -> None:
    for variant in ("price lock", "price-lock", "PRICE LOCK", "Price-Lock"):
        assert sales_hits(f"A {variant} for early businesses.")


def test_td_word_30_removing_any_one_pattern_turns_a_planted_case_red() -> None:
    texts = [filled(c) for c in MUST_FAIL]
    for pattern in SALES_LISTS:
        assert any(pattern.search(t) and not scan.negated(t, pattern.search(t).start()) for t in texts), pattern.pattern


def test_td_txt_27_the_invitation_list_has_the_six_kinds() -> None:
    lists = invitation_lists()
    assert set(lists) == {"recruiting", "referral", "discount", "survey", "new_feature", "social"}
    assert all(lists.values())


def test_td_txt_27_every_notice_source_as_it_stands_is_clean() -> None:
    sources = notice_texts()
    assert any(rel.endswith("email_layout.html") for rel in sources)
    assert any(rel.endswith(".rs") and texts for rel, texts in sources.items())
    bad = {rel: hits for rel, texts in sources.items() if (hits := [h for t in texts for h in invitation_hits(t)])}
    assert not bad, bad


@pytest.mark.parametrize("lang", ["en", "es"])
@pytest.mark.parametrize("case", INVITATION_FAIL)
def test_td_txt_27_planted_invitation_in_a_template_turns_it_red(lang: str, case: str) -> None:
    text = filled(case)
    for variant in (text, text.upper()):
        entries = {"planted": {"text": variant}}
        assert [h for t in template_texts(entries, lang) for h in invitation_hits(t)], variant


@pytest.mark.parametrize("case", INVITATION_PASS)
def test_td_txt_27_transactional_text_passes(case: str) -> None:
    assert not invitation_hits(filled(case))


def test_td_txt_27_a_planted_email_literal_in_a_rust_file_turns_it_red() -> None:
    source = 'fn body() -> String {\n    "Join {brand} today.".into()\n}\n'.replace("{brand}", NAME)
    assert [h for t in literals(source) for h in invitation_hits(t)]
    raw = 'fn body() -> &\'static str {\n    r#"Follow us on x.com."#\n}\n'
    assert [h for t in literals(raw) for h in invitation_hits(t)]
    after_tests = 'fn ok() {}\n#[cfg(test)]\nmod tests { const S: &str = "Join us."; }\n'
    assert not literals(after_tests)


def test_td_txt_27_removing_any_one_invitation_pattern_turns_a_planted_case_red() -> None:
    texts = [filled(c) for c in INVITATION_FAIL]
    for kind, patterns in invitation_lists().items():
        for pattern in patterns:
            assert any(pattern.search(t) for t in texts), (kind, pattern.pattern)

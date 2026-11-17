"""The homeowner and business data pages hold what the spec requires, in English and Spanish (T221;
TD-PRIV-06, TD-PRIV-07, TD-TERMS-04, TD-WEB-05, TD-WEB-01; WEB-014, TERMS-021, TERMS-023, TERMS-025,
AI-018, D-3844, D-3878).

Reads the two templates, `app/site/templates/your_data.html` and `business_data.html`, each of which
renders both languages from the language its route passes: the Spanish branch is the
`{% if es %}` part, the English branch the `{% else %}` part. The spec-fixed sentences are taken
from `docs/spec/`, the brand from `app/core/data/brand.json` (the templates print it with
`{{ brand }}`), and the retention periods from `docs/legal/privacy-policy.md` section 8, so the
pages cannot drift from them. The consent and HELP texts' legal links must be served rows of
`docs/architecture.md` section 13 (WEB-014). The branch rendering itself is tested in
`app/site/src/legal_pages.rs`. Fake data only.

Run: python3 -m pytest tests/docs/test_legal_pages.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "app" / "site" / "templates"
BRAND = json.loads((ROOT / "app" / "core" / "data" / "brand.json").read_text(encoding="utf-8"))["display_name"]
POLICY = (ROOT / "docs" / "legal" / "privacy-policy.md").read_text(encoding="utf-8")
LEGAL_PATHS = ("/terms", "/privacy", "/sms-terms", "/your-data", "/business-data")


def _spec(req: str) -> str:
    """The text of requirement `req` in docs/spec/, on one line."""
    for path in sorted((ROOT / "docs" / "spec").glob("*.md")):
        found = re.search(rf"^- \*\*{req}\*\* (.*)$", path.read_text(encoding="utf-8"), re.M)
        if found:
            return " ".join(found[1].split())
    raise AssertionError(f"{req} not in docs/spec/")


def _quoted(req: str, start: str) -> str:
    """The quoted sentence of `req` that starts with `start`, its brand taken from the brand file."""
    found = re.search(rf'"({re.escape(start)}[^"]*)"', _spec(req))
    assert found, f"{req} quotes no sentence starting {start!r}"
    return found[1].replace("Laftly", BRAND)


def _branches(name: str) -> tuple[str, str]:
    """(English, Spanish) branch of a data template's content block, brand filled, one line each."""
    text = (TEMPLATES / name).read_text(encoding="utf-8").replace("{{ brand }}", BRAND)
    content = text[text.index("{% block content %}") : text.index("{% block footer %}")]
    spanish, english = _if_else(content, "es")
    return " ".join(english.split()), " ".join(spanish.split())


def _if_else(text: str, flag: str) -> tuple[str, str]:
    """The `{% if flag %}` and `{% else %}` parts of `text`, nested ifs kept whole."""
    start = text.index(f"{{%- if {flag} %}}")
    depth, parts, mark = 0, [], start
    for tag in re.finditer(r"\{%-? (if|else|endif)\b[^%]*%\}", text[start:]):
        at = start + tag.start()
        if tag[1] == "if":
            depth += 1
            if depth == 1:
                mark = start + tag.end()
        elif depth == 1:
            parts.append(text[mark:at])
            mark = start + tag.end()
        if tag[1] == "endif":
            depth -= 1
            if depth == 0:
                break
    return parts[0], parts[1]


def _visible(html: str) -> str:
    return " ".join(re.sub(r"<[^>]+>", "", html).split())


AI_OFF = _quoted("AI-018", "Our texts are automated.")
TERMS_025 = _quoted("TERMS-025", "When you send a request,")
AI_OFF_ES = "Nuestros mensajes de texto son automáticos. Cuando se active, un servicio de IA ayuda"


def test_td_priv_06_homeowner_page_in_english_holds_every_terms_023_item() -> None:
    """TD-PRIV-06, TD-PRIV-07 (TERMS-023, TERMS-025, WORD-007, WORD-018, PRIV-011)."""
    english, _ = _branches("your_data.html")
    seen = _visible(english)
    for line in [
        TERMS_025,
        "Only the business you pick gets your first name and address, after you pick it.",
        _quoted("WORD-018", "Laftly never gives"),
        _quoted("PRIV-011", "We keep your phone number"),
        "deleted 180 days after your request closes",
        "Photos: deleted 30 days after your request closes.",
        "kept 3 years",
        "kept locked for 5 years after our last text",
        "kept 7 years after the last pick you made under those terms",
        "We never ask about immigration status. We never ask for ID.",
        "We give your data to police or anyone else only when the law makes us. Examples are a subpoena or a warrant.",
        f"{BRAND}'s owner never uses {BRAND}'s data for any other work or business.",
    ]:
        assert line in seen, line
    assert "<h1>Your data</h1>" in english and '<h1 lang="es">' not in english


def test_td_priv_07_homeowner_page_in_spanish_holds_the_same_items() -> None:
    """TD-PRIV-06, TD-PRIV-07 (TERMS-023, TERMS-025): the Spanish branch, every passage in `lang="es"`."""
    _, spanish = _branches("your_data.html")
    seen = _visible(spanish)
    for line in [
        "Cuando usted envía una solicitud,",
        "le da a ese único negocio su nombre de pila y la dirección del trabajo",
        f"{BRAND} nunca le da su número de teléfono a ningún negocio, y nunca vende ni comparte números",
        "Solo el negocio que usted elige recibe su nombre de pila y su dirección",
        "se borran 180 días después",
        "se borran 30 días después",
        "se guarda 3 años",
        "se guarda 5 años después de nuestro último mensaje",
        "estatus migratorio",
        "una identificación",
        "solo cuando una ley lo exige",
        f"El dueño de {BRAND} nunca usa los datos de {BRAND} para ningún otro trabajo o negocio.",
    ]:
        assert line in seen, line
    assert '<h1 lang="es">Sus datos</h1>' in spanish and "<h1>Your data</h1>" not in spanish


def test_td_web_05_business_page_holds_every_terms_021_item() -> None:
    """TD-WEB-05 (TERMS-021, OPS-037): notice email, channel choice, notice log and timing records,
    no recordings, no question about workers, the legal-demand line, and no pick-call log."""
    english, spanish = _branches("business_data.html")
    seen, visto = _visible(english), _visible(spanish)
    for line in [
        "The email address where we send job requests and picks.",
        "How you chose to get job requests and picks.",
        "A log of each email we send you: when it was sent, arrived and first opened",
        "When each job card was sent, opened and quoted, and when each pick was sent, opened and confirmed",
        "call logs hold no recordings.",
        f"{BRAND} never asks about your workers.",
        "only when the law makes us. Examples are a subpoena or a warrant.",
        _quoted("TERMS-021", "We'll tell you about any legal demand"),
        f"{BRAND}'s owner never uses {BRAND}'s data for any other work or business.",
    ]:
        assert line in seen, line
    for line in ["correo de avisos", "El canal", "registro de avisos", "registros de tiempos",
                 "No contienen grabaciones", "nunca pregunta por sus trabajadores",
                 "Le avisaremos de cualquier exigencia legal", "solo cuando una ley lo exige"]:
        assert line in visto, line
    for text in (seen, visto):
        assert "pick call" not in text.lower() and "llamada de elección" not in text.lower()


def test_td_web_05_business_retention_days_equal_the_privacy_policy_bracket() -> None:
    """D-3878 (TERMS-021): the business data page's retention text is the policy's own bracket."""
    found = re.search(r"\*\*Business records, team phones and call logs:\*\* (\[[^\]]+\])", POLICY)
    assert found, "privacy policy section 8 lost its business retention line"
    for branch in _branches("business_data.html"):
        days = re.findall(r'<span[^>]*class="retention-days">([^<]*)</span>', branch)
        assert days == [found[1]]


def test_td_priv_06_homeowner_retention_matches_the_privacy_policy() -> None:
    """TERMS-023, PRIV-007 to PRIV-011: each period the page names is the policy's section 8 period."""
    section = POLICY[POLICY.index("## 8. How long we keep it") : POLICY.index("## 9.")]
    for period in ["180 days after your job closes", "30 days after your job closes", "3 years",
                   "5 years after our last message to that number", "7 years after the last pick"]:
        assert period in section, period


@pytest.mark.parametrize("name", ["your_data.html", "business_data.html"])
def test_td_terms_04_data_pages_hold_the_ai_018_wording(name: str) -> None:
    """TD-TERMS-04 (AI-018): the switched-off sentence in both languages, the switched-on
    paragraph only in the `ai_on` branch, and no banned AI phrase."""
    for branch, off in zip(_branches(name), (AI_OFF, AI_OFF_ES), strict=True):
        on, rest = _if_else(branch, "ai_on")
        assert off in _visible(rest)
        assert off not in _visible(on)
    assert _quoted("AI-018", "To sort your request") in _visible(_branches(name)[0])
    text = (TEMPLATES / name).read_text(encoding="utf-8").lower()
    for banned in ("ai-powered", "no ai reads your messages today"):
        assert banned not in text


def test_td_web_01_consent_links_are_served_routes() -> None:
    """TD-WEB-01 (WEB-014): every legal link in the text templates is a served section 13 row."""
    table = (ROOT / "docs" / "architecture.md").read_text(encoding="utf-8")
    served = set(re.findall(r"^\| GET \| (\S+) \| site \| yes \|$", table, re.M))
    for path in LEGAL_PATHS:
        assert path in served, path
    for lang in ("en", "es"):
        texts = (ROOT / "app" / "messaging" / "templates" / f"{lang}.json").read_text(encoding="utf-8")
        for link in re.findall(r"\{domain\}(/[a-z/-]*)", texts):
            if link in LEGAL_PATHS:
                assert link in served, link
    assert "/es/your-data" in served and "/es/business-data" in served

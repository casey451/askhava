"""The homeowner texting campaign draft matches the real templates (T113).

TD-TERMS-01, TD-TERMS-02, TD-WEB-12, TD-CON-16, TD-TXT-26 (TERMS-002 to TERMS-004, TERMS-006 to
TERMS-009, TERMS-011, TERMS-013 to TERMS-015, TERMS-018, TERMS-029, TERMS-030, TERMS-032, TXT-043,
CON-033; D-304, D-419, D-490, D-3880, D-4147).

Reads `docs/legal/twilio-campaign.md` and `app/messaging/templates/en.json` and `es.json`. Each sample
in the draft is a fenced block after a line ending "Template `<key>`:" ("Spanish template" for the
Spanish sample); it must equal that template with its brand tokens filled from
app/core/data/brand.json (D-1932), so editing a template without the draft, or the draft without the
template, turns this red. The registry check: a template is routable to the homeowner number when
its `audience` is `homeowner` (`Line::sender` sends from the audience's one number), and the only
business template allowed there is `business_signup_reply` (TERMS-029). The TERMS-002 to TERMS-009
clauses live in the other drafts, checked by tests/docs/test_terms_clauses.py; this file checks the
campaign's share of TD-TERMS-01. Brand text comes from the brand file, never a literal. Fake data only.

Run: python3 -m pytest tests/docs/test_campaign.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

import brand
import check_legal_docs

ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs" / "legal" / "twilio-campaign.md"
TEMPLATES = ROOT / "app" / "messaging" / "templates"
BRAND = brand.load()
# TERMS-013, TERMS-029, TERMS-030, D-3880: the six homeowner samples, in this order.
SAMPLES = ("consent", "first_quote", "text_pick_done", "opt_out_confirmation", "whos_calling", "business_signup_reply")
SAMPLE = re.compile(r"^(?:[^\n]* )?(Spanish template|Template) `([a-z0-9_]+)`:\n\n```text\n(.*?)\n```$", re.M | re.S)
# TERMS-011 and D-088's drafted wording, with D-419's opt-in (no reply of OK) and TERMS-029's sentence.
DESCRIPTION = (
    "{brand_legal} is a local service in Lake Havasu City, AZ. People text us, or start on {domain}, to ask "
    "for quotes on a home or local service they need. They opt in by texting us first, or by entering in the "
    "{domain} chat a code we text to their mobile number; our first reply states message frequency, rates, "
    "HELP and STOP. We text them only about their own request: confirmations, when quotes arrive, the business "
    "they choose and who will call them. We never share, sell or transfer phone numbers or opt-in consent, and "
    "businesses never send messages on this campaign. Businesses get no texts on this number; they are notified "
    "by email. A person who texts asking to join as a business gets one reply with the sign-up link. Use case: "
    "Customer Care, Low-Volume Standard."
)
SIGNUP_REPLY = "business_signup_reply"
# Template keys that name a business message, whatever audience a later edit gives them.
BUSINESS_PREFIXES = (
    "business_", "pick_", "release_", "job_card", "card_fix", "card_approval", "unreachable_no", "unreachable_refunded",
)


def doc() -> str:
    return DOC.read_text(encoding="utf-8")


def load(lang: str) -> dict[str, dict]:
    return json.loads((TEMPLATES / f"{lang}.json").read_text(encoding="utf-8"))


def samples(text: str) -> list[tuple[str, str, str]]:
    """Each sample as (language, template key, text as written in the draft)."""
    return [("es" if kind.startswith("Spanish") else "en", key, body) for kind, key, body in SAMPLE.findall(text)]


def mismatches(text: str, registry: dict[str, dict[str, dict]]) -> list[str]:
    """TERMS-013: every sample that is not its template, brand tokens filled, word for word."""
    errors = []
    for lang, key, body in samples(text):
        entry = registry[lang].get(key)
        if entry is None:
            errors.append(f"{lang}:{key}: no such template")
        elif body != brand.fill(entry["text"], lang == "es", BRAND):
            errors.append(f"{lang}:{key}: sample differs from its template")
    return errors


def business_on_homeowner_number(registry: dict[str, dict[str, dict]]) -> list[str]:
    """TERMS-013, TERMS-029: business templates routable to the homeowner number, the sign-up reply aside."""
    business = {key for entries in registry.values() for key, e in entries.items() if e["audience"] != "homeowner"}
    found = []
    for lang, entries in registry.items():
        for key, entry in entries.items():
            named = key in business or key.startswith(BUSINESS_PREFIXES) or "{domain}/b/" in entry["text"]
            if entry["audience"] == "homeowner" and named and key != SIGNUP_REPLY:
                found.append(f"{lang}:{key}")
    return found


def section(text: str, heading: str) -> str:
    """The body of one `## ` section of the draft."""
    match = re.search(rf"^## {re.escape(heading)}\n(.*?)(?=^## |\Z)", text, re.M | re.S)
    assert match, heading
    return match.group(1)


def registry() -> dict[str, dict[str, dict]]:
    return {"en": load("en"), "es": load("es")}


def test_samples_are_exactly_the_six_templates() -> None:
    """TD-TERMS-02, TD-WEB-12, TERMS-013, TERMS-029, TERMS-030: the six homeowner samples, then the Spanish one."""
    found = samples(doc())
    assert [key for lang, key, _ in found if lang == "en"] == list(SAMPLES)
    assert [key for lang, key, _ in found if lang == "es"] == ["consent"]


def test_every_sample_is_its_template_word_for_word() -> None:
    """TD-TERMS-01, TD-TERMS-02, TERMS-013: no sample differs from its template."""
    assert mismatches(doc(), registry()) == []


@pytest.mark.parametrize("key", SAMPLES)
def test_a_planted_sample_that_differs_fails(key: str) -> None:
    """TD-TERMS-02, TERMS-013: one changed word in any sample turns the check red."""
    text = doc()
    body = next(b for lang, k, b in samples(text) if lang == "en" and k == key)
    planted = text.replace(body, body.replace(" ", "  ", 1), 1)
    assert mismatches(planted, registry()) == [f"en:{key}: sample differs from its template"]


def test_a_planted_template_edit_fails() -> None:
    """TD-TERMS-02, TERMS-013: a template changed without the draft turns the check red too."""
    edited = registry()
    edited["es"]["consent"] = {**edited["es"]["consent"], "text": edited["es"]["consent"]["text"] + " Gracias."}
    assert mismatches(doc(), edited) == ["es:consent: sample differs from its template"]


def test_only_the_signup_reply_business_template_reaches_the_homeowner_number() -> None:
    """TD-TERMS-02, TERMS-013, TERMS-029: every sample is a homeowner template; no other business one is."""
    reg = registry()
    assert business_on_homeowner_number(reg) == []
    for lang in reg:
        assert all(reg[lang][key]["audience"] == "homeowner" for key in SAMPLES)


@pytest.mark.parametrize("key", ["pick_notice", "job_card_notice", "business_signin_link"])
def test_a_planted_business_template_on_the_homeowner_number_fails(key: str) -> None:
    """TD-TERMS-02, TERMS-013: a business template moved to the homeowner audience turns the check red."""
    reg = registry()
    reg["en"][key] = {**reg["en"][key], "audience": "homeowner"}
    assert f"en:{key}" in business_on_homeowner_number(reg)


def test_description_is_the_drafted_wording() -> None:
    """TD-TERMS-02, TERMS-011, TERMS-029: the description, with the business sentences after the no-business line."""
    quoted = section(doc(), "3. Campaign description").strip()
    assert quoted == "> " + brand.fill(DESCRIPTION, data=BRAND)
    never = quoted.index("businesses never send messages on this campaign")
    assert quoted.index("Businesses get no texts on this number; they are notified by email.") > never


def test_registration_ticks_links_and_phone_numbers_after_the_mailing_address() -> None:
    """TD-WEB-12, TD-TERMS-01, TERMS-011, TERMS-030, TERMS-032: use case, ticks and the address first."""
    text = doc()
    settings = section(text, "2. Campaign settings")
    assert "Use case: Customer Care, Low-Volume Standard." in settings
    assert '"Messages will include embedded links": yes' in settings
    assert '"Messages will include phone numbers": yes' in settings
    before = section(text, "1. Before registering")
    assert f"{BRAND['domain']} and the SMS terms show the {BRAND['legal_name']} mailing address" in before
    assert "{number}" in next(b for lang, k, b in samples(text) if k == "whos_calling")


def test_toll_free_fallback_only_with_the_owners_go() -> None:
    """TD-CON-16, CON-033: same content and samples, never business notices, never around a suspension."""
    refused = " ".join(section(doc(), "8. If the campaign is refused").split())
    assert "only with the owner's go, the fallback is toll-free verification for the homeowner texts" in refused
    assert "with the same content, consent text and samples" in refused
    assert "never used for business notices without the written approval business texting needs" in refused
    assert "never used to get around a suspension of a working number" in refused
    assert "Until it is verified, the service runs in web-and-email mode:" in refused


def test_spanish_note_sample_and_review_items() -> None:
    """TD-TXT-26, TD-TERMS-01, TXT-043, TERMS-018: the Spanish note and sample on file, and the review order."""
    spanish = " ".join(section(doc(), "7. Spanish").split())
    assert 'Add the sentence "Texts are in English or Spanish." to the campaign description' in spanish
    assert "2. Add the Spanish sample below as a sample message. 3. Log the change." in spanish
    assert "only the Spanish texts that stay on while Spanish conversations are off are checked" in spanish
    assert "Two AI tools translate them back to English for that check." in spanish
    assert "paid professional review of every Spanish template comes before the Spanish conversations" in spanish


def test_no_business_traffic_and_the_no_sharing_sentence() -> None:
    """TD-TERMS-01, TERMS-009, TERMS-011, TERMS-015: homeowner texts only; business texting is its own campaign."""
    text = " ".join(doc().split())
    assert check_legal_docs.TWILIO_LINE[1] in text
    assert "it gets its own campaign on its own number with its own description" in text
    assert "Its samples are a job card (the new job notice), a pick notice and the STOP reply." in text
    assert "It never runs on this number." in text
    assert "twilio-campaign.md" in check_legal_docs.DOCUMENTS

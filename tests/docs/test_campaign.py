"""The homeowner texting campaign draft matches the real templates (T113, T800).

TD-TERMS-01, TD-TERMS-02, TD-WEB-12, TD-CON-16, TD-TXT-26 (TERMS-002 to TERMS-004, TERMS-006 to
TERMS-009, TERMS-011, TERMS-013 to TERMS-015, TERMS-018, TERMS-029, TERMS-030, TERMS-032, TXT-043,
CON-005, CON-009, CON-018, CON-030, CON-033; D-304, D-419, D-490, D-3880, D-4147, D-5278, D-5280).

Reads `docs/legal/twilio-campaign.md` and `app/messaging/templates/en.json` and `es.json`. Each sample
in the draft is a fenced block after a line ending "Template `<key>`:" ("Spanish template" for the
Spanish sample); it must equal that template with its brand tokens filled from
app/core/data/brand.json (D-1932), so editing a template without the draft, or the draft without the
template, turns this red. The registry check: a template is routable to the homeowner number when
its `audience` is `homeowner` (`Line::sender` sends from the audience's one number), and the only
business template allowed there is `business_signup_reply` (TERMS-029). The TERMS-002 to TERMS-009
clauses live in the other drafts, checked by tests/docs/test_terms_clauses.py; this file checks the
campaign's share of TD-TERMS-01. Brand text comes from the brand file, never a literal. Fake data only.
D-5278 (logged spec change, BUILD_BRIEF.md:69) moved the samples to five, the Spanish consent among
them, the description to the full one with the fee and the release, and added the checks on the
Console copy, the opt-out field (stand_ins/provider_optout.json) and the `help_message` excerpt.

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
# TERMS-013, TERMS-030, D-5278: the four English samples, in this order, then the Spanish consent.
SAMPLES = ("consent", "first_quote", "text_pick_done", "whos_calling")
SAMPLE = re.compile(r"^(?:[^\n]* )?(Spanish template|Template) `([a-z0-9_]+)`:\n\n```text\n(.*?)\n```$", re.M | re.S)
# TERMS-011, TERMS-029 and D-5278: the full description, with the fee, the release and no chat.
DESCRIPTION = (
    "{brand_legal} runs {brand}, a quote-request service for homes in Lake Havasu City, AZ. A homeowner "
    "finds our number on a public page on {domain}, next to our SMS disclosures, and texts us first. Our "
    "first reply is the full consent text: message frequency, rates, HELP and STOP. Only {brand} "
    "texts her, and only about her own request: questions about the job, confirmations, when quotes arrive, "
    "her pick and who will call her. Local businesses get the job without her name, phone, email or street "
    "address and post quotes on {domain}. The business she picks pays {brand} a fixed introduction fee by "
    "job size: $25, $75 or $199. Some jobs are fee-free. Only then does that business see her first name "
    "and the job address. Her phone number and her SMS consent are never given to any business; calls go through a "
    "masked number. We send no marketing, and businesses never send messages on this campaign. Businesses "
    "get no texts on this number; they are notified by email. A person who texts asking to join as a "
    "business gets one reply with the sign-up link. Texts are in English. Once we turn Spanish on, people "
    "who write to us in Spanish get Spanish texts."
)
# D-5278, 30984: each filled value in the Console copy, in [brackets].
BRACKETS = {
    "job": "[request number]",
    "link": f"https://{BRAND['domain']}/q/[private code]",
    "quoted": "[quote number]",
    "business": "[business name]",
    "number": "[masked phone number]",
}
CONSOLE = re.compile(r"^Console sample (\d):\n\n```text\n(.*?)\n```$", re.M | re.S)
FENCE = re.compile(r"^```text\n(.*?)\n```$", re.M | re.S)
OPTOUT = json.loads((ROOT / "stand_ins" / "provider_optout.json").read_text(encoding="utf-8"))["opt_out"]["reply"]
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


def test_samples_are_exactly_the_five_templates() -> None:
    """TD-TERMS-02, TD-WEB-12, TERMS-013, TERMS-030, D-5278: four English samples, then the Spanish consent."""
    found = samples(section(doc(), "6. Sample messages"))
    assert [(lang, key) for lang, key, _ in found] == [("en", key) for key in SAMPLES] + [("es", "consent")]
    assert samples(doc()) == found
    assert "opt_out_confirmation" not in [key for _, key, _ in found]
    assert SIGNUP_REPLY not in [key for _, key, _ in found]


def console_copy(text: str) -> list[str]:
    """The Console samples, in their numbered order."""
    found = CONSOLE.findall(text)
    assert [int(n) for n, _ in found] == list(range(1, len(found) + 1))
    return [body for _, body in found]


def bracketed(body: str) -> str:
    """A sample as the Console takes it: each {token} filled with its [bracketed] value (30984)."""
    return re.sub(r"\{([a-z_]+)\}", lambda m: BRACKETS[m.group(1)], body)


def test_console_copy_is_each_sample_with_brackets_and_no_unfilled_placeholder() -> None:
    """TERMS-013, D-5278: five Console samples, each its sample with [brackets]; no braces or angle brackets."""
    text = doc()
    console = console_copy(text)
    assert console == [bracketed(body) for _, _, body in samples(text)]
    for body in console:
        assert not re.search(r"[{}<>]", body), body


def test_a_planted_unfilled_placeholder_in_the_console_copy_fails() -> None:
    """D-5278, 30984: a token left in braces in the Console copy turns the check red."""
    text = doc()
    planted = text.replace("[request number] is in", "{job} is in", 1)
    assert console_copy(planted) != [bracketed(body) for _, _, body in samples(planted)]


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


def test_description_names_the_fee_and_the_release_and_never_the_chat() -> None:
    """TERMS-011, D-5278, 30988: the fee and the first-name-and-address release are named; no chat or widget."""
    quoted = section(doc(), "3. Campaign description")
    assert "fixed introduction fee by job size: $25, $75 or $199." in quoted
    assert "Only then does that business see her first name and the job address." in quoted
    assert not re.search(r"\b(?:chat|widget)", quoted, re.I)


def test_registration_ticks_links_and_phone_numbers_after_the_mailing_address() -> None:
    """TD-WEB-12, TD-TERMS-01, TERMS-011, TERMS-030, TERMS-032: brand type, use case, ticks and the address first."""
    text = doc()
    settings = section(text, "2. Campaign settings")
    assert "- Brand type: Low-Volume Standard.\n- Use case: Low Volume Mixed.\n" in settings
    assert "pre-clearance" not in text
    assert '"Messages will include embedded links": yes' in settings
    assert '"Messages will include phone numbers": yes' in settings
    before = section(text, "1. Before registering")
    assert f"{BRAND['domain']} and the SMS terms show the {BRAND['legal_name']} mailing address" in before
    assert "{number}" in next(b for lang, k, b in samples(text) if k == "whos_calling")


def test_register_only_after_the_written_answer_and_the_pre_submit_check() -> None:
    """TERMS-014, CON-005, D-5278: the filed answer to the question, then the December 9 codes and the evidence."""
    before = " ".join(section(doc(), "1. Before registering").split())
    assert "sent Twilio the written question in `docs/handoff/Twilio_question.md` and filed Twilio's" in before
    assert "including the request page, the privacy policy and the SMS terms" in before
    assert "whether businesses may get texts" not in doc()
    check = " ".join(section(doc(), "9. Pre-submit check").split())
    for code in ("30977", "30984", "30988", "30965", "30987", "30966", "30978"):
        assert code in check
    assert "public screenshot or evidence link" in check
    assert "No number is public before Gate C, and there is no exception to that." in check
    assert "no unfilled placeholder is left" in check


def test_one_opt_in_path_the_first_inbound_text() -> None:
    """CON-005, CON-025, D-5278: the message flow names the first inbound text only; a Verify code is not opt-in."""
    optin = section(doc(), "5. How people opt in")
    flow = next(line for line in optin.splitlines() if line.startswith("> "))
    first = brand.fill("A person opts in by sending their own first text to the {brand} number.", data=BRAND)
    assert flow.startswith("> " + first)
    assert not re.search(r"\b(?:chat|widget|code)", flow, re.I)
    assert "There is one way to opt in" in optin
    assert "A sign-in code sent by Twilio Verify is not an opt-in." in optin


def test_opt_out_field_is_the_exact_provider_confirmation() -> None:
    """CON-009, CON-030, D-5278: the opt-out field equals stand_ins/provider_optout.json, English then Spanish."""
    optin = section(doc(), "5. How people opt in")
    assert FENCE.findall(optin)[:2] == [OPTOUT["en"], OPTOUT["es"]]


def help_sentences(text: str) -> list[str]:
    """The whole sentences of a help text, in order."""
    return re.split(r"(?<=[.?!])\s+", text.strip())


def live_help_reply() -> str:
    """CON-018: the English `help_reply` as sent, its {reply_time} taken from check_legal_docs.USUALLY."""
    reply_time = re.search(r"usually replies within (.*)\.$", check_legal_docs.USUALLY[1]).group(1)
    return brand.fill(load("en")["help_reply"]["text"].replace("{reply_time}", reply_time), data=BRAND)


def is_excerpt(field: str, live: str) -> bool:
    """D-5280: every sentence of the field is a whole sentence of the live reply, in its order."""
    remaining = iter(help_sentences(live))
    return all(sentence in remaining for sentence in help_sentences(field))


def test_help_message_is_a_whole_sentence_excerpt_of_the_live_reply() -> None:
    """CON-018, CON-030, D-5278, D-5280: the `help_message` field, 20 to 320 characters, cut from `help_reply`."""
    field = FENCE.findall(section(doc(), "5. How people opt in"))[2]
    assert 20 <= len(field) <= 320
    assert is_excerpt(field, live_help_reply())
    assert not is_excerpt(field.replace("one text.", "a text."), live_help_reply())
    assert not is_excerpt(field.replace("Help:", "Help now:"), live_help_reply())


def test_a_30951_refusal_means_no_registration_and_no_toll_free() -> None:
    """TD-CON-16, CON-033, D-5278: after 30951 no toll-free fallback; web-and-email mode and the owner decides."""
    refused = " ".join(section(doc(), "8. If the campaign is refused").split())
    assert "If the refusal is error 30951" in refused
    assert "there is no registration and no toll-free fallback, because toll-free has the same ban (30468)" in refused
    assert "The service stays in web-and-email mode, and the choice goes back to the owner." in refused
    assert "refused for any other reason, then only with the owner's go" in refused


def test_toll_free_fallback_only_with_the_owners_go() -> None:
    """TD-CON-16, CON-033: same content and samples, never business notices, never around a suspension."""
    refused = " ".join(section(doc(), "8. If the campaign is refused").split())
    assert "only with the owner's go, the fallback is toll-free verification for the homeowner texts" in refused
    assert "with the same content, consent text and samples" in refused
    assert "never used for business notices without the written approval business texting needs" in refused
    assert "never used to get around a suspension of a working number" in refused
    assert "Until it is verified, the service runs in web-and-email mode:" in refused


def test_spanish_note_sample_and_review_items() -> None:
    """TD-TXT-26, TD-TERMS-01, TXT-043, TERMS-018, D-5278: both languages declared at once, and the review order."""
    spanish = " ".join(section(doc(), "7. Spanish").split())
    assert "The first registration declares English and Spanish, with the Spanish consent text as sample 5." in spanish
    assert "Languages: English and Spanish" in section(doc(), "2. Campaign settings")
    assert "Add the Spanish sample" not in doc()
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

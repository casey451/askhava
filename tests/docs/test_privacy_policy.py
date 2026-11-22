"""The privacy policy and SMS terms drafts hold every required line, and no draft claims that
phone numbers or opt-ins are shared, sold or transferred.

TD-TERMS-01, TD-PRIV-06, TD-PRIV-07, TD-TERMS-04, TD-WORD-07, TD-PRIV-12 (T112; TERMS-002 to
TERMS-004, TERMS-006 to TERMS-015, TERMS-018, TERMS-020, TERMS-025, TERMS-027, TERMS-031, WORD-007,
WORD-018, WORD-023, AI-018, PRIV-027, PRIV-031, TXT-038; D-492, D-2769, D-4119).

T112 owns `docs/legal/privacy-policy.md` and `docs/legal/sms-terms.md`; the homeowner and business
terms are T111's and only read here (TERMS-002 to TERMS-004, TERMS-006, TERMS-007 are checked in
full by `tests/docs/test_terms_clauses.py`; this file checks the clauses the SMS terms repeat and
that T111's headings are there). TERMS-014 (the registration order) is the account checklist's,
T122's. The texts quoted in the SMS terms are compared with the real templates in
`app/messaging/templates/en.json` (TERMS-013), with the brand from the brand file.

The TERMS-010 scan is D-4119's closed list, the same definition the reviewer uses:
1. Units: each "with ... removed" span is deleted, then the text is split into sentences at `.`, `!`
   or `?` before whitespace, at line breaks and at table `|`; "anyone (and its like) but" reads as
   "except". A sentence is in scope when it names a protected object, or holds it, them, these or
   those right after a sentence that names one (one sentence only; no chaining). An in-scope
   sentence splits into clauses at `;`, `:`, parentheses, a comma before and/but/so/yet, and at
   but, while or whereas.
2. A clause with a negation is skipped unless it also holds a lift word ("only" is not one).
3. A clause is a claim when it holds a standalone verb, or a third party and a party verb, or
   starts with an exception word and names a third party (negated or not).
4. Every word matches case-insensitively as a whole word; n't is the one suffix, affiliat* the one
   prefix. The scan fails closed: a true sentence it flags is reworded in T112's drafts.
Phrasing off these lists is a scan list note, logged as "sharing scan list: add <pattern>".
Fake data only.

Run: python3 -m pytest tests/docs/test_privacy_policy.py -q -p no:cacheprovider
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

import brand
import check_legal_docs

ROOT = Path(__file__).resolve().parents[2]
LEGAL = ROOT / "docs" / "legal"
POLICY = "privacy-policy.md"
SMS = "sms-terms.md"
SCANNED = (POLICY, SMS, "homeowner-terms.md", "business-terms.md")
I = re.IGNORECASE


def words(*alternatives: str) -> str:
    """One whole-word alternation of the listed words or phrases."""
    return r"\b(?:" + "|".join(alternatives) + r")\b"


# D-4119 (1): the units and the protected objects.
WITH_REMOVED = re.compile(r"\bwith\b[^.;|]*?\bremoved\b", I)
ANY_BUT = re.compile(r"\b(anyone|anybody|any one|no one|nobody|none|nothing|anything)\s+but\b", I)
OWNERS = r"your|their|homeowner's|homeowners'|homeowners|homeowner|customer's|customers'|customers|customer"
OBJECT = re.compile(
    words(
        r"phone numbers?",
        r"(?:mobile|cell) numbers?",
        r"mobile information",
        r"opt-ins?",
        r"texting permission",
        r"texting consent",
        r"consent",
    )
    + rf"|(?<!\w)(?:{OWNERS})\s+(?:phones?|numbers?)\b",
    I,
)
PRONOUN = re.compile(words("it", "them", "these", "those"), I)
CLAUSE_SPLIT = re.compile(r"[;:()]|,\s*(?=(?:and|but|so|yet)\b)|\b(?:but|while|whereas)\b", I)

# D-4119 (2): negations and lift words.
NEGATION = re.compile(words("never", "not", "no", "nor", "neither", "none", "nothing", "without") + r"|n't\b", I)
EXCEPTIONS = ("unless", "except", "excluding", "other than", "apart from", "aside from", "besides", "save for")
LIFT = re.compile(words("until", "till", *EXCEPTIONS, "before") + r"|(?<!\w)'til\b", I)
LEADING = re.compile(r"^[\W_]*" + words(*EXCEPTIONS), I)

# D-4119 (3): the verbs and the third parties.
STANDALONE = re.compile(
    words(
        "shares?", "shared", "sharing", "sells?", "selling", "sold", "rents?", "rented", "renting",
        "transfers?", "transferred", "transferring", "discloses?", "disclosed", "disclosing",
        "reveals?", "revealed", "revealing", "trades", "traded", "trading", "licenses", "licensed",
        "licensing", "exchanges", "exchanged", "exchanging",
    )
    + r"|\b(?:trade|license|exchange)\s+"
    + words("it", "them", "your", "their", "phones?", "mobile", "numbers?", "opt-ins?", "texting", "consent", "data"),
    I,
)
THIRD_PARTY = re.compile(
    words(
        "business(?:es)?", "contractors?", "pros?", "professionals?", r"service providers?",
        r"third[ -](?:party|parties)", "partners?", r"affiliat\w*", "marketers?", "advertisers?",
        r"(?:data )?brokers?", "sponsors?", r"other compan(?:y|ies)",
    ),
    I,
)
PARTY_VERB = re.compile(
    words(
        "give[sn]?", "giving", "gave", "pass(?:es|ed|ing)?", "sends?", "sending", "sent",
        "provide[sd]?", "providing", "hand(?:s|ed|ing)?", "forward(?:s|ed|ing)?", "release[sd]?",
        "releasing", "supply", "supplies", "supplied", "supplying", "deliver(?:s|ed|ing)?",
        r"made available", r"available to", "show(?:s|n|ed|ing)?", "display(?:s|ed|ing)?", "visible",
        "see[sn]?", "seeing", "saw", "view(?:s|ed|ing)?", "access(?:es|ed|ing)?", "have", "has", "had",
        "having", "use[sd]?", "using", "receive[sd]?", "receiving", "gets?", "getting", "got",
        "obtain(?:s|ed|ing)?", "learn(?:s|ed|ing)?", "know(?:s|n|ing)?", "knew", "tells?", "telling",
        "told", r"go to", r"goes to", r"going to", r"went to", "trade", "license", "exchange",
    ),
    I,
)


def units(text: str) -> list[str]:
    """D-4119 (1): the sentences of a document, "with ... removed" deleted, "anyone but" read as except."""
    text = text.replace("’", "'").replace("‘", "'")
    text = WITH_REMOVED.sub(" ", text)
    found = []
    for line in re.split(r"[\n|]", text):
        for sentence in re.split(r"(?<=[.!?])\s+", line):
            if sentence.strip():
                found.append(ANY_BUT.sub(r"\1 except", sentence.strip()))
    return found


def is_claim(clause: str) -> bool:
    """D-4119 (2), (3), TERMS-010: one clause says a protected object goes to someone."""
    if LEADING.search(clause) and THIRD_PARTY.search(clause):
        return True
    if NEGATION.search(clause) and not LIFT.search(clause):
        return False
    return bool(STANDALONE.search(clause) or (THIRD_PARTY.search(clause) and PARTY_VERB.search(clause)))


def sharing_claims(text: str) -> list[str]:
    """TERMS-010: every in-scope sentence with a claim clause (the pronoun rule does not chain)."""
    found = []
    named_before = False
    for sentence in units(text):
        named = bool(OBJECT.search(sentence))
        if named or (named_before and PRONOUN.search(sentence)):
            if any(is_claim(clause) for clause in CLAUSE_SPLIT.split(sentence) if clause):
                found.append(sentence)
        named_before = named
    return found


def fill(text: str) -> str:
    return brand.fill(text, data=brand.load(brand.BRAND_FILE))


def raw(name: str) -> str:
    return (LEGAL / name).read_text(encoding="utf-8")


def read(name: str) -> str:
    return check_legal_docs.normalize(raw(name))


# Must fail: both reviews' sentences word for word and D-4119's listed claims.
REVIEW_CLAIMS = (
    "Your number is made available to contractors.",
    "Contractors can see your phone number.",
    "Your phone number is shown to the business you pick.",
    "Your phone number is visible to businesses.",
    "Third parties can access your phone number.",
    "Businesses you pick will have your phone number.",
    "Partners may use your phone number for marketing.",
    "We may trade opt-in data with affiliates.",
    "We do not sell phone numbers; we license them to marketers.",
    "We never sell your number, but businesses can see it.",
    "Businesses don't see your phone number until you pick one.",
    "Your phone number is not shown to a business until it pays its introduction fee.",
    "Your phone number is never shared with businesses before you pick one.",
    "Your phone number is never shown to a business till it pays.",
    "After you pick, we pass your phone number to that business.",
    "We don't sell your phone number. We share it with the business you pick.",
    "We never share your phone number except with the business you pick.",
    "We never share your phone number (except with the business you pick).",
    "We do not share your phone number with anyone but the business you pick.",
    "No business gets your phone number unless you pick it.",
    "Your phone number is not given to anyone other than the business you pick.",
    "Your phone number is used by service providers to reach you.",
    "Your phone number goes to the business you pick.",
    "Your phone number is released to the pro you pick.",
    "Your phone number is revealed after the pick.",
    "The business you pick is told your phone number.",
    "Your phone number is kept safe. Businesses can see it.",
    # Fails closed: true, but it has a lift word, so the drafts word it otherwise.
    "Your phone number is never shared with any business, before or after you pick.",
    # Whole words: "no" is not found inside "know".
    "We know businesses see your phone number.",
)

# Must fail: one sentence per standalone verb group.
STANDALONE_CLAIMS = tuple(
    f"We {verb} your phone number."
    for verb in (
        "share", "shared", "are sharing", "sell", "are selling", "sold", "rent", "rented", "are renting",
        "transfer", "transferred", "are transferring", "disclose", "disclosed", "are disclosing",
        "reveal", "revealed", "are revealing", "trade", "license", "exchange",
    )
) + (
    "Laftly trades phone numbers.",
    "Your phone number is traded.",
    "Trading your phone number pays.",
    "Laftly licenses phone numbers.",
    "Your phone number is licensed.",
    "Licensing your phone number pays.",
    "Laftly exchanges phone numbers.",
    "Your phone number is exchanged.",
    "Exchanging your phone number pays.",
    "Your phone number is kept. We trade it.",
    "Your phone number is kept. We license them.",
    "Your phone number is kept. We exchange their numbers.",
    "We trade data on your phone number.",
    "We trade mobile numbers.",
    "We license opt-ins.",
    "We exchange texting permission.",
    "We trade consent.",
    "We license phones and your phone number.",
)

# Must fail: one sentence per party verb group (each with "businesses").
PARTY_CLAIMS = tuple(
    f"Your phone number {phrase} businesses."
    for phrase in (
        "is given to", "is in what we are giving", "we gave", "passes to", "is passed to", "is sent to",
        "we send to", "is in what we are sending", "is provided to", "is in what we are providing",
        "is handed to", "is forwarded to", "is released to", "is in what we are releasing",
        "is what we supply to", "is supplied to", "is delivered to", "is made available to",
        "is available to", "is shown to", "is displayed to", "is visible to", "is seen by",
        "is in what we saw with", "is viewed by", "is accessed by", "is used by", "is received by",
        "is obtained by", "is known by", "is told to", "goes to", "went to", "is going to",
        "is what we trade with", "is what we license to", "is what we exchange with",
    )
) + tuple(
    f"Businesses {verb} your phone number."
    for verb in (
        "see", "saw", "view", "access", "have", "has", "had", "use", "get", "got", "obtain",
        "learn", "learned", "know", "knew", "receive", "are having", "are using", "are getting",
        "are receiving", "are seeing", "are viewing", "are accessing", "are obtaining", "are learning",
        "are knowing",
    )
) + ("We tell businesses your phone number.", "We go to businesses with your phone number.")

# Must fail: one sentence per third-party word.
THIRD_PARTY_CLAIMS = tuple(
    f"Your phone number is given to {party}."
    for party in (
        "a business", "businesses", "a contractor", "contractors", "a pro", "pros", "a professional",
        "professionals", "a service provider", "service providers", "a third party", "third parties",
        "a third-party firm", "third-party firms", "a partner", "partners", "an affiliate", "affiliated firms",
        "a marketer", "marketers", "an advertiser", "advertisers", "a broker", "data brokers", "a sponsor",
        "sponsors", "some other company", "other companies",
    )
)

# Must fail: one sentence per object form.
OBJECT_CLAIMS = tuple(
    f"Businesses see {thing}."
    for thing in (
        "your phone number", "phone numbers", "mobile numbers", "your cell number", "mobile information",
        "your phone", "their phones", "the homeowner's number", "the homeowners' numbers",
        "homeowners phones", "the homeowner number", "the customer's phone", "the customers' numbers",
        "customers numbers", "the customer phone", "your opt-in", "opt-ins", "your texting permission",
        "texting consent", "your consent",
    )
) + (
    "Your phone number is locked. Businesses see it.",
    "Your phone number is locked. Businesses see them.",
    "Your phone number is locked. Businesses see these.",
    "Your phone number is locked. Businesses see those.",
)

# Must fail: one sentence per lift word, each with a negation.
LIFT_CLAIMS = tuple(
    f"Businesses never see your phone number {lift} the pick."
    for lift in (
        "until", "till", "'til", "’til", "before", "unless you allow it at", "except at",
        "excluding the one you pick at", "other than at", "apart from at", "aside from at",
        "besides at", "save for at",
    )
)

# Must pass: true sentences, D-4119's list and the whole-word cases.
TRUE_SENTENCES = (
    "Your number is never shared.",
    "Your first name and address go only to the business you pick.",
    "No business can see your phone number.",
    "We do not trade, license or exchange phone numbers or opt-in data.",
    "Businesses see your first name, never your phone number.",
    "Businesses never see your phone number, only your first name.",
    "Your phone number is not shared, only your first name and job address.",
    "Your phone number is never shared. Your first name goes to the business you pick.",
    "Calls go through a masked {brand} number, so the business never learns your phone number.",
    "Your phone number stays in the vault; we provide a masked line.",
    "Your phone number stays private because a business calls through the masked line.",
    "Your phone number and license plates stay out of photos.",
    "Your phone number stays private for jobs in that trade.",
    "Businesses see your first name and the job address.",
)

# Must pass: the spec's own sentences, from `scripts/check_legal_docs.py` and the spec text.
SPEC_SENTENCES = (
    "Your phone number and your texting permission are never shared with any business or marketer; "
    "our messaging vendor processes it to deliver texts, and your texts are never used for marketing.",
    check_legal_docs.TWILIO_LINE[1],
    "Text messaging originator opt-in data and consent will not be shared with any third parties.",
    "{brand} never gives your number to any business. If a business asks for it on the call, that's your choice.",
    "To sort your request and help word our questions, an AI service from Anthropic reads your job "
    "description after our software removes names, phone numbers, emails and street numbers. It also "
    "writes your job card, which you see before it goes out. The AI never sees your contact details, "
    "never gives prices or diagnoses, and never makes decisions about money or who gets your details.",
)
TERMS_025 = (
    next(text for req, text in check_legal_docs.REQUIRED[POLICY] if req == "TERMS-025")
    + " "
    + check_legal_docs.NO_PHONE[1]
)

PLANTED = REVIEW_CLAIMS + STANDALONE_CLAIMS + PARTY_CLAIMS + THIRD_PARTY_CLAIMS + OBJECT_CLAIMS + LIFT_CLAIMS


@pytest.mark.parametrize("sentence", PLANTED)
def test_planted_sharing_claim_fails(sentence: str) -> None:
    """TD-PRIV-07, TERMS-010, D-4119: each listed claim is caught."""
    assert sharing_claims(fill(sentence)), sentence


@pytest.mark.parametrize("sentence", TRUE_SENTENCES + SPEC_SENTENCES + (TERMS_025,))
def test_true_sentence_passes(sentence: str) -> None:
    """TD-PRIV-07, TERMS-010, TERMS-009, TERMS-025, TERMS-031, WORD-018, AI-018: no claim."""
    assert sharing_claims(fill(sentence)) == []


@pytest.mark.parametrize("name", SCANNED)
def test_drafts_make_no_sharing_claim(name: str) -> None:
    """TD-PRIV-07, TERMS-010: the privacy policy, SMS terms and both terms give no claim."""
    assert sharing_claims(raw(name)) == []


def test_units_and_clauses_follow_the_closed_list() -> None:
    """D-4119 (1): spans removed, sentences split at table cells and lines, "anyone but" read."""
    assert units("A with your phone number removed. B | C\nD") == ["A  .", "B", "C", "D"]
    assert units("Shared with anyone but you.") == ["Shared with anyone except you."]


# Required lines: (document, requirement, text with brand tokens), each its own assertion.
FLAG = "unless its safety systems flag it, when it may keep it up to <flag period>."
REQUIRED_LINES: tuple[tuple[str, str, str], ...] = (
    (POLICY, "WORD-023", check_legal_docs.HONEST[1]),
    (SMS, "WORD-023", check_legal_docs.HONEST[1]),
    (POLICY, "TERMS-008", "and, if you choose, your email, photos of the job and pictures of the look you want."),
    (POLICY, "TERMS-008", "The one business you pick | Your first name and the job address"),
    (POLICY, "TERMS-008", "Businesses that get your job card | The job card you saw"),
    (POLICY, "TERMS-008", "texting and calls (Twilio), email (Resend)"),
    (POLICY, "TERMS-008", "payments (Stripe)"),
    (POLICY, "TERMS-008", "letters for business identity checks (Lob)"),
    (POLICY, "TERMS-008", "automated text reading (Anthropic; see section 6)"),
    (POLICY, "TERMS-008", "an AI image service can make a concept picture from your written notes"),
    (POLICY, "TERMS-008", "You can see and change your details"),
    (POLICY, "TERMS-008", "We'll post any change here with its effective date"),
    (POLICY, "TERMS-008", "## 13. Do Not Track"),
    (POLICY, "TXT-038", "one-time codes sent by email (Twilio SendGrid, which sees the email address we send each code to)"),
    (POLICY, "PRIV-027", "website delivery and security (Cloudflare)"),
    (POLICY, "TERMS-009", check_legal_docs.TWILIO_LINE[1]),
    (SMS, "TERMS-009", check_legal_docs.TWILIO_LINE[1]),
    (POLICY, "TERMS-009", "Text messaging originator opt-in data and consent will not be shared with any third parties."),
    (SMS, "TERMS-025", check_legal_docs.NO_PHONE[1]),
    (POLICY, "TERMS-025", TERMS_025),
    (
        POLICY,
        "TERMS-020",
        "**Your phone number, locked (encrypted), with your texting permission and opt-out records:** 5 "
        "years after our last message to that number, only as proof of your texting choices.",
    ),
    (POLICY, "WORD-018", "{brand} never gives your number to any business. If a business asks for it on the call, that's your choice."),
    (POLICY, "WORD-007", "Your number is never shared. Your first name and address go only to the business you pick."),
    (POLICY, "AI-018", "Switch to the second, with the change dated, only on the day AI is switched on."),
    (POLICY, "AI-018", "The AI provider doesn't use that text to train its AI and deletes it within <days> days, " + FLAG),
    (
        POLICY,
        "AI-018",
        "Under our zero-retention agreement the AI provider doesn't keep that text or train on it, except "
        "text its safety systems flag, which it may keep up to <flag period>.",
    ),
    (POLICY, "AI-018", "If you send photos, the AI also looks at them to describe what they show on your job card."),
    (
        POLICY,
        "AI-018",
        "For some design jobs, an AI image service can make a concept picture from your written notes, "
        "never from your photos. It's labeled AI-generated.",
    ),
    (POLICY, "PRIV-031", '"Download my data" gives you a file of your requests'),
    (POLICY, "PRIV-031", "we text a code to the number on file and you type it back, so no one else can ask in your name."),
    (SMS, "TERMS-002", "You must be 18 or older."),
    (SMS, "TERMS-003", "Don't rely on {brand} in an emergency: call 911."),
    (SMS, "TERMS-011", "We send automated texts about that request only"),
    (
        SMS,
        "TERMS-011",
        "Businesses get no texts on this number; they are notified by email. A person who texts asking to "
        "join as a business gets one reply with the sign-up link.",
    ),
    (SMS, "TERMS-018", "We won't send this text in Spanish until a native Spanish speaker we pay has checked it:"),
    ("homeowner-terms.md", "TERMS-004", "**Limit of liability.**"),
    ("business-terms.md", "TERMS-006", "**Non-discrimination.**"),
    ("business-terms.md", "TERMS-007", "## 6. Card and charge authorization"),
)


@pytest.mark.parametrize(("name", "req", "text"), REQUIRED_LINES)
def test_required_line_is_in_its_draft(name: str, req: str, text: str) -> None:
    """TD-TERMS-01, TD-PRIV-06, TD-PRIV-07, TD-TERMS-04, TD-WORD-07, TD-PRIV-12: each line is there."""
    assert check_legal_docs.normalize(fill(text)) in read(name), f"{name} lacks {req}: {text}"


def test_privacy_policy_ai_off_sentence_and_no_banned_ai_wording() -> None:
    """TD-PRIV-06, TD-TERMS-04, AI-018: the switched-off and switched-on paragraphs; no banned AI wording."""
    text = read(POLICY)
    for req, line in check_legal_docs.REQUIRED[POLICY]:
        if req == "AI-018":
            assert check_legal_docs.normalize(fill(line)) in text
    for name in (POLICY, SMS):
        lowered = read(name).lower()
        assert "ai-powered" not in lowered and "no ai reads your messages today" not in lowered


def section(text: str, heading: str) -> str:
    """The text from a heading to the next heading of the same or a higher level."""
    level = heading.split(" ")[0]
    start = text.index(heading + "\n")
    rest = text[start + len(heading) + 1 :]
    stop = re.search(rf"^#{{1,{len(level)}}} ", rest, re.MULTILINE)
    return rest[: stop.start()] if stop else rest


def test_sharing_sentence_and_texting_section_are_separate() -> None:
    """TD-TERMS-04, TERMS-031, TERMS-025: the sharing sentence sits under "When you pick a business";
    the texting section carries the never-shared sentence and not the sharing sentence."""
    text = raw(POLICY)
    picked = check_legal_docs.normalize(section(text, "### When you pick a business"))
    texting = check_legal_docs.normalize(section(text, "## 5. Text messages"))
    assert check_legal_docs.normalize(fill(TERMS_025)) in picked
    assert (
        "Your phone number and your texting permission are never shared with any business or marketer; "
        "our messaging vendor processes it to deliver texts, and your texts are never used for marketing."
        in texting
    )
    assert "does {brand} give that one business".format(brand=fill("{brand}")) not in texting


TEMPLATES = json.loads((ROOT / "app" / "messaging" / "templates" / "en.json").read_text(encoding="utf-8"))


@pytest.mark.parametrize("key", ("consent", "opt_out_confirmation", "start_reply"))
def test_quoted_texts_are_the_real_templates(key: str) -> None:
    """TD-TERMS-01, TERMS-013: the texts the SMS terms quote are the templates sent, word for word."""
    assert "> " + fill(TEMPLATES[key]["text"]) in raw(SMS).splitlines()


BANNED_EVERYWHERE = (
    "connect you with", "connects homeowners with", "referral network", "referral service",
    "lead generation", "leads", "marketplace of providers", "commission",
)
BANNED_FEE_FRAMING = (
    "license fee", "licence", "data access", "access to homeowners", "right to use",
    "purchase of contact", "sale of information",
)


@pytest.mark.parametrize("name", (POLICY, SMS))
def test_drafts_use_no_banned_wording(name: str) -> None:
    """TD-WORD-07, WORD-023, TERMS-012, TERMS-027: no banned description or fee framing."""
    text = read(name)
    for phrase in BANNED_EVERYWHERE + BANNED_FEE_FRAMING:
        assert not re.search(rf"\b{re.escape(phrase)}\b", text, I), f"{name}: {phrase}"


@pytest.mark.parametrize("name", (POLICY, SMS))
def test_drafts_open_with_the_draft_note_and_pass_the_legal_check(name: str) -> None:
    """TD-TERMS-01, TERMS-015: each draft exists, opens with the draft note and version line, and
    `scripts/check_legal_docs.py` finds no error in it."""
    lines = raw(name).splitlines()
    assert lines[0] == "Draft prepared with AI research; not reviewed by a lawyer."
    assert check_legal_docs.VERSION_LINE.match(lines[1])
    assert [e for e in check_legal_docs.run(ROOT).errors if e.startswith(name)] == []

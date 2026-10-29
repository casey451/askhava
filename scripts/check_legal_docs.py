"""Check the legal document drafts in docs/legal/ against the spec (TERMS-028, D-389).

Fails (exit 1) unless:
- every document starts with the draft note and a version line with a date;
- each key phrase listed in REQUIRED appears word for word in the text of its
  spec requirement (so the phrase is the spec's own wording) and in its document;
- no document uses banned wording: the WORD-022/WORD-023 list from
  scripts/trace_spec.py, "vetted", "best", "#1", "top-rated", "lead(s)",
  "exclusive", "pre-vetted", "concierge", pest control, a personal name or
  "Hava" alone (WORD-021); spec phrases quoted to ban them are allowed;
- business documents never frame the fee as a license or data access (TERMS-027);
- homeowner documents never ask for a reply of OK or YES to agree (D-419);
- any arbitration text sits under a heading marked OPTIONAL and OFF.

Usage: python3 scripts/check_legal_docs.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trace_spec  # sibling script, imported after its folder is on the path

DOCS_DIR = Path("docs/legal")
NOTE = "Draft prepared with AI research; not reviewed by a lawyer."
VERSION_LINE = re.compile(
    r"^Version \d+\.\d+\b.*\b(?:January|February|March|April|May|June|July|August|"
    r"September|October|November|December) \d{1,2}, \d{4}\b"
)
DOCUMENTS = (
    "README.md",
    "homeowner-terms.md",
    "business-terms.md",
    "privacy-policy.md",
    "sms-terms.md",
    "business-email-notices.md",
    "accessibility-statement.md",
    "information-security-program.md",
    "incident-response-plan.md",
)
BUSINESS_DOCUMENTS = frozenset({"business-terms.md", "business-email-notices.md"})

HONEST = (
    "WORD-023",
    "a quote-request service; the business the homeowner chooses pays Ask Hava a set "
    "introduction fee",
)
LONG_STATEMENT = (
    "WORD-002",
    "Ask Hava is not a contractor and does not perform, bid on or supervise work. We pass "
    "along your request and show you quotes from independent local businesses, as they wrote "
    "them. You choose, and you hire the business directly.",
)
NO_PHONE = (
    "TERMS-025",
    "Ask Hava never gives your phone number to any business, and never sells or shares phone "
    "numbers or texting permission for marketing.",
)
TWILIO_LINE = (
    "TERMS-009",
    "No mobile information will be shared with third parties or affiliates for marketing or "
    "promotional purposes.",
)
NO_EMERGENCY_RELIANCE = ("WEB-022", "Don't rely on Ask Hava in an emergency: call 911.")
USUALLY = (
    "CON-018",
    "The Ask Hava owner gets HELP messages right away and usually replies within 2 days, or "
    "within 24 hours if your job is urgent.",
)
NO_CALLS = ("CON-019", "Ask Hava never places recorded or automated calls to you.")

REQUIRED: dict[str, tuple[tuple[str, str], ...]] = {
    "homeowner-terms.md": (
        HONEST,
        LONG_STATEMENT,
        (
            "TERMS-004",
            "You and the business make your own agreement; Ask Hava is not part of it and "
            "doesn't guarantee the work.",
        ),
        (
            "TERMS-004",
            "To the extent the law allows, Ask Hava's total liability to you for any claim about "
            "the service is limited to $100. This limit does not apply to gross negligence, "
            "intentional misconduct, or anything the law does not allow us to limit.",
        ),
        (
            "WEB-026",
            "The business you choose pays Ask Hava a set introduction fee ($25, $75 or $199, by "
            "quote size). You pay Ask Hava nothing.",
        ),
        ("PICK-016", "By picking, you agree to the Terms"),
        ("TERMS-004", "Limit of liability"),
        ("TERMS-001", "A.R.S. 44-7007"),
        ("TERMS-003", "Ask Hava is not an emergency service"),
        NO_EMERGENCY_RELIANCE,
        (
            "WORD-018",
            "Ask Hava never gives your number to any business. If a business asks for it on the "
            "call, that's your choice.",
        ),
        ("LIC-026", "showed Active on the ROC website when we checked on <date>. Check it yourself."),
        USUALLY,
    ),
    "business-terms.md": (
        HONEST,
        (
            "WORD-002",
            "Ask Hava is not a contractor. It passes along this request and the homeowner's "
            "choice. If the homeowner picks you, they hire you directly; Ask Hava doesn't direct, "
            "schedule, supervise or pay for the work.",
        ),
        (
            "TERMS-026",
            "Businesses on Ask Hava are independent. Ask Hava never directs, controls, schedules, "
            "supervises, inspects, warrants or pays for your work, never sets or changes your "
            "prices, and never negotiates for you or the homeowner. There is no subcontract, "
            "employment, agency, partnership or joint venture between you and Ask Hava. You and "
            "the homeowner make your own agreement, and Ask Hava is not part of it. The "
            "introduction fee pays only for the introduction service: a fixed amount by job size "
            "($25, $75 or $199), never a percentage or share of the job's price, and it doesn't "
            "depend on whether the work is done, finished or paid for. The refunds are the ones "
            "these terms list, and Ask Hava may also refund any charge at its own choice. No "
            "refund is owed because the work wasn't done or paid for.",
        ),
        (
            "TERMS-024",
            "As part of the introduction, Ask Hava gives you the homeowner's first name and the "
            "job address so you can contact them about this job. Use them only for this job and "
            "follow-up about it. No marketing lists, no resale.",
        ),
        ("TERMS-022", "If a charge was our mistake, it is refunded in full."),
        (
            "PICK-021",
            "Refund if you can't reach them: 2 call attempts at least 3 hours apart, inside their "
            "window, within 72 hours. They can be on different days.",
        ),
        (
            "FEE-015",
            "I authorize Ask Hava LLC to save this card with Stripe for introduction fees. I'm "
            "charged only when I press Confirm and pay for a homeowner's choice of my quote, and "
            "the fee is shown before I press it: $25, $75 or $199 by job size, ongoing services "
            "counted at 4 months and capped at $75. First win free if I join in the first 90 days "
            "or within 90 days after my trade opens. Refunds per the Terms. I can remove this "
            "card at any time.",
        ),
        ("WORD-017", "You quote once. No bidding rounds, no paid placement."),
        ("WORD-017", "Every card says how many businesses got it."),
        ("BIZ-030", "Repeat picks from the same homeowner are free for 180 days after the job closes."),
        ("BIZ-030", "A new fee never applies until you accept it."),
        (
            "WEB-005",
            "Launch offer: your first win is free, on any job size, when you join in the first 90 "
            "days or within 90 days after your trade opens.",
        ),
        NO_CALLS,
    ),
    "privacy-policy.md": (
        HONEST,
        LONG_STATEMENT,
        (
            "TERMS-025",
            "When you send a request, Ask Hava sends a description of the job, with your name, "
            "phone number, email and street address removed, to local businesses that do that kind "
            "of work. Only after you pick a business, and it confirms and pays its introduction "
            "fee, does Ask Hava give that one business your first name and the job address, plus "
            "your email if you chose to share it.",
        ),
        NO_PHONE,
        TWILIO_LINE,
        ("WORD-007", "Your first name and address go only to the business you pick."),
        ("WORD-007", "Your number is never shared"),
        (
            "AI-018",
            "Our texts are automated and come from templates, and no AI reads your messages "
            "right now.",
        ),
        (
            "AI-018",
            "To sort your request and help word our questions, an AI service from Anthropic reads "
            "your job description after our software removes names, phone numbers, emails and "
            "street numbers. It also writes your job card, which you see before it goes out. The "
            "AI never sees your contact details, never gives prices or diagnoses, and never makes "
            "decisions about money or who gets your details.",
        ),
        (
            "AI-018",
            "AI may also shorten a business's quote into a summary shown next to the business's "
            "own words.",
        ),
        (
            "PRIV-011",
            "We keep your phone number, locked, for 5 years only as proof of your texting "
            "permission and opt-out.",
        ),
        (
            "TERMS-021",
            "We'll tell you about any legal demand for your business's data unless the law "
            "forbids it.",
        ),
    ),
    "sms-terms.md": (
        HONEST,
        (
            "CON-005",
            "Texting Ask Hava, or giving us your mobile number in the chat on askhava.com and "
            "typing the code we send to it, is your OK to get automated texts about your own "
            "request.",
        ),
        ("PICK-016", "By picking, you agree to the Terms"),
        ("CON-003", "Free for you; the business you choose pays Ask Hava a set fee of $25 to $199, by quote size."),
        (
            "CON-031",
            "Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you "
            "hire the business you pick directly.",
        ),
        (
            "CON-009",
            "Ask Hava: You're unsubscribed. You won't get more texts or automated calls from Ask "
            "Hava at this number. To use Ask Hava again, text START.",
        ),
        (
            "CON-030",
            "Ask Hava: You can text us again. Send your request to start. Msg & data rates may "
            "apply. Reply HELP for help, STOP to opt out.",
        ),
        ("CON-018", "This number is text-only. If anyone is in danger, call 911."),
        ("CON-002", "Msg frequency varies"),
        ("CON-002", "Automated texts about your request"),
        ("CON-002", "Msg & data rates may apply"),
        USUALLY,
        NO_PHONE,
        TWILIO_LINE,
        NO_EMERGENCY_RELIANCE,
    ),
    "business-email-notices.md": (
        NO_CALLS,
        ("WEB-024", "Job requests and picks come by email. We don't text or call businesses."),
        (
            "SEC-028",
            "You get these emails because your business signed up for job requests from Ask Hava.",
        ),
        (
            "CON-020",
            "Ask Hava emails come only from addresses ending in askhava.com, never ask for your "
            "card or a code, and every button goes to askhava.com.",
        ),
        ("BIZ-048", "Ask Hava never texts or calls a business to recruit it."),
        (
            "BIZ-042",
            "Skip this and you may miss picks. A pick you don't confirm in 4 business hours may "
            "go to another business.",
        ),
        ("BIZ-045", "Delivered means your mail server accepted it; it may still be in Spam."),
    ),
    "accessibility-statement.md": (
        ("A11Y-020", "If you use a relay service, you can reach us by text or email."),
        ("A11Y-001", "WCAG 2.2"),
    ),
    "information-security-program.md": (("PRIV-002", "AES-GCM"),),
    "incident-response-plan.md": (
        ("OPS-021", "pausing texting and charging and preserving the logs"),
    ),
}
BANNED: tuple[tuple[re.Pattern[str], str], ...] = (
    *((pat, "WORD-022/WORD-023 wording") for pat, _ in trace_spec.NOT_A_CONTRACTOR),
    (re.compile(r"\b(?:pre-)?vetted\b", re.IGNORECASE), "banned word"),
    (re.compile(r"\bbest\b", re.IGNORECASE), "banned word"),
    (re.compile(r"#1\b"), "banned word"),
    (re.compile(r"\btop[- ]rated\b", re.IGNORECASE), "banned word"),
    (re.compile(r"\bleads?\b(?![- ](?:paint|pipes?|based)\b)", re.IGNORECASE), "banned word"),
    (re.compile(r"\bexclusive\b", re.IGNORECASE), "banned word"),
    (re.compile(r"\bconcierge\b", re.IGNORECASE), "banned word"),
    (re.compile(r"\bpests?\b", re.IGNORECASE), "pest control is never advertised (WEB-010)"),
    (re.compile(r"\bCasey\b"), "personal name (WORD-021)"),
    (re.compile(r"(?<!Ask )\bHava\b"), "'Hava' alone (WORD-021)"),
)
BUSINESS_BANNED: tuple[tuple[re.Pattern[str], str], ...] = tuple(
    (re.compile(p, re.IGNORECASE), "fee framed as a license or data access (TERMS-027)")
    for p in (
        r"\blicen[cs]e fee",
        r"\blicence\b",
        r"\bdata access\b",
        r"\baccess to homeowners\b",
        r"\bright to use\b",
        r"\bpurchase of contact\b",
        r"\bsale of information\b",
    )
)
HOMEOWNER_DOCUMENTS = frozenset({"homeowner-terms.md", "sms-terms.md", "privacy-policy.md"})
# D-419: texting in is the opt-in; no homeowner document asks for a reply of OK or YES to agree.
HOMEOWNER_BANNED: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(r"\b(?:reply|replying|replies|responda)\s+(?:OK|YES|S[IÍ])\b", re.IGNORECASE),
        "asks for a reply of OK or YES to agree (D-419)",
    ),
)
ARBITRATION = re.compile(r"arbitrat", re.IGNORECASE)
HEADING = re.compile(r"^#{1,6} ")


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    checked: int = 0


def normalize(text: str) -> str:
    """One space for any whitespace run, straight quotes."""
    text = text.replace("’", "'").replace("‘", "'")
    text = text.replace("“", '"').replace("”", '"')
    return re.sub(r"\s+", " ", text)


def spec_texts(root: Path) -> dict[str, str]:
    """Requirement ID to its normalized text (before "Source:")."""
    report = trace_spec.run(root)
    return {req.req_id: normalize(req.text) for req in report.requirements}


def check_header(name: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    if not lines or lines[0].strip() != NOTE:
        errors.append(f"{name}: first line must be the draft note {NOTE!r}")
    if len(lines) < 2 or not VERSION_LINE.match(lines[1].strip()):
        errors.append(f"{name}: second line must be 'Version <n.n> ... <Month d, yyyy>'")
    return errors


def check_phrases(name: str, body: str, specs: dict[str, str]) -> tuple[list[str], str]:
    """Each required phrase is in its spec requirement and in the document.

    Returns the errors and the document text with the verified phrases removed, so the
    banned-word scan doesn't fail on the spec's own quoted wording (D-360).
    """
    errors: list[str] = []
    rest = body
    for req_id, phrase in REQUIRED.get(name, ()):
        want = normalize(phrase)
        if want not in specs.get(req_id, ""):
            errors.append(f"{name}: phrase not in {req_id}'s spec text: {phrase[:60]!r}")
        if want not in body:
            errors.append(f"{name}: missing {req_id} wording: {phrase[:60]!r}")
        rest = rest.replace(want, " ")
    return errors, rest


def check_banned(name: str, text: str) -> list[str]:
    patterns = (
        BANNED
        + (BUSINESS_BANNED if name in BUSINESS_DOCUMENTS else ())
        + (HOMEOWNER_BANNED if name in HOMEOWNER_DOCUMENTS else ())
    )
    return [
        f"{name}: {label}: {match.group(0)!r}"
        for pat, label in patterns
        for match in [pat.search(text)]
        if match
    ]


def check_arbitration(name: str, lines: list[str]) -> list[str]:
    """Arbitration text only under a heading marked OPTIONAL and OFF, and nowhere before it."""
    in_optional = False
    for lineno, line in enumerate(lines, start=1):
        if HEADING.match(line):
            in_optional = "OPTIONAL" in line and "OFF" in line
            continue
        if ARBITRATION.search(line) and not in_optional and name != "README.md":
            return [f"{name}:{lineno}: arbitration text outside an OPTIONAL (OFF) section"]
    return []


def run(root: Path) -> Report:
    report = Report()
    docs = root / DOCS_DIR
    specs = spec_texts(root)
    for name in DOCUMENTS:
        path = docs / name
        if not path.is_file():
            report.errors.append(f"missing {DOCS_DIR.as_posix()}/{name}")
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        body = normalize("\n".join(lines))
        report.errors += check_header(name, lines)
        phrase_errors, rest = check_phrases(name, body, specs)
        report.errors += phrase_errors
        report.errors += check_banned(name, rest)
        report.errors += check_arbitration(name, lines)
        report.checked += 1
    extra = sorted(p.name for p in docs.glob("*.md") if p.name not in DOCUMENTS)
    report.errors += [f"{DOCS_DIR.as_posix()}/{n}: not listed in DOCUMENTS" for n in extra]
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (default: the parent of scripts/)",
    )
    args = parser.parse_args(argv)
    report = run(args.root)
    print(f"Legal documents checked: {report.checked}")
    print(f"Key phrases: {sum(len(v) for v in REQUIRED.values())}")
    print(f"Errors: {len(report.errors)}")
    for error in report.errors:
        print(f"ERROR {error}", file=sys.stderr)
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())

"""The account checklist holds every item its requirements name (T122).

TD-PRIV-09's inventory case (PRIV-024), TD-OPS-20 (OPS-042, OPS-043, OPS-048), TD-WEB-12
(TERMS-014, TERMS-032), TD-SEC-27's Postmaster step (SEC-050), TD-OPS-21's Stripe details
(OPS-044) and TD-TXT-28's CON-032 rows (CON-032), with AI-008's records and plan changes
D-2763, D-2769, D-2771, D-3891, D-4293, D-5278 and D-5585. Each check reads one section, so a
phrase moved to the wrong section fails. Fake data only.

Run: python3 -m pytest tests/docs/test_account_checklist.py -q -p no:cacheprovider
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

import brand

ROOT = Path(__file__).resolve().parents[2]
CHECKLIST = ROOT / "docs" / "account-checklist.md"
TEST_DESIGN = ROOT / "docs" / "test-design.md"
BRAND = brand.load()


def flat(body: str) -> str:
    """The text with line breaks and repeated spaces folded, so a phrase may wrap."""
    return re.sub(r"\s+", " ", body)


def section(title: str) -> str:
    """The folded body of the `##` or `###` heading starting with `title`."""
    source = CHECKLIST.read_text(encoding="utf-8")
    match = re.search(rf"^(#{{2,3}}) {re.escape(title)}.*$", source, re.M)
    assert match, f"no section {title!r}"
    level = len(match.group(1))
    end = re.compile(rf"^#{{1,{level}}} ", re.M).search(source, match.end())
    return flat(source[match.end() : end.start() if end else len(source)])


def missing(title: str, phrases: list[str]) -> list[str]:
    body = section(title)
    return [p for p in phrases if p not in body]


TWILIO_HOOK = [
    "`/webhooks/twilio/sms`",
    "connection-override fragment",
    "Webhooks configuration API",
    "Fallback URL",
    "same route",
    "voice webhook",
]
TWILIO_HOOK_PART1 = TWILIO_HOOK + [
    "most retries Twilio allows on 5xx responses, connection failures and read timeouts",
    "https://twilio.com/docs/usage/webhooks/webhooks-connection-overrides",
    "The fragment is not signed, so nothing in the app changes",
    "No console status-callback URL is needed",
]
SENDGRID = [
    "SendGrid",
    "domain authentication for the sending domain",
    "API key kept as a secret",
    "code email template",
    "Verify service's email integration",
]
DISPUTE_EVENTS = ["`charge.dispute.updated`", "`charge.dispute.funds_withdrawn`"]


def test_twilio_webhooks_both_numbers() -> None:
    """D-2763: the test and the live number each get the override, fallback and voice hook."""
    assert missing("Twilio (test number)", TWILIO_HOOK_PART1) == []
    assert missing("Twilio (live number)", TWILIO_HOOK + ["no console status-callback URL"]) == []


def test_sendgrid_verify_email_both_parts() -> None:
    """D-2769: SendGrid integration in Part 1 (before T131) and Part 2 (before Gate C)."""
    assert missing("SendGrid for Twilio Verify email", SENDGRID + ["before T131"]) == []
    assert missing("Twilio (live number)", SENDGRID + ["before Gate C"]) == []
    assert "SendGrid (email addresses" in section("Providers that receive personal data")


def test_stripe_dispute_events_both_parts() -> None:
    """D-2771: both endpoints enable the two dispute events beside the handled ones."""
    assert missing("Stripe (test mode)", DISPUTE_EVENTS + ["`payment_intent.succeeded`"]) == []
    assert missing("Stripe activation", DISPUTE_EVENTS) == []


def test_restore_and_backup_per_project() -> None:
    """D-4293: the restore service and the backup login per Railway project, in the inventory."""
    names = ["`ASKHAVA_RESTORE_ADMIN_URL`", "`ASKHAVA_BACKUP_PASSWORD`", "`ASKHAVA_BACKUP_DATABASE_URL`"]
    phrases = names + ["For each Railway project", "throwaway PostgreSQL 16 service",
                       "monthly test restore", "`askhava_backup` role its login", "on `worker` only"]
    assert missing("Railway test project", phrases) == []
    assert missing("Key inventory", names) == []


def test_ops042_before_any_live_key() -> None:
    """TD-OPS-20, OPS-042: every hardware-key, recovery and spend item, and break-glass."""
    phrases = [
        "before any live key", "Hardware-key second factor on the host, GitHub, Cloudflare, the "
        "registrar, Stripe, Twilio, Resend, the AI provider, Sentry and the storage account",
        "Registrar lock", "dedicated recovery email account used for nothing else",
        "Recovery codes and the key escrow on paper, apart from the password manager",
        "Twilio sub-accounts with restricted keys, geo permissions, Verify Fraud Guard and a spend cap",
        "Stripe restricted keys", "usage email alert on the Railway workspace at twice the projected "
        "monthly bill", "no hard usage limit", "rehearsed break-glass procedure",
    ]
    assert missing("Before any live key", phrases) == []
    glass = ["Loss of every owner factor", "Loss of host access", "Loss of `VAULT_KEY`"]
    assert missing("Break-glass", glass) == []


def test_ops043_release_record() -> None:
    """TD-OPS-20, OPS-043, D-5585: the signed release record after an Opus review."""
    phrases = [
        "release record that Casey signs off after an independent Opus security review of the release diff",
        "protected `release` branch that only Casey moves to that SHA",
        "automatic source connection stays off",
        "`protect-main` ruleset has no bypass actors, including administrators",
        "linked from the handover pack",
    ]
    assert missing("Release record", phrases) == []


def test_stripe_public_details() -> None:
    """TD-OPS-21, OPS-044: public details with the LLC mailing address, never a home one."""
    phrases = ["legal name, support address, support email and privacy URL",
               "LLC's mailing address, never a home address", "before the first live charge"]
    assert missing("Stripe activation", phrases) == []


CAMPAIGN = [
    "written eligibility question",
    "`docs/handoff/Twilio_question.md`",
    "only if its answer is unclear to one alternative provider",
    "files the written answer before any registration",
    "Registered only once the full site is live, including the request page, privacy policy and SMS terms",
    brand.fill("{domain} and the SMS terms show the {brand_legal} mailing address"),
    "`docs/legal/twilio-campaign.md`",
    "30977, 30984, 30988, 30965, 30987, 30966 and 30978",
    "public screenshot or evidence link of the call-to-action page",
    "no placeholder is left unfilled",
    "registered even if the business answer is no, except after a 30951 refusal",
    "A 30951 answer or refusal means no registration and web-and-email mode, back to Casey",
    "Business texting stays off unless the filed answer is yes",
]


def test_campaign_registration() -> None:
    """TD-WEB-12, TERMS-014, TERMS-032, D-5278, D-5585: the question before registration."""
    assert missing("Texting campaign registration", CAMPAIGN) == []
    body = section("Texting campaign registration")
    assert body.index("written eligibility question") < body.index("Registered only once")


@pytest.mark.parametrize("phrase", CAMPAIGN)
def test_campaign_phrase_deleted_fails(phrase: str) -> None:
    """D-5585: deleting any one campaign phrase turns the check red."""
    body = section("Texting campaign registration").replace(phrase, "")
    assert [p for p in CAMPAIGN if p not in body] == [phrase]


def test_ador_ruling_request() -> None:
    """TD-OPS-20, OPS-048: every point of the ADOR item."""
    phrases = ["A.R.S. 42-2101", brand.fill("in writing in {brand_legal}'s name and signed"),
               "TERMS-027 words", "business terms and a Confirm-page screenshot", "under no audit, protest or appeal",
               "filed with no other taxing authority", "FEE-046 set-aside stays until the request is filed"]
    assert missing("ADOR ruling request", phrases) == []


def test_postmaster_and_ai_records() -> None:
    """TD-SEC-27, SEC-050 and AI-008: Postmaster before business email; AI terms and limit."""
    phrases = ["Google Postmaster Tools", "before the first real business email",
               "before the AI switch first goes on", "not used for training",
               "flagged-content retention period", "Console spend limit of no more than 3 times"]
    assert missing("Email, AI and consent records", phrases) == []


CON032_ROWS = {"CON-008": "TD-CON-04", "CON-010": "TD-CON-32", "processed at once": "TD-CON-20",
               "CON-013": "TD-CON-05", "CON-009": "TD-CON-05", "one suppression list": "TD-CON-20",
               "CON-002": "TD-CON-01"}


def test_con032_rows_map_to_test_design() -> None:
    """TD-TXT-28, CON-032, D-3891: each row names a TD ID present in the test design."""
    body = section("Email, AI and consent records")
    design = TEST_DESIGN.read_text(encoding="utf-8")
    for item, td in CON032_ROWS.items():
        row = re.search(rf"\| [^|]*{re.escape(item)}[^|]* \| (TD-[A-Z]+-\d+) \|", body)
        assert row and row.group(1) == td, item
        assert f"| {td} |" in design, td
    assert "last re-checked November 21, 2026" in body
    assert "confirmed from the nightly before Gate C" in body


def test_key_inventory() -> None:
    """TD-PRIV-09, PRIV-024: every secret with owner, storage, paper escrow and trigger."""
    body = section("Key inventory")
    for name in ["`VAULT_KEY`", "`VAULT_INDEX_KEY`", "`ID_SECRET`", "the photo-link key", "the export key",
                 "vendor keys"]:
        assert re.search(rf"\| {re.escape(name)}[^|]* \| [^|]+ \| paper \| [^|]+ \|", body), name
    assert "Owner Casey" in body and "apart from the password manager" in body


def test_brand_from_brand_file() -> None:
    """D-3347: the checklist names the brand file's legal name and domain, never the old brand."""
    text = CHECKLIST.read_text(encoding="utf-8")
    assert BRAND["legal_name"] in text and BRAND["domain"] in text
    assert [word for word in BRAND["forbidden_alone"] if word in text] == []

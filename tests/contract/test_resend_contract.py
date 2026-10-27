"""Contract tests for stand_ins.resend, from Resend's public API reference (fake data only)."""

from __future__ import annotations

import base64
import hashlib
import hmac
from typing import Any

from stand_ins import resend

MAIL = {"from": "Ask Hava <hello@example.com>", "to": ["canary@example.com"], "subject": "Hi", "text": "Hello"}


def test_send_returns_id() -> None:
    fake = resend.FakeResend()
    status, body = fake.send(MAIL)
    assert status == 200 and list(body) == ["id"]
    assert len(body["id"]) == 36 and body["id"].count("-") == 4


def test_missing_fields_are_422_with_resend_envelope() -> None:
    fake = resend.FakeResend()
    for field in ("from", "to", "subject"):
        status, body = fake.send({k: v for k, v in MAIL.items() if k != field})
        assert status == 422 and body["name"] == "missing_required_field"
        assert body["statusCode"] == 422 and field in body["message"]
    status, body = fake.send({k: v for k, v in MAIL.items() if k != "text"})
    assert status == 422 and "html" in body["message"]


def test_invalid_address_and_string_recipient() -> None:
    fake = resend.FakeResend()
    assert fake.send({**MAIL, "to": ["nope"]})[1]["name"] == "invalid_parameter"
    assert fake.send({**MAIL, "to": "canary@example.com"})[0] == 200


def test_idempotency() -> None:
    fake = resend.FakeResend()
    one = fake.send(MAIL, "key-1")
    assert fake.send(MAIL, "key-1") == one
    assert len(fake.sent) == 1
    status, body = fake.send({**MAIL, "subject": "Other"}, "key-1")
    assert status == 409 and body["name"] == "invalid_idempotent_request"


def test_test_addresses_drive_events() -> None:
    fake = resend.FakeResend()
    for addr, event in [
        ("delivered@resend.dev", "email.delivered"),
        ("bounced@resend.dev", "email.bounced"),
        ("complained@resend.dev", "email.complained"),
    ]:
        fake.events.clear()
        fake.send({**MAIL, "to": [addr]})
        assert [e["type"] for e in fake.events] == ["email.sent", event]
    fake.events.clear()
    fake.send(MAIL)
    assert [e["type"] for e in fake.events] == ["email.sent"]


def test_svix_signature_matches_documented_scheme() -> None:
    secret = "whsec_" + base64.b64encode(b"fake-key").decode()
    fake = resend.FakeResend(secret)
    payload, headers = fake.signed_webhook({"type": "email.sent"}, 1_800_000_000)
    content = f"{headers['svix-id']}.1800000000.{payload}".encode()
    expected = "v1," + base64.b64encode(hmac.new(b"fake-key", content, hashlib.sha256).digest()).decode()
    assert headers["svix-signature"] == expected
    assert resend.verify(secret, headers["svix-id"], 1_800_000_000, payload, headers["svix-signature"])
    assert resend.verify(secret, headers["svix-id"], 1_800_000_000, payload, "v1,bad " + expected)
    assert not resend.verify(secret, headers["svix-id"], 1_800_000_000, payload + "x", expected)
    assert not resend.verify(secret, headers["svix-id"], 1_800_000_001, payload, expected)


def test_sent_record_keeps_request() -> None:
    fake = resend.FakeResend()
    fake.send(MAIL)
    record: dict[str, Any] = fake.sent[0]
    assert record["subject"] == "Hi" and record["to"] == ["canary@example.com"]

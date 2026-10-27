"""Fake Resend, written from Resend's public API reference.

POST /emails (from, to, subject, html or text) returns 200 {"id"}. Errors are
{"statusCode", "name", "message"}: 422 missing_required_field or invalid_parameter,
409 invalid_idempotent_request / concurrent_idempotent_requests. Test addresses
delivered@resend.dev, bounced@resend.dev, complained@resend.dev drive the events
email.delivered, email.bounced, email.complained. Webhooks are Svix-signed:
svix-signature "v1,<base64 HMAC-SHA256(key, '<id>.<timestamp>.<payload>')>" where the
key is the base64-decoded part of the secret after "whsec_".
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import re
from typing import Any

from stand_ins.common import IdempotencyStore, Reply, fake_id

EMAIL = re.compile(r"^[^@\s<>]+@[^@\s<>]+\.[^@\s<>]+$")
OUTCOMES = {
    "delivered@resend.dev": "email.delivered",
    "bounced@resend.dev": "email.bounced",
    "complained@resend.dev": "email.complained",
}
DEFAULT_SECRET = "whsec_" + base64.b64encode(b"fake-resend-signing-key").decode()


def error(status: int, name: str, message: str) -> Reply:
    return status, {"statusCode": status, "name": name, "message": message}


def _address(value: str) -> str:
    match = re.search(r"<([^>]+)>", value)
    return match.group(1) if match else value


def sign(secret: str, msg_id: str, timestamp: int, payload: str) -> str:
    key = base64.b64decode(secret.removeprefix("whsec_"))
    digest = hmac.new(key, f"{msg_id}.{timestamp}.{payload}".encode(), hashlib.sha256).digest()
    return "v1," + base64.b64encode(digest).decode()


def verify(secret: str, msg_id: str, timestamp: int, payload: str, header: str) -> bool:
    expected = sign(secret, msg_id, timestamp, payload)
    return any(hmac.compare_digest(expected, part) for part in header.split(" "))


class FakeResend:
    def __init__(self, secret: str = DEFAULT_SECRET) -> None:
        self.secret = secret
        self.sent: list[dict[str, Any]] = []
        self.events: list[dict[str, Any]] = []
        self._idem = IdempotencyStore()

    def send(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        state, reply = self._idem.lookup(idempotency_key, params)
        if state == "replay" and reply is not None:
            return reply
        if state == "conflict":
            return error(
                409, "invalid_idempotent_request", "Same idempotency key used with a different request payload."
            )
        return self._idem.store(idempotency_key, params, self._send_new(params))

    def _send_new(self, params: dict[str, Any]) -> Reply:
        for field in ("from", "to", "subject"):
            if not params.get(field):
                return error(422, "missing_required_field", f"Missing `{field}` field.")
        if not params.get("html") and not params.get("text"):
            return error(422, "missing_required_field", "Missing `html` or `text` field.")
        recipients = [params["to"]] if isinstance(params["to"], str) else list(params["to"])
        if not EMAIL.match(_address(str(params["from"]))) or not all(EMAIL.match(_address(r)) for r in recipients):
            return error(422, "invalid_parameter", "Invalid `to` or `from` field.")
        email_id = fake_id("", len(self.sent) + 1, 32)
        email_id = "-".join([email_id[:8], email_id[8:12], email_id[12:16], email_id[16:20], email_id[20:32]])
        self.sent.append({"id": email_id, "to": recipients, **params})
        self._record("email.sent", email_id, recipients)
        for rcpt in recipients:
            if rcpt in OUTCOMES:
                self._record(OUTCOMES[rcpt], email_id, [rcpt])
        return 200, {"id": email_id}

    def _record(self, kind: str, email_id: str, recipients: list[str]) -> None:
        self.events.append({"type": kind, "data": {"email_id": email_id, "to": recipients}})

    def signed_webhook(self, event: dict[str, Any], timestamp: int) -> tuple[str, dict[str, str]]:
        payload = json.dumps(event, sort_keys=True)
        msg_id = fake_id("msg_", len(self.events), 16)
        headers = {
            "svix-id": msg_id,
            "svix-timestamp": str(timestamp),
            "svix-signature": sign(self.secret, msg_id, timestamp, payload),
        }
        return payload, headers

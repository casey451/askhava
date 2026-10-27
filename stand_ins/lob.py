"""Fake Lob, written from Lob's public API reference.

POST /v1/letters needs to and from (name, address_line1, address_city, address_state,
address_zip), file (HTML or URL) and color; returns 200 with a letter object (id
"ltr_...", object "letter"). Errors: {"error": {"message", "status_code", "code"}}, 422
for validation and 409 for an Idempotency-Key reused with a different body. Test keys
(prefix test_) never mail anything; this fake behaves as a test key. Tracking events
walk letter.created, letter.rendered_pdf, letter.mailed, letter.in_transit_local,
letter.delivered. Webhooks: lob-signature = hex HMAC-SHA256(secret, "<timestamp>.<body>")
with lob-signature-timestamp.
"""

from __future__ import annotations

import hashlib
import hmac
import json
from typing import Any

from stand_ins.common import IdempotencyStore, Reply, fake_id

ADDRESS_FIELDS = ("name", "address_line1", "address_city", "address_state", "address_zip")
TRACKING = ("letter.created", "letter.rendered_pdf", "letter.mailed", "letter.in_transit_local", "letter.delivered")


def error(status: int, message: str, code: str = "invalid") -> Reply:
    return status, {"error": {"message": message, "status_code": status, "code": code}}


def sign(secret: str, timestamp: str, body: str) -> str:
    return hmac.new(secret.encode(), f"{timestamp}.{body}".encode(), hashlib.sha256).hexdigest()


def verify(secret: str, timestamp: str, body: str, header: str) -> bool:
    return hmac.compare_digest(sign(secret, timestamp, body), header)


def _address_problem(label: str, address: Any) -> str | None:
    if not isinstance(address, dict):
        return f"{label} is required"
    for field in ADDRESS_FIELDS:
        if not address.get(field):
            return f"{label}.{field} is required"
    return None


class FakeLob:
    def __init__(self, secret: str = "fake-lob-secret") -> None:
        self.secret = secret
        self.letters: dict[str, dict[str, Any]] = {}
        self.events: list[dict[str, Any]] = []
        self._idem = IdempotencyStore()

    def create_letter(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        state, reply = self._idem.lookup(idempotency_key, params)
        if state == "replay" and reply is not None:
            return reply
        if state == "conflict":
            return error(409, "Idempotency key was already used with a different request body.", "conflict")
        return self._idem.store(idempotency_key, params, self._create(params))

    def _create(self, params: dict[str, Any]) -> Reply:
        for label in ("to", "from"):
            problem = _address_problem(label, params.get(label))
            if problem:
                return error(422, problem)
        if not params.get("file"):
            return error(422, "file is required")
        if not isinstance(params.get("color"), bool):
            return error(422, "color is required")
        letter_id = fake_id("ltr_", len(self.letters) + 1, 16)
        letter = {
            "id": letter_id,
            "object": "letter",
            "description": params.get("description"),
            "to": params["to"],
            "from": params["from"],
            "color": params["color"],
            "double_sided": True,
            "mail_type": "usps_first_class",
            "carrier": "USPS",
            "tracking_events": [],
            "expected_delivery_date": "2026-11-12",
            "url": f"https://assets.example.com/{letter_id}.pdf",
        }
        self.letters[letter_id] = letter
        self._emit(letter_id, TRACKING[0])
        return 200, dict(letter)

    def _emit(self, letter_id: str, kind: str) -> None:
        self.letters[letter_id]["tracking_events"].append({"name": kind})
        self.events.append({"event_type": {"id": kind}, "body": {"id": letter_id}})

    def advance(self, letter_id: str) -> str:
        """Move a letter to its next tracking state; returns the event name."""
        done = len(self.letters[letter_id]["tracking_events"])
        if done >= len(TRACKING):
            raise ValueError("letter already delivered")
        self._emit(letter_id, TRACKING[done])
        return TRACKING[done]

    def signed_webhook(self, event: dict[str, Any], timestamp: str) -> tuple[str, dict[str, str]]:
        body = json.dumps(event, sort_keys=True)
        return body, {"lob-signature": sign(self.secret, timestamp, body), "lob-signature-timestamp": timestamp}

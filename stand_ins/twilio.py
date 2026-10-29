"""Fake Twilio Messaging and Verify, written from Twilio's public API reference.

Messages: POST /2010-04-01/Accounts/{AccountSid}/Messages.json (form fields To, From,
Body), 201 with a message resource, errors as {code, message, more_info, status}.
Magic numbers (Twilio test credentials docs): From +15005550006 is valid; To
+15005550001 invalid (21211), +15005550002 cannot be routed (21612), +15005550003 no
permission (21408), +15005550004 blocked (21610), +15005550009 cannot receive SMS (21614).
Opt-out: STOP, STOPALL, UNSUBSCRIBE, CANCEL, END, QUIT, REVOKE, OPTOUT block later sends (21610) until
START, YES or UNSTOP. Webhooks carry X-Twilio-Signature: base64(HMAC-SHA1(auth token,
URL + sorted key+value pairs)). Verify: POST /v2/Services/{sid}/Verifications and
/VerificationCheck.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import math
import re
from typing import Any

from stand_ins.common import Reply, fake_id

VALID_FROM = "+15005550006"
MAGIC_TO = {
    "+15005550001": (21211, "The 'To' number {to} is not a valid phone number."),
    "+15005550002": (
        21612,
        "The 'To' phone number: {to}, is not currently reachable using the 'From' phone number: {frm} via SMS.",
    ),
    "+15005550003": (
        21408,
        "Permission to send an SMS has not been enabled for the region indicated by the 'To' number: {to}.",
    ),
    "+15005550004": (21610, "Attempt to send to unsubscribed recipient"),
    "+15005550009": (21614, "'To' number is not a valid mobile number"),
}
OPT_OUT = {"STOP", "STOPALL", "UNSUBSCRIBE", "CANCEL", "END", "QUIT", "REVOKE", "OPTOUT"}
OPT_IN = {"START", "YES", "UNSTOP"}
E164 = re.compile(r"^\+[1-9]\d{7,14}$")
GSM_BASIC = (
    "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡"
    "ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà"
)
GSM_EXTENDED = "^{}\\[~]|€"
MAX_BODY = 1600
MAX_CHECK_ATTEMPTS = 5


def error(code: int, message: str, status: int = 400) -> Reply:
    return status, {
        "code": code,
        "message": message,
        "more_info": f"https://www.twilio.com/docs/errors/{code}",
        "status": status,
    }


def segment_count(body: str) -> int:
    """GSM-7: 160 per segment (153 when concatenated); otherwise UCS-2: 70 (67)."""
    if all(c in GSM_BASIC or c in GSM_EXTENDED for c in body):
        units = sum(2 if c in GSM_EXTENDED else 1 for c in body)
        return 1 if units <= 160 else math.ceil(units / 153)
    return 1 if len(body) <= 70 else math.ceil(len(body) / 67)


def compute_signature(auth_token: str, url: str, params: dict[str, str]) -> str:
    data = url + "".join(k + params[k] for k in sorted(params))
    digest = hmac.new(auth_token.encode(), data.encode(), hashlib.sha1).digest()
    return base64.b64encode(digest).decode()


def validate_signature(auth_token: str, url: str, params: dict[str, str], header: str) -> bool:
    return hmac.compare_digest(compute_signature(auth_token, url, params), header)


class FakeTwilio:
    def __init__(self, account_sid: str = "AC" + "0" * 32, auth_token: str = "fake-auth-token") -> None:
        self.account_sid = account_sid
        self.auth_token = auth_token
        self.messages: list[dict[str, Any]] = []
        self.inbound: list[dict[str, str]] = []
        self.opted_out: set[tuple[str, str]] = set()
        self.verifications: dict[str, dict[str, Any]] = {}
        self.callbacks: list[dict[str, str]] = []

    def create_message(self, to: str, frm: str, body: str) -> Reply:
        if not body:
            return error(21602, "Message body is required.")
        if len(body) > MAX_BODY:
            return error(21617, "The concatenated message body exceeds the 1600 character limit")
        if to in MAGIC_TO:
            code, text = MAGIC_TO[to]
            return error(code, text.format(to=to, frm=frm))
        if not E164.match(to):
            return error(21211, f"The 'To' number {to} is not a valid phone number.")
        if frm != VALID_FROM:
            return error(21606, f"The From phone number {frm} is not a valid, SMS-capable Twilio phone number.")
        if (frm, to) in self.opted_out:
            return error(21610, "Attempt to send to unsubscribed recipient")
        msg = self._resource(to, frm, body)
        self.messages.append(msg)
        return 201, dict(msg)

    def _resource(self, to: str, frm: str, body: str) -> dict[str, Any]:
        sid = fake_id("SM", len(self.messages) + 1, 32)
        return {
            "sid": sid,
            "account_sid": self.account_sid,
            "to": to,
            "from": frm,
            "body": body,
            "status": "queued",
            "num_segments": str(segment_count(body)),
            "num_media": "0",
            "direction": "outbound-api",
            "error_code": None,
            "error_message": None,
            "uri": f"/2010-04-01/Accounts/{self.account_sid}/Messages/{sid}.json",
        }

    def status_callback(self, sid: str, status: str, error_code: int | None = None) -> dict[str, str]:
        """Move a message to sent, delivered, undelivered or failed; returns the webhook params."""
        msg = next(m for m in self.messages if m["sid"] == sid)
        msg["status"] = status
        params = {"MessageSid": sid, "MessageStatus": status, "To": msg["to"], "From": msg["from"]}
        if error_code is not None:
            msg["error_code"] = error_code
            params["ErrorCode"] = str(error_code)
        self.callbacks.append(params)
        return params

    def receive(self, frm: str, to: str, body: str) -> dict[str, str]:
        """A homeowner texts us; returns the webhook params (Twilio posts these form fields)."""
        word = body.strip().upper()
        if word in OPT_OUT:
            self.opted_out.add((to, frm))
        elif word in OPT_IN:
            self.opted_out.discard((to, frm))
        params = {
            "MessageSid": fake_id("SM", 1000 + len(self.inbound), 32),
            "AccountSid": self.account_sid,
            "From": frm,
            "To": to,
            "Body": body,
            "NumMedia": "0",
        }
        self.inbound.append(params)
        return params

    def signed(self, url: str, params: dict[str, str]) -> tuple[dict[str, str], str]:
        return params, compute_signature(self.auth_token, url, params)

    def start_verification(self, to: str, channel: str = "sms") -> Reply:
        if not E164.match(to):
            return error(60200, "Invalid parameter: To", 400)
        if channel not in ("sms", "call", "email"):
            return error(60200, "Invalid parameter: Channel", 400)
        code = f"{sum(map(ord, to)) % 1000000:06d}"
        sid = fake_id("VE", len(self.verifications) + 1, 32)
        self.verifications[to] = {"sid": sid, "code": code, "attempts": 0, "status": "pending"}
        return 201, {"sid": sid, "to": to, "channel": channel, "status": "pending", "valid": False}

    def check_verification(self, to: str, code: str) -> Reply:
        record = self.verifications.get(to)
        if record is None or record["status"] == "approved":
            return error(20404, "The requested resource was not found", 404)
        if record["attempts"] >= MAX_CHECK_ATTEMPTS:
            return error(60202, "Max check attempts reached", 429)
        record["attempts"] += 1
        ok = hmac.compare_digest(record["code"], code)
        record["status"] = "approved" if ok else "pending"
        return 200, {"sid": record["sid"], "to": to, "status": record["status"], "valid": ok}

    def last_code(self, to: str) -> str:
        """Test hook standing in for the phone that would receive the code."""
        return str(self.verifications[to]["code"])

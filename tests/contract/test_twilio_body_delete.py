"""TD-PRIV-19 (PRIV-009, PRIV-027, TXT-020; T275, D-1841): the Twilio stand-in answers the body delete.

Twilio documents removing a message's text as an update of the Message with an empty `Body`
(redaction): the record stays, its body is "" from then on, and a repeat answers the same 200; an
unknown sid is 404 (20404). `adapters::SmsBodyDelete::delete_message_body` makes exactly this call.
Cases run over a real socket against `stand_ins/server.py`, as the Rust adapter does. Fake data only.
"""

from __future__ import annotations

import base64
import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlencode

import pytest

from stand_ins import server, twilio

HOMEOWNER = "+19285550142"
OURS = twilio.VALID_FROM
AC = "AC" + "0" * 32
ROOT = f"/twilio/2010-04-01/Accounts/{AC}/Messages"
AUTH = "Basic " + base64.b64encode(f"{AC}:fake-auth-token".encode()).decode()


@pytest.fixture()
def base() -> Iterator[str]:
    srv = server.serve()
    yield f"http://127.0.0.1:{srv.server_address[1]}"
    srv.shutdown()
    srv.server_close()


def call(base: str, method: str, path: str, form: dict[str, str] | None = None, auth: bool = True) -> tuple[int, Any]:
    data = urlencode(form).encode() if form is not None else None
    headers = {"authorization": AUTH} if auth else {}
    req = urllib.request.Request(base + path, data, headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, json.loads(err.read())


def sent(base: str) -> str:
    status, msg = call(base, "POST", ROOT + ".json", {"To": HOMEOWNER, "From": OURS, "Body": "Ask Hava: fake text"})
    assert status == 201
    return str(msg["sid"])


def test_redaction_empties_the_body_and_keeps_the_record(base: str) -> None:
    sid = sent(base)
    status, msg = call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""})
    assert status == 200 and msg["sid"] == sid and msg["body"] == ""
    assert msg["to"] == HOMEOWNER and msg["status"] == "queued"
    status, msg = call(base, "GET", f"{ROOT}/{sid}.json")
    assert status == 200 and msg["body"] == ""
    status, listed = call(base, "GET", ROOT + ".json")
    assert [m["body"] for m in listed["messages"] if m["sid"] == sid] == [""]


def test_a_repeat_answers_as_twilio_does(base: str) -> None:
    sid = sent(base)
    first = call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""})
    again = call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""})
    assert first == again and again[0] == 200 and again[1]["body"] == ""


def test_a_received_text_body_is_removed_too(base: str) -> None:
    text = {"from": HOMEOWNER, "to": OURS, "body": "fake homeowner words", "url": "https://example.com/sms"}
    json_type = {"content-type": "application/json"}
    req = urllib.request.Request(base + "/_stand_in/twilio/inbound", json.dumps(text).encode(), json_type)
    with urllib.request.urlopen(req, timeout=5) as resp:
        sid = json.loads(resp.read())["params"]["MessageSid"]
    assert call(base, "GET", f"{ROOT}/{sid}.json")[1]["body"] == "fake homeowner words"
    status, msg = call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""})
    assert status == 200 and msg["direction"] == "inbound" and msg["from"] == HOMEOWNER
    assert call(base, "GET", f"{ROOT}/{sid}.json")[1]["body"] == ""


def test_unknown_sid_auth_and_non_empty_body_are_refused(base: str) -> None:
    missing = "SM" + "f" * 32
    status, err = call(base, "POST", f"{ROOT}/{missing}.json", {"Body": ""})
    assert status == 404 and err["code"] == 20404
    assert call(base, "GET", f"{ROOT}/{missing}.json")[0] == 404
    sid = sent(base)
    status, err = call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""}, auth=False)
    assert status == 401 and err["code"] == 20003
    assert call(base, "POST", f"{ROOT}/{sid}.json", {"Body": "new words"})[0] == 400
    assert call(base, "GET", f"{ROOT}/{sid}.json")[1]["body"] == "Ask Hava: fake text"

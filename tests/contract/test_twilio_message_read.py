"""TD-TXT-35 contract half (TXT-012, TXT-020; T336, D-2307): the Twilio stand-in answers the body read.

`adapters::SmsBodyRead::fetch_message_body` sends `GET Messages/{sid}.json` and reads `from`, `to`
and `body` from Twilio's Message resource; an empty body (redacted on day 31 or by `Delete my
data`) or a 404 with code 20404 is `Gone`. These cases pin that shape over a real socket against
`stand_ins/server.py` (D-2156 serves the GET; the stand-in gained nothing). Fake data only.
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

HOMEOWNER = "+19285550143"
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


def received(base: str, words: str) -> str:
    """She texts us through the stand-in; returns the inbound MessageSid."""
    text = {"from": HOMEOWNER, "to": OURS, "body": words, "url": "https://example.com/sms"}
    json_type = {"content-type": "application/json"}
    req = urllib.request.Request(base + "/_stand_in/twilio/inbound", json.dumps(text).encode(), json_type)
    with urllib.request.urlopen(req, timeout=5) as resp:
        return str(json.loads(resp.read())["params"]["MessageSid"])


def test_td_txt_35_the_body_read_has_the_numbers_and_the_body(base: str) -> None:
    sid = received(base, "fake words about a dripping tap")
    status, msg = call(base, "GET", f"{ROOT}/{sid}.json")
    assert status == 200
    assert msg["sid"] == sid and msg["direction"] == "inbound"
    assert msg["from"] == HOMEOWNER and msg["to"] == OURS
    assert msg["body"] == "fake words about a dripping tap"


def test_td_txt_35_a_redacted_body_reads_empty(base: str) -> None:
    sid = received(base, "fake words to redact")
    assert call(base, "POST", f"{ROOT}/{sid}.json", {"Body": ""})[0] == 200
    status, msg = call(base, "GET", f"{ROOT}/{sid}.json")
    assert status == 200 and msg["body"] == "" and msg["from"] == HOMEOWNER


def test_td_txt_35_an_unknown_message_is_404_with_20404(base: str) -> None:
    status, err = call(base, "GET", f"{ROOT}/SM{'e' * 32}.json")
    assert status == 404 and err["code"] == 20404


def test_td_txt_35_the_read_needs_the_account_credentials(base: str) -> None:
    sid = received(base, "fake private words")
    status, err = call(base, "GET", f"{ROOT}/{sid}.json", auth=False)
    assert status == 401 and err["code"] == 20003
    assert "fake private words" not in json.dumps(err)

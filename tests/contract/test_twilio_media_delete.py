"""INT-044, SEC-032 (T794, D-5251): the Twilio stand-in answers the picture-text media delete.

`adapters::SmsMediaDelete::delete_message_media` sends DELETE
`/2010-04-01/Accounts/{AC}/Messages/{MM}/Media/{ME}.json` under Basic auth and takes 204 and 404 (already
gone) as done. After the delete the media's bytes and its resource answer 404. Cases run over a real
socket against `stand_ins/server.py`, as the Rust adapter does. Fake data only.
"""

from __future__ import annotations

import base64
import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from urllib.parse import urlsplit

import pytest

from stand_ins import server, twilio

HOMEOWNER = "+19285550142"
OURS = twilio.VALID_FROM
AC = "AC" + "0" * 32
AUTH = "Basic " + base64.b64encode(f"{AC}:fake-auth-token".encode()).decode()


@pytest.fixture()
def base() -> Iterator[str]:
    srv = server.serve()
    yield f"http://127.0.0.1:{srv.server_address[1]}"
    srv.shutdown()
    srv.server_close()


def call(url: str, method: str) -> tuple[int, bytes]:
    req = urllib.request.Request(url, headers={"authorization": AUTH}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.read()


def picture_text(base: str) -> str:
    """A fake homeowner picture text; returns its media URL as Twilio posts it."""
    text = {"from": HOMEOWNER, "to": OURS, "body": "", "media": ["image/jpeg"], "url": "https://example.com/sms"}
    headers = {"content-type": "application/json"}
    req = urllib.request.Request(base + "/_stand_in/twilio/inbound", json.dumps(text).encode(), headers)
    with urllib.request.urlopen(req, timeout=5) as resp:
        return str(json.loads(resp.read())["params"]["MediaUrl0"])


def test_the_media_path_has_the_shape_the_adapter_parses(base: str) -> None:
    parts = urlsplit(picture_text(base)).path.split("/")[-7:]
    assert parts[:3] == ["2010-04-01", "Accounts", AC]
    assert parts[3] == "Messages" and parts[4].startswith("MM") and len(parts[4]) == 34
    assert parts[5] == "Media" and parts[6].startswith("ME") and len(parts[6]) == 34


def test_a_delete_answers_204_and_the_media_is_gone(base: str) -> None:
    url = picture_text(base)
    assert call(url, "GET")[0] == 200
    assert call(url + ".json", "GET")[0] == 200
    assert call(url + ".json", "DELETE")[0] == 204
    assert call(url, "GET")[0] == 404
    assert call(url + ".json", "GET")[0] == 404


def test_a_repeat_delete_answers_404_which_the_adapter_takes_as_done(base: str) -> None:
    url = picture_text(base)
    assert call(url + ".json", "DELETE")[0] == 204
    status, body = call(url + ".json", "DELETE")
    assert status == 404 and json.loads(body)["code"] == 20404

"""Contract tests for stand_ins.lob, from Lob's public API reference (fake data only)."""

from __future__ import annotations

import hashlib
import hmac
from typing import Any

import pytest

from stand_ins import lob

ADDRESS = {"name": "Zelda Canary", "address_line1": "1 Test Way", "address_city": "Lake Havasu City",
           "address_state": "AZ", "address_zip": "86403"}
LETTER = {"description": "identity letter", "to": ADDRESS, "from": {**ADDRESS, "name": "Ask Hava"},
          "file": "<html>Code 000000</html>", "color": False}


def test_create_letter_shape() -> None:
    fake = lob.FakeLob()
    status, letter = fake.create_letter(LETTER)
    assert status == 200 and letter["object"] == "letter" and letter["id"].startswith("ltr_")
    assert letter["mail_type"] == "usps_first_class" and letter["carrier"] == "USPS"
    assert letter["to"]["address_zip"] == "86403" and "expected_delivery_date" in letter
    assert letter["tracking_events"] == [{"name": "letter.created"}]


@pytest.mark.parametrize("missing", ["to", "from", "file", "color"])
def test_required_fields_are_422(missing: str) -> None:
    status, body = lob.FakeLob().create_letter({k: v for k, v in LETTER.items() if k != missing})
    assert status == 422 and body["error"]["status_code"] == 422 and missing in body["error"]["message"]
    assert set(body["error"]) == {"message", "status_code", "code"}


def test_address_fields_checked() -> None:
    bad: dict[str, Any] = {**LETTER, "to": {k: v for k, v in ADDRESS.items() if k != "address_zip"}}
    status, body = lob.FakeLob().create_letter(bad)
    assert status == 422 and "to.address_zip" in body["error"]["message"]


def test_idempotency_key() -> None:
    fake = lob.FakeLob()
    first = fake.create_letter(LETTER, "k")
    assert fake.create_letter(LETTER, "k") == first and len(fake.letters) == 1
    status, body = fake.create_letter({**LETTER, "description": "other"}, "k")
    assert status == 409 and body["error"]["code"] == "conflict"


def test_tracking_walk_ends_at_delivered() -> None:
    fake = lob.FakeLob()
    letter_id = fake.create_letter(LETTER)[1]["id"]
    walked = [fake.advance(letter_id) for _ in range(4)]
    assert walked == ["letter.rendered_pdf", "letter.mailed", "letter.in_transit_local", "letter.delivered"]
    with pytest.raises(ValueError):
        fake.advance(letter_id)


def test_signature_matches_documented_scheme() -> None:
    fake = lob.FakeLob("s3")
    body, headers = fake.signed_webhook({"id": "evt_x"}, "1800000000")
    expected = hmac.new(b"s3", f"1800000000.{body}".encode(), hashlib.sha256).hexdigest()
    assert headers == {"lob-signature": expected, "lob-signature-timestamp": "1800000000"}
    assert lob.verify("s3", "1800000000", body, expected)
    assert not lob.verify("s3", "1800000001", body, expected)

"""Sticky vendor faults on stand_ins/server.py over HTTP (D-3844, D-3940; for the J4 outage journey, T119).

`/_stand_in/faults` sets, shows and clears faults that hold until changed: a faulted Twilio send,
Stripe request or Claude API message answers the vendor's error (or Stripe's dropped connection)
and stores nothing; control routes, Verify, Resend and Lob never fail. Fake data only.
"""

from __future__ import annotations

import http.client
from typing import Any

import pytest
from stand_ins import ai
from tests.contract.test_stand_in_server import (  # noqa: F401  (the `base` fixture)
    AI_HEADERS,
    HOMEOWNER,
    LOB_AUTH,
    MSGS,
    OURS,
    VERIFY,
    base,
    call,
    tw,
)

STRIPE = {"authorization": "Bearer sk_test_fake"}
AI_BODY = {"model": "claude-fake", "max_tokens": 64, "messages": [{"role": "user", "content": "fake leaky faucet"}]}
EMAIL = {"from": "Fake <fake@example.test>", "to": "owner@example.test", "subject": "Fake", "text": "Fake body"}
ADDRESS = {"name": "Fake Person", "address_line1": "1 Test Way", "address_city": "Testville", "address_state": "AZ",
           "address_zip": "86403"}
LETTER = {"to": ADDRESS, "from": {**ADDRESS, "name": "Fake Sender"}, "file": "<html>000000</html>", "color": False}


def faults(base: str, method: str = "GET", body: Any = None) -> tuple[int, Any]:
    status, reply, _ = call(base, method, "/_stand_in/faults", body=body)
    return status, reply


def send_text(base: str) -> tuple[int, Any]:
    status, reply, _ = tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "Fake hello"})
    return status, reply


def sent_count(base: str) -> int:
    return len(tw(base, "GET", MSGS)[1]["messages"])


def stripe_customer(base: str, email: str = "fake@example.test", **headers: str) -> tuple[int, Any]:
    status, reply, _ = call(base, "POST", "/stripe/v1/customers", {"email": email}, **STRIPE, **headers)
    return status, reply


def assert_key_unused(base: str, key: str) -> None:
    """The key tried under the fault was never stored: other values under it make a new customer, no conflict."""
    status, customer = stripe_customer(base, "other@example.test", idempotency_key=key)
    assert status == 200, customer
    assert customer["id"].startswith("cus_") and customer["email"] == "other@example.test"
    assert stripe_customer(base, "other@example.test", idempotency_key=key) == (200, customer)


def test_no_fault_by_default_and_unknown_vendor_or_value_changes_nothing(base: str) -> None:
    assert faults(base) == (200, {"faults": {}})
    assert faults(base, "POST", {"twilio": "fail"}) == (200, {"faults": {"twilio": "fail"}})
    for bad in ({"fax": "fail"}, {"twilio": "unavailable"}, {"ai": "500"}, {"stripe": 1}, {}):
        status, reply = faults(base, "POST", bad)
        assert status == 400, bad
        assert "unknown fault" in reply["error"]
    status, _ = faults(base, "POST", {"twilio": None, "stripe": "nope"})
    assert status == 400
    assert faults(base) == (200, {"faults": {"twilio": "fail"}})


def test_twilio_fail_answers_500_stores_nothing_until_cleared(base: str) -> None:
    faults(base, "POST", {"twilio": "fail"})
    for _ in range(2):
        status, reply = send_text(base)
        assert status == 500
        assert reply["code"] == 20500 and reply["status"] == 500
    assert sent_count(base) == 0
    assert faults(base, "POST", {"twilio": None}) == (200, {"faults": {}})
    assert send_text(base)[0] == 201
    assert sent_count(base) == 1


def test_twilio_suspended_answers_21606_stores_nothing(base: str) -> None:
    faults(base, "POST", {"twilio": "suspended"})
    status, reply = send_text(base)
    assert status == 400
    assert reply["code"] == 21606
    assert reply["message"] == f"The From phone number {OURS} is not a valid, SMS-capable Twilio phone number."
    assert sent_count(base) == 0
    faults(base, "DELETE")
    assert send_text(base)[0] == 201


def test_stripe_unavailable_answers_503_creates_nothing(base: str) -> None:
    faults(base, "POST", {"stripe": "unavailable"})
    status, reply = stripe_customer(base, idempotency_key="fake-key-unavailable")
    assert status == 503
    assert reply["error"]["type"] == "api_error"
    assert call(base, "GET", "/stripe/v1/events", **STRIPE)[0] == 503
    faults(base, "POST", {"stripe": None})
    status, events, _ = call(base, "GET", "/stripe/v1/events", **STRIPE)
    assert status == 200 and events["data"] == []
    status, customer = stripe_customer(base)
    assert status == 200 and customer["id"].startswith("cus_")
    assert_key_unused(base, "fake-key-unavailable")


def test_stripe_drop_closes_the_connection_with_no_answer(base: str) -> None:
    faults(base, "POST", {"stripe": "drop"})
    host, port = base.removeprefix("http://").split(":")
    conn = http.client.HTTPConnection(host, int(port), timeout=5)
    conn.request("GET", "/stripe/v1/events", headers=STRIPE)
    with pytest.raises((http.client.RemoteDisconnected, ConnectionError)):
        conn.getresponse()
    conn.close()
    conn = http.client.HTTPConnection(host, int(port), timeout=5)
    form = {**STRIPE, "content-type": "application/x-www-form-urlencoded", "idempotency-key": "fake-key-drop"}
    conn.request("POST", "/stripe/v1/customers", body="email=fake%40example.test", headers=form)
    with pytest.raises((http.client.RemoteDisconnected, ConnectionError)):
        conn.getresponse()
    conn.close()
    faults(base, "DELETE")
    status, events, _ = call(base, "GET", "/stripe/v1/events", **STRIPE)
    assert status == 200 and events["data"] == []
    assert_key_unused(base, "fake-key-drop")


def test_ai_529_sticky_and_header_faults_unchanged(base: str) -> None:
    path = "/anthropic/v1/messages"
    assert call(base, "POST", path, body=AI_BODY, **AI_HEADERS)[0] == 200
    faults(base, "POST", {"ai": "529"})
    status, reply, _ = call(base, "POST", path, body=AI_BODY, **AI_HEADERS)
    assert status == 529 and reply["error"]["type"] == ai.FAULTS["529"][1]
    status, _, _ = call(base, "POST", path, body=AI_BODY, x_stand_in_fault="500", **AI_HEADERS)
    assert status == ai.FAULTS["500"][0]
    faults(base, "DELETE")
    status, _, headers = call(base, "POST", path, body=AI_BODY, x_stand_in_fault="429-retry-after", **AI_HEADERS)
    assert status == 429 and headers["retry-after"] == "3"
    assert call(base, "POST", path, body=AI_BODY, **AI_HEADERS)[0] == 200


def test_control_routes_verify_resend_and_lob_never_fail(base: str) -> None:
    every = {"twilio": "fail", "stripe": "unavailable", "ai": "529"}
    assert faults(base, "POST", every) == (200, {"faults": every})
    assert tw(base, "POST", VERIFY + "/Verifications", {"To": HOMEOWNER, "Channel": "sms"})[0] == 201
    assert call(base, "GET", "/_stand_in/twilio/code?to=%2B19285550142")[0] == 200
    assert call(base, "GET", "/_stand_in/resend/emails")[0] == 200
    resend_status, _, _ = call(base, "POST", "/resend/emails", body=EMAIL, authorization="Bearer re_stand_in_fake")
    assert resend_status == 200
    lob_status, _, _ = call(base, "POST", "/lob/v1/letters", body=LETTER, authorization=LOB_AUTH)
    assert lob_status == 200
    assert faults(base, "DELETE") == (200, {"faults": {}})

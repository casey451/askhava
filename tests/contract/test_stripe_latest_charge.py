"""TD-FEE-66: every succeeded PaymentIntent names its charge, and the Rust adapter can read it.

Against the in-process fake (`stand_ins/stripe.py`) and over HTTP (`stand_ins/server.py`): a
confirmed and a fetched succeeded intent carry `latest_charge` as a `ch_` ID string (unexpanded,
as Stripe sends it; D-3118 keeps the `expand[]=latest_charge` swap as it is), and an intent not
yet succeeded carries none (FEE-042, SEC-030, D-2623). Each body has every field the Rust
`PaymentIntent` (`app/adapters/src/payments.rs`) requires, so it decodes; that struct's own test
`td_fee_66_the_intent_decodes_its_latest_charge` decodes the same shapes, with and without the
field. Fake data only.
"""

from __future__ import annotations

import json
import urllib.request
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlencode

import pytest

from stand_ins import server, stripe

AUTH = "Bearer sk_test_fake"
SUCCESS_CARD = "pm_card_visa"
DECLINE_CARD = "pm_card_chargeDeclined"
RUST_REQUIRED = ("id", "status", "amount", "currency", "customer", "metadata")


@pytest.fixture
def base() -> Iterator[str]:
    httpd = server.serve()
    yield "http://127.0.0.1:{}".format(httpd.server_address[1])
    httpd.shutdown()
    httpd.server_close()


def call(base: str, method: str, path: str, form: dict[str, str] | None = None, key: str = "") -> Any:
    data = urlencode(form).encode() if form is not None else None
    headers = {"authorization": AUTH, **({"idempotency-key": key} if key else {})}
    req = urllib.request.Request(base + "/stripe/v1/" + path, data, headers, method=method)
    with urllib.request.urlopen(req, timeout=5) as resp:
        assert resp.status == 200
        return json.loads(resp.read())


def unconfirmed(fake: stripe.FakeStripe, pick: str) -> dict[str, Any]:
    cus = fake.create_customer({"metadata": {"business_id": "biz-fake-352"}})[1]["id"]
    params = {"amount": 7500, "currency": "usd", "customer": cus, "confirm": False, "metadata": {"pick_id": pick}}
    status, pi = fake.create_payment_intent(params, f"{pick}-intent")
    assert status == 200
    return pi


def decodes_in_rust(body: dict[str, Any]) -> bool:
    """Every field the Rust `PaymentIntent` requires, `latest_charge` absent, null or a string."""
    has_fields = all(key in body for key in RUST_REQUIRED) and "pick_id" in body["metadata"]
    return has_fields and isinstance(body.get("latest_charge"), (str, type(None)))


def assert_charged(body: dict[str, Any]) -> None:
    assert body["status"] == "succeeded"
    assert isinstance(body["latest_charge"], str) and body["latest_charge"].startswith("ch_")
    assert decodes_in_rust(body)


def assert_uncharged(body: dict[str, Any]) -> None:
    assert body["status"] != "succeeded"
    assert body.get("latest_charge") is None
    assert decodes_in_rust(body)


def test_td_fee_66_a_confirmed_intent_names_its_charge() -> None:
    """TD-FEE-66: the confirm answer of a paid intent carries its `ch_` charge, which exists."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake, "pick-fake-352a")
    assert_uncharged(pi)
    status, answer = fake.confirm_payment_intent(pi["id"], {"payment_method": SUCCESS_CARD}, "pick-fake-352a-try1")
    assert status == 200
    assert_charged(answer)
    assert fake.objects[answer["latest_charge"]]["payment_intent"] == pi["id"]


def test_td_fee_66_an_approved_three_d_secure_intent_names_its_charge() -> None:
    """TD-FEE-66: a 3-D Secure intent has no charge until the bank approves, then has one."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake, "pick-fake-352b")
    status, asked = fake.confirm_payment_intent(
        pi["id"], {"payment_method": stripe.THREE_D_SECURE}, "pick-fake-352b-try1"
    )
    assert status == 200 and asked["status"] == "requires_action"
    assert_uncharged(asked)
    status, settled = fake.authenticate(pi["id"], True, 1_700_000_000)
    assert status == 200
    assert_charged(settled["payment_intent"])


def test_td_fee_66_a_declined_intent_has_no_charge() -> None:
    """TD-FEE-66: a declined confirm leaves the intent without a `latest_charge`."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake, "pick-fake-352c")
    status, _ = fake.confirm_payment_intent(pi["id"], {"payment_method": DECLINE_CARD}, "pick-fake-352c-try1")
    assert status == 402
    assert_uncharged(fake.objects[pi["id"]])


def test_td_fee_66_a_fetched_intent_carries_the_same_charge_over_http(base: str) -> None:
    """TD-FEE-66: over HTTP, the unpaid fetch has none; the paid confirm and fetch agree on one `ch_`."""
    cus = call(base, "POST", "customers", {"metadata[business_id]": "biz-fake-352"})["id"]
    form = {"amount": "7500", "currency": "usd", "customer": cus, "confirm": "false", "metadata[pick_id]": "pick-d"}
    pi = call(base, "POST", "payment_intents", form, "pick-fake-352d-intent")["id"]
    assert_uncharged(call(base, "GET", f"payment_intents/{pi}"))
    answer = call(base, "POST", f"payment_intents/{pi}/confirm", {"payment_method": SUCCESS_CARD}, "pick-352d-try1")
    assert_charged(answer)
    fetched = call(base, "GET", f"payment_intents/{pi}")
    assert_charged(fetched)
    assert fetched["latest_charge"] == answer["latest_charge"]

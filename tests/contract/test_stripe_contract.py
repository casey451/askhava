"""Contract tests for stand_ins.stripe, from Stripe's public API reference (fake data only)."""

from __future__ import annotations

import hashlib
import hmac
import json
from typing import Any

from stand_ins import stripe


def setup(method: str = "pm_card_visa") -> tuple[stripe.FakeStripe, str]:
    fake = stripe.FakeStripe()
    cus = fake.create_customer({"email": "canary@example.com"})[1]["id"]
    fake.create_setup_intent({"customer": cus, "payment_method": method})
    return fake, cus


def charge(fake: stripe.FakeStripe, cus: str, method: str = "pm_card_visa", amount: int = 7500, key: str | None = None) -> tuple[int, dict[str, Any]]:
    params = {"amount": amount, "currency": "usd", "customer": cus, "payment_method": method,
              "off_session": True, "confirm": True}
    return fake.create_payment_intent(params, key)


def test_resource_shapes_and_id_prefixes() -> None:
    fake, cus = setup()
    assert cus.startswith("cus_")
    status, seti = fake.create_setup_intent({"customer": cus, "payment_method": "pm_card_visa"})
    assert status == 200 and seti["object"] == "setup_intent" and seti["status"] == "succeeded"
    assert seti["id"].startswith("seti_") and seti["usage"] == "off_session"
    status, pi = charge(fake, cus)
    assert status == 200 and pi["id"].startswith("pi_")
    assert pi["status"] == "succeeded" and pi["amount_received"] == 7500 and pi["currency"] == "usd"


def test_setup_intent_without_method_needs_one_and_unknown_customer_errors() -> None:
    fake, cus = setup()
    assert fake.create_setup_intent({"customer": cus})[1]["status"] == "requires_payment_method"
    status, body = fake.create_setup_intent({"customer": "cus_missing"})
    assert status == 400 and body["error"]["code"] == "resource_missing" and body["error"]["param"] == "customer"


def test_declined_card_error_envelope() -> None:
    fake, cus = setup("pm_card_chargeDeclined")
    status, body = charge(fake, cus, "pm_card_chargeDeclined")
    err = body["error"]
    assert status == 402 and err["type"] == "card_error" and err["code"] == "card_declined"
    assert err["decline_code"] == "generic_decline"
    assert err["payment_intent"]["status"] == "requires_payment_method"


def test_authentication_required() -> None:
    fake, cus = setup("pm_card_authenticationRequired")
    status, body = charge(fake, cus, "pm_card_authenticationRequired")
    assert status == 402 and body["error"]["code"] == "authentication_required"


def test_amount_rules() -> None:
    fake, cus = setup()
    assert charge(fake, cus, amount=49)[1]["error"]["code"] == "amount_too_small"
    assert charge(fake, cus, amount=50)[0] == 200
    status, body = fake.create_payment_intent({"currency": "usd"})
    assert status == 400 and body["error"]["code"] == "parameter_missing"


def test_idempotency_key_replays_and_never_charges_twice() -> None:
    fake, cus = setup()
    first = charge(fake, cus, key="k1")
    second = charge(fake, cus, key="k1")
    assert first == second
    assert len([o for o in fake.objects.values() if o["object"] == "payment_intent"]) == 1
    status, body = charge(fake, cus, amount=2500, key="k1")
    assert status == 400 and body["error"]["type"] == "idempotency_error"


def test_refunds_are_limited_to_the_unrefunded_amount() -> None:
    fake, cus = setup()
    pi = charge(fake, cus)[1]["id"]
    status, refund = fake.create_refund({"payment_intent": pi, "amount": 2500})
    assert status == 200 and refund["id"].startswith("re_") and refund["amount"] == 2500
    assert fake.create_refund({"payment_intent": pi, "amount": 5001})[0] == 400
    assert fake.create_refund({"payment_intent": pi})[1]["amount"] == 5000
    assert fake.create_refund({"payment_intent": pi})[1]["error"]["code"] == "charge_already_refunded"
    assert fake.create_refund({"payment_intent": "pi_missing"})[1]["error"]["code"] == "resource_missing"


def test_events_list() -> None:
    fake, cus = setup("pm_card_visa")
    pi = charge(fake, cus)[1]["id"]
    fake.create_refund({"payment_intent": pi})
    fake.open_dispute(pi)
    status, listing = fake.list_events()
    types = [e["type"] for e in listing["data"]]
    assert status == 200 and listing["object"] == "list"
    assert types == ["payment_intent.succeeded", "charge.refunded", "charge.dispute.created"]
    assert [e["type"] for e in fake.list_events("charge.refunded")[1]["data"]] == ["charge.refunded"]


def test_webhook_signature_matches_documented_scheme() -> None:
    fake = stripe.FakeStripe("whsec_fake")
    payload, header = fake.webhook({"id": "evt_1", "type": "x"}, 1_800_000_000)
    digest = hmac.new(b"whsec_fake", f"1800000000.{payload}".encode(), hashlib.sha256).hexdigest()
    assert header == f"t=1800000000,v1={digest}"
    assert json.loads(payload)["id"] == "evt_1"
    assert stripe.verify_signature("whsec_fake", payload, header, 1_800_000_100)


def test_webhook_signature_rejections() -> None:
    payload, header = stripe.FakeStripe().webhook({"a": 1}, 1_800_000_000)
    assert not stripe.verify_signature("whsec_fake", payload, header, 1_800_000_000 + 301)
    assert not stripe.verify_signature("whsec_fake", payload + " ", header, 1_800_000_000)
    assert not stripe.verify_signature("wrong", payload, header, 1_800_000_000)
    assert not stripe.verify_signature("whsec_fake", payload, "garbage", 1_800_000_000)
    assert not stripe.verify_signature("whsec_fake", payload, "t=abc,v1=00", 1_800_000_000)

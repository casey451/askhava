"""TD-FEE-44: the unconfirmed PaymentIntent, its on-session confirm and the 3-D Secure step.

Against the in-process fake (`stand_ins/stripe.py`) and the same calls over HTTP
(`stand_ins/server.py`), as `app/adapters/src/payments.rs` sends them (FEE-014, FEE-016,
FEE-036, FEE-047, D-1424, D-1754). The Rust side (`impl IntentStripe for StripePayments`) is
`app/payments/src/intent_http.rs`'s tests. Fake data only.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlencode

import pytest

from stand_ins import server, stripe

Reply = tuple[int, dict[str, Any]]
AUTH = "Bearer sk_test_fake"
THREE_D_SECURE = stripe.THREE_D_SECURE
FIRST_TRY = "pick-1042-try1"


def unconfirmed(fake: stripe.FakeStripe, key: str = "pick-1042-intent") -> dict[str, Any]:
    cus = fake.create_customer({"metadata": {"business_id": "biz-fake-1"}})[1]["id"]
    params = {"amount": 7500, "currency": "usd", "customer": cus, "confirm": False, "metadata": {"pick_id": "pick-1"}}
    status, pi = fake.create_payment_intent(params, key)
    assert status == 200
    return pi


def confirm(fake: stripe.FakeStripe, pi: dict[str, Any], method: str, key: str) -> Reply:
    return fake.confirm_payment_intent(pi["id"], {"payment_method": method}, key)


def kinds(fake: stripe.FakeStripe) -> list[str]:
    return [e["type"] for e in fake.events]


def test_an_unconfirmed_intent_has_a_client_secret_and_no_charge() -> None:
    """TD-FEE-44 (FEE-036, FEE-047): created unconfirmed, a client secret, nothing charged, one per key."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake)
    assert pi["id"].startswith("pi_") and pi["status"] == "requires_payment_method"
    assert pi["client_secret"].startswith(pi["id"] + "_secret_")
    assert (pi["amount_received"], pi["payment_method"], fake.events) == (0, None, [])
    replay = {k: pi[k] for k in ("amount", "currency", "customer", "metadata")}
    assert fake.create_payment_intent({**replay, "confirm": False}, "pick-1042-intent") == (200, pi)
    status, err = fake.create_payment_intent({"amount": 7500, "currency": "usd", "confirm": False})
    assert status == 400 and err["error"]["param"] == "customer"
    status, err = fake.create_payment_intent({"amount": 10, "currency": "usd", "confirm": False})
    assert status == 400 and err["error"]["code"] == "amount_too_small"


def test_the_success_card_succeeds_once_per_key() -> None:
    """TD-FEE-44 (FEE-016): the success card pays once; the same key answers the same result."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake)
    status, paid = confirm(fake, pi, "pm_card_visa", "pick-1042-try1")
    assert status == 200 and paid["status"] == "succeeded" and paid["amount_received"] == 7500
    assert confirm(fake, pi, "pm_card_visa", "pick-1042-try1") == (status, paid)
    assert kinds(fake) == ["payment_intent.succeeded"]
    status, err = confirm(fake, pi, "pm_card_visa", "pick-1042-try2")
    assert status == 400 and err["error"]["code"] == "payment_intent_unexpected_state"
    status, err = confirm(fake, pi, "pm_card_chargeDeclined", "pick-1042-try1")
    assert status == 400 and err["error"]["type"] == "idempotency_error"
    assert kinds(fake) == ["payment_intent.succeeded"]


def test_a_confirm_without_a_key_or_with_an_unknown_intent_or_card_is_refused() -> None:
    """TD-FEE-44 (FEE-016, SEC-018): no key, no intent or no card charges nothing."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake)
    assert confirm(fake, pi, "pm_card_visa", "")[1]["error"]["code"] == "idempotency_key_missing"
    assert fake.confirm_payment_intent("pi_none", {"payment_method": "pm_card_visa"}, "k1")[0] == 404
    status, err = confirm(fake, pi, "pm_FakeUnknown", "k2")
    assert status == 400 and err["error"]["param"] == "payment_method"
    assert fake.objects[pi["id"]]["status"] == "requires_payment_method" and fake.events == []


def test_the_decline_card_declines_and_a_new_key_may_try_again() -> None:
    """TD-FEE-44 (FEE-016): a decline never blocks a later attempt under its own key."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake)
    status, err = confirm(fake, pi, "pm_card_chargeDeclined", "pick-1042-try1")
    assert status == 402 and err["error"]["decline_code"] == "generic_decline"
    assert err["error"]["payment_intent"]["status"] == "requires_payment_method"
    assert confirm(fake, pi, "pm_card_chargeDeclined", "pick-1042-try1") == (status, err)
    status, paid = confirm(fake, pi, "pm_card_visa", "pick-1042-try2")
    assert status == 200 and paid["status"] == "succeeded" and paid["id"] == pi["id"]
    assert kinds(fake) == ["payment_intent.payment_failed", "payment_intent.succeeded"]


@pytest.mark.parametrize(
    ("passed", "status", "event"),
    [
        (True, "succeeded", "payment_intent.succeeded"),
        (False, "requires_payment_method", "payment_intent.payment_failed"),
    ],
)
def test_the_3ds_card_asks_for_approval_then_the_bank_answer_settles_it(passed: bool, status: str, event: str) -> None:
    """TD-FEE-44 (FEE-014): the bank approval passes (succeeded) or fails, with the signed event."""
    fake = stripe.FakeStripe()
    pi = unconfirmed(fake)
    code, wants = confirm(fake, pi, THREE_D_SECURE, FIRST_TRY)
    assert code == 200 and wants["status"] == "requires_action"
    assert wants["client_secret"] == pi["client_secret"] and wants["next_action"]["type"] == "use_stripe_sdk"
    code, settled = fake.authenticate(pi["id"], passed, 1_800_000_000)
    assert code == 200 and settled["payment_intent"]["status"] == status
    sent = json.loads(settled["payload"])
    assert sent["type"] == event and sent["data"]["object"]["id"] == pi["id"]
    signature = settled["headers"]["stripe-signature"]
    assert stripe.verify_signature(fake.webhook_secret, settled["payload"], signature, 1_800_000_000)
    assert fake.authenticate(pi["id"], passed, 1_800_000_000)[1]["error"]["code"] == "payment_intent_unexpected_state"
    assert fake.authenticate("pi_none", passed, 1_800_000_000)[0] == 404
    assert confirm(fake, pi, THREE_D_SECURE, FIRST_TRY) == (200, wants)


@pytest.fixture
def base() -> Iterator[str]:
    httpd = server.serve()
    yield "http://127.0.0.1:{}".format(httpd.server_address[1])
    httpd.shutdown()
    httpd.server_close()


def call(base: str, method: str, path: str, form: Any = None, **headers: str) -> tuple[int, Any]:
    data = urlencode(form).encode() if form is not None else None
    req = urllib.request.Request(base + path, data, {k.replace("_", "-"): v for k, v in headers.items()}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            status, raw = resp.status, resp.read()
    except urllib.error.HTTPError as err:
        status, raw = err.code, err.read()
    return status, json.loads(raw)


def post(base: str, path: str, form: dict[str, str], key: str = "") -> Any:
    extra = {"idempotency_key": key} if key else {}
    return call(base, "POST", f"/stripe/v1/{path}", form, authorization=AUTH, **extra)


def test_the_same_steps_over_http(base: str) -> None:
    """TD-FEE-44: the adapter's paths over HTTP: create unconfirmed, confirm, approve, fetch."""
    cus = post(base, "customers", {"metadata[business_id]": "biz-fake-2"})[1]["id"]
    form = {"amount": "2500", "currency": "usd", "customer": cus, "confirm": "false", "metadata[pick_id]": "pick-2"}
    form["payment_method_types[]"] = "card"
    status, pi = post(base, "payment_intents", form, "pick-2-intent")
    assert status == 200 and pi["status"] == "requires_payment_method" and pi["client_secret"]
    path = f"payment_intents/{pi['id']}"
    status, wants = post(base, path + "/confirm", {"payment_method": "pm_card_authenticationRequired"}, "try1")
    assert status == 200 and wants["status"] == "requires_action"
    assert post(base, path + "/stand_in_authenticate", {"result": "maybe"})[0] == 400
    status, settled = post(base, path + "/stand_in_authenticate", {"result": "pass"})
    assert status == 200 and settled["payment_intent"]["status"] == "succeeded"
    assert "stripe-signature" in settled["headers"]
    fetched = call(base, "GET", f"/stripe/v1/{path}", authorization=AUTH)[1]
    assert fetched["status"] == "succeeded" and fetched["amount_received"] == 2500

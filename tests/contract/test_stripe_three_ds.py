"""TD-FEE-69: the stand-in's latest_charge and its 3-D Secure result, as Stripe sends them.

A succeeded PaymentIntent names its charge in `latest_charge` as a `ch_` ID string; a fetch with
`expand[]=latest_charge` returns the charge object instead, whose
`payment_method_details.card.three_d_secure` is `authenticated` after a passed bank approval and
null for a card never asked for 3-D Secure (FEE-050, FEE-027, D-3118). The Rust read is
`StripePayments::fetch_three_ds` (`app/adapters/src/payments.rs`). Fake data only.
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


def paid(base: str, card: str, pick: str) -> tuple[str, dict[str, Any]]:
    """An unconfirmed intent confirmed with `card` (and approved when the card asks); the confirm's answer."""
    cus = call(base, "POST", "customers", {"metadata[business_id]": "biz-fake-405"})["id"]
    form = {"amount": "7500", "currency": "usd", "customer": cus, "confirm": "false", "metadata[pick_id]": pick}
    pi = call(base, "POST", "payment_intents", form, f"{pick}-intent")["id"]
    answer = call(base, "POST", f"payment_intents/{pi}/confirm", {"payment_method": card}, f"{pick}-try1")
    if card == stripe.THREE_D_SECURE:
        answer = call(base, "POST", f"payment_intents/{pi}/stand_in_authenticate", {"result": "pass"})["payment_intent"]
    return pi, answer


def three_d_secure(charge: dict[str, Any]) -> Any:
    return charge["payment_method_details"]["card"]["three_d_secure"]


def test_a_plain_fetch_names_the_charge_by_id(base: str) -> None:
    """TD-FEE-69: confirm answer and plain fetch carry `latest_charge` as a `ch_` string."""
    pi, answer = paid(base, "pm_card_visa", "pick-fake-405a")
    assert answer["status"] == "succeeded"
    assert isinstance(answer["latest_charge"], str) and answer["latest_charge"].startswith("ch_")
    fetched = call(base, "GET", f"payment_intents/{pi}")
    assert fetched["latest_charge"] == answer["latest_charge"]
    assert call(base, "GET", f"payment_intents/{pi}?expand[]=customer")["latest_charge"] == answer["latest_charge"]


def test_an_expanded_fetch_after_approval_is_authenticated(base: str) -> None:
    """TD-FEE-69 (FEE-050): a passed 3-D Secure step expands to `result` `authenticated`."""
    pi, answer = paid(base, stripe.THREE_D_SECURE, "pick-fake-405b")
    assert answer["status"] == "succeeded" and isinstance(answer["latest_charge"], str)
    expanded = call(base, "GET", f"payment_intents/{pi}?" + urlencode({"expand[]": "latest_charge"}))
    charge = expanded["latest_charge"]
    assert charge["id"] == answer["latest_charge"] and charge["payment_intent"] == pi
    assert three_d_secure(charge)["result"] == "authenticated"


def test_an_expanded_fetch_without_three_d_secure_has_none(base: str) -> None:
    """TD-FEE-69 (FEE-050): a card never asked for 3-D Secure expands with no result."""
    pi, answer = paid(base, "pm_card_visa", "pick-fake-405c")
    expanded = call(base, "GET", f"payment_intents/{pi}?" + urlencode({"expand[]": "latest_charge"}))
    assert expanded["latest_charge"]["id"] == answer["latest_charge"]
    assert three_d_secure(expanded["latest_charge"]) is None


def test_an_unpaid_intent_has_no_charge_even_expanded(base: str) -> None:
    """TD-FEE-69: nothing charged, nothing to expand."""
    cus = call(base, "POST", "customers", {"metadata[business_id]": "biz-fake-405"})["id"]
    form = {"amount": "7500", "currency": "usd", "customer": cus, "confirm": "false", "metadata[pick_id]": "pick-d"}
    pi = call(base, "POST", "payment_intents", form, "pick-fake-405d-intent")["id"]
    expanded = call(base, "GET", f"payment_intents/{pi}?expand[]=latest_charge")
    assert "latest_charge" not in expanded

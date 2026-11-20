"""BIZ-051 (D-4749): the Stripe stand-in's card detach and its `stripe_detach` failure switch.

Against `stand_ins/stripe.py` over HTTP (`stand_ins/server.py`), as T734's card detach
(`POST /v1/payment_methods/{pm}/detach`) sends it. Fake data only, never a real key.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from typing import Any

import pytest

from stand_ins import server

AUTH = "Bearer sk_test_fake"
CARD = "pm_card_visa"
Reply = tuple[int, dict[str, Any]]


@pytest.fixture()
def base() -> Iterator[str]:
    srv = server.serve()
    yield f"http://127.0.0.1:{srv.server_address[1]}"
    srv.shutdown()
    srv.server_close()


def call(url: str, data: bytes | None, headers: dict[str, str], method: str = "POST") -> Reply:
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def post(base: str, path: str, form: str = "", key: str | None = None, auth: str = AUTH) -> Reply:
    headers = {"authorization": auth, "content-type": "application/x-www-form-urlencoded"}
    if key:
        headers["idempotency-key"] = key
    return call(f"{base}/stripe/v1/{path}", form.encode(), headers)


def faults(base: str, body: dict[str, Any]) -> Reply:
    return call(f"{base}/_stand_in/faults", json.dumps(body).encode(), {"content-type": "application/json"})


def attach(base: str) -> str:
    """Saves the success card for a fake customer, the way a business's setup step does."""
    status, cus = post(base, "customers", "metadata[business_id]=biz-fake-1")
    assert status == 200
    status, seti = post(base, "setup_intents", f"customer={cus['id']}&payment_method={CARD}")
    assert (status, seti["status"]) == (200, "succeeded")
    return cus["id"]


def detach(base: str, method: str = CARD, key: str | None = None, auth: str = AUTH) -> Reply:
    return post(base, f"payment_methods/{method}/detach", key=key, auth=auth)


def test_an_attached_card_detaches_to_no_customer(base: str) -> None:
    """BIZ-051: the saved card comes back in Stripe's payment_method shape with customer null."""
    attach(base)
    status, pm = detach(base, key="leave-biz-fake-1-card")
    assert status == 200
    assert (pm["id"], pm["object"], pm["type"], pm["customer"]) == (CARD, "payment_method", "card", None)


def test_an_unknown_card_is_stripes_404(base: str) -> None:
    status, err = detach(base, "pm_fake_never_made")
    assert status == 404
    assert (err["error"]["type"], err["error"]["code"]) == ("invalid_request_error", "resource_missing")


def test_a_detached_card_cannot_detach_again(base: str) -> None:
    attach(base)
    assert detach(base)[0] == 200
    status, err = detach(base)
    assert status == 400
    assert (err["error"]["type"], err["error"]["code"]) == ("invalid_request_error", "payment_method_unexpected_state")


def test_the_switch_fails_detaches_until_cleared(base: str) -> None:
    """D-4749: `stripe_detach` `fail` answers 500 api_error and stores nothing; clearing it lets the
    same key detach."""
    attach(base)
    assert faults(base, {"stripe_detach": "fail"}) == (200, {"faults": {"stripe_detach": "fail"}})
    status, err = detach(base, key="leave-biz-fake-1-card")
    assert status == 500 and err["error"]["type"] == "api_error"
    assert post(base, "customers", "metadata[business_id]=biz-fake-2")[0] == 200
    assert faults(base, {"stripe_detach": None}) == (200, {"faults": {}})
    status, pm = detach(base, key="leave-biz-fake-1-card")
    assert (status, pm["customer"]) == (200, None)


def test_the_switch_takes_only_its_own_value(base: str) -> None:
    status, _ = faults(base, {"stripe_detach": "unavailable"})
    assert status == 400
    assert call(f"{base}/_stand_in/faults", None, {}, "GET") == (200, {"faults": {}})


def test_a_replayed_key_answers_the_first_detach(base: str) -> None:
    attach(base)
    first = detach(base, key="leave-biz-fake-1-card")
    assert first[0] == 200
    assert detach(base, key="leave-biz-fake-1-card") == first
    assert detach(base, key="leave-biz-fake-1-card-again")[0] == 400


def test_a_key_that_is_not_a_test_key_is_refused(base: str) -> None:
    attach(base)
    status, err = detach(base, auth="Bearer not-a-test-key")
    assert status == 401 and err["error"]["code"] == "api_key_invalid"
    assert detach(base)[0] == 200

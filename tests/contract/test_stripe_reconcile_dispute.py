"""TD-FEE-61: the stand-in's paged charge list and dispute evidence submit, in Stripe's shapes.

`GET /v1/charges` lists the charge objects stored under each succeeded intent's `ch_` ID, newest
first, filtered by `created[gte]`, paged by `limit` and `starting_after` with `has_more`; with
`expand[]=data.balance_transaction` each carries its balance transaction's processing fee, and
each carries its refunded amount and `disputed`, whose dispute `GET /v1/disputes?charge=` names.
`POST /v1/disputes/{id}` with `evidence[...]` and `submit=true` submits once per idempotency key
(FEE-029, FEE-051, FEE-027, FEE-050, D-2305). The Rust reads are `ReconcileStripe` and
`DisputeStripe` for `StripePayments` (`app/payments/src/reconcile_http.rs`, `dispute_http.rs`).
Fake data only.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlencode

import pytest

from stand_ins import server

AUTH = "Bearer sk_test_fake"
EXPAND = "expand[]=data.balance_transaction"


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


def refused(base: str, method: str, path: str, form: dict[str, str] | None = None, key: str = "") -> int:
    with pytest.raises(urllib.error.HTTPError) as caught:
        call(base, method, path, form, key)
    body = json.loads(caught.value.read())
    assert set(body["error"]) >= {"type", "code", "message"}
    return caught.value.code


def paid(base: str, pick: str, cents: int = 7500) -> str:
    """A fake pick's intent, charged at once with the success card; its `pi_` ID."""
    cus = call(base, "POST", "customers", {"metadata[business_id]": "biz-fake-335"})["id"]
    form = {
        "amount": str(cents),
        "currency": "usd",
        "customer": cus,
        "payment_method": "pm_card_visa",
        "confirm": "true",
        "metadata[pick_id]": pick,
    }
    return call(base, "POST", "payment_intents", form, f"{pick}-intent")["id"]


def disputed(base: str, pi: str) -> dict[str, Any]:
    body = json.dumps({"payment_intent": pi}).encode()
    req = urllib.request.Request(base + "/_stand_in/stripe/disputes", body, method="POST")
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


def test_the_charge_page_carries_fee_refund_and_dispute(base: str) -> None:
    """TD-FEE-61 (FEE-029, FEE-051): the listed charge is the stored one, with its fee expanded."""
    pi = paid(base, "pick-fake-335a")
    call(base, "POST", "refunds", {"payment_intent": pi, "amount": "2500"}, "pick-fake-335a-refund")
    dispute = disputed(base, pi)
    page = call(base, "GET", f"charges?created[gte]=0&limit=10&{EXPAND}")
    assert (page["object"], page["has_more"], len(page["data"])) == ("list", False, 1)
    charge = page["data"][0]
    stored = call(base, "GET", f"payment_intents/{pi}")["latest_charge"]
    assert (charge["id"], charge["object"], charge["status"], charge["payment_intent"]) == (
        stored,
        "charge",
        "succeeded",
        pi,
    )
    txn = charge["balance_transaction"]
    assert (txn["object"], txn["id"][:4], txn["fee"], txn["net"]) == ("balance_transaction", "txn_", 248, 7252)
    assert (charge["amount"], charge["amount_refunded"], charge["disputed"]) == (7500, 2500, True)
    assert charge["metadata"] == {"pick_id": "pick-fake-335a"}
    plain = call(base, "GET", "charges?limit=1")["data"][0]
    assert plain["balance_transaction"] == txn["id"]
    found = call(base, "GET", f"disputes?charge={charge['id']}&limit=1")["data"]
    assert [(d["id"], d["charge"], d["amount"]) for d in found] == [(dispute["id"], charge["id"], 7500)]


def test_the_pages_follow_starting_after_to_the_end(base: str) -> None:
    """TD-FEE-61 (FEE-029): newest first, `limit` per page, `has_more` until the last page."""
    intents = [paid(base, f"pick-fake-335-{n}", 2500 + n) for n in range(5)]
    seen, after = [], ""
    while True:
        cursor = f"&starting_after={after}" if after else ""
        page = call(base, "GET", f"charges?created[gte]=0&limit=2&{EXPAND}{cursor}")
        assert len(page["data"]) <= 2
        seen += [c["payment_intent"] for c in page["data"]]
        if not page["has_more"]:
            break
        after = page["data"][-1]["id"]
    assert seen == intents[::-1]
    assert call(base, "GET", "charges?created[gte]=99999999999")["data"] == []
    assert refused(base, "GET", "charges?starting_after=ch_fake0") == 400
    assert refused(base, "GET", "charges?limit=101") == 400


def test_the_evidence_submit_posts_submit_true_once_per_key(base: str) -> None:
    """TD-FEE-61 (FEE-027, FEE-050): evidence and submit=true submit once; the key replays."""
    dispute = disputed(base, paid(base, "pick-fake-335b"))
    assert (dispute["status"], dispute["evidence_details"]["submission_count"]) == ("needs_response", 0)
    form = {"evidence[uncategorized_text]": "fake record", "submit": "true"}
    path = f"disputes/{dispute['id']}"
    assert refused(base, "POST", path, form) == 400
    first = call(base, "POST", path, form, "dp-fake-335b")
    again = call(base, "POST", path, form, "dp-fake-335b")
    assert first == again
    assert (first["status"], first["evidence"]["uncategorized_text"]) == ("under_review", "fake record")
    assert first["evidence_details"]["submission_count"] == 1
    assert refused(base, "POST", path, form, "dp-fake-335b-2") == 400
    listed = call(base, "GET", f"disputes?charge={dispute['charge']}")["data"][0]
    assert listed["evidence_details"]["submission_count"] == 1
    assert refused(base, "POST", "disputes/dp_fake0", form, "dp-fake-0") == 404

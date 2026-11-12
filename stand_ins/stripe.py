"""Stripe mock-compatible fake, written from Stripe's public API reference.

Same resource shapes and error envelope as stripe/stripe-mock, plus what the mock
lacks: state (declines by test payment method, idempotency keys, refund limits, events
and signed webhooks). Amounts are integer cents; the minimum USD charge is 50 cents.
Error envelope: {"error": {"type", "code", "message", "param"?}}. Test payment methods:
pm_card_visa succeeds; pm_card_chargeDeclined and pm_card_chargeCustomerFail decline
(card_declined); pm_card_authenticationRequired needs 3-D Secure (authentication_required).
A PaymentIntent created with confirm=false (FEE-047, D-1754) has status
requires_payment_method, a client_secret and no charge; any other create charges at once (the
older off-session shape the contract tests pin). confirm_payment_intent (on-session, FEE-014,
FEE-016) needs an idempotency key: the success card succeeds, a decline card answers 402, and
the 3-D Secure card answers requires_action with the client_secret, which the stand-in-only
authenticate then settles (succeeded, or requires_payment_method on a failed approval).
A succeeded intent names its charge in latest_charge (a ch_ ID string, as Stripe sends it
unexpanded); the charge object, stored under that ID, carries payment_method_details.card.
three_d_secure: result authenticated after a passed approval, null for a card never asked (D-3118).
Webhook header: Stripe-Signature: t=<unix>,v1=<hex HMAC-SHA256(secret, "<t>.<payload>")>.
"""

from __future__ import annotations

import hashlib
import hmac
import json
from typing import Any

from stand_ins.common import IdempotencyStore, Reply, fake_id

MIN_USD_CENTS = 50
TOLERANCE_SECONDS = 300
DECLINES = {
    "pm_card_chargeDeclined": ("card_declined", "generic_decline", "Your card was declined."),
    "pm_card_chargeCustomerFail": ("card_declined", "generic_decline", "Your card was declined."),
    "pm_card_authenticationRequired": (
        "authentication_required",
        "authentication_required",
        "This payment required an authentication step.",
    ),
}
KNOWN_METHODS = {"pm_card_visa", *DECLINES}
THREE_D_SECURE = "pm_card_authenticationRequired"
AUTH_FAILED = (
    "payment_intent_authentication_failure",
    "payment_intent_authentication_failure",
    "The provided PaymentMethod has failed authentication.",
)
CONFIRMABLE = {"requires_payment_method", "requires_confirmation", "requires_action"}


def error(kind: str, code: str, message: str, status: int = 400, param: str | None = None) -> Reply:
    body: dict[str, Any] = {"type": kind, "code": code, "message": message}
    if param:
        body["param"] = param
    return status, {"error": body}


def sign_payload(secret: str, payload: str, timestamp: int) -> str:
    digest = hmac.new(secret.encode(), f"{timestamp}.{payload}".encode(), hashlib.sha256).hexdigest()
    return f"t={timestamp},v1={digest}"


def verify_signature(secret: str, payload: str, header: str, now: int) -> bool:
    parts = dict(p.split("=", 1) for p in header.split(",") if "=" in p)
    if "t" not in parts or "v1" not in parts or not parts["t"].isdigit():
        return False
    if abs(now - int(parts["t"])) > TOLERANCE_SECONDS:
        return False
    return hmac.compare_digest(sign_payload(secret, payload, int(parts["t"])), header)


class FakeStripe:
    def __init__(self, webhook_secret: str = "whsec_fake") -> None:
        self.webhook_secret = webhook_secret
        self.objects: dict[str, dict[str, Any]] = {}
        self.events: list[dict[str, Any]] = []
        self._idem = IdempotencyStore()
        self._n = 0

    def _new(self, prefix: str, **fields: Any) -> dict[str, Any]:
        self._n += 1
        obj = {"id": fake_id(prefix, self._n, 14), **fields}
        self.objects[obj["id"]] = obj
        return obj

    def _event(self, kind: str, obj: dict[str, Any]) -> None:
        self._n += 1
        self.events.append(
            {"id": fake_id("evt_", self._n, 14), "object": "event", "type": kind, "data": {"object": dict(obj)}}
        )

    def _once(self, key: str | None, params: dict[str, Any], make: Any) -> Reply:
        state, reply = self._idem.lookup(key, params)
        if state == "replay" and reply is not None:
            return reply
        if state == "conflict":
            return error(
                "idempotency_error",
                "idempotency_key_in_use",
                "Keys for idempotent requests can only be used with the same parameters they were first used with.",
            )
        return self._idem.store(key, params, make())

    def create_customer(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        def make() -> Reply:
            return 200, dict(
                self._new("cus_", object="customer", email=params.get("email"), metadata=params.get("metadata", {}))
            )

        return self._once(idempotency_key, params, make)

    def create_setup_intent(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        def make() -> Reply:
            cust, method = params.get("customer"), params.get("payment_method")
            if cust not in self.objects:
                return error(
                    "invalid_request_error", "resource_missing", f"No such customer: '{cust}'", 400, "customer"
                )
            if method is not None and method not in KNOWN_METHODS:
                return error(
                    "invalid_request_error",
                    "resource_missing",
                    f"No such PaymentMethod: '{method}'",
                    400,
                    "payment_method",
                )
            status = "succeeded" if method else "requires_payment_method"
            return 200, dict(
                self._new(
                    "seti_",
                    object="setup_intent",
                    customer=cust,
                    payment_method=method,
                    status=status,
                    usage="off_session",
                )
            )

        return self._once(idempotency_key, params, make)

    def create_payment_intent(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        make = self._unconfirmed if params.get("confirm") is False else self._charge
        return self._once(idempotency_key, params, lambda: make(params))

    @staticmethod
    def _amount_problem(params: dict[str, Any]) -> Reply | None:
        amount = params.get("amount")
        if not isinstance(amount, int) or isinstance(amount, bool):
            return error("invalid_request_error", "parameter_missing", "Missing required param: amount.", 400, "amount")
        if params.get("currency") != "usd":
            return error(
                "invalid_request_error", "parameter_invalid_empty", "Missing required param: currency.", 400, "currency"
            )
        if amount < MIN_USD_CENTS:
            return error(
                "invalid_request_error", "amount_too_small", "Amount must be at least $0.50 usd", 400, "amount"
            )
        return None

    def _intent(self, params: dict[str, Any], method: str | None) -> dict[str, Any]:
        pi = self._new(
            "pi_",
            object="payment_intent",
            status="requires_payment_method",
            amount=params["amount"],
            currency="usd",
            customer=params["customer"],
            payment_method=method,
            amount_received=0,
            amount_refunded=0,
            metadata=params.get("metadata", {}),
        )
        pi["client_secret"] = f"{pi['id']}_secret_{fake_id('', self._n, 24)}"
        return pi

    def _unconfirmed(self, params: dict[str, Any]) -> Reply:
        problem = self._amount_problem(params)
        if problem is not None:
            return problem
        if params.get("customer") not in self.objects:
            return error("invalid_request_error", "resource_missing", "No such customer", 400, "customer")
        return 200, dict(self._intent(params, None))

    def _charge(self, params: dict[str, Any]) -> Reply:
        problem = self._amount_problem(params)
        if problem is not None:
            return problem
        method = params.get("payment_method")
        if method not in KNOWN_METHODS or params.get("customer") not in self.objects:
            return error("invalid_request_error", "resource_missing", "No such customer or payment method", 400)
        pi = self._intent(params, method)
        decline = DECLINES.get(method)
        return (200, self._succeed(pi)) if decline is None else self._decline(pi, decline)

    def _succeed(self, pi: dict[str, Any], three_d_secure: dict[str, Any] | None = None) -> dict[str, Any]:
        """Pays the intent: its charge is stored under its own `ch_` ID, which `latest_charge`
        names; `card.three_d_secure` is null unless the bank's 3-D Secure step ran (D-3118)."""
        charge_id = "ch_" + pi["id"].removeprefix("pi_")
        card = {"brand": "visa", "last4": "4242", "three_d_secure": three_d_secure}
        self.objects[charge_id] = {
            "id": charge_id,
            "object": "charge",
            "payment_intent": pi["id"],
            "amount": pi["amount"],
            "currency": pi["currency"],
            "status": "succeeded",
            "payment_method_details": {"type": "card", "card": card},
        }
        pi.update(
            status="succeeded",
            amount_received=pi["amount"],
            next_action=None,
            last_payment_error=None,
            latest_charge=charge_id,
        )
        self._event("payment_intent.succeeded", pi)
        return dict(pi)

    def _decline(self, pi: dict[str, Any], decline: tuple[str, str, str]) -> Reply:
        code, decline_code, message = decline
        pi.update(status="requires_payment_method", next_action=None)
        pi["last_payment_error"] = {"type": "card_error", "code": code, "decline_code": decline_code}
        self._event("payment_intent.payment_failed", pi)
        status, body = error("card_error", code, message, 402)
        body["error"]["decline_code"] = decline_code
        body["error"]["payment_intent"] = dict(pi)
        return status, body

    def confirm_payment_intent(
        self, intent_id: str, params: dict[str, Any], idempotency_key: str | None = None
    ) -> Reply:
        """On-session confirm under one attempt's key (FEE-016); a repeat of the key replays."""
        if not idempotency_key:
            return error("invalid_request_error", "idempotency_key_missing", "Every confirm needs an Idempotency-Key.")
        scoped = {"confirm": intent_id, **params}
        return self._once(idempotency_key, scoped, lambda: self._confirm(intent_id, params))

    def _confirm(self, intent_id: str, params: dict[str, Any]) -> Reply:
        pi = self.objects.get(intent_id)
        if pi is None or pi.get("object") != "payment_intent":
            return error("invalid_request_error", "resource_missing", "No such payment_intent", 404, "intent")
        method = params.get("payment_method")
        if method not in KNOWN_METHODS:
            return error("invalid_request_error", "resource_missing", "No such PaymentMethod", 400, "payment_method")
        if pi["status"] not in CONFIRMABLE:
            return error(
                "invalid_request_error", "payment_intent_unexpected_state", "This PaymentIntent cannot be confirmed."
            )
        pi["payment_method"] = method
        if method == THREE_D_SECURE:
            pi.update(status="requires_action", next_action={"type": "use_stripe_sdk"})
            self._event("payment_intent.requires_action", pi)
            return 200, dict(pi)
        decline = DECLINES.get(method)
        return (200, self._succeed(pi)) if decline is None else self._decline(pi, decline)

    def authenticate(self, intent_id: str, passed: bool, timestamp: int) -> Reply:
        """Stand-in only: the bank's 3-D Secure answer settles a requires_action intent and
        returns it with the signed delivery of the event it emitted."""
        pi = self.objects.get(intent_id)
        if pi is None or pi.get("object") != "payment_intent":
            return error("invalid_request_error", "resource_missing", "No such payment_intent", 404, "intent")
        if pi["status"] != "requires_action":
            return error(
                "invalid_request_error", "payment_intent_unexpected_state", "This PaymentIntent needs no approval."
            )
        authenticated = {"result": "authenticated", "version": "2.2.0"}
        status, body = (200, self._succeed(pi, authenticated)) if passed else self._decline(pi, AUTH_FAILED)
        payload, signature = self.webhook(self.events[-1], timestamp)
        settled = body if status == 200 else body["error"]["payment_intent"]
        return 200, {"payment_intent": settled, "payload": payload, "headers": {"stripe-signature": signature}}

    def create_refund(self, params: dict[str, Any], idempotency_key: str | None = None) -> Reply:
        return self._once(idempotency_key, params, lambda: self._refund(params))

    def _refund(self, params: dict[str, Any]) -> Reply:
        pi = self.objects.get(str(params.get("payment_intent")))
        if pi is None or pi.get("object") != "payment_intent":
            return error("invalid_request_error", "resource_missing", "No such payment_intent", 400, "payment_intent")
        left = pi["amount_received"] - pi["amount_refunded"]
        amount = params.get("amount", left)
        if not isinstance(amount, int) or amount <= 0 or amount > left:
            return error(
                "invalid_request_error",
                "charge_already_refunded" if left == 0 else "parameter_invalid_integer",
                "Refund amount is greater than unrefunded amount on charge",
                400,
                "amount",
            )
        pi["amount_refunded"] += amount
        refund = self._new(
            "re_", object="refund", payment_intent=pi["id"], amount=amount, currency="usd", status="succeeded"
        )
        self._event("charge.refunded", pi)
        return 200, dict(refund)

    def open_dispute(self, payment_intent: str) -> dict[str, Any]:
        dispute = self._new("dp_", object="dispute", payment_intent=payment_intent, status="needs_response")
        self._event("charge.dispute.created", dispute)
        return dict(dispute)

    def list_events(self, event_type: str | None = None) -> Reply:
        data = [e for e in self.events if event_type in (None, e["type"])]
        return 200, {"object": "list", "data": data, "has_more": False}

    def webhook(self, event: dict[str, Any], timestamp: int) -> tuple[str, str]:
        payload = json.dumps(event, sort_keys=True)
        return payload, sign_payload(self.webhook_secret, payload, timestamp)

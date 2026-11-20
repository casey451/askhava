"""One standard-library HTTP server for every stand-in, at each vendor's URL shapes (SEC-032, D-453).

Each vendor lives under its own prefix, so an adapter's base URL setting is the only difference
between stand-in, sandbox and live: `/twilio` (api.twilio.com and verify.twilio.com), `/stripe`
(api.stripe.com), `/resend` (api.resend.com), `/lob` (api.lob.com) and `/anthropic`
(api.anthropic.com). Below the prefix every path, form or JSON body, auth header, error envelope
and Idempotency-Key behaves as the in-process fake in `stand_ins/<vendor>.py` does, so the
contract tests hold over HTTP. Only stand-in credentials pass (D-557, D-561): Twilio's Basic user
must be the stand-in account SID (`AC` and 32 zeros), Stripe's key must start `sk_test_`, Resend's
`re_stand_in_`, Lob's `test_`, and a Claude API key starting `sk-ant-` is refused, so a real key
put in a setting by mistake gets the vendor's 401. Paths under `/_stand_in/` drive what the vendor
would do on its own (an inbound text, the code on the phone, a status callback, a dispute, a
letter's next tracking step, a signed webhook for an event), and so does Stripe's
`/v1/payment_intents/{id}/stand_in_authenticate` (the bank's 3-D Secure answer, D-1754); a real
vendor has none of them. The
derived key of a text travels in the `X-Askhava-Message-Key` header; the Messages list takes
Twilio's own filters `To`, `From`, `DateSent`, `DateSent<` and `DateSent>` (`YYYY-MM-DD`, UTC,
inclusive), which recovery matches on (OPS-017, D-2764), and the test-only `MessageKey`, which
Twilio cannot store: only `core::test_control` uses it, never app code. The provider's own opt-out
replies are listed only with `Direction=outbound-reply`. A create's `StatusCallback` is kept, and
`/_stand_in/twilio/status` answers 409 (nothing recorded) for a message created without one, else
the URL, params and signature Twilio would post there (D-2761). A call create keeps its script and
callback fields, and `/_stand_in/twilio/call-progress` gives the script and the signed callbacks
Twilio would post as the call ends, only to the URLs the create gave (D-2762).
`/_stand_in/faults` holds sticky vendor faults until changed (D-3844, D-3940): POST sets
`twilio` `fail` (every message send answers Twilio's 500) or `suspended` (error 21606, HTTP 400),
`stripe` `unavailable` (every `/stripe/v1/` request answers a 503) or `drop` (the connection
closes with no answer), `stripe_detach` `fail` (every card detach answers Stripe's 500
`api_error`, D-4749), `ai` `529` (a `/anthropic/v1/messages` request without its own
`x-stand-in-fault` header answers as if it carried `529`), and `null` clears one; DELETE clears
all, GET shows them. A faulted request stores nothing; control routes, Verify, Resend and Lob
never fail. It binds to 127.0.0.1 only and holds fake data only.

Usage: python3 -m stand_ins.server [--port 4010]; `--port 0` binds a free port, and the
"listening" line names the port actually bound, so a test can start it without a port race.
"""

from __future__ import annotations

import argparse
import base64
import json
import re
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import date
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import parse_qsl, urlsplit

from stand_ins import ai, lob, resend, stripe, twilio
from stand_ins.common import fake_id

Out = tuple[int, Any, dict[str, str]]
AC = r"/twilio/2010-04-01/Accounts/(?P<ac>AC\w{32})"
RESEND_KEY_PREFIX = "re_stand_in_"
FAULT_VALUES: dict[str, tuple[str, ...]] = {
    "twilio": ("fail", "suspended"),
    "stripe": ("unavailable", "drop"),
    "stripe_detach": ("fail",),
    "ai": ("529",),
}
DROP = -1


@dataclass
class Request:
    method: str
    path: str
    query: dict[str, str]
    headers: dict[str, str]
    raw: bytes
    base: str = ""

    def form(self) -> dict[str, str]:
        return dict(parse_qsl(self.raw.decode(), keep_blank_values=True))

    def json(self) -> dict[str, Any]:
        value = json.loads(self.raw or b"{}")
        return value if isinstance(value, dict) else {}


@dataclass
class StandIns:
    twilio: twilio.FakeTwilio = field(default_factory=twilio.FakeTwilio)
    stripe: stripe.FakeStripe = field(default_factory=stripe.FakeStripe)
    resend: resend.FakeResend = field(default_factory=resend.FakeResend)
    lob: lob.FakeLob = field(default_factory=lob.FakeLob)
    faults: dict[str, str] = field(default_factory=dict)
    lock: threading.Lock = field(default_factory=threading.Lock)


def _basic_user(req: Request) -> str:
    auth = req.headers.get("authorization", "")
    if not auth.startswith("Basic "):
        return ""
    return base64.b64decode(auth[6:]).decode().partition(":")[0]


def _bearer(req: Request) -> str:
    return req.headers.get("authorization", "").removeprefix("Bearer ")


def auth_problem(s: StandIns, req: Request) -> Out | None:
    """The vendor's own 401 unless the credentials are the stand-in's own kind (D-557, D-561)."""
    vendor = req.path.split("/")[1]
    if vendor == "twilio" and _basic_user(req) != s.twilio.account_sid:
        return (*twilio.error(20003, "Authenticate", 401), {})
    if vendor == "stripe" and not _bearer(req).startswith("sk_test_"):
        return (*stripe.error("invalid_request_error", "api_key_invalid", "Invalid API Key provided", 401), {})
    if vendor == "resend" and not _bearer(req).startswith(RESEND_KEY_PREFIX):
        return (*resend.error(401, "validation_error", "API key is invalid"), {})
    if vendor == "lob" and not _basic_user(req).startswith("test_"):
        return (*lob.error(401, "Your API key is not valid. Please sign up on lob.com to get a valid api key."), {})
    key = req.headers.get("x-api-key", "")
    if vendor == "anthropic" and (not key or key.startswith("sk-ant-")):
        return ai.api_error(401, "authentication_error", "invalid x-api-key")
    return None


def stripe_params(pairs: dict[str, str]) -> dict[str, Any]:
    """Stripe's form encoding: metadata[k]=v nests, amount is an integer, true/false are booleans."""
    params: dict[str, Any] = {}
    for key, value in pairs.items():
        parsed: Any = {"true": True, "false": False}.get(value, value)
        if key == "amount" and value.isdigit():
            parsed = int(value)
        outer, _, inner = key.partition("[")
        if inner:
            params.setdefault(outer, {})[inner.rstrip("]")] = parsed
        else:
            params[key] = parsed
    return params


def twilio_send(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """A message create; a sticky `twilio` fault answers Twilio's 500 or 21606 and stores nothing (D-3940)."""
    f = req.form()
    key = req.headers.get("x-askhava-message-key")
    to, frm, body = f.get("To", ""), f.get("From", ""), f.get("Body", "")
    if s.faults.get("twilio") == "fail":
        return (*twilio.error(20500, "An internal server error has occurred", 500), {})
    if s.faults.get("twilio") == "suspended":
        return (*twilio.error(21606, f"The From phone number {frm} is not a valid, SMS-capable Twilio phone number."), {})
    return (*s.twilio.create_message(to, frm, body, key, f.get("StatusCallback")), {})


def _day(text: str | None) -> date | None:
    """A `DateSent` filter's `YYYY-MM-DD`; anything else raises ValueError, answered 400."""
    if text is None:
        return None
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", text):
        raise ValueError("bad DateSent")
    return date.fromisoformat(text)


def twilio_list(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """App sends only, by Twilio's `To`, `From` and `DateSent` filters and the test-only
    `MessageKey` (D-2764); `Direction=outbound-reply` lists the provider's opt-out replies (D-561)."""
    q = req.query
    replies = q.get("Direction") == "outbound-reply"
    on, before, after = _day(q.get("DateSent")), _day(q.get("DateSent<")), _day(q.get("DateSent>"))
    if on is not None and (before, after) != (None, None):
        raise ValueError("DateSent with DateSent< or DateSent>")
    window = (on, on) if on is not None else (after, before)
    sent = s.twilio.sent_log(q.get("MessageKey"), q.get("To"), replies, q.get("From"), window)
    return 200, {"messages": sent, "page": 0, "page_size": 50, "next_page_uri": None}, {}


def twilio_message(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """GET one Message; POST with an empty Body redacts its body (D-1841)."""
    if req.method == "GET":
        return (*s.twilio.message(m.group("sid")), {})
    return (*s.twilio.redact_message(m.group("sid"), req.form().get("Body")), {})


def twilio_media(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Bytes (with their stored content type) without a suffix, the Media resource JSON with `.json`, DELETE either way (D-492, D-561)."""
    path = req.path.removeprefix("/twilio").removesuffix(".json")
    if req.method == "DELETE":
        return (*s.twilio.delete_media(path), {})
    if req.path.endswith(".json"):
        return (*s.twilio.media_resource(path), {})
    if path not in s.twilio.media:
        return (*twilio.error(20404, "The requested resource was not found", 404), {})
    return 200, s.twilio.media[path], {"content-type": s.twilio.media_types[path]}


def twilio_calls(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    if req.method == "GET":
        status = req.query.get("Status")
        return 200, {"calls": [c for c in s.twilio.calls if status in (None, c["status"])]}, {}
    f = req.form()
    if m.group("call"):
        return (*s.twilio.update_call(m.group("call"), f.get("Status", "")), {})
    plan: dict[str, Any] = {k: v for k, v in f.items() if k not in ("To", "From")}
    pairs = parse_qsl(req.raw.decode(), keep_blank_values=True)
    plan["StatusCallbackEvent"] = [v for k, v in pairs if k == "StatusCallbackEvent"]
    return (*s.twilio.create_call(f.get("To", ""), f.get("From", ""), plan), {})


def twilio_verify(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    f = req.form()
    if m.group("step") == "Verifications":
        return (*s.twilio.start_verification(f.get("To", ""), f.get("Channel", "sms")), {})
    return (*s.twilio.check_verification(f.get("To", ""), f.get("Code", "")), {})


def control_inbound(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """The homeowner's phone texts us: returns the webhook form and its X-Twilio-Signature for `url`."""
    b = req.json()
    params = s.twilio.receive(b["from"], b["to"], b.get("body", ""), b.get("media"))
    for n in range(int(params["NumMedia"])):
        params[f"MediaUrl{n}"] = req.base + "/twilio" + params[f"MediaUrl{n}"]
    signature = twilio.compute_signature(s.twilio.auth_token, b.get("url", ""), params)
    return 200, {"params": params, "signature": signature}, {}


def control_code(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    return 200, {"code": s.twilio.last_code(req.query.get("to", ""))}, {}


def control_status(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Twilio moves a message's status: 409 for a message created without `StatusCallback` (no
    callback recorded), else the URL its create gave, the params and their X-Twilio-Signature (D-2761)."""
    b = req.json()
    found = s.twilio.status_callback(b["sid"], b["status"], b.get("error_code"))
    if found is None:
        return 409, {"error": "message created without StatusCallback; Twilio posts no status"}, {}
    url, params = found
    signature = twilio.compute_signature(s.twilio.auth_token, url, params)
    return 200, {"url": url, "params": params, "signature": signature}, {}


def control_call_progress(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Twilio runs call `sid` to `outcome`: the script its create gave and each callback Twilio would
    post (URL, params, X-Twilio-Signature), only to the URLs the create gave (D-2762, D-2771)."""
    b = req.json()
    posts = s.twilio.progress(b["sid"], b["outcome"])
    signed = [
        {"url": url, "params": params, "signature": twilio.compute_signature(s.twilio.auth_token, url, params)}
        for url, params in posts
    ]
    return 200, {"twiml": s.twilio.call_plans[b["sid"]].get("Twiml"), "callbacks": signed}, {}


def _stamp(body: dict[str, Any]) -> int:
    return int(body.get("timestamp") or time.time())


def control_dispute(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """The cardholder disputes a charge: a dispute object and its charge.dispute.created event."""
    pi = req.json()["payment_intent"]
    if s.stripe.objects.get(pi, {}).get("object") != "payment_intent":
        return (*stripe.error("invalid_request_error", "resource_missing", "No such payment_intent", 404), {})
    return 200, s.stripe.open_dispute(pi), {}


def control_stripe_webhook(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """The signed delivery of one event: the raw payload and its Stripe-Signature."""
    b = req.json()
    event = next(e for e in s.stripe.events if e["id"] == b["id"])
    payload, signature = s.stripe.webhook(event, _stamp(b))
    return 200, {"payload": payload, "headers": {"stripe-signature": signature}}, {}


def _bare_address(value: str) -> str:
    """The address inside `Name <addr>` (as stand_ins/resend.py reads it), lower-cased for matching."""
    match = re.search(r"<([^>]+)>", value)
    return (match.group(1) if match else value).strip().lower()


def control_resend_emails(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """The emails the fake Resend accepted, oldest first, optionally only those sent to the `to` address.

    The filter compares bare addresses case-insensitively against `to` only (not cc or bcc); an
    empty `?to=` is dropped by the query parser and lists every email (D-3272).
    """
    want = req.query.get("to")
    emails = []
    for sent in s.resend.sent:
        to = [sent["to"]] if isinstance(sent["to"], str) else list(sent["to"])
        if want is None or _bare_address(want) in {_bare_address(str(r)) for r in to}:
            emails.append({**{k: sent.get(k, "") for k in ("id", "from", "subject", "text", "html")}, "to": to})
    return 200, {"emails": emails}, {}


def control_resend_webhooks(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Svix-signed deliveries of every event of one email (the test addresses drive bounce and complaint)."""
    b = req.json()
    stamp, out = _stamp(b), []
    for n, event in enumerate(s.resend.events):
        if event["data"]["email_id"] == b["email_id"]:
            payload, msg_id = json.dumps(event, sort_keys=True), fake_id("msg_", n + 1, 16)
            signature = resend.sign(s.resend.secret, msg_id, stamp, payload)
            headers = {"svix-id": msg_id, "svix-timestamp": str(stamp), "svix-signature": signature}
            out.append({"payload": payload, "headers": headers})
    return 200, {"webhooks": out}, {}


def control_lob_advance(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Move a letter to its next tracking step; 400 once it is delivered."""
    return 200, {"event": s.lob.advance(req.json()["id"])}, {}


def control_faults(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Show (GET), set (POST, `null` clears one) or clear (DELETE) the sticky vendor faults; an
    unknown vendor or value is a 400 that changes nothing (D-3844, D-3940)."""
    if req.method == "DELETE":
        s.faults.clear()
    elif req.method == "POST":
        wanted = req.json()
        bad = [k for k, v in wanted.items() if k not in FAULT_VALUES or (v is not None and v not in FAULT_VALUES[k])]
        if bad or not wanted:
            return 400, {"error": f"unknown fault vendor or value: {sorted(bad)}", "allowed": FAULT_VALUES}, {}
        for vendor, value in wanted.items():
            if value is None:
                s.faults.pop(vendor, None)
            else:
                s.faults[vendor] = value
    return 200, {"faults": dict(s.faults)}, {}


def stripe_fault(s: StandIns, req: Request) -> Out | None:
    """A sticky `stripe` fault for any `/stripe/v1/` request: a 503 or a dropped connection (D-3940)."""
    fault = s.faults.get("stripe") if req.path.startswith("/stripe/v1/") else None
    if fault == "drop":
        return DROP, b"", {}
    if fault == "unavailable":
        return (*stripe.error("api_error", "service_unavailable", "Stripe is temporarily unavailable", 503), {})
    return None


def control_lob_webhooks(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """Signed deliveries (lob-signature) of every tracking event of one letter."""
    b = req.json()
    stamp = str(_stamp(b))
    signed = [s.lob.signed_webhook(e, stamp) for e in s.lob.events if e["body"]["id"] == b["id"]]
    return 200, {"webhooks": [{"payload": p, "headers": h} for p, h in signed]}, {}


def stripe_create(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    make = {
        "customers": s.stripe.create_customer,
        "setup_intents": s.stripe.create_setup_intent,
        "payment_intents": s.stripe.create_payment_intent,
        "refunds": s.stripe.create_refund,
    }[m.group("kind")]
    return (*make(stripe_params(req.form()), req.headers.get("idempotency-key")), {})


def stripe_intent_step(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """The on-session confirm, or the stand-in-only 3-D Secure answer (`result` pass or fail)."""
    params = stripe_params(req.form())
    if m.group("step") == "confirm":
        return (*s.stripe.confirm_payment_intent(m.group("id"), params, req.headers.get("idempotency-key")), {})
    result = params.get("result")
    if result not in ("pass", "fail"):
        return (*stripe.error("invalid_request_error", "parameter_invalid", "result is pass or fail", 400), {})
    return (*s.stripe.authenticate(m.group("id"), result == "pass", int(time.time())), {})


def stripe_fetch(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """One stored object; `expand[]=latest_charge` swaps the charge ID for the charge (D-3118)."""
    obj = s.stripe.objects.get(m.group("id"))
    if obj is None:
        return (*stripe.error("invalid_request_error", "resource_missing", "No such object", 404, "id"), {})
    found = dict(obj)
    charge = s.stripe.objects.get(str(found.get("latest_charge")))
    if req.query.get("expand[]") == "latest_charge" and charge is not None:
        found["latest_charge"] = dict(charge)
    return 200, found, {}


def stripe_detach(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """BIZ-051 (D-4749): a card detach, or Stripe's 500 while the `stripe_detach` fault is set."""
    if s.faults.get("stripe_detach") == "fail":
        return (*stripe.error("api_error", "api_error", "An unexpected error occurred on Stripe's end.", 500), {})
    params = stripe_params(req.form())
    return (*s.stripe.detach_payment_method(m.group("id"), params, req.headers.get("idempotency-key")), {})


def stripe_events(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    return (*s.stripe.list_events(req.query.get("type")), {})


def stripe_charges(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """One page of the charges T405 stores, with `expand[]=data.balance_transaction` (D-2305)."""
    since, limit = req.query.get("created[gte]", "0"), req.query.get("limit", "10")
    if not since.isdigit() or not limit.isdigit() or not 1 <= int(limit) <= stripe.LIST_MAX:
        return (*stripe.error("invalid_request_error", "parameter_invalid_integer", "Bad created or limit", 400), {})
    expand = req.query.get("expand[]") == "data.balance_transaction"
    return (*s.stripe.list_charges(int(since), int(limit), req.query.get("starting_after"), expand), {})


def stripe_charge(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    """One stored charge, with `expand[]=balance_transaction` (D-3538)."""
    return (*s.stripe.fetch_charge(m.group("id"), req.query.get("expand[]") == "balance_transaction"), {})


def stripe_disputes(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    return (*s.stripe.list_disputes(req.query.get("charge")), {})


def stripe_dispute_update(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    params = stripe_params(req.form())
    return (*s.stripe.update_dispute(m.group("id"), params, req.headers.get("idempotency-key")), {})


def resend_send(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    return (*s.resend.send(req.json(), req.headers.get("idempotency-key")), {})


def lob_create(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    return (*s.lob.create_letter(req.json(), req.headers.get("idempotency-key")), {})


def lob_fetch(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    letter = s.lob.letters.get(m.group("id"))
    return (200, dict(letter), {}) if letter else (*lob.error(404, "letter not found", "not_found"), {})


def ai_messages(s: StandIns, req: Request, m: re.Match[str]) -> Out:
    if not req.headers.get("anthropic-version"):
        return ai.api_error(400, "invalid_request_error", "anthropic-version header is required")
    if m.group("count"):
        return ai.count_tokens(req.json())
    fault = req.headers.get("x-stand-in-fault") or s.faults.get("ai", "")
    return ai.messages(req.json(), fault, req.headers.get("x-stand-in-plant", ""))


PI = r"/stripe/v1/payment_intents/(?P<id>pi_\w+)"
Handler = Callable[[StandIns, Request, re.Match[str]], Out]
ROUTES: list[tuple[str, re.Pattern[str], Handler]] = [
    (r, re.compile(p + r"\Z"), h)
    for r, p, h in [
        ("POST", AC + r"/Messages\.json", twilio_send),
        ("GET", AC + r"/Messages\.json", twilio_list),
        ("GET|POST", AC + r"/Messages/(?P<sid>(SM|MM)\w{32})\.json", twilio_message),
        ("GET|DELETE", AC + r"/Messages/MM\w{32}/Media/ME\w{32}(\.json)?", twilio_media),
        ("GET|POST", AC + r"/Calls(/(?P<call>CA\w{32}))?\.json", twilio_calls),
        ("POST", r"/twilio/v2/Services/VA\w{32}/(?P<step>Verifications|VerificationCheck)", twilio_verify),
        ("POST", r"/_stand_in/twilio/inbound", control_inbound),
        ("GET", r"/_stand_in/twilio/code", control_code),
        ("POST", r"/_stand_in/twilio/status", control_status),
        ("POST", r"/_stand_in/twilio/call-progress", control_call_progress),
        ("POST", r"/_stand_in/stripe/disputes", control_dispute),
        ("POST", r"/_stand_in/stripe/webhook", control_stripe_webhook),
        ("GET", r"/_stand_in/resend/emails", control_resend_emails),
        ("POST", r"/_stand_in/resend/webhooks", control_resend_webhooks),
        ("POST", r"/_stand_in/lob/advance", control_lob_advance),
        ("POST", r"/_stand_in/lob/webhooks", control_lob_webhooks),
        ("GET|POST|DELETE", r"/_stand_in/faults", control_faults),
        ("POST", r"/stripe/v1/(?P<kind>customers|setup_intents|payment_intents|refunds)", stripe_create),
        ("POST", PI + r"/(?P<step>confirm|stand_in_authenticate)", stripe_intent_step),
        ("GET", r"/stripe/v1/(payment_intents|setup_intents|customers|refunds)/(?P<id>\w+)", stripe_fetch),
        ("POST", r"/stripe/v1/payment_methods/(?P<id>pm_\w+)/detach", stripe_detach),
        ("GET", r"/stripe/v1/events", stripe_events),
        ("GET", r"/stripe/v1/charges", stripe_charges),
        ("GET", r"/stripe/v1/charges/(?P<id>(ch|py)_\w+)", stripe_charge),
        ("GET", r"/stripe/v1/disputes", stripe_disputes),
        ("POST", r"/stripe/v1/disputes/(?P<id>dp_\w+)", stripe_dispute_update),
        ("POST", r"/resend/emails", resend_send),
        ("POST", r"/lob/v1/letters", lob_create),
        ("GET", r"/lob/v1/letters/(?P<id>ltr_\w+)", lob_fetch),
        ("POST", r"/anthropic/v1/messages(?P<count>/count_tokens)?", ai_messages),
    ]
]


def dispatch(s: StandIns, req: Request) -> Out:
    """Route one request; control paths need no credentials, vendor paths need the vendor's."""
    for methods, pattern, handler in ROUTES:
        match = pattern.match(req.path)
        if match and req.method in methods.split("|"):
            problem = None if req.path.startswith("/_stand_in/") else auth_problem(s, req)
            with s.lock:
                return problem or stripe_fault(s, req) or handler(s, req, match)
    return 404, {"error": f"no stand-in route for {req.method} {req.path}"}, {}


def body_length(value: str | None) -> int:
    """Content-Length as a non-negative integer; anything else raises ValueError, answered 400."""
    text = (value or "0").strip()
    if not text.isdigit():
        raise ValueError("bad content-length")
    return int(text)


def make_handler(state: StandIns) -> type[BaseHTTPRequestHandler]:
    class StandInHandler(BaseHTTPRequestHandler):
        def _serve(self) -> None:
            url = urlsplit(self.path)
            headers = {k.lower(): v for k, v in self.headers.items()}
            base = "http://{}:{}".format(*self.server.server_address[:2])
            try:
                raw = self.rfile.read(body_length(self.headers.get("content-length")))
                req = Request(self.command, url.path, dict(parse_qsl(url.query)), headers, raw, base)
                status, body, extra = dispatch(state, req)
                if status == DROP:
                    self.close_connection = True
                    return
            except (KeyError, ValueError, TypeError, StopIteration) as exc:
                status, body, extra = 400, {"error": f"bad stand-in request: {type(exc).__name__}"}, {}
                self.close_connection = True
            data = body if isinstance(body, bytes) else json.dumps(body).encode()
            data = b"" if status == 204 else data
            self.send_response(status)
            extra.setdefault("content-type", "application/json")
            for name, value in {**extra, "content-length": str(len(data))}.items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        do_GET = do_POST = do_DELETE = _serve

        def log_message(self, format: str, *args: Any) -> None:
            """Silent: request lines could carry fake phone numbers into CI logs."""

    return StandInHandler


def serve(port: int = 0, state: StandIns | None = None) -> ThreadingHTTPServer:
    """Start the server on 127.0.0.1 in a background thread; port 0 picks a free one."""
    server = ThreadingHTTPServer(("127.0.0.1", port), make_handler(state or StandIns()))
    threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True).start()
    return server


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Serve every stand-in over HTTP (SEC-032).")
    parser.add_argument("--port", type=int, default=4010)
    port = parser.parse_args(argv).port
    server = ThreadingHTTPServer(("127.0.0.1", port), make_handler(StandIns()))
    print(f"stand-ins listening on http://127.0.0.1:{server.server_address[1]}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

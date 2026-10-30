"""The contract cases over HTTP against stand_ins/server.py (TD-SEC-13; SEC-032, AI-022, D-453, D-491..D-493).

Every case talks to a real socket on 127.0.0.1, the way the Rust adapters will. Fake data only.
"""

from __future__ import annotations

import base64
import http.client
import json
import urllib.error
import urllib.request
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlencode, urlsplit

import pytest

from stand_ins import ai, lob, resend, server, stripe, twilio

HOMEOWNER = "+19285550142"
OURS = twilio.VALID_FROM
AC = "AC" + "0" * 32
MSGS = f"/twilio/2010-04-01/Accounts/{AC}/Messages.json"
VERIFY = "/twilio/v2/Services/VA" + "0" * 32
TWILIO_AUTH = "Basic " + base64.b64encode(f"{AC}:fake-auth-token".encode()).decode()
LOB_AUTH = "Basic " + base64.b64encode(b"test_fake:").decode()
AI_HEADERS = {"x-api-key": "fake-ai-key", "anthropic-version": "2023-06-01"}
Reply = tuple[int, Any, dict[str, str]]


@pytest.fixture()
def base() -> Iterator[str]:
    srv = server.serve()
    yield f"http://127.0.0.1:{srv.server_address[1]}"
    srv.shutdown()
    srv.server_close()


def call(base: str, method: str, path: str, form: Any = None, body: Any = None, **headers: str) -> Reply:
    data = urlencode(form).encode() if form is not None else None
    if body is not None:
        data, headers = json.dumps(body).encode(), {**headers, "content-type": "application/json"}
    req = urllib.request.Request(base + path, data, {k.replace("_", "-"): v for k, v in headers.items()}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            status, raw, got = resp.status, resp.read(), dict(resp.headers)
    except urllib.error.HTTPError as err:
        status, raw, got = err.code, err.read(), dict(err.headers)
    is_json = got.get("content-type") == "application/json" and raw
    return status, json.loads(raw) if is_json else raw, {k.lower(): v for k, v in got.items()}


def tw(base: str, method: str, path: str, form: Any = None, **headers: str) -> Reply:
    return call(base, method, path, form, authorization=TWILIO_AUTH, **headers)


def text_in(base: str, body: str, media: list[str] | None = None) -> dict[str, Any]:
    payload = {"from": HOMEOWNER, "to": OURS, "body": body, "media": media, "url": "https://example.com/sms"}
    reply: dict[str, Any] = call(base, "POST", "/_stand_in/twilio/inbound", body=payload)[1]
    return reply


def test_twilio_send_errors_and_auth_over_http(base: str) -> None:
    status, msg, _ = tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "Ask Hava: hi"})
    assert status == 201 and msg["sid"].startswith("SM") and msg["status"] == "queued"
    status, err, _ = tw(base, "POST", MSGS, {"To": "+15005550004", "From": OURS, "Body": "x"})
    assert status == 400 and err["code"] == 21610 and set(err) == {"code", "message", "more_info", "status"}
    status, err, _ = call(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "x"})
    assert status == 401 and err["code"] == 20003
    other = "Basic " + base64.b64encode(f"AC{'1' * 32}:token".encode()).decode()
    assert call(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "x"}, authorization=other)[0] == 401


@pytest.mark.parametrize(
    ("to", "code"), [("+15005550001", 21211), ("+15005550002", 21612), ("+15005550003", 21408), ("+15005550009", 21614)]
)
def test_twilio_magic_numbers_over_http(base: str, to: str, code: int) -> None:
    status, err, _ = tw(base, "POST", MSGS, {"To": to, "From": OURS, "Body": "x"})
    assert (status, err["code"]) == (400, code)
    assert tw(base, "GET", MSGS)[1]["messages"] == []


def test_derived_key_and_sent_log_let_a_replay_skip_a_sent_text(base: str) -> None:
    """OPS-017 (D-491): the replay asks the sent log for the derived key before sending."""
    tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "Ask Hava: a"}, x_askhava_message_key="job1:t1")
    found = tw(base, "GET", MSGS + "?MessageKey=job1:t1")[1]["messages"]
    assert [m["body"] for m in found] == ["Ask Hava: a"]
    assert tw(base, "GET", MSGS + "?MessageKey=job1:t2")[1]["messages"] == []
    assert len(tw(base, "GET", MSGS + "?To=" + HOMEOWNER.replace("+", "%2B"))[1]["messages"]) == 1


def test_provider_optout_file_is_con_030() -> None:
    settings = twilio.OPTOUT_SETTINGS
    assert settings["opt_out"]["keywords"] == [
        *("stop", "stopall", "unsubscribe", "cancel", "end", "revoke", "optout", "quit"),
        *("parar", "alto", "cancelar", "baja"),
    ]
    assert settings["opt_in"]["keywords"] == ["start", "unstop"] and "YES" not in twilio.OPT_IN
    assert settings["opt_out"]["reply"]["en"].startswith("Ask Hava: You're unsubscribed.")
    assert settings["opt_out"]["reply"]["es"].startswith("Ask Hava: Se canceló su suscripción.")


def test_spanish_opt_out_gets_the_spanish_reply_and_yes_resubscribes_nothing(base: str) -> None:
    """CON-030 (D-492): the fake Twilio matches the file's words and replies from the file."""
    params = text_in(base, " Baja ")["params"]
    assert params["OptOutType"] == "STOP"
    replies = tw(base, "GET", MSGS + "?Direction=outbound-reply")[1]["messages"]
    assert [(m["body"], m["direction"]) for m in replies] == [
        (twilio.OPTOUT_SETTINGS["opt_out"]["reply"]["es"], "outbound-reply")
    ]
    assert tw(base, "GET", MSGS)[1]["messages"] == []
    assert tw(base, "GET", MSGS + "?To=" + HOMEOWNER.replace("+", "%2B"))[1]["messages"] == []
    assert "OptOutType" not in text_in(base, "yes")["params"]
    assert tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "x"})[1]["code"] == 21610
    assert text_in(base, "UNSTOP")["params"]["OptOutType"] == "START"
    assert tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "x"})[0] == 201


def test_picture_text_media_can_be_fetched_then_deleted(base: str) -> None:
    """D-492: NumMedia and MediaUrl{n} arrive signed; DELETE removes the stored picture."""
    reply = text_in(base, "my sink", ["image/jpeg"])
    params = reply["params"]
    assert params["NumMedia"] == "1" and params["MediaContentType0"] == "image/jpeg"
    assert twilio.validate_signature("fake-auth-token", "https://example.com/sms", params, reply["signature"])
    path = params["MediaUrl0"].removeprefix(base)
    assert tw(base, "GET", path)[:2] == (200, b"fake-image-bytes")
    status, meta, headers = tw(base, "GET", path + ".json")
    assert status == 200 and headers["content-type"] == "application/json"
    assert meta["content_type"] == "image/jpeg" and meta["parent_sid"] == params["MessageSid"]
    assert path.endswith(meta["sid"]) and meta["uri"] == path.removeprefix("/twilio") + ".json"
    assert tw(base, "DELETE", path + ".json")[0] == 204
    assert tw(base, "GET", path + ".json")[0] == 404
    assert tw(base, "GET", path)[0] == 404 and tw(base, "DELETE", path)[0] == 404


def test_media_url_uses_the_server_address_not_the_host_header(base: str) -> None:
    payload = {"from": HOMEOWNER, "to": OURS, "body": "", "media": ["image/png"], "url": "https://example.com/sms"}
    reply = call(base, "POST", "/_stand_in/twilio/inbound", body=payload, host="attacker.example")[1]
    assert reply["params"]["MediaUrl0"].startswith(base + "/twilio/")
    assert twilio.validate_signature("fake-auth-token", "https://example.com/sms", reply["params"], reply["signature"])


def test_verify_and_voice_session_over_http(base: str) -> None:
    assert tw(base, "POST", VERIFY + "/Verifications", {"To": HOMEOWNER, "Channel": "sms"})[0] == 201
    code = call(base, "GET", "/_stand_in/twilio/code?to=" + HOMEOWNER.replace("+", "%2B"))[1]["code"]
    checked = tw(base, "POST", VERIFY + "/VerificationCheck", {"To": HOMEOWNER, "Code": code})[1]
    assert checked["status"] == "approved"
    calls = f"/twilio/2010-04-01/Accounts/{AC}/Calls"
    status, opened, _ = tw(base, "POST", calls + ".json", {"To": HOMEOWNER, "From": OURS})
    assert status == 201 and tw(base, "GET", calls + ".json?Status=queued")[1]["calls"] == [opened]
    assert tw(base, "POST", f"{calls}/{opened['sid']}.json", {"Status": "completed"})[1]["status"] == "completed"
    assert tw(base, "GET", calls + ".json?Status=queued")[1]["calls"] == []


def test_status_callback_control(base: str) -> None:
    sid = tw(base, "POST", MSGS, {"To": HOMEOWNER, "From": OURS, "Body": "hi"})[1]["sid"]
    params = call(base, "POST", "/_stand_in/twilio/status", body={"sid": sid, "status": "delivered"})[1]["params"]
    assert params == {"MessageSid": sid, "MessageStatus": "delivered", "To": HOMEOWNER, "From": OURS}


def stripe_post(base: str, kind: str, form: dict[str, Any], key: str = "", auth: str = "sk_test_fake") -> Reply:
    extra = {"idempotency_key": key} if key else {}
    return call(base, "POST", f"/stripe/v1/{kind}", form, authorization=f"Bearer {auth}", **extra)


def test_stripe_charge_idempotency_decline_and_live_key(base: str) -> None:
    cus = stripe_post(base, "customers", {"email": "canary@example.com", "metadata[ref]": "t1"})[1]
    assert cus["id"].startswith("cus_") and cus["metadata"] == {"ref": "t1"}
    pay = {"amount": "7500", "currency": "usd", "customer": cus["id"], "payment_method": "pm_card_visa"}
    first = stripe_post(base, "payment_intents", pay, "pick-1")
    assert first[0] == 200 and first[1]["amount_received"] == 7500
    assert stripe_post(base, "payment_intents", pay, "pick-1")[1] == first[1]
    status, err, _ = stripe_post(base, "payment_intents", {**pay, "amount": "2500"}, "pick-1")
    assert status == 400 and err["error"]["type"] == "idempotency_error"
    status, err, _ = stripe_post(base, "payment_intents", {**pay, "payment_method": "pm_card_chargeDeclined"})
    assert status == 402 and err["error"]["decline_code"] == "generic_decline"
    fetched = call(base, "GET", f"/stripe/v1/payment_intents/{first[1]['id']}", authorization="Bearer sk_test_fake")
    assert fetched[1] == first[1]
    events = call(base, "GET", "/stripe/v1/events?type=payment_intent.succeeded", authorization="Bearer sk_test_fake")
    assert len(events[1]["data"]) == 1
    assert stripe_post(base, "customers", {}, auth="sk_live_fake")[0] == 401


def test_stripe_dispute_and_signed_webhook_controls(base: str) -> None:
    """A dispute and a signed delivery are reachable over HTTP (SEC-016, FEE-026; D-561)."""
    cus = stripe_post(base, "customers", {})[1]
    pay = {"amount": "7500", "currency": "usd", "customer": cus["id"], "payment_method": "pm_card_visa"}
    pi = stripe_post(base, "payment_intents", pay)[1]
    dispute = call(base, "POST", "/_stand_in/stripe/disputes", body={"payment_intent": pi["id"]})[1]
    assert dispute["id"].startswith("dp_") and dispute["status"] == "needs_response"
    assert call(base, "POST", "/_stand_in/stripe/disputes", body={"payment_intent": "pi_none"})[0] == 404
    events = call(base, "GET", "/stripe/v1/events?type=charge.dispute.created", authorization="Bearer sk_test_fake")
    event = events[1]["data"][0]
    signed = call(base, "POST", "/_stand_in/stripe/webhook", body={"id": event["id"], "timestamp": 1_800_000_000})[1]
    assert json.loads(signed["payload"]) == event
    header = signed["headers"]["stripe-signature"]
    assert stripe.verify_signature("whsec_fake", signed["payload"], header, 1_800_000_000)
    assert not stripe.verify_signature("whsec_fake", signed["payload"], header, 1_800_000_000 + 301)
    assert call(base, "POST", "/_stand_in/stripe/webhook", body={"id": "evt_none"})[0] == 400


def test_resend_test_addresses_give_signed_webhooks(base: str) -> None:
    """bounced@resend.dev drives email.bounced; each delivery is Svix-signed with its own id (D-561)."""
    mail = {"from": "Ask Hava <jobs@example.com>", "to": ["bounced@resend.dev"], "subject": "Hi", "text": "Hello"}
    sent = call(base, "POST", "/resend/emails", body=mail, authorization="Bearer re_stand_in_fake")[1]
    hooks = call(base, "POST", "/_stand_in/resend/webhooks", body={"email_id": sent["id"], "timestamp": 1_800_000_000})
    deliveries = hooks[1]["webhooks"]
    assert [json.loads(d["payload"])["type"] for d in deliveries] == ["email.sent", "email.bounced"]
    assert len({d["headers"]["svix-id"] for d in deliveries}) == 2
    for d in deliveries:
        h = d["headers"]
        assert resend.verify(resend.DEFAULT_SECRET, h["svix-id"], int(h["svix-timestamp"]), d["payload"],
                             h["svix-signature"])


def test_lob_tracking_advance_and_signed_webhooks(base: str) -> None:
    """A letter walks its tracking steps on demand; each event comes signed (D-561)."""
    address = {"name": "Zelda Canary", "address_line1": "1 Test Way", "address_city": "Lake Havasu City",
               "address_state": "AZ", "address_zip": "86403"}
    letter = {"to": address, "from": {**address, "name": "Ask Hava"}, "file": "<html>000000</html>", "color": False}
    made = call(base, "POST", "/lob/v1/letters", body=letter, authorization=LOB_AUTH)[1]
    steps = [call(base, "POST", "/_stand_in/lob/advance", body={"id": made["id"]})[1]["event"] for _ in range(4)]
    assert steps == list(lob.TRACKING[1:])
    assert call(base, "POST", "/_stand_in/lob/advance", body={"id": made["id"]})[0] == 400
    fetched = call(base, "GET", f"/lob/v1/letters/{made['id']}", authorization=LOB_AUTH)[1]
    assert [e["name"] for e in fetched["tracking_events"]] == list(lob.TRACKING)
    hooks = call(base, "POST", "/_stand_in/lob/webhooks", body={"id": made["id"], "timestamp": 1_800_000_000})[1]
    assert len(hooks["webhooks"]) == len(lob.TRACKING)
    for d in hooks["webhooks"]:
        h = d["headers"]
        assert lob.verify("fake-lob-secret", h["lob-signature-timestamp"], d["payload"], h["lob-signature"])


@pytest.mark.parametrize(
    ("path", "headers"),
    [("/resend/emails", {"authorization": "Bearer re_looks_real"}),
     ("/anthropic/v1/messages", {"x_api_key": "sk-ant-fake", "anthropic_version": "2023-06-01"}),
     ("/lob/v1/letters", {"authorization": "Basic " + base64.b64encode(b"live_fake:").decode()})],
)
def test_real_looking_keys_are_refused(base: str, path: str, headers: dict[str, str]) -> None:
    """Only stand-in credentials pass (D-557, D-561): a real key in a setting gets the vendor's 401."""
    assert call(base, "POST", path, body={}, **headers)[0] == 401


def test_resend_send_idempotency_and_errors(base: str) -> None:
    mail = {"from": "Ask Hava <jobs@example.com>", "to": ["canary@example.com"], "subject": "Hi", "text": "Hello"}
    one = call(base, "POST", "/resend/emails", body=mail, authorization="Bearer re_stand_in_fake", idempotency_key="e1")
    assert one[0] == 200 and len(one[1]["id"]) == 36
    again = call(base, "POST", "/resend/emails", body=mail, authorization="Bearer re_stand_in_fake", idempotency_key="e1")
    assert again[1] == one[1]
    other = call(base, "POST", "/resend/emails", body={**mail, "subject": "B"}, authorization="Bearer re_stand_in_fake",
                 idempotency_key="e1")
    assert other[0] == 409 and other[1]["name"] == "invalid_idempotent_request"
    assert call(base, "POST", "/resend/emails", body={**mail, "to": []}, authorization="Bearer re_stand_in_fake")[0] == 422
    assert call(base, "POST", "/resend/emails", body=mail)[1]["statusCode"] == 401


def test_lob_letter_idempotency_and_errors(base: str) -> None:
    address = {"name": "Zelda Canary", "address_line1": "1 Test Way", "address_city": "Lake Havasu City",
               "address_state": "AZ", "address_zip": "86403"}
    letter = {"to": address, "from": {**address, "name": "Ask Hava"}, "file": "<html>000000</html>", "color": False}
    status, made, _ = call(base, "POST", "/lob/v1/letters", body=letter, authorization=LOB_AUTH, idempotency_key="l1")
    assert status == 200 and made["id"].startswith("ltr_")
    assert call(base, "POST", "/lob/v1/letters", body=letter, authorization=LOB_AUTH, idempotency_key="l1")[1] == made
    clash = call(base, "POST", "/lob/v1/letters", body={**letter, "color": True}, authorization=LOB_AUTH,
                 idempotency_key="l1")
    assert clash[0] == 409
    assert call(base, "POST", "/lob/v1/letters", body={**letter, "file": ""}, authorization=LOB_AUTH)[0] == 422
    assert call(base, "GET", f"/lob/v1/letters/{made['id']}", authorization=LOB_AUTH)[1] == made
    assert call(base, "POST", "/lob/v1/letters", body=letter)[0] == 401


def ask(base: str, body: dict[str, Any], **headers: str) -> Reply:
    return call(base, "POST", "/anthropic/v1/messages", body=body, **AI_HEADERS, **headers)


READ = {"model": "stand-in", "max_tokens": 512,
        "output_config": {"format": {"type": "json_schema", "schema": ai.SCHEMA}},
        "messages": [{"role": "user", "content": [{"type": "text", "text": "<request>my sink leaks today</request>"}]}]}


def test_ai_reading_over_http_validates_against_the_schema(base: str) -> None:
    """AI-022 (D-492): the reading carries both confidences and no rental field."""
    status, msg, _ = ask(base, READ)
    assert status == 200 and msg["type"] == "message" and msg["stop_reason"] == "end_turn"
    reading = ai.validate(json.loads(msg["content"][0]["text"]))
    assert "rental" not in reading and reading["trade"] == "plumbing" and reading["urgent"] is True
    assert 0 <= reading["trade_confidence"] <= 1 and 0 <= reading["job_type_confidence"] <= 1
    assert ask(base, {**READ, "tools": [{"name": "send_text"}]})[1]["error"]["type"] == "invalid_request_error"
    assert call(base, "POST", "/anthropic/v1/messages/count_tokens", body=READ, **AI_HEADERS)[1]["input_tokens"] > 0
    assert call(base, "POST", "/anthropic/v1/messages", body=READ, x_api_key="k")[0] == 400
    assert call(base, "POST", "/anthropic/v1/messages", body=READ)[0] == 401


@pytest.mark.parametrize(
    ("fault", "status", "kind"),
    [("500", 500, "api_error"), ("504", 504, "timeout_error"), ("529", 529, "overloaded_error"),
     ("429", 429, "rate_limit_error"), ("429-retry-after", 429, "rate_limit_error"),
     ("400-spend-limit", 400, "invalid_request_error"), ("401", 401, "authentication_error"),
     ("403", 403, "permission_error")],
)
def test_ai_fault_modes(base: str, fault: str, status: int, kind: str) -> None:
    """TD-AI-14 (AI-023, D-493): each fault comes back in the API's error envelope."""
    got, body, headers = ask(base, READ, x_stand_in_fault=fault)
    assert (got, body["type"], body["error"]["type"]) == (status, "error", kind)
    assert headers.get("retry-after") == ("3" if fault == "429-retry-after" else None)


def test_ai_refusal_max_tokens_and_planted_outputs(base: str) -> None:
    """TD-AI-14 and TD-AI-10 (AI-021, AI-023, D-493)."""
    assert ask(base, READ, x_stand_in_fault="refusal")[1]["stop_reason"] == "refusal"
    cut = ask(base, READ, x_stand_in_fault="max_tokens")[1]
    assert cut["stop_reason"] == "max_tokens"
    with pytest.raises(json.JSONDecodeError):
        json.loads(cut["content"][0]["text"])
    plain = {**READ, "output_config": None}
    planted = {p: ask(base, plain, x_stand_in_plant=p)[1]["content"][0]["text"] for p in ai.PLANTS}
    assert "cannabis" in planted["restricted-word"] and "https://" in planted["url"] and len(planted["long"]) == 600
    assert ask(base, plain)[1]["content"][0]["text"] == "<request>my sink leaks today</request>"


def test_unknown_route_and_bad_body(base: str) -> None:
    assert call(base, "GET", "/twilio/nothing")[0] == 404
    bad = call(base, "POST", "/_stand_in/twilio/inbound", body={"to": OURS})
    assert bad[0] == 400 and "KeyError" in bad[1]["error"]


@pytest.mark.parametrize("length", ["-1", "abc", "1.5"])
def test_invalid_content_length_is_400(base: str, length: str) -> None:
    conn = http.client.HTTPConnection(urlsplit(base).netloc, timeout=5)
    conn.putrequest("POST", "/_stand_in/twilio/status")
    conn.putheader("content-length", length)
    conn.endheaders()
    resp = conn.getresponse()
    assert resp.status == 400 and "ValueError" in json.loads(resp.read())["error"]
    conn.close()

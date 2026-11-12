"""Contract tests for stand_ins.twilio, from Twilio's public API reference (fake data only)."""

from __future__ import annotations

import base64
import hashlib
import hmac
from datetime import UTC, date, datetime

from stand_ins import twilio

HOMEOWNER = "+19285550142"
OURS = twilio.VALID_FROM


def test_send_returns_201_message_resource() -> None:
    fake = twilio.FakeTwilio()
    status, msg = fake.create_message(HOMEOWNER, OURS, "Hello from Ask Hava")
    assert status == 201
    assert msg["sid"].startswith("SM") and len(msg["sid"]) == 34
    assert msg["status"] == "queued" and msg["direction"] == "outbound-api"
    assert msg["account_sid"].startswith("AC")
    assert msg["num_segments"] == "1" and msg["to"] == HOMEOWNER


def test_errors_use_twilio_envelope() -> None:
    fake = twilio.FakeTwilio()
    status, err = fake.create_message("not-a-number", OURS, "hi")
    assert status == 400 and err["code"] == 21211
    assert err["more_info"] == "https://www.twilio.com/docs/errors/21211"
    assert set(err) == {"code", "message", "more_info", "status"}


def test_magic_numbers() -> None:
    fake = twilio.FakeTwilio()
    for to, code in [
        ("+15005550001", 21211),
        ("+15005550002", 21612),
        ("+15005550003", 21408),
        ("+15005550004", 21610),
        ("+15005550009", 21614),
    ]:
        assert fake.create_message(to, OURS, "x")[1]["code"] == code
    assert fake.messages == []


def test_empty_body_bad_from_and_long_body() -> None:
    fake = twilio.FakeTwilio()
    assert fake.create_message(HOMEOWNER, OURS, "")[1]["code"] == 21602
    assert fake.create_message(HOMEOWNER, "+19285550100", "x")[1]["code"] == 21606
    assert fake.create_message(HOMEOWNER, OURS, "x" * 1601)[1]["code"] == 21617


def test_segment_counting() -> None:
    assert twilio.segment_count("a" * 160) == 1
    assert twilio.segment_count("a" * 161) == 2
    assert twilio.segment_count("a" * 307) == 3
    assert twilio.segment_count("á" * 70) == 1
    assert twilio.segment_count("á" * 71) == 2
    assert twilio.segment_count("[" * 81) == 2


def test_stop_words_opt_out_until_start() -> None:
    fake = twilio.FakeTwilio()
    for word in ["STOP", "stopall", "Unsubscribe", "CANCEL", "end", "QUIT", "revoke", "OptOut"]:
        fake.opted_out.clear()
        fake.receive(HOMEOWNER, OURS, word)
        assert fake.create_message(HOMEOWNER, OURS, "x")[1]["code"] == 21610
    fake.receive(HOMEOWNER, OURS, "start")
    assert fake.create_message(HOMEOWNER, OURS, "x")[0] == 201


def test_opt_out_is_exact_word_only() -> None:
    fake = twilio.FakeTwilio()
    fake.receive(HOMEOWNER, OURS, "please cancel my job")
    assert fake.create_message(HOMEOWNER, OURS, "x")[0] == 201


HOOK = "https://hooks.example.test/webhooks/twilio/status"


def test_status_callback_params() -> None:
    """TD-TXT-36 (TXT-013, SEC-016, D-2761): the callback goes to the create's StatusCallback."""
    fake = twilio.FakeTwilio()
    sid = fake.create_message(HOMEOWNER, OURS, "hi", status_callback=HOOK)[1]["sid"]
    url, params = fake.status_callback(sid, "delivered") or ("", {})
    assert url == HOOK and params["MessageStatus"] == "delivered" and params["MessageSid"] == sid
    _, failed = fake.status_callback(sid, "undelivered", 30003) or ("", {})
    assert failed["ErrorCode"] == "30003"


def test_td_txt_36_no_status_callback_without_one_on_create() -> None:
    """TD-TXT-36 (TXT-013, TXT-014, D-2761): Twilio posts no status for a message created without
    StatusCallback, so the fake records none and leaves the message as it was."""
    fake = twilio.FakeTwilio()
    sid = fake.create_message(HOMEOWNER, OURS, "hi")[1]["sid"]
    assert fake.status_callback(sid, "failed", 30008) is None
    assert fake.callbacks == [] and fake.message(sid)[1]["status"] == "queued"


def test_td_txt_36_list_filters_from_and_date_sent() -> None:
    """TD-TXT-36 (OPS-017, D-2764): Twilio's From and DateSent window (UTC days, inclusive)."""
    fake = twilio.FakeTwilio()
    for day in (10, 11, 12):
        fake.clock = lambda day=day: datetime(2026, 11, day, 23, 30, tzinfo=UTC)
        fake.create_message(HOMEOWNER, OURS, f"Ask Hava: fake text {day}")
    assert fake.sent_log()[0]["date_sent"] == "Tue, 10 Nov 2026 23:30:00 +0000"
    days = [m["body"][-2:] for m in fake.sent_log(sent=(date(2026, 11, 11), date(2026, 11, 11)))]
    assert days == ["11"]
    assert len(fake.sent_log(sent=(date(2026, 11, 11), None))) == 2
    assert len(fake.sent_log(sent=(None, date(2026, 11, 11)))) == 2
    assert len(fake.sent_log(frm=OURS)) == 3 and fake.sent_log(frm="+19285550199") == []


def test_inbound_webhook_fields() -> None:
    fake = twilio.FakeTwilio()
    params = fake.receive(HOMEOWNER, OURS, "my sink leaks")
    assert {"MessageSid", "AccountSid", "From", "To", "Body", "NumMedia"} <= set(params)


def test_signature_matches_documented_algorithm() -> None:
    url = "https://example.com/twilio/sms"
    params = {"To": OURS, "From": HOMEOWNER, "Body": "hi"}
    data = url + "BodyhiFrom" + HOMEOWNER + "To" + OURS
    expected = base64.b64encode(hmac.new(b"tok", data.encode(), hashlib.sha1).digest()).decode()
    assert twilio.compute_signature("tok", url, params) == expected
    assert twilio.validate_signature("tok", url, params, expected)
    assert not twilio.validate_signature("tok", url, {**params, "Body": "changed"}, expected)
    assert not twilio.validate_signature("other", url, params, expected)


def test_verify_flow() -> None:
    fake = twilio.FakeTwilio()
    status, started = fake.start_verification(HOMEOWNER)
    assert status == 201 and started["status"] == "pending" and started["sid"].startswith("VE")
    assert fake.check_verification(HOMEOWNER, "000000")[1]["status"] == "pending"
    ok = fake.check_verification(HOMEOWNER, fake.last_code(HOMEOWNER))[1]
    assert ok["status"] == "approved" and ok["valid"] is True
    assert fake.check_verification(HOMEOWNER, fake.last_code(HOMEOWNER))[0] == 404


def test_verify_errors_and_attempt_limit() -> None:
    fake = twilio.FakeTwilio()
    assert fake.start_verification("bad")[1]["code"] == 60200
    assert fake.start_verification(HOMEOWNER, "pigeon")[1]["code"] == 60200
    assert fake.check_verification(HOMEOWNER, "123456")[0] == 404
    fake.start_verification(HOMEOWNER)
    for _ in range(5):
        fake.check_verification(HOMEOWNER, "bad000")
    status, err = fake.check_verification(HOMEOWNER, fake.last_code(HOMEOWNER))
    assert status == 429 and err["code"] == 60202

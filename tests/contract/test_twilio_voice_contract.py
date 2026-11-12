"""TD-CALL-22 (T378; CALL-022, OPS-028, SEC-016; D-2762, D-2771): the voice stand-in's call create
and callbacks, from Twilio's Calls API reference. A create needs one of `Url`, `Twiml` or
`ApplicationSid`; Twilio posts a call's progress and detection result only to the URLs its create
gave. Fake numbers only."""

from __future__ import annotations

from stand_ins import twilio

HOMEOWNER = "+19285550142"
OURS = "+15005550100"
STATUS = "https://hooks.example.test/webhooks/twilio/voice/status?alert=1"
SCRIPT = "<Response><Hangup/></Response>"
EVENTS = ["initiated", "ringing", "answered", "completed"]


def test_td_call_22_a_create_with_no_script_is_refused() -> None:
    fake = twilio.FakeTwilio()
    status, err = fake.create_call(HOMEOWNER, OURS, {"StatusCallback": STATUS})
    assert status == 400 and err["code"] == 21205
    assert set(err) == {"code", "message", "more_info", "status"}
    assert fake.calls == []
    for field in ("Url", "Twiml", "ApplicationSid"):
        assert fake.create_call(HOMEOWNER, OURS, {field: SCRIPT})[0] == 201


def test_td_call_22_callbacks_go_only_to_the_stored_urls() -> None:
    fake = twilio.FakeTwilio()
    plan = {
        "Twiml": SCRIPT,
        "StatusCallback": STATUS,
        "StatusCallbackEvent": EVENTS,
        "MachineDetection": "DetectMessageEnd",
        "AsyncAmd": "true",
        "AsyncAmdStatusCallback": STATUS,
    }
    status, call = fake.create_call(HOMEOWNER, OURS, plan)
    assert status == 201 and "Twiml" not in call
    posts = fake.progress(call["sid"], "machine_end_beep")
    assert {url for url, _ in posts} == {STATUS}
    shown = [p.get("CallStatus") or p["AnsweredBy"] for _, p in posts]
    assert shown == ["initiated", "ringing", "in-progress", "machine_end_beep", "completed"]
    assert all("AnsweredBy" not in p for _, p in posts if "CallStatus" in p)


def test_td_call_22_a_final_status_posts_no_detection_result() -> None:
    fake = twilio.FakeTwilio()
    plan = {"Twiml": SCRIPT, "StatusCallback": STATUS, "MachineDetection": "DetectMessageEnd"}
    plan["AsyncAmdStatusCallback"] = STATUS
    sid = fake.create_call(HOMEOWNER, OURS, plan)[1]["sid"]
    posts = fake.progress(sid, "no-answer")
    assert [p for _, p in posts] == [
        {
            "CallSid": sid,
            "AccountSid": fake.account_sid,
            "To": HOMEOWNER,
            "From": OURS,
            "CallStatus": "no-answer",
            "Direction": "outbound-api",
        }
    ]


def test_td_call_22_no_callback_without_one() -> None:
    fake = twilio.FakeTwilio()
    sid = fake.create_call(HOMEOWNER, OURS, {"Twiml": SCRIPT})[1]["sid"]
    assert fake.progress(sid, "human") == []
    assert fake.calls[0]["status"] == "completed"

"""D-1155: the plan usage the build action reports about itself, read from its execution file."""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

from runner import state as st, usage_reading as ur
from scripts import pacing as pc

NOW = datetime(2026, 11, 1, 0, 30, tzinfo=timezone.utc)


def event(util: float, reset: int = 1793898000, five: float | None = None) -> dict:
    windows = {"seven_day": {"utilization": util, "resetsAt": reset, "status": "allowed"}}
    if five is not None:
        windows["five_hour"] = {"utilization": five, "resetsAt": reset - 500000, "status": "allowed"}
    return {"type": "rate_limit_event", "rate_limit_info": {"status": "allowed_warning", "resetsAt": reset,
            "rateLimitType": "seven_day", "utilization": util, "unifiedWindows": windows}, "uuid": "x"}


def test_latest_reading_takes_the_last_event_from_an_array_or_jsonl() -> None:
    messages = [{"type": "system", "subtype": "init"}, event(0.51), {"type": "assistant", "message": {}}, event(0.53, five=0.2)]
    found = ur.latest_reading(json.dumps(messages))
    assert found == {"week": {"utilization": 0.53, "resets_at": "2026-11-05T17:00:00Z"},
                     "five_hour": {"utilization": 0.2, "resets_at": "2026-10-30T22:06:40Z"}}
    assert ur.latest_reading("\n".join(json.dumps(m) for m in messages) + "\nnot json\n") == found
    assert ur.latest_reading(json.dumps([{"type": "assistant"}])) is None and ur.latest_reading("") is None
    bare = {"type": "rate_limit_event", "rate_limit_info": {"rateLimitType": "seven_day", "utilization": 1.4, "resetsAt": 1793898000}}
    assert ur.latest_reading(json.dumps([bare])) == {"week": {"utilization": 1.0, "resets_at": "2026-11-05T17:00:00Z"}}
    five = {"type": "rate_limit_event", "rate_limit_info": {"rateLimitType": "five_hour", "utilization": 0.3, "resetsAt": 1793400000}}
    both = ur.latest_reading(json.dumps([bare, five]))
    assert both and both["week"]["utilization"] == 1.0 and both["five_hour"]["utilization"] == 0.3, "a later event naming one window keeps the other"


def test_record_and_read_file(tmp_path: Path) -> None:
    path = tmp_path / "execution.json"
    path.write_text(json.dumps([event(0.6)]), encoding="utf-8")
    found = ur.read_file(path)
    assert found and found["week"]["utilization"] == 0.6 and ur.read_file(tmp_path / "missing.json") is None
    state = ur.record(st.new_state(), found, NOW)
    assert state["usage"]["session_reading"] == {"at": "2026-11-01T00:30:00Z", "week": found["week"]}


def test_pacing_uses_a_fresh_session_reading_for_the_windows_it_holds() -> None:
    cfg = {"budget_tokens_five_hour": 1_000_000, "budget_tokens_week": 10_000_000, "headroom": 0.1,
           "window_hours": {"five_hour": 5, "week": 168}, "weights": {}, "bytes_per_token": 4, "turn_factor": 3.0,
           "model_multiplier": {"opus": 1.6}, "calibrate_after": 3}
    reading = {"week": {"utilization": 0.6, "resets_at": "2026-11-05T17:00:00Z"}}
    state = ur.record({"tasks": [], "usage": {"windows": {"five_hour": {"start": pc.iso(NOW), "tokens": 250_000},
                                                            "week": {"start": pc.iso(NOW - timedelta(days=2)), "tokens": 1_000_000}}}},
                      reading, NOW)
    stand, source = pc.standing(state, cfg, NOW, None)
    assert source.startswith("session_reading 2026-11-01T00:30:00Z (week)")
    assert stand["week"]["used"] == 6_000_000 and stand["week"]["resets_at"] == "2026-11-05T17:00:00Z"
    assert stand["five_hour"]["used"] == 250_000, "a window the reading lacks keeps the ledger"
    stale, source = pc.standing(state, cfg, NOW + timedelta(hours=7), None)
    assert source == "ledger" and stale["week"]["used"] == 1_000_000
    quota = {"five_hour": {"utilization": 0.1}, "week": {"utilization": 0.2}}
    assert pc.standing(state, cfg, NOW, quota)[1] == "quota_cmd", "a probe reading still wins"
    past = ur.record(state, {"week": {"utilization": 0.9, "resets_at": pc.iso(NOW - timedelta(minutes=1))},
                             "five_hour": {"utilization": 0.9, "resets_at": pc.iso(NOW + timedelta(hours=1))}}, NOW)
    stand, source = pc.standing(past, cfg, NOW, None)
    assert "(five_hour)" in source and stand["week"]["used"] == 1_000_000, "a window already reset is dropped, the other kept"

"""The plan usage the build action reports about itself, for pacing (D-1155).

The Claude Code action streams `rate_limit_event` messages while it runs, each carrying the plan's
own windows as the API reported them ({"rate_limit_info": {"unifiedWindows": {"seven_day":
{"utilization": 0.51, "resetsAt": 1793898000, ...}, "five_hour": {...}}}}), and writes every message
of the session to its execution file. The probe (scripts/quota_probe.py) cannot reach that figure
from the runner's subprocess, which has no login; the execution file is read after the session ends,
needs no token, and holds the same number Casey reads under /usage.

`python3 -m runner --usage-reading FILE` (the backup-runner job's step after the action) keeps the
LAST such event as state.usage.session_reading = {"at": the time of the read, "week": {"utilization",
"resets_at"}, "five_hour": {...} when the event has it}; scripts/pacing.py `session_reading` uses it
while it is fresh. A file with no such event changes nothing.
"""

from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from runner import state as st

WINDOWS = {"week": "seven_day", "five_hour": "five_hour"}


def _messages(text: str) -> list[Any]:
    """The messages of an execution file: a JSON array, or one JSON object per line (either is read)."""
    try:
        found = json.loads(text)
        return found if isinstance(found, list) else [found]
    except ValueError:
        out: list[Any] = []
        for line in text.splitlines():
            try:
                out.append(json.loads(line))
            except ValueError:
                continue
        return out


def _window(raw: Any) -> dict[str, Any] | None:
    if not isinstance(raw, dict) or not isinstance(raw.get("utilization"), (int, float)):
        return None
    reset = raw.get("resetsAt")
    out: dict[str, Any] = {"utilization": max(0.0, min(1.0, float(raw["utilization"])))}
    if isinstance(reset, (int, float)):
        out["resets_at"] = st.iso(datetime.fromtimestamp(float(reset), tz=timezone.utc))
    elif isinstance(reset, str):
        out["resets_at"] = reset
    return out


def latest_reading(text: str) -> dict[str, Any] | None:
    """The latest value of each window across the `rate_limit_event` messages of an execution file's
    text, in pacing's names, or None when the file holds none. `unifiedWindows` is read first; a bare
    `rateLimitType` with a `utilization` names one window the same way."""
    found: dict[str, Any] = {}
    for message in _messages(text):
        if not isinstance(message, dict) or message.get("type") != "rate_limit_event":
            continue
        info = message.get("rate_limit_info") if isinstance(message.get("rate_limit_info"), dict) else message
        unified = info.get("unifiedWindows") if isinstance(info.get("unifiedWindows"), dict) else {}
        for ours, theirs in WINDOWS.items():
            window = _window(unified.get(theirs)) or (_window(info) if info.get("rateLimitType") == theirs else None)
            if window is not None:
                found[ours] = window  # the latest value per window; an event naming one window keeps the other
    return found or None


def record(state: st.State, reading: dict[str, Any], now: datetime) -> st.State:
    """The state with `usage.session_reading` set to `reading` stamped `at` now."""
    out = json.loads(json.dumps(state))
    usage = out.setdefault("usage", {})
    usage["session_reading"] = {"at": st.iso(now), **reading}
    return out


def read_file(path: Path) -> dict[str, Any] | None:
    """`latest_reading` over a file; None when it cannot be read."""
    try:
        return latest_reading(path.read_text(encoding="utf-8"))
    except OSError:
        return None

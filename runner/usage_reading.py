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
while it is fresh. A file with no such event changes nothing. A window the API reports with `status`
"rejected" keeps that status (runner/model_window.py reads it as the limit, D-2225).

`python3 -m runner.usage_reading --at-limit FILE` exits 0 when the file's last reading shows the plan's
limit reached (`at_limit`), else 1: backup-runner.yml's safety hand-in then hands in `usage_limit` instead of
`tests_failed`, so a session the limit stopped mid-task costs no attempt (D-2225).
"""

from __future__ import annotations

import json
import sys
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


def messages(text: str) -> list[Any]:
    """`_messages` for other readers of the execution file (runner/model_window.py, D-2225)."""
    return _messages(text)


def _window(raw: Any) -> dict[str, Any] | None:
    if not isinstance(raw, dict):
        return None
    rejected = raw.get("status") == "rejected"
    if not isinstance(raw.get("utilization"), (int, float)) and not rejected:
        return None
    reset = raw.get("resetsAt")
    used = float(raw["utilization"]) if isinstance(raw.get("utilization"), (int, float)) else 1.0
    out: dict[str, Any] = {"utilization": max(0.0, min(1.0, used))}
    if rejected:
        out["status"] = "rejected"  # the API refused for this window: the limit, whatever the utilization says
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


def at_limit(text: str) -> bool:
    """Whether the last reading of an execution file's text shows the plan's limit reached: a window at 100%
    or rejected, or a `rate_limit_event` whose own `status` is "rejected" (D-2225)."""
    found = latest_reading(text) or {}
    if any(w.get("utilization", 0) >= 1.0 or w.get("status") == "rejected" for w in found.values() if isinstance(w, dict)):
        return True
    events = [m for m in _messages(text) if isinstance(m, dict) and m.get("type") == "rate_limit_event"]
    info = events[-1].get("rate_limit_info", events[-1]) if events else {}
    return isinstance(info, dict) and info.get("status") == "rejected"


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


def main(argv: list[str] | None = None) -> int:
    """`--at-limit FILE`: exit 0 when the file shows the limit reached, 1 otherwise or when unreadable."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 2 or args[0] != "--at-limit":
        print("usage: python3 -m runner.usage_reading --at-limit FILE", file=sys.stderr)
        return 2
    try:
        text = Path(args[1]).read_text(encoding="utf-8")
    except OSError:
        return 1
    return 0 if at_limit(text) else 1


if __name__ == "__main__":
    sys.exit(main())

"""A worker session killed by an Anthropic API error uses no attempt (D-3443, BUILD_BRIEF.md:271).

On November 13, 2026 three sessions (T255, T428, T312) died at the API's edge ("API Error: 403 ...
authentication_failed", then the action's "result is_error:true") with no turn of work done; the D-503
safety hand-in recorded "session ended without handing in" and each lost an attempt for a fault outside
the task (D-3442 gave T255's back by hand). This module makes that case cost nothing, narrowly:

- `signature(text)`: the execution file's LAST `result` message must say `is_error: true` and its `result`
  string (never `errors`, `permission_denials` or a tool's output, which may quote these words) must
  name an API status 401, 403, 429 or 5xx, or the error type `authentication_failed` or `overloaded_error`. The answer is a short label built only
  from that status and error type ("API Error 403 authentication_failed"), never the raw text, so no
  header, token or request detail can reach the state, the log or an outbox item.
- `python3 -m runner.api_death FILE` prints the label and exits 0 (backup-runner.yml's safety step then
  hands in `--outcome api_error`), else exits 1; 2 for a bad call.
- `unpushed(handoff, tip_now)`: `--after` (runner/__main__.py `api_outcome`) also requires that the
  session never handed in (no held pre-hand-in round in the hand-off) and that origin's task branch is
  where the claim left it (the hand-off's `remote_at_start`); anything unknown counts as pushed.
- `settle(state, tid, signature, now)`: the death is noted in the top-level `api_deaths` (one entry per
  death, keyed "<task>@<at>", kept for `WINDOW`); the task goes back to QUEUED with `attempts` unchanged
  and `api_death_requeues` one higher, while it has had fewer than `FREE_REQUEUES` such requeues in a
  row; the next death counts as a failed attempt. Any other end of an attempt clears the run (`end_run`).
- `ALARM_COUNT` deaths across lanes within `WINDOW` write the outbox item `api-deaths` (`alarm_note`),
  so Casey hears of a credential or outage problem, at most once per `WINDOW` (`alarm_due`, the state's
  `api_deaths_alarm_at`), so a dead token does not re-ping him on every later death.
"""

from __future__ import annotations

import copy
import re
import sys
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from runner import state as st

API_ERROR = "api_error"  # the `--after --outcome` for such a death, and the history mark
FREE_REQUEUES = 2  # consecutive free requeues per task; the next API death counts as an attempt (D-3443)
WINDOW = timedelta(minutes=60)  # the span the global guard counts deaths in, across lanes
ALARM_COUNT = 3  # deaths within WINDOW that write the outbox item
OUTBOX_ID = "api-deaths"
DEATHS = "api_deaths"  # the state's top-level record of recent deaths
ALARMED = "api_deaths_alarm_at"  # when the outbox item was last written
RUN = "api_death_requeues"  # a task's consecutive free requeues
LOG = "session died of an Anthropic API error: "  # the safety hand-in's `--log`, followed by the label
STATUSES = ("401", "403", "429")  # and any 5xx
STATUS = re.compile(r"API Error:? (\d{3})\b")  # the colon is absent in a label read again
TYPES = ("authentication_failed", "overloaded_error")
TYPE = re.compile(r"\b(authentication_failed|authentication_error|overloaded_error|rate_limit_error|api_error)\b")


def label(text: str) -> str:
    """The safe label of one text: 'API Error <status> <type>' from a matching status or type, else ''."""
    code = next((c for c in STATUS.findall(text) if c in STATUSES or c.startswith("5")), "")
    kind = next((k for k in TYPE.findall(text) if code or k in TYPES), "")
    if not code and not kind:
        return ""
    return " ".join(p for p in ("API Error", code, kind) if p)


def signature(text: str) -> str:
    """The label of an execution file whose last `result` message is an API-error death; '' otherwise."""
    from runner import usage_reading

    results = [m for m in usage_reading.messages(text) if isinstance(m, dict) and m.get("type") == "result"]
    last = results[-1] if results else {}
    text = last.get("result")
    if last.get("is_error") is not True or not isinstance(text, str):
        return ""
    return label(text)  # only the result's own text: `errors` and `permission_denials` carry tool input


def unpushed(handoff: dict[str, Any], tip_now: str | None) -> bool:
    """Whether the attempt neither handed in nor moved origin's task branch: the hand-off records no held
    pre-hand-in round and a known `remote_at_start` ('' when origin had no branch) equal to `tip_now`
    (None: git could not say, which counts as moved)."""
    start = handoff.get("remote_at_start")
    if handoff.get("prehandin_holds") or not isinstance(start, str) or tip_now is None:
        return False
    return tip_now == start


def end_run(state: st.State, tid: str) -> st.State:
    """The state with the task's run of free requeues cleared, after an attempt that ended any other way."""
    if RUN not in st.entry(state, tid):
        return state
    out = copy.deepcopy(state)
    next(t for t in out["tasks"] if t["id"] == tid).pop(RUN, None)
    return out


def note(state: st.State, tid: str, label: str, now: datetime) -> st.State:
    """The state with this death added to `api_deaths` and entries older than WINDOW dropped."""
    kept = {k: v for k, v in (state.get(DEATHS) or {}).items() if isinstance(v, dict) and _recent(v, now)}
    kept[f"{tid}@{st.iso(now)}"] = {"task": tid, "at": st.iso(now), "signature": label}
    return dict(state, **{DEATHS: kept})


def _recent(death: dict[str, Any], now: datetime) -> bool:
    try:
        return now - st.parse(str(death.get("at"))) < WINDOW
    except ValueError:
        return False


def recent(state: st.State, now: datetime) -> list[dict[str, Any]]:
    """The deaths recorded within WINDOW before `now`, oldest first."""
    found = [d for d in (state.get(DEATHS) or {}).values() if isinstance(d, dict) and _recent(d, now)]
    return sorted(found, key=lambda d: str(d.get("at")))


def alarm_due(state: st.State, now: datetime) -> bool:
    """Whether ALARM_COUNT deaths fall within WINDOW and no alarm was written within WINDOW (an unreadable
    marker counts as none)."""
    if len(recent(state, now)) < ALARM_COUNT:
        return False
    return not _recent({"at": state.get(ALARMED)}, now)


def settle(state: st.State, tid: str, label: str, now: datetime) -> st.State | None:
    """The death noted, and the task QUEUED with `attempts` unchanged while its run of free requeues is
    below FREE_REQUEUES; None (with the death noted only in the caller's count) when it must count."""
    runs = int(st.entry(state, tid).get(RUN, 0))
    if runs >= FREE_REQUEUES:
        return None
    why = (f"the session died of an Anthropic API error ({label}) before handing in, with nothing pushed; "
           f"no attempt used (free requeue {runs + 1} of {FREE_REQUEUES} in a row, D-3443)")
    return st.with_task(state, tid, status=st.QUEUED, last_failure=why, **{RUN: runs + 1})


def alarm_note(deaths: list[dict[str, Any]]) -> tuple[str, str]:
    """The outbox item's title and body for ALARM_COUNT or more deaths within WINDOW."""
    rows = "\n".join(f"- {d.get('at')}: {d.get('task')} ({d.get('signature')})" for d in deaths)
    body = (f"{len(deaths)} worker sessions died of an Anthropic API error within {int(WINDOW.total_seconds() // 60)} "
            f"minutes, across lanes:\n\n{rows}\n\nThis usually means the runner's Claude login (the action's OAuth token) "
            "was refused or the API was down. Each death went back to the queue without using an attempt, at most "
            f"{FREE_REQUEUES} times in a row per task (D-3443). If they keep coming, check the token and the Anthropic "
            "status page; nothing else needs you.")
    return f"{len(deaths)} worker sessions died of an API error within an hour", body


def main(argv: list[str] | None = None) -> int:
    """`FILE`: print the death's label and exit 0 when the execution file shows an API-error death; 1 when
    it does not or cannot be read; 2 for a bad call."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        print("usage: python3 -m runner.api_death EXECUTION_FILE", file=sys.stderr)
        return 2
    try:
        found = signature(Path(args[0]).read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError):
        return 1
    if found:
        print(found)
    return 0 if found else 1


if __name__ == "__main__":
    sys.exit(main())

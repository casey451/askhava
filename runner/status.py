"""The owner's dashboard at the top of STATUS.md (D-1462): what Casey reads on GitHub without asking anyone.

Pure functions over the state, the task graph and a few readings the runner takes (the time of the
last cycle-log line, the strict plan check, recent plan and harness merges); `runner.github_env.
render_status` puts these lines above the progress block (`scripts/progress.py --markdown`, governance 6)
and the older sections. Nothing here reads homeowner data: the state holds task records, checks and
token counts only, and every failure text shown is cut short and passed through the caller's `redact`.

- `dashboard`: the verdict line, then done count, stage, lane, week budget, last run and keeper.
- `last_day`: tasks whose attempt ended in the last 24 hours (title from kit/tasks.json, result,
  attempt, duration, and the retry reason: the first 120 characters of the task's last failure, shown
  on a retry or a failed row of its latest attempt only, since older failures are not kept).
- `attention_extra`: the build hold, the strict plan check and the keeper's lines for "Needs attention".

The `keeper` key ({next_wake_at, last_run_at}, kit/state_schema.json) is written by the self-scheduling
runner (D-1451); while it is absent the dashboard says "keeper: unknown".
"""

from __future__ import annotations

import re
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from typing import Any

from runner import state as st
from runner.state import State

PHOENIX = timezone(timedelta(hours=-7))  # Arizona keeps standard time all year
QUIET_RUN = timedelta(minutes=90)  # a last run older than this gets a warning line
DAY = timedelta(hours=24)
REASON_CHARS = 120
TASK_ID = re.compile(r"^T\d{3}$")
Redact = Callable[[str], str]


def _plain(text: str) -> str:
    return text


def _at(stamp: Any) -> datetime | None:
    try:
        return st.parse(str(stamp)) if stamp else None
    except ValueError:
        return None


def clock(moment: datetime | None) -> str:
    """'Thu 2026-11-05 17:00Z (10:00 am Phoenix)', or 'unknown'."""
    if moment is None:
        return "unknown"
    local = moment.astimezone(PHOENIX)
    hour = local.strftime("%I:%M %p").lstrip("0").lower()
    return f"{moment.astimezone(timezone.utc):%a %Y-%m-%d %H:%MZ} ({hour} Phoenix)"


def _short(moment: datetime | None) -> str:
    return f"{moment.astimezone(timezone.utc):%H:%MZ}" if moment else "unknown"


def _age(now: datetime, moment: datetime | None) -> str:
    if moment is None:
        return "unknown"
    minutes = max(0, int((now - moment).total_seconds() // 60))
    return f"{minutes} min" if minutes < 120 else f"{minutes // 60} h {minutes % 60} min"


def build_tasks(graph: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The T### tasks that are not owner gates: the denominator of the done count (as scripts/progress.py)."""
    return [t for t in graph if TASK_ID.match(str(t.get("id"))) and t.get("type") != "gate"]


def done_count(state: State, graph: list[dict[str, Any]]) -> tuple[int, int]:
    ids = {t["id"] for t in build_tasks(graph)}
    done = sum(1 for t in state.get("tasks", []) if t.get("id") in ids and t.get("status") == st.DONE)
    return done, len(ids)


def _stage_key(stage: str) -> tuple[int, float, str]:
    try:
        return (0, float(stage), stage)
    except ValueError:
        return (1, 0.0, stage)


def current_stage(state: State, graph: list[dict[str, Any]]) -> str:
    """The lowest stage with a build task not yet DONE or SKIPPED, and the stage proofs recorded."""
    status = {t.get("id"): t.get("status") for t in state.get("tasks", [])}
    open_stages = {str(t.get("stage")) for t in build_tasks(graph) if status.get(t["id"]) not in st.SATISFIED}
    proofs = sorted((state.get("stage_proofs") or {}), key=_stage_key)
    stage = f"stage {min(open_stages, key=_stage_key)}" if open_stages else "every stage's tasks done"
    return f"{stage}; stage proofs recorded: {', '.join(proofs) or 'none'}"


def _last_finish(state: State) -> datetime | None:
    stamps = [_at(h.get("ended_at")) for t in state.get("tasks", []) for h in t.get("history") or []]
    stamps += [_at(t.get("merged_at")) for t in state.get("tasks", [])]
    found = [s for s in stamps if s]
    return max(found) if found else None


def lane_line(state: State) -> str:
    """Each held lane's task, claim time and lease expiry; or idle since the last attempt ended."""
    tasks = {t.get("id"): t for t in state.get("tasks", [])}
    held = [(lane, lease) for lane, lease in sorted(st.leases_of(state).items()) if lease]
    if not held and state.get("lease"):
        held = [(st.LANE_ONE, state["lease"])]
    if not held:
        return f"idle since {_short(_last_finish(state))} (no task in progress)"
    parts = []
    for lane, lease in held:
        claimed = _at((tasks.get(lease.get("task")) or {}).get("attempt_started_at"))
        parts.append(f"lane {lane}: {lease.get('task')} claimed {_short(claimed)}, lease until {_short(_at(lease.get('expires')))}")
    return "; ".join(parts)


def week(state: State) -> tuple[str, datetime | None]:
    """The week's reading ('93% used, 8.0M tokens left') and its reset time."""
    reading = ((state.get("usage") or {}).get("session_reading") or {}).get("week") or {}
    standing = ((state.get("pacing") or {}).get("standing") or {}).get("week") or {}
    reset = _at(reading.get("resets_at") or standing.get("resets_at"))
    if isinstance(reading.get("utilization"), (int, float)):
        used = f"{round(reading['utilization'] * 100)}% used"
    elif standing.get("budget"):
        used = f"{round(100 * standing.get('used', 0) / standing['budget'])}% used (ledger)"
    else:
        return "no reading yet", reset
    left = standing.get("left")
    return used + (f", {left / 1e6:.1f}M tokens left" if isinstance(left, (int, float)) else ""), reset


def _waiting_until(state: State, now: datetime) -> datetime | None:
    pacing = state.get("pacing") or {}
    until = _at(pacing.get("waiting_until")) if pacing.get("decision") == "wait" else None
    return until if until and until > now else None


def verdict(state: State, graph: list[dict[str, Any]], now: datetime) -> str:
    """One line: what the build is doing now, the done count and the week."""
    done, total = done_count(state, graph)
    used, reset = week(state)
    tail = f"{done} of {total} done; week {used.split(',')[0]}, resets {clock(reset)}"
    lease = state.get("lease") or next((v for v in st.leases_of(state).values() if v), None)
    if state.get("main_red"):
        head = "Stopped: main is red, only FIX-MAIN runs"
    elif state.get("build_hold"):
        head = f"On hold: {state['build_hold']}"
    elif lease:
        task = next((t for t in state.get("tasks", []) if t.get("id") == lease.get("task")), {})
        head = f"Building: {lease.get('task')} in progress since {_short(_at(task.get('attempt_started_at')))}"
    elif _waiting_until(state, now):
        head = f"Waiting for tokens until {_short(_waiting_until(state, now))}"
    else:
        head = f"Idle since {_short(_last_finish(state))}"
    return f"**{head}; {tail}.**"


def keeper_line(state: State) -> str:
    keeper = state.get("keeper")
    if not isinstance(keeper, dict):
        return "unknown (the keeper has not recorded a wake yet)"
    return f"last ran {_short(_at(keeper.get('last_run_at')))}, next wake {_short(_at(keeper.get('next_wake_at')))}"


def dashboard(state: State, graph: list[dict[str, Any]], now: datetime, last_run: datetime | None) -> list[str]:
    """The lines above the progress block."""
    done, total = done_count(state, graph)
    used, reset = week(state)
    lines = [
        verdict(state, graph, now), "",
        f"- **Tasks done:** {done} of {total} build tasks (owner gates not counted)",
        f"- **Stage:** {current_stage(state, graph)}",
        f"- **Lane:** {lane_line(state)}",
        f"- **Week:** {used}; resets {clock(reset)}",
        f"- **Last run:** {_short(last_run)}, {_age(now, last_run)} before this update",
        f"- **Keeper:** {keeper_line(state)}",
    ]
    if last_run is None or now - last_run > QUIET_RUN:
        lines.append(f"- **WARNING:** no runner cycle logged for over {int(QUIET_RUN.total_seconds() // 60)} minutes before this update")
    return lines


def _duration(row: dict[str, Any]) -> str:
    start, end = _at(row.get("started_at")), _at(row.get("ended_at"))
    return _age(end, start) if start and end else "?"


def _cell(text: str, width: int = REASON_CHARS) -> str:
    """One table cell: lines joined with ' / ', cut to `width` characters, then '|' escaped."""
    return " / ".join(part.strip() for part in text.splitlines() if part.strip())[:width].replace("|", "\\|")


def _reason(task: dict[str, Any], row: dict[str, Any], latest: bool, redact: Redact) -> str:
    retried = int(row.get("attempt") or 1) > 1 or row.get("result") != "done"
    failure = str(task.get("last_failure") or "")
    return _cell(redact(failure)) if latest and retried and failure else ""


def last_day(state: State, graph: list[dict[str, Any]], now: datetime, redact: Redact = _plain) -> list[str]:
    """The "Last 24 hours" table: one row per attempt that ended in the last day, newest first."""
    titles = {t["id"]: str(t.get("title", "")) for t in graph}
    rows = []
    for task in state.get("tasks", []):
        history = task.get("history") or []
        for index, row in enumerate(history):
            ended = _at(row.get("ended_at"))
            if ended and now - ended <= DAY:
                reason = _reason(task, row, index == len(history) - 1, redact)
                rows.append((ended, f"| {task['id']} | {_cell(titles.get(task['id'], ''), 60)} | {row.get('result')} | "
                                    f"{row.get('attempt', '?')} | {_duration(row)} | {_short(ended)} | {reason} |"))
    if not rows:
        return ["- no attempt ended in the last 24 hours"]
    head = ["| Task | Title | Result | Attempt | Took | Ended | Retry reason |", "|---|---|---|---|---|---|---|"]
    return head + [line for _, line in sorted(rows, key=lambda r: r[0], reverse=True)]


def attention_extra(state: State, strict: str) -> list[str]:
    """Lines for "Needs attention" beyond blocks, plan gaps, deviations and red main: the build hold and
    the strict plan check (`gen_prompts.py --check --strict`; '' when green or not run)."""
    lines = []
    if state.get("build_hold"):
        lines.append(f"- BUILD HOLD: {state['build_hold']}")
    if strict:
        lines.append(f"- Strict plan check red: {strict}")
    return lines

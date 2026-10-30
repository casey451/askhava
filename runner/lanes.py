"""Two lanes, stage D (D-620): which lane a cycle may claim on, which task may run beside another, and
the automatic fall back to one lane.

Occupancy lives in runner/state.py (`in_flight`: a live lease, or MERGE_PENDING on the task's own
`lane`). This module is the policy over it, pure functions over the state and the task graph:

- `claim_lane`: at most `lanes` tasks are in flight (the effective count: 1 while `lanes_fallback` is
  set); a run started for one lane (RUNNER_LANE, from backup-runner.yml) claims only that lane, so a
  run never takes a lane another run's concurrency group serves; a run with no lane takes the first
  free one.
- `compatible`: beside a task in flight, a task may be claimed only when its dependencies are all DONE
  or SKIPPED (no proof-waiting exception, D-551), it is in the same stage as every task in flight, its
  owned paths overlap none of theirs (a task owning a locked file also owns `kit/plan.lock`, which it
  must rewrite), and neither it nor any task in flight is a stage's cleanup task, which runs alone.
  FIX-MAIN is exempt: while `main` is red nothing else is eligible, and it must not wait.
- strikes: a harness-type failure on a lane above 1 (a rejected branch push, an attempt that lost its
  writes, a required check that failed after another lane's merge, an update merge that conflicted or
  could not be pushed) is a strike; a task on that lane reaching DONE, or failing for its own work,
  clears them. Two strikes in a row set `lanes_fallback`, and the runner keeps to one lane until a
  person runs `python3 -m runner --restore-lanes`, so it never flaps between one and two.
"""

from __future__ import annotations

import copy
from collections.abc import Callable, Iterable
from datetime import datetime
from typing import Any

from runner import state as st
from runner.state import State

STRIKES_TO_FALL_BACK = 2  # consecutive harness-type failures on a lane above 1 (D-620)
STRIKES_KEPT = 5  # the last strikes kept per lane as evidence
PLAN_LOCK = "kit/plan.lock"
Locked = Callable[[str], bool]


def effective_lanes(lanes: int, state: State) -> int:
    """The lanes a cycle may fill: the configured count, or 1 while `lanes_fallback` is set."""
    return 1 if state.get("lanes_fallback") else max(int(lanes), 1)


def free_lanes(lanes: int, state: State, now: datetime) -> list[str]:
    """Numbered lanes 1..effective with no task in flight; [] once `effective` tasks are in flight."""
    count = effective_lanes(lanes, state)
    busy = st.in_flight(state, now)
    if len(busy) >= count:
        return []
    return [str(n) for n in range(1, count + 1) if str(n) not in busy.values()]


def claim_lane(lanes: int, state: State, now: datetime, hint: str | None) -> str | None:
    """The lane this cycle may claim on, or None. A run started for a lane claims only that lane."""
    free = free_lanes(lanes, state, now)
    if hint:
        return hint if hint in free else None
    return free[0] if free else None


def owned(task: dict[str, Any], locked: Locked | None) -> list[str]:
    """The paths a task owns: its `owns`, plus kit/plan.lock when it owns a locked file (it rewrites the lock)."""
    paths = [str(p) for p in task.get("owns") or []]
    if locked is not None and any(locked(p) for p in paths):
        paths.append(PLAN_LOCK)
    return paths


def overlaps(first: Iterable[str], second: Iterable[str]) -> list[str]:
    """Pairs of owned paths that cover the same file ("dir/" covers everything under it)."""
    def meet(a: str, b: str) -> bool:
        return a == b or (a.endswith("/") and b.startswith(a)) or (b.endswith("/") and a.startswith(b))

    return sorted({f"{a} ~ {b}" for a in first for b in second if meet(a, b)})


def refusal(task: dict[str, Any], others: list[dict[str, Any]], done: set[str], locked: Locked | None) -> str:
    """Why `task` must not run beside the tasks in flight `others`; '' when it may (D-620)."""
    if task.get("id") == st.FIX_MAIN or not others:
        return ""
    if task.get("type") == "cleanup":
        return "a stage's cleanup task runs alone"
    for other in others:
        if other.get("type") == "cleanup":
            return f"{other['id']} is a cleanup task, which runs alone"
        if str(other.get("stage")) != str(task.get("stage")):
            return f"{other['id']} is in stage {other.get('stage')}, not stage {task.get('stage')}"
        clash = overlaps(owned(task, locked), owned(other, locked))
        if clash:
            return f"owns what {other['id']} owns ({clash[0]})"
    waiting = [d for d in task.get("deps") or [] if d not in done]
    return f"dependencies not DONE: {', '.join(waiting)}" if waiting else ""


def compatible_ids(
    ids: list[str], state: State, now: datetime, task_def: Callable[[str], dict[str, Any]], locked: Locked | None
) -> list[str]:
    """The eligible ids that may be claimed beside the tasks now in flight, in order."""
    flying = [task_def(tid) for tid in st.in_flight(state, now)]
    done = {t["id"] for t in state["tasks"] if t.get("status") in st.SATISFIED}
    return [i for i in ids if not refusal(task_def(i), [t for t in flying if t["id"] != i], done, locked)]


def strike(state: State, lane: str, why: str, now: datetime) -> State:
    """Record a harness-type failure on a lane above 1; the second in a row sets `lanes_fallback`."""
    if lane in (st.LANE_ONE, st.FIX_LANE) or not lane:
        return state
    out = copy.deepcopy(state)
    strikes = dict(out.get("lane_strikes") or {})
    mine = dict(strikes.get(lane) or {"count": 0, "last": []})
    mine["count"] = int(mine.get("count", 0)) + 1
    mine["last"] = [*mine.get("last", []), {"at": st.iso(now), "why": why[:300]}][-STRIKES_KEPT:]
    strikes[lane] = mine
    out["lane_strikes"] = strikes
    if mine["count"] >= STRIKES_TO_FALL_BACK and not out.get("lanes_fallback"):
        out["lanes_fallback"] = {"at": st.iso(now), "lane": lane, "why": [s["why"] for s in mine["last"][-STRIKES_TO_FALL_BACK:]]}
    return out


def clear_strikes(state: State, lane: str) -> State:
    """A task on the lane reached DONE or failed for its own work: the run of strikes is broken."""
    if not (state.get("lane_strikes") or {}).get(lane, {}).get("count"):
        return state
    out = copy.deepcopy(state)
    out["lane_strikes"][lane] = dict(out["lane_strikes"][lane], count=0)
    return out


def fell_back(before: State, after: State) -> bool:
    """Whether this write is the one that set `lanes_fallback` (the cycle then writes an outbox item)."""
    return bool(after.get("lanes_fallback")) and not before.get("lanes_fallback")


def fallback_note(state: State) -> str:
    """The outbox body for a fall back to one lane: why, and how to return to two."""
    fell = state.get("lanes_fallback") or {}
    reasons = "\n".join(f"- {why}" for why in fell.get("why", []))
    return (f"Lane {fell.get('lane')} had {STRIKES_TO_FALL_BACK} harness-type failures in a row, so the runner now "
            f"works one task at a time (kit/runner.json still says more):\n{reasons}\n\nTasks already in flight "
            "finish normally. Once the cause is fixed, run `python3 -m runner --restore-lanes` to use every lane "
            "again (D-620).")


def restore(state: State) -> State | None:
    """`--restore-lanes`: clear `lanes_fallback` and the strikes; None when there is nothing to clear."""
    if not state.get("lanes_fallback"):
        return None
    out = copy.deepcopy(state)
    history = [*out.get("lanes_fallback_history", []), out.pop("lanes_fallback")][-10:]
    out["lanes_fallback_history"] = history
    out.pop("lane_strikes", None)
    return out

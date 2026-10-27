"""Task state, leases and eligibility (BUILD_BRIEF.md:215-224, 234).

State is a plain dict, the shape of kit/state.json on claude/kit-state:
    {"version": int, "build_hold": bool, "main_red": bool,
     "tasks": {"T001": {"status": str, "attempts": int, "lease": {...}, ...}}}
Every function returns a new state; nothing mutates its argument.
"""

from __future__ import annotations

import copy
from datetime import datetime, timedelta
from typing import Any

State = dict[str, Any]
Task = dict[str, Any]

PENDING = "PENDING"
IN_PROGRESS = "IN_PROGRESS"
MERGE_PENDING = "MERGE_PENDING"
DONE = "DONE"
BLOCKED = "BLOCKED"
RETRY_AFTER_RESET = "RETRY_AFTER_RESET"
RETRY_LATER = "RETRY_LATER"
WAITING_FOR_OWNER = "WAITING_FOR_OWNER"
SKIPPED = "SKIPPED"

LEASE_HOURS = 3  # BUILD_BRIEF.md:218
RENEW_MINUTES = 30  # BUILD_BRIEF.md:218
FIX_MAIN = "FIX-MAIN"  # the only task while main is red (BUILD_BRIEF.md:240)
RETRYABLE = (PENDING, RETRY_AFTER_RESET, RETRY_LATER)
SATISFIED = (DONE, SKIPPED)


class LeaseError(Exception):
    """A claim or renewal that must not go ahead; the runner exits."""


def new_state() -> State:
    return {"version": 0, "build_hold": False, "main_red": False, "tasks": {}}


def iso(moment: datetime) -> str:
    return moment.isoformat()


def entry(state: State, task_id: str) -> Task:
    found: Task = state["tasks"].get(task_id, {})
    return found


def status(state: State, task_id: str) -> str:
    return str(entry(state, task_id).get("status", PENDING))


def with_task(state: State, task_id: str, **fields: Any) -> State:
    out = copy.deepcopy(state)
    current = out["tasks"].setdefault(task_id, {"status": PENDING, "attempts": 0})
    current.update(fields)
    out["version"] = int(out.get("version", 0)) + 1
    return out


def live_lease(state: State, now: datetime) -> tuple[str, Task] | None:
    for task_id, task in state["tasks"].items():
        lease = task.get("lease")
        if task.get("status") == IN_PROGRESS and lease:
            if datetime.fromisoformat(lease["expires"]) > now:
                return task_id, dict(lease)
    return None


def claim(state: State, task_id: str, session: str, now: datetime) -> State:
    """Claim a task. One task is in flight at a time (BUILD_BRIEF.md:222)."""
    held = live_lease(state, now)
    if held is not None:
        raise LeaseError(f"lease on {held[0]} is live")
    for other, task in state["tasks"].items():
        if task.get("status") == MERGE_PENDING:
            raise LeaseError(f"{other} is MERGE_PENDING; nothing new is claimed")
    if status(state, task_id) not in RETRYABLE:
        raise LeaseError(f"{task_id} is {status(state, task_id)}, not claimable")
    lease = {
        "session": session,
        "expires": iso(now + timedelta(hours=LEASE_HOURS)),
        "renewed": iso(now),
    }
    return with_task(state, task_id, status=IN_PROGRESS, lease=lease)


def renew(state: State, task_id: str, session: str, now: datetime) -> State:
    lease = entry(state, task_id).get("lease")
    if status(state, task_id) != IN_PROGRESS or not lease or lease["session"] != session:
        raise LeaseError(f"{session} does not hold the lease on {task_id}")
    if datetime.fromisoformat(lease["expires"]) <= now:
        raise LeaseError("lease already expired; the task went back to the queue")
    fresh = dict(lease, expires=iso(now + timedelta(hours=LEASE_HOURS)), renewed=iso(now))
    return with_task(state, task_id, lease=fresh)


def renewal_due(state: State, task_id: str, now: datetime) -> bool:
    lease = entry(state, task_id).get("lease")
    if not lease:
        return False
    renewed = datetime.fromisoformat(lease["renewed"])
    return now - renewed >= timedelta(minutes=RENEW_MINUTES)


def release_expired(state: State, now: datetime) -> State:
    """Lease expired with no PR: back to the queue, branch kept (BUILD_BRIEF.md:271)."""
    out = state
    for task_id, task in state["tasks"].items():
        lease = task.get("lease")
        if task.get("status") != IN_PROGRESS or not lease:
            continue
        if datetime.fromisoformat(lease["expires"]) <= now:
            stale = list(task.get("stale_leases", [])) + [lease]
            out = with_task(out, task_id, status=PENDING, lease=None, stale_leases=stale)
    return out


def backoff_minutes(attempt: int) -> int:
    """RETRY_LATER backoff for vendor outages: 15 minutes doubling to a 6 hour cap."""
    return int(min(15 * 2 ** max(attempt, 0), 360))


def _ready(state: State, task_id: str, now: datetime) -> bool:
    task = entry(state, task_id)
    if task.get("status", PENDING) not in RETRYABLE:
        return False
    after = task.get("retry_after")
    return after is None or datetime.fromisoformat(after) <= now


def eligible(graph: list[Task], state: State, now: datetime) -> list[str]:
    """IDs claimable now, in graph order; a BLOCKED task holds only its dependents."""
    if state.get("main_red"):
        return [FIX_MAIN]
    if state.get("build_hold"):
        return []
    done = {t["id"] for t in graph if status(state, t["id"]) in SATISFIED}
    return [
        t["id"]
        for t in graph
        if t.get("type") != "gate"
        and set(t["deps"]) <= done
        and _ready(state, t["id"], now)
    ]


def next_gate(graph: list[Task], state: State) -> Task | None:
    """A gate whose dependencies are all satisfied and that is not yet resolved."""
    done = {t["id"] for t in graph if status(state, t["id"]) in SATISFIED}
    for task in graph:
        if task.get("type") == "gate" and set(task["deps"]) <= done:
            if status(state, task["id"]) == PENDING:
                return task
    return None

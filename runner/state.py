"""Task state, the lease and eligibility, over the real kit/state.json shape (BUILD_BRIEF.md:215-224, 234).

The state is a dict as kept on claude/kit-state:
    {"schema": 1, "lease": {"task", "session", "expires"} | None, "build_hold": str | None,
     "tasks": [{"id", "deps", "status", "attempts", ...}, ...], ...other keys kept as they are}
Times are UTC ISO strings ending in "Z". Every function returns a new state; nothing mutates
its argument. `main_red` (a bool) is the one key this runner adds, for "fix main".
"""

from __future__ import annotations

import copy
import re
from datetime import datetime, timedelta, timezone
from typing import Any

State = dict[str, Any]
Task = dict[str, Any]

QUEUED = "QUEUED"
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
RETRYABLE = (QUEUED, RETRY_AFTER_RESET, RETRY_LATER)
SATISFIED = (DONE, SKIPPED)
BUILD_TASK = re.compile(r"^T\d+$")  # the tasks Gate P holds


class LeaseError(Exception):
    """A claim or renewal that must not go ahead; the runner exits."""


def new_state() -> State:
    return {"schema": 1, "lease": None, "build_hold": None, "tasks": []}


def iso(moment: datetime) -> str:
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def parse(stamp: str) -> datetime:
    return datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def entry(state: State, task_id: str) -> Task:
    for task in state["tasks"]:
        if task["id"] == task_id:
            found: Task = task
            return found
    return {}


def status(state: State, task_id: str) -> str:
    return str(entry(state, task_id).get("status", QUEUED))


def with_task(state: State, task_id: str, **fields: Any) -> State:
    out = copy.deepcopy(state)
    found = next((t for t in out["tasks"] if t["id"] == task_id), None)
    if found is None:
        found = {"id": task_id, "deps": [], "status": QUEUED, "attempts": 0}
        out["tasks"].append(found)
    found.update(fields)
    return out


def sync_graph(state: State, graph: list[Task]) -> State:
    """Add graph tasks (kit/tasks.json) that the state does not hold yet, as QUEUED."""
    out = state
    for task in graph:
        if not entry(out, task["id"]):
            out = with_task(out, task["id"], slug=task.get("slug"), type=task.get("type"), deps=list(task["deps"]))
    return out


def live_lease(state: State, now: datetime) -> Task | None:
    lease = state.get("lease")
    if lease and parse(lease["expires"]) > now:
        held: Task = lease
        return held
    return None


def _lease_for(task_id: str, session: str, now: datetime) -> Task:
    return {"task": task_id, "session": session, "expires": iso(now + timedelta(hours=LEASE_HOURS))}


def claim(state: State, task_id: str, session: str, now: datetime) -> State:
    """Claim a task. One task is in flight at a time (BUILD_BRIEF.md:222)."""
    held = live_lease(state, now)
    if held is not None:
        raise LeaseError(f"lease on {held['task']} is live")
    for task in state["tasks"]:
        if task.get("status") == MERGE_PENDING:
            raise LeaseError(f"{task['id']} is MERGE_PENDING; nothing new is claimed")
    if status(state, task_id) not in RETRYABLE:
        raise LeaseError(f"{task_id} is {status(state, task_id)}, not claimable")
    out = with_task(state, task_id, status=IN_PROGRESS)
    out["lease"] = _lease_for(task_id, session, now)
    out["current"] = task_id
    return out


def renew(state: State, task_id: str, session: str, now: datetime) -> State:
    lease = state.get("lease")
    if not lease or lease["task"] != task_id or lease["session"] != session:
        raise LeaseError(f"{session} does not hold the lease on {task_id}")
    if parse(lease["expires"]) <= now:
        raise LeaseError("lease already expired; the task went back to the queue")
    out = copy.deepcopy(state)
    out["lease"] = _lease_for(task_id, session, now)
    return out


def renewal_due(state: State, now: datetime) -> bool:
    """True once 30 minutes have passed since the lease was last set (it lasts 3 hours)."""
    lease = state.get("lease")
    if not lease:
        return False
    renewed = parse(lease["expires"]) - timedelta(hours=LEASE_HOURS)
    return now - renewed >= timedelta(minutes=RENEW_MINUTES)


def release_expired(state: State, now: datetime) -> State:
    """Lease expired with no PR: back to the queue, branch kept (BUILD_BRIEF.md:271)."""
    lease = state.get("lease")
    if not lease or parse(lease["expires"]) > now:
        return state
    task_id = lease["task"]
    stale = list(entry(state, task_id).get("stale_leases", [])) + [lease]
    out = state
    if status(state, task_id) == IN_PROGRESS:
        out = with_task(state, task_id, status=QUEUED, stale_leases=stale)
    out = copy.deepcopy(out)
    out["lease"] = None
    return out


def backoff_minutes(attempt: int) -> int:
    """RETRY_LATER backoff for vendor outages: 15 minutes doubling to a 6 hour cap."""
    return int(min(15 * 2 ** max(attempt, 0), 360))


def _ready(state: State, task_id: str, now: datetime) -> bool:
    task = entry(state, task_id)
    if task.get("status", QUEUED) not in RETRYABLE:
        return False
    after = task.get("retry_after")
    return after is None or parse(after) <= now


def eligible(state: State, now: datetime) -> list[str]:
    """IDs claimable now, in state order; a BLOCKED task holds only its dependents.

    While `build_hold` is set (Gate P) the T### build tasks wait; kit tasks do not.
    """
    if state.get("main_red"):
        return [FIX_MAIN]
    done = {t["id"] for t in state["tasks"] if t.get("status") in SATISFIED}
    held = bool(state.get("build_hold"))
    return [
        t["id"]
        for t in state["tasks"]
        if t.get("type") != "gate"
        and not (held and BUILD_TASK.match(t["id"]))
        and set(t.get("deps", [])) <= done
        and _ready(state, t["id"], now)
    ]


def next_gate(state: State, skip: tuple[str, ...] = ()) -> Task | None:
    """A gate with all its dependencies satisfied that has not been opened yet."""
    done = {t["id"] for t in state["tasks"] if t.get("status") in SATISFIED}
    for task in state["tasks"]:
        if task.get("type") == "gate" and task["id"] not in skip and task.get("status") == QUEUED:
            if set(task.get("deps", [])) <= done:
                found: Task = task
                return found
    return None

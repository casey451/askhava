"""Task state, the lease and eligibility, over the real kit/state.json shape (BUILD_BRIEF.md:215-224, 234).

The state is a dict as kept on claude/kit-state:
    {"schema": 1, "lease": {"task", "session", "expires"} | None, "build_hold": str | None,
     "tasks": [{"id", "deps", "status", "attempts", ...}, ...], ...other keys kept as they are}
Times are UTC ISO strings ending in "Z". Every function returns a new state; nothing mutates
its argument. Keys this runner adds (D-486, kit/state_schema.json): `main_red` (falsy, or the failing
check, SHA and offending merge: docs/governance.md section 5), `stage_proofs`, `smoke_runs`,
`deviations`, `usage.by_task`, `runner_sha`/`starter`, and per task `history`, `reviewed_sha`.
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
    """An ISO time; a stamp without a zone (or a bare date) is read as UTC."""
    moment = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    return moment if moment.tzinfo else moment.replace(tzinfo=timezone.utc)


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
    """Add graph tasks (kit/tasks.json) that the state does not hold yet, as QUEUED.

    A task's `stage` is copied too (also onto entries that lack it), because eligibility gates
    each stage on the previous stage's exit test (docs/governance.md section 6, D-486).
    """
    out = state
    for task in graph:
        found = entry(out, task["id"])
        if not found:
            extra = {"stage": task["stage"]} if "stage" in task else {}
            out = with_task(out, task["id"], slug=task.get("slug"), type=task.get("type"), deps=list(task["deps"]), **extra)
        elif "stage" in task and "stage" not in found:
            out = with_task(out, task["id"], stage=task["stage"])
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
    out = with_task(state, task_id, status=IN_PROGRESS, attempt_started_at=iso(now))
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


def _numeric_stage(stage: Any) -> float | None:
    try:
        return float(stage)
    except (TypeError, ValueError):
        return None


def previous_stage(state: State, stage: Any) -> str | None:
    """The numeric stage just before `stage` among the state's tasks, or None for the first.

    Stages are compared as numbers; "checkpoint" and "0.5" (gates only) are never a previous stage
    of a build stage, so stage 6 follows stage 5 (docs/build-readiness.md section 3).
    """
    mine = _numeric_stage(stage)
    if mine is None:
        return None
    earlier = {str(t["stage"]) for t in state["tasks"] if (n := _numeric_stage(t.get("stage"))) is not None and n < mine}
    earlier.discard("0.5")
    return max(earlier, key=float) if earlier else None


def _stage_open(state: State, task: Task, passed: set[str] | None) -> bool:
    """A task may start only once the previous stage passed scripts/stage_exit.py (D-486, governance 6)."""
    if passed is None:
        return True
    before = previous_stage(state, task.get("stage"))
    return before is None or before in passed


def _fix_main_ids(state: State) -> list[str]:
    """While main is red only FIX-MAIN is eligible, and only while it can be claimed (governance 5.1)."""
    fix = entry(state, FIX_MAIN)
    return [FIX_MAIN] if not fix or fix.get("status", QUEUED) in RETRYABLE else []


def eligible(state: State, now: datetime, passed: set[str] | None = None) -> list[str]:
    """IDs claimable now, in state order; a BLOCKED task holds only its dependents.

    While `build_hold` is set (Gate P) the T### build tasks wait; kit tasks do not. With `passed`
    (the stages whose exit test passed) a task of stage N waits for stage N-1's exit test, not for
    every task of stage N-1 through a dependency chain (D-486).
    """
    if state.get("main_red"):
        return _fix_main_ids(state)
    done = {t["id"] for t in state["tasks"] if t.get("status") in SATISFIED}
    held = bool(state.get("build_hold"))
    return [
        t["id"]
        for t in state["tasks"]
        if t.get("type") != "gate"
        and t["id"] != FIX_MAIN
        and not (held and BUILD_TASK.match(t["id"]))
        and set(t.get("deps", [])) <= done
        and _stage_open(state, t, passed)
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


def open_deviations(state: State, now: datetime) -> list[Task]:
    """Entries of the open-deviations register (id, rule, decision, expiry, owner) not yet expired."""
    out: list[Task] = []
    for dev in state.get("deviations", []):
        expiry = dev.get("expiry")
        if not expiry or parse(str(expiry)) > now:
            out.append(dev)
    return out

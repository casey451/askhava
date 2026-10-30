"""Task state, the lease and eligibility, over the real kit/state.json shape (BUILD_BRIEF.md:215-224, 234).

The state is a dict as kept on claude/kit-state:
    {"schema": 1, "lease": {"task", "session", "expires"} | None, "build_hold": str | None,
     "leases": {"1": {"task", "session", "expires", "lane"} | None, "2": ..., "fix": ...},
     "tasks": [{"id", "deps", "status", "attempts", ...}, ...], ...other keys kept as they are}
Lanes (D-515): `leases` holds one lease per lane ("1", "2", ... and "fix", reserved for FIX-MAIN).
Lane "1" is also kept in the old top-level `lease` key, without its "lane" field, so every reader
that knows only `lease` keeps working; `lease` is what lane "1" reads, and every writer goes
through `with_lease`, which keeps the two in step. A state without `leases` (written before D-515)
reads as lane "1" holding `lease`, and stays in that old shape while only lane "1" is written.
The lane count is `lanes` in kit/runner.json (1 for now: one task in flight, BUILD_BRIEF.md:222).
Times are UTC ISO strings ending in "Z". Every function returns a new state; nothing mutates
its argument. Keys this runner adds (D-486, kit/state_schema.json): `main_red` (falsy, or the failing
check, SHA and offending merge: docs/governance.md section 5), `stage_proofs`, `smoke_runs`,
`deviations`, `usage.by_task`, `runner_sha`/`starter`, and per task `history`, `reviewed_sha`.
Decision numbers (D-530): `next_decision` is the next D-number the runner may hand out, and each
claimed task carries `decision_ids`, the block of DECISION_BLOCK numbers its worker numbers new
docs/decisions.md entries from, so parallel work never takes the same number twice.
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
DECISION_BLOCK = 5  # D-numbers reserved per claimed task (D-530)
DECISION_ENTRY = re.compile(r"^- \*\*D-(\d+)\*\*", re.M)  # an entry line of docs/decisions.md
PROOF_MISSING = "stage proof missing"  # the BLOCKED reason of a cleanup task waiting only for its stage proof


class LeaseError(Exception):
    """A claim or renewal that must not go ahead; the runner exits."""


def new_state() -> State:
    """An empty state in the pre-D-515 shape; `leases` is added by the first write that needs it."""
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


LANE_ONE = "1"  # the lane mirrored by the top-level `lease` key (D-515)
FIX_LANE = "fix"  # reserved for FIX-MAIN; never counted against `lanes` (D-515)


def lane_ok(lane: str) -> bool:
    """A lane name is "fix" or a whole number from 1."""
    return lane == FIX_LANE or (lane.isdigit() and int(lane) >= 1)


def leases_of(state: State) -> dict[str, Task | None]:
    """Every lane's lease as {lane: {task, session, expires, lane} | None}; lane "1" always first.

    Lane "1" is read from `lease`, so a state written before D-515 (no `leases`) loads as lane "1"
    holding its lease, and a stray `leases["1"]` can never disagree with what old readers see.
    """
    lease = state.get("lease")
    out: dict[str, Task | None] = {LANE_ONE: dict(lease, lane=LANE_ONE) if lease else None}
    stored = state.get("leases")
    for lane, held in stored.items() if isinstance(stored, dict) else ():
        if str(lane) != LANE_ONE:
            out[str(lane)] = copy.deepcopy(held) if held else None
    return out


def lease_in(state: State, lane: str = LANE_ONE) -> Task | None:
    """The lease stored for one lane, live or not; lane "1" is the `lease` value itself."""
    if lane == LANE_ONE:
        held: Task | None = state.get("lease")
        return held
    return leases_of(state).get(lane)


def with_lease(state: State, lane: str, lease: Task | None) -> State:
    """A copy with `lane` holding `lease` (None frees it) and `lease` kept as the mirror of lane "1".

    A state without `leases` keeps its old shape while only lane "1" is written; the first write
    to another lane adds the map (the real Env adds it on every read, runner/github_env.py).
    """
    if not lane_ok(lane):
        raise LeaseError(f"{lane!r} is not a lane")
    out = copy.deepcopy(state)
    value = dict(lease, lane=lane) if lease else None
    if lane == LANE_ONE:
        out["lease"] = {k: v for k, v in value.items() if k != "lane"} if value else None
    if "leases" in state or lane != LANE_ONE:
        out["leases"] = {**leases_of(state), lane: value}
    return out


def with_leases_map(state: State) -> State:
    """A copy that carries `leases` (derived from `lease` when missing), the shape the real Env loads."""
    out = copy.deepcopy(state)
    out["leases"] = leases_of(state)
    return out


def live_lease(state: State, now: datetime, lane: str = LANE_ONE) -> Task | None:
    """The lane's lease while it has not expired; for lane "1" the `lease` value itself."""
    lease = lease_in(state, lane)
    if lease and parse(lease["expires"]) > now:
        held: Task = lease
        return held
    return None


def live_leases(state: State, now: datetime) -> dict[str, Task]:
    """{lane: lease} for every lane whose lease has not expired, lane "1" first."""
    return {lane: lease for lane, lease in leases_of(state).items() if lease and parse(lease["expires"]) > now}


def free_lanes(state: State, now: datetime, lanes: int) -> list[str]:
    """The numbered lanes "1" to `lanes` with no live lease, in order; "fix" is never one of them."""
    held = live_leases(state, now)
    return [str(n) for n in range(1, max(lanes, 0) + 1) if str(n) not in held]


def lane_of(state: State, task_id: str) -> str | None:
    """The lane whose lease (live or not) names the task, or None."""
    return next((lane for lane, lease in leases_of(state).items() if lease and lease.get("task") == task_id), None)


def _lease_for(task_id: str, session: str, now: datetime) -> Task:
    return {"task": task_id, "session": session, "expires": iso(now + timedelta(hours=LEASE_HOURS))}


def _claim_refusal(state: State, task_id: str, now: datetime, lane: str) -> str:
    """Why a claim of the task on this lane must not go ahead; '' when it may."""
    held = live_lease(state, now, lane)
    if held is not None:
        return f"lease on {held['task']} is live"
    other = next((n for n, lease in live_leases(state, now).items() if lease.get("task") == task_id), None)
    if other is not None:
        return f"{task_id} already holds lane {other}"
    for task in state["tasks"]:
        if task.get("status") == MERGE_PENDING:
            return f"{task['id']} is MERGE_PENDING; nothing new is claimed"
    if status(state, task_id) not in RETRYABLE:
        return f"{task_id} is {status(state, task_id)}, not claimable"
    return ""


def decision_numbers(text: str) -> list[int]:
    """The D-numbers of docs/decisions.md's entry lines (`- **D-###** ...`), in file order, repeats kept."""
    return [int(n) for n in DECISION_ENTRY.findall(text)]


def _reserved(state: State) -> list[int]:
    """Every D-number a task of the state holds in `decision_ids`."""
    return [n for t in state["tasks"] for n in t.get("decision_ids") or []]


def reserve_decisions(state: State, task_id: str, taken: set[int]) -> State:
    """Give the task a block of DECISION_BLOCK D-numbers and advance `next_decision` (D-530).

    `taken` is every D-number on main's docs/decisions.md. A task that already holds a block keeps
    it (a re-claim) unless one of its numbers is on main by now. A new block starts above main's
    highest entry, above every block already handed out and at `next_decision` or later, so the
    counter starts lazily from main's file and never hands out a number main already uses."""
    held = list(entry(state, task_id).get("decision_ids") or [])
    if held and not set(held) & taken:
        return state
    start = max([int(state.get("next_decision") or 1), max(taken, default=0) + 1, max(_reserved(state), default=0) + 1])
    out = with_task(state, task_id, decision_ids=list(range(start, start + DECISION_BLOCK)))
    out["next_decision"] = start + DECISION_BLOCK
    return out


def claim(
    state: State, task_id: str, session: str, now: datetime, lane: str = LANE_ONE, taken: set[int] | None = None
) -> State:
    """Claim a task on a lane. With one lane, one task is in flight at a time (BUILD_BRIEF.md:222).

    With `taken` (the D-numbers on main's docs/decisions.md) the same write reserves the task's
    decision numbers (reserve_decisions, D-530); without it the claim reserves none."""
    refusal = _claim_refusal(state, task_id, now, lane)
    if refusal:
        raise LeaseError(refusal)
    out = with_task(state, task_id, status=IN_PROGRESS, attempt_started_at=iso(now))
    out = with_lease(out, lane, _lease_for(task_id, session, now))
    out["current"] = task_id
    return out if taken is None else reserve_decisions(out, task_id, taken)


def renew(state: State, task_id: str, session: str, now: datetime, lane: str = LANE_ONE) -> State:
    """Set the lane's lease to three hours from now, only for the session that holds it."""
    lease = lease_in(state, lane)
    if not lease or lease["task"] != task_id or lease["session"] != session:
        raise LeaseError(f"{session} does not hold the lease on {task_id}")
    if parse(lease["expires"]) <= now:
        raise LeaseError("lease already expired; the task went back to the queue")
    return with_lease(state, lane, _lease_for(task_id, session, now))


def renewal_due(state: State, now: datetime, lane: str = LANE_ONE) -> bool:
    """True once 30 minutes have passed since the lease was last set (it lasts 3 hours)."""
    lease = lease_in(state, lane)
    if not lease:
        return False
    renewed = parse(lease["expires"]) - timedelta(hours=LEASE_HOURS)
    return now - renewed >= timedelta(minutes=RENEW_MINUTES)


def _release_lane(state: State, lane: str, lease: Task) -> State:
    """Free one lane; its task, if still IN_PROGRESS, goes back to the queue with the lease kept as evidence."""
    task_id = lease["task"]
    stale = list(entry(state, task_id).get("stale_leases", [])) + [lease]
    out = state
    if status(state, task_id) == IN_PROGRESS:
        out = with_task(state, task_id, status=QUEUED, stale_leases=stale)
    return with_lease(out, lane, None)


def release_expired(state: State, now: datetime) -> State:
    """Each lane whose lease expired with no PR: back to the queue, branch kept (BUILD_BRIEF.md:271)."""
    out = state
    for lane in leases_of(state):
        lease = lease_in(state, lane)
        if lease and parse(lease["expires"]) <= now:
            out = _release_lane(out, lane, lease)
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


def awaits_proof(task: Task) -> bool:
    """A cleanup task that merged (or handed in nothing to merge) and is BLOCKED only for its stage's
    named proof (runner.cycle, docs/governance.md section 6)."""
    return task.get("type") == "cleanup" and task.get("status") == BLOCKED and task.get("reason") == PROOF_MISSING


def _deps_met(task: Task, done: set[str], early: dict[str, str]) -> bool:
    """Every dependency DONE or SKIPPED, or a proof-waiting cleanup task of this task's own stage (D-551)."""
    stage = str(task.get("stage"))
    return all(dep in done or early.get(dep) == stage for dep in task.get("deps", []))


def eligible(
    state: State, now: datetime, passed: set[str] | None = None, proof_stages: frozenset[str] = frozenset()
) -> list[str]:
    """IDs claimable now, in state order; a BLOCKED task holds only its dependents.

    While `build_hold` is set (Gate P) the T### build tasks wait; kit tasks do not. With `passed`
    (the stages whose exit test passed) a task of stage N waits for stage N-1's exit test, not for
    every task of stage N-1 through a dependency chain (D-486). In a stage of `proof_stages` (those
    whose proof the runner records itself from the whole stage's results, runner/proof.py) a cleanup
    task waiting only for that proof does not hold the later tasks of its own stage, whose work the
    proof needs; tasks of later stages still wait for the stage exit test (D-551).
    """
    if state.get("main_red"):
        return _fix_main_ids(state)
    done = {t["id"] for t in state["tasks"] if t.get("status") in SATISFIED}
    early = {t["id"]: str(t.get("stage")) for t in state["tasks"] if awaits_proof(t) and str(t.get("stage")) in proof_stages}
    held = bool(state.get("build_hold"))
    return [
        t["id"]
        for t in state["tasks"]
        if t.get("type") != "gate"
        and t["id"] != FIX_MAIN
        and not (held and BUILD_TASK.match(t["id"]))
        and _deps_met(t, done, early)
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

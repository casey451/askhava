"""One runner cycle (BUILD_BRIEF.md:205-292). The outside world comes in through Env."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Protocol

from runner import outbox, state as st, verify
from runner.state import State

MAX_ATTEMPTS = 3  # BUILD_BRIEF.md:540
HOURLY = timedelta(hours=1)
WAIT_RESULTS = ("merge_pending", "not_done", "check_failed")


@dataclass
class WorkerResult:
    """What a worker session reports. 'outcome' is one of: submitted, tests_failed,
    usage_limit, permission_blocked, vendor_outage. A crash is an exception, not a result."""

    outcome: str
    changed_paths: list[str] = field(default_factory=list)
    model_evidence: list[str] = field(default_factory=list)
    reset_at: datetime | None = None
    log: str = ""


class Env(Protocol):
    """The runner's outside world.

    push_state(new, base) writes kit/state.json to claude/kit-state and returns False when the
    push is rejected. The real Env fetches and retries when only inbox/outbox files moved and
    returns False only if the state itself (the lease) changed (K0 review N2).
    """

    def read_state(self) -> State: ...
    def push_state(self, new: State, base: State) -> bool: ...
    def read_inbox(self) -> list[tuple[str, str, str]]:
        """New inbox files as (file id, outbox id, text); they stay until ack_inbox."""
        ...

    def ack_inbox(self, file_ids: list[str]) -> None:
        """Move the files to inbox/done/; called only after the state recording them was pushed."""
        ...

    def pr_for(self, task_id: str) -> dict[str, Any] | None: ...
    def gate_body(self, gate: dict[str, Any]) -> str: ...
    def run_worker(self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]) -> WorkerResult: ...


@dataclass
class Config:
    graph: list[dict[str, Any]]
    models: dict[str, str]
    protected: list[str]
    shared: list[str]
    outbox_dir: Path
    session: str
    clock: Callable[[], datetime]


def attempt_model(task: dict[str, Any], attempts: int, models: dict[str, str]) -> str:
    """Same model first, the same again with the log, Opus on the last try (BUILD_BRIEF.md:540)."""
    if attempts >= MAX_ATTEMPTS - 1:
        return models["opus"]
    return str(task["model"])


def record_failure(state: State, task_id: str, reason: str) -> State:
    attempts = int(st.entry(state, task_id).get("attempts", 0)) + 1
    if attempts >= MAX_ATTEMPTS:
        return st.with_task(state, task_id, status=st.BLOCKED, attempts=attempts, reason=reason)
    return st.with_task(state, task_id, status=st.QUEUED, attempts=attempts, last_failure=reason)


def _outage(state: State, tid: str, now: datetime) -> State:
    n = int(st.entry(state, tid).get("outages", 0))
    after = now + timedelta(minutes=st.backoff_minutes(n))
    return st.with_task(state, tid, status=st.RETRY_LATER, outages=n + 1, retry_after=st.iso(after))


def _after_worker(
    state: State, task: dict[str, Any], res: WorkerResult, cfg: Config, model: str, now: datetime
) -> State:
    tid = task["id"]
    if res.outcome == "usage_limit":
        when = st.iso(res.reset_at or now + HOURLY)
        return st.with_task(state, tid, status=st.RETRY_AFTER_RESET, retry_after=when)
    if res.outcome == "vendor_outage":
        return _outage(state, tid, now)
    if res.outcome == "permission_blocked":
        return st.with_task(state, tid, status=st.BLOCKED, reason=f"permission blocked: {res.log}")
    if res.outcome != "submitted":
        return record_failure(state, tid, res.log or res.outcome)
    problems = verify.check_paths(res.changed_paths, task, cfg.protected, cfg.shared)
    problems += verify.check_model(res.model_evidence, model, cfg.models.values())
    if problems:
        return record_failure(state, tid, "; ".join(problems))
    return st.with_task(state, tid, status=st.MERGE_PENDING)


def _resolve_merge(state: State, env: Env) -> tuple[State, str]:
    for task in state["tasks"]:
        if task.get("status") != st.MERGE_PENDING:
            continue
        tid = task["id"]
        pr = env.pr_for(tid)
        if pr is None:
            return state, "merge_pending"
        checks = verify.checks_state(pr)
        if not pr.get("merged"):
            if checks == "failed":
                reason = pr.get("failure_log") or "required check failed on head"
                return record_failure(state, tid, reason), "check_failed"
            return state, "merge_pending"
        if checks == "pending":
            return state, "merge_pending"
        problems = verify.check_done(pr, pr.get("head_tree", ""), pr.get("merge_tree", ""))
        if problems:
            out = st.with_task(state, tid, status=st.BLOCKED, reason="; ".join(problems))
            out["main_red"] = True
            return out, "not_done"
        done = st.with_task(
            state,
            tid,
            status=st.DONE,
            merged_sha=pr.get("merged_sha", ""),
            merged_at=pr.get("merged_at", ""),  # read by scripts/progress.py (D-479)
        )
        return done, "done"
    return state, ""


def _record_inbox(state: State, cfg: Config, items: list[tuple[str, str, str]], now: datetime) -> State:
    gates = {t["id"]: t for t in cfg.graph if t.get("type") == "gate"}
    for file_id, outbox_id, text in items:
        if outbox_id == outbox.GATE_P:
            state = outbox.apply_gate_p_answer(state, text)
        elif outbox_id in gates and st.status(state, outbox_id) == st.WAITING_FOR_OWNER:
            state = outbox.apply_owner_answer(state, gates[outbox_id], text, now)
        state = dict(state, inbox_processed=[*state.get("inbox_processed", []), file_id])
        note = {"file": file_id, "outbox_id": outbox_id, "note": text[:200]}
        state["owner_answers"] = [*state.get("owner_answers", []), note]
    return state


def _adopt_orphan_pr(before: State, after: State, env: Env) -> State:
    """A worker that died after opening a PR but before MERGE_PENDING: look at the PR, don't redo the work."""
    lease = before.get("lease")
    if not lease or st.status(before, lease["task"]) != st.IN_PROGRESS:
        return after
    if st.status(after, lease["task"]) == st.QUEUED and env.pr_for(lease["task"]) is not None:
        return st.with_task(after, lease["task"], status=st.MERGE_PENDING)
    return after


def run_cycle(cfg: Config, env: Env, now: datetime) -> str:
    """Run one cycle and return what it did (for logs and the drills)."""
    base = env.read_state()
    items = env.read_inbox()
    state = st.release_expired(_record_inbox(st.sync_graph(base, cfg.graph), cfg, items, now), now)
    state = _adopt_orphan_pr(base, state, env)
    if state != base:
        if not env.push_state(state, base):
            return "lost_race"  # inbox files stay put and are read again next cycle
        base = state
    env.ack_inbox([file_id for file_id, _, _ in items])
    if st.live_lease(state, now) is not None:
        return "lease_live"
    state, result = _resolve_merge(state, env)
    if result in WAIT_RESULTS or result == "done":
        if state != base and not env.push_state(state, base):
            return "lost_race"
        if result in WAIT_RESULTS:
            return result
        base = state
    return _next_step(cfg, env, state, base, now, result)


def _open_gate_p(cfg: Config, state: State) -> State | None:
    if st.status(state, "K9") != st.DONE:
        return None
    if st.status(state, outbox.GATE_P) in (st.DONE, st.WAITING_FOR_OWNER, st.SKIPPED):
        return None
    return outbox.hold_for_gate_p(state, cfg.outbox_dir)


def _next_gate(cfg: Config, env: Env, state: State) -> State | None:
    specs = {t["id"]: t for t in cfg.graph if t.get("type") == "gate" and "gate" in t}
    skip = (outbox.GATE_P, *[t["id"] for t in state["tasks"] if t.get("type") == "gate" and t["id"] not in specs])
    gate = st.next_gate(state, skip)
    if gate is None:
        return None
    spec = specs[gate["id"]]
    return outbox.open_gate(state, spec, env.gate_body(spec), cfg.outbox_dir)


def _next_step(cfg: Config, env: Env, state: State, base: State, now: datetime, result: str) -> str:
    opened = _open_gate_p(cfg, state) or _next_gate(cfg, env, state)
    if opened is not None:
        return "gate_open" if env.push_state(opened, base) else "lost_race"
    ids = st.eligible(state, now)
    if not ids or ids[0] == st.FIX_MAIN:
        if state != base and not env.push_state(state, base):
            return "lost_race"
        return "fix_main" if ids else (result or "idle")
    graph_def = next((t for t in cfg.graph if t["id"] == ids[0]), {})
    task = {**st.entry(state, ids[0]), **graph_def}
    claimed = st.claim(state, ids[0], cfg.session, now)
    if not env.push_state(claimed, base):
        return "lost_race"
    return _work(cfg, env, claimed, task, now)


def _work(cfg: Config, env: Env, claimed: State, task: dict[str, Any], now: datetime) -> str:
    tid = task["id"]
    entry = st.entry(claimed, tid)
    model = attempt_model(task, int(entry.get("attempts", 0)), cfg.models)
    current = {"state": claimed}

    def heartbeat() -> None:
        moment = cfg.clock()
        if not st.renewal_due(current["state"], moment):
            return
        renewed = st.renew(current["state"], tid, cfg.session, moment)
        if env.push_state(renewed, current["state"]):
            current["state"] = renewed

    res = env.run_worker(task, model, str(entry.get("last_failure", "")), heartbeat)
    final = _after_worker(current["state"], task, res, cfg, model, now)
    final["lease"] = None
    if not env.push_state(final, current["state"]):
        return "lost_race"
    return str(st.entry(final, tid)["status"])

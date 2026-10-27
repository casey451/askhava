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
    def read_state(self) -> State: ...
    def push_state(self, new: State, base_version: int) -> bool: ...
    def read_inbox(self) -> list[tuple[str, str]]: ...
    def pr_for(self, task_id: str) -> dict[str, Any] | None: ...
    def gate_body(self, gate: dict[str, Any]) -> str: ...
    def run_worker(
        self, task: dict[str, Any], model: str, log: str, heartbeat: Callable[[], None]
    ) -> WorkerResult: ...


@dataclass
class Config:
    graph: list[dict[str, Any]]
    models: dict[str, str]
    protected: list[str]
    shared: list[str]
    outbox_dir: Path
    session: str


def attempt_model(task: dict[str, Any], attempts: int, models: dict[str, str]) -> str:
    """Same model first, the same again with the log, Opus on the last try (BUILD_BRIEF.md:540)."""
    if attempts >= MAX_ATTEMPTS - 1:
        return models["opus"]
    return str(task["model"])


def record_failure(state: State, task_id: str, reason: str) -> State:
    attempts = int(st.entry(state, task_id).get("attempts", 0)) + 1
    if attempts >= MAX_ATTEMPTS:
        return st.with_task(
            state, task_id, status=st.BLOCKED, attempts=attempts, lease=None, reason=reason
        )
    return st.with_task(
        state, task_id, status=st.PENDING, attempts=attempts, lease=None, last_failure=reason
    )


def _after_worker(state: State, task: dict[str, Any], res: WorkerResult, cfg: Config, model: str, now: datetime) -> State:
    tid = task["id"]
    if res.outcome == "usage_limit":
        when = res.reset_at or now + HOURLY
        return st.with_task(state, tid, status=st.RETRY_AFTER_RESET, lease=None, retry_after=st.iso(when))
    if res.outcome == "vendor_outage":
        n = int(st.entry(state, tid).get("outages", 0))
        after = now + timedelta(minutes=st.backoff_minutes(n))
        return st.with_task(state, tid, status=st.RETRY_LATER, lease=None, outages=n + 1, retry_after=st.iso(after))
    if res.outcome == "permission_blocked":
        return st.with_task(state, tid, status=st.BLOCKED, lease=None, reason=f"permission blocked: {res.log}")
    if res.outcome != "submitted":
        return record_failure(state, tid, res.log or res.outcome)
    problems = verify.check_paths(res.changed_paths, task, cfg.protected, cfg.shared)
    problems += verify.check_model(res.model_evidence, model, cfg.models.values())
    if problems:
        return record_failure(state, tid, "; ".join(problems))
    return st.with_task(state, tid, status=st.MERGE_PENDING, lease=None)


def _resolve_merge(state: State, cfg: Config, env: Env) -> tuple[State, str]:
    for tid, task in state["tasks"].items():
        if task.get("status") != st.MERGE_PENDING:
            continue
        pr = env.pr_for(tid)
        if pr is None or verify.checks_state(pr) == "pending":
            return state, "merge_pending"
        if verify.checks_state(pr) == "failed" and not pr.get("merged"):
            return record_failure(state, tid, "required check failed on head"), "check_failed"
        problems = verify.check_done(pr, pr.get("head_tree", ""), pr.get("merge_tree", ""))
        if problems:
            out = st.with_task(state, tid, status=st.BLOCKED, reason="; ".join(problems))
            out["main_red"] = bool(pr.get("merged"))
            return out, "not_done"
        return st.with_task(state, tid, status=st.DONE), "done"
    return state, ""


def _apply_inbox(state: State, cfg: Config, env: Env, now: datetime) -> State:
    gates = {t["id"]: t for t in cfg.graph if t.get("type") == "gate"}
    for outbox_id, text in env.read_inbox():
        if outbox_id == outbox.GATE_P:
            state = outbox.apply_gate_p_answer(state, text)
        elif outbox_id in gates and st.status(state, outbox_id) == st.WAITING_FOR_OWNER:
            state = outbox.apply_owner_answer(state, gates[outbox_id], text, now)
    return state


def _push(env: Env, new: State, base: State) -> bool:
    return env.push_state(new, int(base.get("version", 0)))


def run_cycle(cfg: Config, env: Env, now: datetime) -> str:
    """Run one cycle and return what it did (for logs and the drills)."""
    base = env.read_state()
    state = st.release_expired(_apply_inbox(base, cfg, env, now), now)
    held = st.live_lease(state, now)
    if held is not None:
        return "lease_live"
    state, result = _resolve_merge(state, cfg, env)
    if result in ("merge_pending", "not_done", "check_failed"):
        if state is not base:
            _push(env, state, base)
        return result
    gate = st.next_gate(cfg.graph, state)
    if gate is not None:
        new = outbox.open_gate(state, gate, env.gate_body(gate), cfg.outbox_dir)
        return "gate_open" if _push(env, new, base) else "lost_race"
    return _pick_and_work(cfg, env, state, base, now, result)


def _pick_and_work(cfg: Config, env: Env, state: State, base: State, now: datetime, result: str) -> str:
    ids = st.eligible(cfg.graph, state, now)
    if not ids or ids[0] == st.FIX_MAIN:
        if state is not base:
            _push(env, state, base)
        return "fix_main" if ids else (result or "idle")
    task = next(t for t in cfg.graph if t["id"] == ids[0])
    claimed = st.claim(state, task["id"], cfg.session, now)
    if not _push(env, claimed, base):
        return "lost_race"
    return _work(cfg, env, claimed, task, now)


def _work(cfg: Config, env: Env, claimed: State, task: dict[str, Any], now: datetime) -> str:
    tid = task["id"]
    attempts = int(st.entry(claimed, tid).get("attempts", 0))
    model = attempt_model(task, attempts, cfg.models)
    current = {"state": claimed}

    def heartbeat() -> None:
        renewed = st.renew(current["state"], tid, cfg.session, datetime.now(now.tzinfo))
        if env.push_state(renewed, int(current["state"]["version"])):
            current["state"] = renewed

    res = env.run_worker(task, model, str(st.entry(claimed, tid).get("last_failure", "")), heartbeat)
    final = _after_worker(current["state"], task, res, cfg, model, now)
    env.push_state(final, int(current["state"]["version"]))
    return str(st.entry(final, tid)["status"])

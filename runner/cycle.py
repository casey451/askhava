"""One runner cycle (BUILD_BRIEF.md:205-292). The outside world comes in through Env."""

from __future__ import annotations

import copy
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
    usage_limit, permission_blocked, vendor_outage, or handed_off (the starting session does the
    work itself and calls `python3 -m runner --after`; the lease stays held). A crash is an
    exception, not a result. changed_paths, model_evidence, reviewer_evidence and usage are read by
    the runner from git and the transcripts, never from what the worker says (BUILD_BRIEF.md:247-248).
    """

    outcome: str
    changed_paths: list[str] = field(default_factory=list)
    model_evidence: list[str] = field(default_factory=list)
    reset_at: datetime | None = None
    log: str = ""
    branch: str = ""
    head_sha: str = ""
    reviewer_evidence: list[str] = field(default_factory=list)
    usage: dict[str, int] = field(default_factory=dict)


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
    def read_nightly(self) -> dict[str, Any] | None:
        """kit/nightly.json from claude/kit-state (written by nightly-main), or None."""
        ...

    def main_head(self) -> str:
        """The current SHA of origin/main."""
        ...

    def smoke_main(self) -> tuple[bool, str]:
        """Run scripts/smoke.sh on main's head with the runner's own tool run: (green, head SHA)."""
        ...

    def main_checks_green(self, head: str) -> bool | None:
        """Whether `Ci`, `Twin` and `Ally` all passed on that commit of main (None when unknown)."""
        ...

    def push_branch(self, branch: str, head_sha: str) -> bool:
        """Push the reviewed task branch (never force); called only after the review record is pushed."""
        ...

    def enable_auto_merge(self, branch: str, head_sha: str) -> tuple[bool, str]:
        """Find (or open, REST) the branch's PR and turn on squash auto-merge as the runner's own identity,
        so the merge starts workflows (pull_request.closed); returns (done, how or why not)."""
        ...

    def merge_main(self) -> str:
        """Merge main into claude/kit-state with a normal merge commit (BUILD_BRIEF.md Appendix B, K0 step 4)."""
        ...


@dataclass
class Config:
    graph: list[dict[str, Any]]
    models: dict[str, str]
    protected: list[str]
    shared: list[str]
    outbox_dir: Path
    session: str
    clock: Callable[[], datetime]
    stage_gate: Callable[[State], set[str]] | None = None  # stages whose exit test passed (D-486)
    pacer: Callable[[State, dict[str, Any], datetime], tuple[str, str | None, State]] | None = None  # scripts/pacing.py
    on_usage: Callable[[State, dict[str, Any], datetime], State] | None = None  # usage windows (scripts/pacing.py)


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
        usage = dict(state.get("usage") or {}, limit_hit={"task": tid, "at": st.iso(now), "reset_at": when})
        return st.with_task(dict(state, usage=usage), tid, status=st.RETRY_AFTER_RESET, retry_after=when)
    if res.outcome == "vendor_outage":
        return _outage(state, tid, now)
    if res.outcome == "permission_blocked":
        return st.with_task(state, tid, status=st.BLOCKED, reason=f"permission blocked: {res.log}")
    if res.outcome != "submitted":
        return record_failure(state, tid, res.log or res.outcome)
    problems = verify.check_paths(res.changed_paths, task, cfg.protected, cfg.shared)
    problems += verify.check_model(res.model_evidence, model, cfg.models.values())
    problems += _review_problems(task, res, cfg)
    if problems:
        return record_failure(state, tid, "; ".join(problems))
    return st.with_task(state, tid, status=st.MERGE_PENDING)


def _review_problems(task: dict[str, Any], res: WorkerResult, cfg: Config) -> list[str]:
    """The fresh-eyes reviewer must have run on the task's reviewer model (BUILD_BRIEF.md:250, governance 7)."""
    want = task.get("reviewer_model")
    if not want:
        return []
    return [f"review: {p}" for p in verify.check_model(res.reviewer_evidence, str(want), cfg.models.values())]


def _close_history(state: State, tid: str, pr: dict[str, Any], result: str) -> State:
    """Fill the open attempt's pr, checks and result once its PR resolves (per-attempt history, D-486)."""
    history = [dict(h) for h in st.entry(state, tid).get("history", [])]
    if not history:
        return state
    head = pr.get("head_sha", "")
    checks = [{"name": c.get("name"), "conclusion": c.get("conclusion")} for c in pr.get("checks", []) if c.get("sha") == head]
    history[-1].update(pr=pr.get("number"), checks=checks, result=result)
    return st.with_task(state, tid, history=history)


def _resolve_merge(state: State, env: Env, cfg: Config, now: datetime) -> tuple[State, str]:
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
                return _close_history(record_failure(state, tid, reason), tid, pr, "check_failed"), "check_failed"
            return _auto_merge(state, env, tid, pr.get("head_sha", "")) if not pr.get("auto_merge") else state, "merge_pending"
        if checks == "pending":
            return state, "merge_pending"
        return _merged(state, tid, pr, cfg, now)
    return state, ""


def _merged(state: State, tid: str, pr: dict[str, Any], cfg: Config, now: datetime) -> tuple[State, str]:
    """A merged PR: DONE only from check_done; otherwise main is red (governance section 5)."""
    problems = verify.check_done(pr, pr.get("head_tree", ""), pr.get("merge_tree", ""))
    if problems and tid == st.FIX_MAIN:
        return _close_history(record_failure(state, tid, "; ".join(problems)), tid, pr, "not_done"), "not_done"
    if problems:
        out = st.with_task(state, tid, status=st.BLOCKED, reason="; ".join(problems))
        merge = str(pr.get("merged_sha", ""))
        out = set_main_red(out, cfg, "tree", merge, (tid, merge, list(pr.get("files", []))), now)
        return _close_history(out, tid, pr, "not_done"), "not_done"
    facts = {
        "merged_sha": pr.get("merged_sha", ""),
        "merged_at": pr.get("merged_at", ""),  # read by scripts/progress.py (D-479)
        "files": list(pr.get("files", [])),  # what FIX-MAIN may touch if this merge turns main red
    }
    if not _proof_ok(state, tid):
        blocked = st.with_task(state, tid, status=st.BLOCKED, reason=PROOF_MISSING, **facts)
        return _close_history(blocked, tid, pr, "proof_missing"), "done"
    return _close_history(st.with_task(state, tid, status=st.DONE, **facts), tid, pr, "done"), "done"


PROOF_MISSING = "stage proof missing"


def _proof_ok(state: State, tid: str) -> bool:
    """A cleanup task is DONE only with its stage's named proof recorded (governance section 6)."""
    task = st.entry(state, tid)
    if task.get("type") != "cleanup":
        return True
    proof = (state.get("stage_proofs") or {}).get(str(task.get("stage"))) or {}
    return bool(str(proof.get("evidence", "")).strip())


def release_proven(state: State) -> State:
    """A cleanup task blocked only for its missing proof becomes DONE once the proof is recorded."""
    out = state
    for task in state["tasks"]:
        if task.get("status") == st.BLOCKED and task.get("reason") == PROOF_MISSING and _proof_ok(state, task["id"]):
            out = st.with_task(out, task["id"], status=st.DONE, reason=None)
    return out


def _auto_merge(state: State, env: Env, tid: str, head: str) -> State:
    """Turn on auto-merge only for the reviewed head under the task's lease (D-486); record the outcome."""
    task = st.entry(state, tid)
    reason = verify.auto_merge_reason(state, str(task.get("branch", "")), head, state.get("lease"))
    if reason:
        return st.with_task(state, tid, auto_merge={"enabled": False, "why": reason, "head": head})
    done, how = env.enable_auto_merge(str(task.get("branch", "")), head)
    return st.with_task(state, tid, auto_merge={"enabled": done, "why": how, "head": head})


# ---- red main (docs/governance.md section 5)


def _fix_main_fields(cfg: Config, offending: tuple[str, str, list[str]]) -> dict[str, Any]:
    """FIX-MAIN as a real task: harness class, Sonnet worker, Opus reviewer, revert first (5.2)."""
    tid, sha, files = offending
    goal = (
        f"Fix main. First option: `git revert --no-edit {sha or '<offending squash commit>'}` "
        f"(the merge of {tid or 'an unknown task'}). Patch over only if the revert itself would break main, "
        "and say why in the report."
    )
    return {
        "type": "harness", "model": cfg.models["sonnet"], "reviewer_model": cfg.models["opus"],
        "owns": sorted(set(files)), "deps": [], "status": st.QUEUED, "attempts": 0, "slug": "revert",
        "branch": "claude/task/FIX-MAIN-revert", "revert_sha": sha, "offending_task": tid, "goal": goal,
        "retry_after": None, "last_failure": None, "reason": None,
    }


def set_main_red(
    state: State, cfg: Config, check: str, sha: str, offending: tuple[str, str, list[str]], now: datetime
) -> State:
    """Set main_red with the failing check and SHA and queue FIX-MAIN for the offending merge (5.1, 5.2)."""
    tid, merge, files = offending
    out = copy.deepcopy(state)
    out["main_red"] = {"check": check, "sha": sha, "task": tid, "revert_sha": merge, "files": files, "set_at": st.iso(now)}
    return st.with_task(out, st.FIX_MAIN, **_fix_main_fields(cfg, offending))


def offending_merge(state: State) -> tuple[str, str, list[str]]:
    """The latest DONE merge (task, squash SHA, files): what a red nightly or smoke run blames first."""
    merged = [t for t in state["tasks"] if t.get("status") == st.DONE and t.get("merged_sha") and t["id"] != st.FIX_MAIN]
    if not merged:
        return "", "", []
    last = max(merged, key=lambda t: str(t.get("merged_at", "")))
    return str(last["id"]), str(last["merged_sha"]), list(last.get("files", []))


def _smoke(state: State, env: Env, now: datetime, why: str) -> tuple[State, bool, str]:
    """Run the smoke test on main and record the run in state (the 'verified' of governance 5.3)."""
    ok, head = env.smoke_main()
    run = {"sha": head, "ok": ok, "at": st.iso(now), "why": why}
    out = dict(state, smoke_runs=[*state.get("smoke_runs", [])[-19:], run])
    return out, ok, head


def _nightly_red_on_head(env: Env) -> str:
    nightly = env.read_nightly() or {}
    sha = str(nightly.get("sha", ""))
    if nightly.get("conclusion") == "failure" and sha and sha == env.main_head():
        return sha
    return ""


def clear_main_red(state: State, env: Env, now: datetime) -> tuple[State, str]:
    """Clear main_red only on a recorded green smoke run on main's head and, when kit/nightly.json is
    for that head, a successful nightly; then the reverted task returns with attempts+1 (5.3, 5.4)."""
    state, ok, head = _smoke(state, env, now, "verify FIX-MAIN")
    if not ok:
        return record_failure(state, st.FIX_MAIN, f"smoke test red on main {head[:12]} after FIX-MAIN"), "main_red"
    nightly = env.read_nightly() or {}
    if nightly.get("sha") == head and nightly.get("conclusion") != "success":
        return state, "main_red"
    red = dict(state["main_red"], cleared_at=st.iso(now), cleared_on=head)
    out = dict(state, main_red=None, main_red_history=[*state.get("main_red_history", []), red])
    if red.get("task"):
        out = record_failure(out, str(red["task"]), f"reverted by FIX-MAIN ({red['check']} red on {str(red['sha'])[:12]})")
    return out, "main_cleared"


def _red_main_step(cfg: Config, env: Env, state: State, now: datetime) -> tuple[State, str]:
    """Turn main red on a failed nightly for its head; try to clear once FIX-MAIN is DONE."""
    if not state.get("main_red"):
        sha = _nightly_red_on_head(env)
        if not sha:
            return state, ""
        return set_main_red(state, cfg, "nightly", sha, offending_merge(state), now), "main_red"
    if _false_smoke_red(state) and env.main_checks_green(str(state["main_red"]["sha"])) is True:
        return clear_false_red(state, now), "false_red_cleared"
    if st.status(state, st.FIX_MAIN) != st.DONE:
        return state, ""
    return clear_main_red(state, env, now)


def _false_smoke_red(state: State) -> bool:
    """main_red came from a smoke run and FIX-MAIN has not started: still safe to withdraw."""
    red = state.get("main_red") or {}
    fix = st.entry(state, st.FIX_MAIN) or {}
    return red.get("check") == "smoke" and fix.get("status") in (st.QUEUED, None) and not fix.get("attempts")


def clear_false_red(state: State, now: datetime) -> State:
    """Withdraw a smoke-set main_red that the head's own checks contradict: no revert, no blame (D-498).

    The first Actions cycle under the OAuth token marked main red because the smoke test failed in a
    runner image without the test's prerequisites while Ci, Twin and Ally were green on the same
    commit; FIX-MAIN would have reverted an unrelated, older merge."""
    red = dict(state["main_red"], cleared_at=st.iso(now), false_red=True)
    out = dict(state, main_red=None, main_red_history=[*state.get("main_red_history", []), red])
    out = st.with_task(out, st.FIX_MAIN, status=st.SKIPPED, reason="false red: the head's checks were green (D-498)")
    return _note_smoke_env(out, str(red.get("sha", "")), now)


def _note_smoke_env(state: State, head: str, now: datetime) -> State:
    note = {"sha": head, "at": st.iso(now), "why": "smoke red while Ci, Twin and Ally were green on the head"}
    return dict(state, smoke_env_problems=[*state.get("smoke_env_problems", [])[-9:], note])


def _record_inbox(state: State, cfg: Config, items: list[tuple[str, str, str]], now: datetime) -> State:
    gates = {t["id"]: t for t in cfg.graph if t.get("type") == "gate"}
    for file_id, raw_id, text in items:
        # Outbox ids are task ids, but a hand-written outbox file may carry the lowercase form
        # (`gate-p` on October 27): the first live cycle recorded Casey's answer and cleared
        # nothing because of the case (D-497). Match the id the way GitHub shows it.
        outbox_id = raw_id.strip().upper()
        if outbox_id == outbox.GATE_P:
            state = outbox.apply_gate_p_answer(state, text)
        elif outbox_id in gates and st.status(state, outbox_id) == st.WAITING_FOR_OWNER:
            state = outbox.apply_owner_answer(state, gates[outbox_id], text, now)
        state = dict(state, inbox_processed=[*state.get("inbox_processed", []), file_id])
        note = {"file": file_id, "outbox_id": raw_id, "note": text[:200]}
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
    env.merge_main()
    base = env.read_state()
    items = env.read_inbox()
    state = st.release_expired(_record_inbox(st.sync_graph(base, cfg.graph), cfg, items, now), now)
    state = release_proven(state)
    state = _adopt_orphan_pr(base, state, env)
    if state != base:
        if not env.push_state(state, base):
            return "lost_race"  # inbox files stay put and are read again next cycle
        base = state
    env.ack_inbox([file_id for file_id, _, _ in items])
    if st.live_lease(state, now) is not None:
        return "lease_live"
    state, result = _resolve_merge(state, env, cfg, now)
    if result in WAIT_RESULTS or result == "done":
        if state != base and not env.push_state(state, base):
            return "lost_race"
        if result in WAIT_RESULTS:
            return result
        base = state
    state, red = _red_main_step(cfg, env, state, now)
    if red:
        return red if state == base or env.push_state(state, base) else "lost_race"
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


def _passed(cfg: Config, state: State) -> set[str] | None:
    return cfg.stage_gate(state) if cfg.stage_gate else None


def _next_step(cfg: Config, env: Env, state: State, base: State, now: datetime, result: str) -> str:
    opened = _open_gate_p(cfg, state) or _next_gate(cfg, env, state)
    if opened is not None:
        return "gate_open" if env.push_state(opened, base) else "lost_race"
    ids = st.eligible(state, now, _passed(cfg, state))
    if not ids:
        if state != base and not env.push_state(state, base):
            return "lost_race"
        return "fix_main" if state.get("main_red") else (result or "idle")
    if ids[0] != st.FIX_MAIN:
        state, ok, head = _smoke(state, env, now, "start routine")  # BUILD_BRIEF.md:240
        if not ok:
            if env.main_checks_green(head) is True:
                # Ci, Twin and Ally passed on this very commit: the smoke failed in the runner's own
                # environment (a missing tool or database), not on main. Record it and stop; nothing is
                # claimed and main is not blamed (D-498).
                state = _note_smoke_env(state, head, now)
                return "smoke_env" if state == base or env.push_state(state, base) else "lost_race"
            red = set_main_red(state, cfg, "smoke", head, offending_merge(state), now)
            return "main_red" if env.push_state(red, base) else "lost_race"
    task, state = _paced_pick(cfg, state, ids, now)
    if task is None:
        return "paced" if state == base or env.push_state(state, base) else "lost_race"
    claimed = st.claim(state, task["id"], cfg.session, now)
    if not env.push_state(claimed, base):
        return "lost_race"
    done = _work(cfg, env, claimed, task, now)
    return "fix_main" if task["id"] == st.FIX_MAIN and done != "lost_race" else done


def _task_def(cfg: Config, state: State, tid: str) -> dict[str, Any]:
    graph_def = next((t for t in cfg.graph if t["id"] == tid), {})
    return {**st.entry(state, tid), **graph_def}


def _paced_pick(cfg: Config, state: State, ids: list[str], now: datetime) -> tuple[dict[str, Any] | None, State]:
    """The first eligible task that fits the usage windows (scripts/pacing.py); never idle while one fits.

    'defer' (too big for even an empty window) is claimed when nothing fits: waiting cannot help. When
    nothing fits, the state records the earliest time something will (pacing.waiting_until)."""
    if cfg.pacer is None:
        return _task_def(cfg, state, ids[0]), state
    waits: list[tuple[str, State]] = []
    deferred: tuple[dict[str, Any], State] | None = None
    for tid in ids:
        task = _task_def(cfg, state, tid)
        verdict, detail, noted = cfg.pacer(state, task, now)
        if verdict == "claim":
            return task, noted
        if verdict == "defer" and deferred is None:
            deferred = (task, noted)
        elif verdict == "wait":
            waits.append((str(detail), noted))
    if deferred is not None:
        return deferred
    return None, min(waits, key=lambda w: w[0])[1]


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
    if res.outcome == "handed_off":
        return "handed_off"  # the session works the task and calls `python3 -m runner --after`
    return finish(cfg, env, current["state"], task, res, model, now)


def _history_entry(state: State, task: dict[str, Any], res: WorkerResult, model: str, cfg: Config) -> dict[str, Any]:
    entry = st.entry(state, task["id"])
    path_ok = None
    if res.outcome == "submitted":
        path_ok = not verify.check_paths(res.changed_paths, task, cfg.protected, cfg.shared)
    return {
        "attempt": int(entry.get("attempts", 0)) + 1, "model": model, "model_evidence_count": len(res.model_evidence),
        "head_sha": res.head_sha, "pr": None, "checks": None, "reviewer_model": task.get("reviewer_model"),
        "path_check_ok": path_ok, "started_at": entry.get("attempt_started_at"), "ended_at": st.iso(cfg.clock()),
        "result": res.outcome,
    }


def _record_usage(state: State, tid: str, model: str, res: WorkerResult, now: datetime, cfg: Config) -> State:
    """Add the attempt's transcript token totals to usage.by_task and the pacing windows (BUILD_BRIEF.md:510)."""
    if not res.usage:
        return state
    if cfg.on_usage is not None:
        state = cfg.on_usage(state, res.usage, now)
    usage = copy.deepcopy(state.get("usage") or {})
    mine = usage.setdefault("by_task", {}).setdefault(tid, {})
    for key, value in res.usage.items():
        mine[key] = int(mine.get(key, 0)) + int(value)
    mine["last_model"] = model
    usage["last_reading"] = {"task": tid, "at": st.iso(now), **res.usage}
    return dict(state, usage=usage)


def finish(
    cfg: Config, env: Env, state: State, task: dict[str, Any], res: WorkerResult, model: str, now: datetime
) -> str:
    """After the worker: checks, history, usage; then the review gate: reviewed_sha and the lease are
    pushed to claude/kit-state, then the branch, then the runner itself turns on auto-merge for exactly
    that head (so the merge is the runner's identity and starts workflows), then MERGE_PENDING (D-486)."""
    tid = task["id"]
    record = _history_entry(state, task, res, model, cfg)
    final = _after_worker(state, task, res, cfg, model, now)
    history = [*st.entry(state, tid).get("history", []), dict(record, result=st.status(final, tid))]
    final = _record_usage(st.with_task(final, tid, history=history), tid, model, res, now, cfg)
    if st.status(final, tid) == st.MERGE_PENDING:
        review = {"reviewed_sha": res.head_sha, "review_lease": copy.deepcopy(state.get("lease")), "branch": res.branch}
        reviewed = st.with_task(state, tid, **review)
        if not env.push_state(reviewed, state):
            return "lost_race"
        if not env.push_branch(res.branch, res.head_sha):
            return "push_failed"  # lease stays; on expiry the task returns to the queue (BUILD_BRIEF.md:271)
        merged = _auto_merge(reviewed, env, tid, res.head_sha)
        state, final = reviewed, st.with_task(final, tid, **review, auto_merge=st.entry(merged, tid)["auto_merge"])
    final["lease"] = None
    if not env.push_state(final, state):
        return "lost_race"
    return str(st.entry(final, tid)["status"])


def after_handoff(cfg: Config, env: Env, res_for: Callable[[dict[str, Any]], WorkerResult]) -> str:
    """`python3 -m runner --after`: finish the task this session holds the lease on."""
    state = env.read_state()
    lease = st.live_lease(state, cfg.clock())
    if lease is None or lease.get("session") != cfg.session:
        return "not_lease_holder"
    tid = str(lease["task"])
    graph_def = next((t for t in cfg.graph if t["id"] == tid), {})
    task = {**st.entry(state, tid), **graph_def}
    if st.status(state, tid) != st.IN_PROGRESS:
        return f"{tid} is {st.status(state, tid)}"
    model = attempt_model(task, int(st.entry(state, tid).get("attempts", 0)), cfg.models)
    return finish(cfg, env, state, task, res_for(task), model, cfg.clock())

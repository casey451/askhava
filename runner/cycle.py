"""One runner cycle (BUILD_BRIEF.md:205-292). The outside world comes in through Env."""

from __future__ import annotations

import copy
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Protocol

from runner import outbox, state as st, verify
from runner.state import State

MAX_ATTEMPTS = 3  # BUILD_BRIEF.md:540
HOURLY = timedelta(hours=1)
WAIT_RESULTS = ("merge_pending", "not_done", "check_failed", "no_changes")
# An empty hand-in (no commits on top of main): GitHub opens no pull request for it, so it can never
# merge. Only a cleanup task may legitimately find nothing to change (D-551).
NO_CHANGES = "no changes: the hand-in has no commits on top of main (only a cleanup task may hand in nothing, D-551)"


@dataclass
class WorkerResult:
    """What a worker session reports. 'outcome' is one of: submitted, tests_failed,
    usage_limit, permission_blocked, vendor_outage, or handed_off (the starting session does the
    work itself and calls `python3 -m runner --after`; the lease stays held). A crash is an
    exception, not a result. changed_paths, model_evidence, reviewer_evidence, usage and
    commits_ahead are read by the runner from git and the transcripts, never from what the worker
    says (BUILD_BRIEF.md:247-248). commits_ahead counts the head's commits that main lacks
    (`git rev-list --count main..head`): 0 is an empty hand-in, None unknown (D-551).
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
    commits_ahead: int | None = None


class Env(Protocol):
    """The runner's outside world.

    push_state(new, base) writes kit/state.json to claude/kit-state and returns False when the
    push is rejected. The real Env fetches and retries when only inbox/outbox files moved and
    returns False only if the state itself (the lease) changed (K0 review N2).

    update_state(transform) reads the state fresh, applies transform (State -> State, or None to
    give up) and pushes the result, reading and applying again up to 5 times when the push loses a
    race; it returns the state it pushed (the fresh state when transform changed nothing) or None.
    The transform re-checks its own preconditions on the fresh state each time (D-515).
    """

    def read_state(self) -> State: ...
    def push_state(self, new: State, base: State) -> bool: ...
    def update_state(self, transform: Callable[[State], State | None]) -> State | None: ...
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

    def in_main(self, sha: str) -> bool | None:
        """Whether the commit is origin/main's head or one of its ancestors (None when git cannot say)."""
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
    owner_paths: tuple[str, ...] = ()  # .github/CODEOWNERS entries: a PR touching one waits for Casey (D-511)
    lanes: int = 1  # kit/runner.json "lanes"; only lane "1" is ever claimed until a later stage (D-515)
    # Reads the D-numbers on main's docs/decisions.md (None when unreadable). A claim reserves the
    # task's decision numbers only when this is set and returns a set (D-530).
    decisions: Callable[[], set[int] | None] | None = None
    # The stages whose named proof the runner records itself (runner/proof.py, D-551), and the check that
    # returns (the proof record, or None, and what the proof still waits for) for one of them.
    proof_stages: frozenset[str] = frozenset()
    stage_proof: Callable[[State, str], tuple[dict[str, Any] | None, list[str]]] | None = None


# Casey reviews code-owner PRs by day. Such a task is claimed only from 06:00 to 19:00 Arizona time
# (UTC-7, no daylight saving), so its PR is ready while he is awake and never holds the serial build
# overnight; at night the runner takes the next task that merges on its own (D-511).
OWNER_HOURS = (6, 19)
ARIZONA = timezone(timedelta(hours=-7))


def needs_owner(task: dict[str, Any], owner_paths: tuple[str, ...]) -> bool:
    """True when the task owns a path in .github/CODEOWNERS (a directory entry ends with "/")."""
    owns = [str(o) for o in task.get("owns", [])]
    return any(o.startswith(p) if p.endswith("/") else o == p for o in owns for p in owner_paths)


def owner_hours(now: datetime) -> bool:
    return OWNER_HOURS[0] <= now.astimezone(ARIZONA).hour < OWNER_HOURS[1]


def _by_day(cfg: Config, ids: list[str], now: datetime) -> list[str]:
    """Outside owner hours, drop the tasks whose PR needs Casey's review (D-511).

    Off while kit/runner.json sets "owner_review" to false (D-512): with no code-owner review on the
    ruleset, every task PR merges on its own after the Opus review and the required checks."""
    if not cfg.owner_paths or owner_hours(now):
        return ids
    graph = {t["id"]: t for t in cfg.graph}
    return [i for i in ids if not needs_owner(graph.get(i, {}), cfg.owner_paths)]


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
    if res.commits_ahead == 0 and task.get("type") != "cleanup":
        return record_failure(state, tid, NO_CHANGES)
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
            return _without_pr(state, env, task, now)
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
    return _mark_done(state, tid, pr, facts)


def _mark_done(state: State, tid: str, pr: dict[str, Any], facts: dict[str, Any]) -> tuple[State, str]:
    """DONE with the merge facts, or BLOCKED for a cleanup task whose stage proof is not recorded yet."""
    if not _proof_ok(state, tid):
        blocked = st.with_task(state, tid, status=st.BLOCKED, reason=PROOF_MISSING, **facts)
        return _close_history(blocked, tid, pr, "proof_missing"), "done"
    return _close_history(st.with_task(state, tid, status=st.DONE, **facts), tid, pr, "done"), "done"


def _without_pr(state: State, env: Env, task: dict[str, Any], now: datetime) -> tuple[State, str]:
    """MERGE_PENDING with no pull request. A reviewed head that main already contains is an empty
    hand-in, for which GitHub opens no pull request, so it is resolved here instead of waiting
    forever (T008 on October 30 stalled the build this way, D-551): a cleanup task's is resolved
    by _resolve_empty, any other task's is a failed attempt. Otherwise the pull request is still to come."""
    tid, head = task["id"], str(task.get("reviewed_sha") or "")
    if not head or env.in_main(head) is not True:
        return state, "merge_pending"
    if task.get("type") != "cleanup":
        failed = record_failure(state, tid, NO_CHANGES)
        return _close_history(failed, tid, {"head_sha": head}, "no_changes"), "no_changes"
    return _resolve_empty(state, env, tid, head, now)


def _resolve_empty(state: State, env: Env, tid: str, head: str, now: datetime) -> tuple[State, str]:
    """A cleanup task that found nothing to change certifies main as it was at hand-in, `head` (D-551).

    Nothing merges, so `merged_sha` is that commit and no files move. It counts as DONE (or BLOCKED
    for its stage proof, like any cleanup merge) once `Ci`, `Twin` and `Ally` are known green on it:
    either it is a merge the runner already marked DONE from its pull request's green checks and equal
    trees, or main's own checks say so (BUILD_BRIEF.md:261-263). A check that failed on it is a failed
    attempt (the next one runs on the newer main); while the checks are unknown it waits, saying why,
    for at most EMPTY_WAIT after the attempt ended, then that too is a failed attempt, so it never
    stalls the build the way the missing pull request did."""
    green = _green_on_main(state, env, head)
    if green is False:
        return _empty_failed(state, tid, head, f"empty hand-in: Ci, Twin or Ally failed on main {head[:12]}")
    if green is None:
        since = _empty_since(state, tid, now)
        if now - st.parse(since) >= EMPTY_WAIT:
            return _empty_failed(state, tid, head, f"empty hand-in: Ci, Twin and Ally still unknown on main {head[:12]} after {EMPTY_WAIT}")
        why = f"empty hand-in, nothing to merge; waiting for Ci, Twin and Ally green on main {head[:12]}"
        return st.with_task(state, tid, auto_merge={"enabled": False, "why": why, "head": head}, empty_since=since), "merge_pending"
    facts = {"merged_sha": head, "merged_at": st.iso(now), "files": [], "empty_handin": True}
    return _mark_done(state, tid, {"head_sha": head}, facts)


EMPTY_WAIT = timedelta(hours=4)  # how long an empty hand-in waits for main's checks to be known (D-551)


def _green_on_main(state: State, env: Env, head: str) -> bool | None:
    """True for a merge the runner marked DONE (not an empty hand-in), else main's checks on it (None: unknown)."""
    merges = {t.get("merged_sha") for t in state["tasks"] if t.get("status") == st.DONE and not t.get("empty_handin")}
    return True if head in merges else env.main_checks_green(head)


def _empty_since(state: State, tid: str, now: datetime) -> str:
    """When the empty hand-in started waiting: kept on the task, else its attempt's end, else now."""
    task = st.entry(state, tid)
    history = task.get("history") or [{}]
    return str(task.get("empty_since") or history[-1].get("ended_at") or st.iso(now))


def _empty_failed(state: State, tid: str, head: str, reason: str) -> tuple[State, str]:
    failed = st.with_task(record_failure(state, tid, reason), tid, empty_since=None)
    return _close_history(failed, tid, {"head_sha": head}, "check_failed"), "check_failed"


PROOF_MISSING = st.PROOF_MISSING


def _proof_ok(state: State, tid: str) -> bool:
    """A cleanup task is DONE only with its stage's named proof recorded (governance section 6)."""
    task = st.entry(state, tid)
    if task.get("type") != "cleanup":
        return True
    proof = (state.get("stage_proofs") or {}).get(str(task.get("stage"))) or {}
    return bool(str(proof.get("evidence", "")).strip())


def record_proofs(cfg: Config, state: State, now: datetime) -> State:
    """For each cleanup task waiting for its stage proof in a stage the runner proves itself
    (cfg.proof_stages, runner/proof.py): record `stage_proofs[N]` once the check finds the evidence
    true, else note on the task what the proof still waits for (`proof_waits_for`, shown in STATUS.md).
    Nothing is ever recorded that the check did not find true (D-551)."""
    if cfg.stage_proof is None:
        return state
    out = state
    for task in state["tasks"]:
        stage = str(task.get("stage"))
        if not st.awaits_proof(task) or stage not in cfg.proof_stages or _proof_ok(out, task["id"]):
            continue
        try:
            record, missing = cfg.stage_proof(out, stage)
        except Exception as exc:  # noqa: BLE001  # reason: an unreadable nightly or git error must not stop the cycle (D-551)
            record, missing = None, [f"the proof check failed: {str(exc)[:200]}"]
        if record:
            proofs = dict(out.get("stage_proofs") or {}, **{stage: dict(record, at=st.iso(now))})
            out = st.with_task(dict(out, stage_proofs=proofs), task["id"], proof_waits_for=None)
        elif task.get("proof_waits_for") != missing:
            out = st.with_task(out, task["id"], proof_waits_for=missing)
    return out


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
    reason = verify.auto_merge_reason(state, str(task.get("branch", "")), head, st.leases_of(state))
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
    """The latest DONE merge (task, squash SHA, files): what a red nightly or smoke run blames first.
    An empty hand-in merged nothing (its `merged_sha` is another task's merge), so it is never blamed (D-551)."""
    merged = [
        t for t in state["tasks"]
        if t.get("status") == st.DONE and t.get("merged_sha") and t["id"] != st.FIX_MAIN and not t.get("empty_handin")
    ]
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
    red_sha = str(state["main_red"].get("sha", ""))
    if _false_smoke_red(state) and _smoke_env_strikes(state, red_sha) < SMOKE_ENV_STRIKES and env.main_checks_green(red_sha) is True:
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

    The third Actions cycle under the OAuth token marked main red because the smoke test failed in a
    runner image without the test's prerequisites while Ci, Twin and Ally were green on the same
    commit; FIX-MAIN would have reverted an unrelated, older merge."""
    red = dict(state["main_red"], cleared_at=st.iso(now), false_red=True)
    out = dict(state, main_red=None, main_red_history=[*state.get("main_red_history", []), red])
    out = st.with_task(out, st.FIX_MAIN, status=st.SKIPPED, reason="false red: the head's checks were green (D-498)")
    return _note_smoke_env(out, str(red.get("sha", "")), now)


SMOKE_ENV_STRIKES = 3  # smoke reds on one head that green checks excuse before the red is believed (D-498)


def _smoke_env_strikes(state: State, head: str) -> int:
    return sum(1 for n in state.get("smoke_env_problems", []) if n.get("sha") == head)


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
    """A worker that died after opening a PR but before MERGE_PENDING: look at the PR, don't redo the work.

    Every lane's lease is looked at (D-515); with one lane that is the single lease, as before."""
    for lease in st.leases_of(before).values():
        if not lease or st.status(before, lease["task"]) != st.IN_PROGRESS:
            continue
        if st.status(after, lease["task"]) == st.QUEUED and env.pr_for(lease["task"]) is not None:
            after = st.with_task(after, lease["task"], status=st.MERGE_PENDING)
    return after


def run_cycle(cfg: Config, env: Env, now: datetime) -> str:
    """Run one cycle and return what it did (for logs and the drills)."""
    env.merge_main()
    base = env.read_state()
    items = env.read_inbox()
    state = st.release_expired(_record_inbox(st.sync_graph(base, cfg.graph), cfg, items, now), now)
    state = release_proven(record_proofs(cfg, state, now))
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
    ids = _by_day(cfg, st.eligible(state, now, _passed(cfg, state), cfg.proof_stages), now)
    if not ids:
        if state != base and not env.push_state(state, base):
            return "lost_race"
        return "fix_main" if state.get("main_red") else (result or "idle")
    if ids[0] != st.FIX_MAIN:
        state, ok, head = _smoke(state, env, now, "start routine")  # BUILD_BRIEF.md:240
        if not ok:
            if env.main_checks_green(head) is True and _smoke_env_strikes(state, head) < SMOKE_ENV_STRIKES:
                # Ci, Twin and Ally passed on this very commit: the smoke failed in the runner's own
                # environment (a missing tool or database), not on main. Record it and stop; nothing is
                # claimed and main is not blamed (D-498). Three strikes on the same head and the red
                # is believed after all: two green branches can still break main together.
                state = _note_smoke_env(state, head, now)
                return "smoke_env" if state == base or env.push_state(state, base) else "lost_race"
            red = set_main_red(state, cfg, "smoke", head, offending_merge(state), now)
            return "main_red" if env.push_state(red, base) else "lost_race"
    task, state = _paced_pick(cfg, state, ids, now)
    if task is None:
        return "paced" if state == base or env.push_state(state, base) else "lost_race"
    claimed = st.claim(state, task["id"], cfg.session, now, taken=cfg.decisions() if cfg.decisions else None)
    if not env.push_state(claimed, base):
        return "lost_race"
    done = _work(cfg, env, claimed, task, now)
    # A handed-off FIX-MAIN must reach the session as `handed_off`, or the session stops and the claimed
    # revert is never worked (D-501); only a finished FIX-MAIN cycle reports `fix_main`.
    return "fix_main" if task["id"] == st.FIX_MAIN and done not in ("lost_race", "handed_off") else done


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
    current: dict[str, Any] = {"state": claimed, "lost": False}
    lane = st.lane_of(claimed, tid) or st.LANE_ONE

    def heartbeat() -> None:
        """Renew when due, on the fresh state (update_state). It never raises. Only a fresh read showing
        this session no longer holds the task's lane lease (LeaseError: gone, expired or another
        session's) marks the lease lost, and the attempt then ends as lost_race with nothing pushed.
        A transient failure (None after every try, or any other error) changes nothing: update_state
        put its heads back, so the next beat simply tries again (D-515)."""
        moment = cfg.clock()
        if current["lost"] or not st.renewal_due(current["state"], moment):
            return
        try:
            renewed = env.update_state(lambda fresh: st.renew(fresh, tid, cfg.session, moment, lane))
        except st.LeaseError:
            current["lost"] = True
            return
        except Exception:  # noqa: BLE001  # reason: a heartbeat must never crash the worker loop (D-515)
            return
        if renewed is not None:
            current["state"] = renewed

    res = env.run_worker(task, model, str(entry.get("last_failure", "")), heartbeat)
    if res.outcome == "handed_off":
        return "handed_off"  # the session works the task and calls `python3 -m runner --after`
    if current["lost"]:
        return "lost_race"  # the lease may belong to another session now: write nothing (D-515)
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
    that head (so the merge is the runner's identity and starts workflows), then MERGE_PENDING (D-486).
    An empty hand-in (no commits on top of main) is a failed attempt, except a cleanup task's, which has
    nothing to push and is resolved at once by _resolve_empty (D-551)."""
    tid = task["id"]
    lane = st.lane_of(state, tid) or st.LANE_ONE
    record = _history_entry(state, task, res, model, cfg)
    final = _after_worker(state, task, res, cfg, model, now)
    history = [*st.entry(state, tid).get("history", []), dict(record, result=st.status(final, tid))]
    final = _record_usage(st.with_task(final, tid, history=history), tid, model, res, now, cfg)
    if st.status(final, tid) == st.MERGE_PENDING and res.commits_ahead == 0:
        # An empty cleanup hand-in: nothing to push or merge (GitHub opens no pull request), so no
        # branch push and no auto-merge; it is resolved now, or by _without_pr in a later cycle (D-551).
        final = st.with_task(final, tid, reviewed_sha=res.head_sha, branch=res.branch)
        final = _resolve_empty(final, env, tid, res.head_sha, now)[0]
    elif st.status(final, tid) == st.MERGE_PENDING:
        reviewed = env.update_state(_review_record(tid, lane, cfg, res))
        if reviewed is None:
            return "lost_race"  # no review record, no branch push: the lease is not this session's (D-515)
        if not env.push_branch(res.branch, res.head_sha):
            return _push_failed(env, reviewed, tid, lane, res.head_sha)
        merged = _auto_merge(reviewed, env, tid, res.head_sha)
        review = {k: st.entry(reviewed, tid)[k] for k in ("reviewed_sha", "review_lease", "branch")}
        final = st.with_task(_ours_onto(state, final, reviewed), tid, **review, auto_merge=st.entry(merged, tid)["auto_merge"])
        state = reviewed
    final = st.with_lease(final, lane, None)
    if env.update_state(_release_onto(state, final, (tid, lane, cfg.session))) is None:
        return "lost_race"
    return str(st.entry(final, tid)["status"])


def _review_record(tid: str, lane: str, cfg: Config, res: WorkerResult) -> Callable[[State], State | None]:
    """The transform for the review record (update_state, D-515): `reviewed_sha`, the lease held and the
    branch go on the task only while, on the fresh state, this session holds the task's lane lease (live)
    and the task is IN_PROGRESS; otherwise nothing is written, so auto-merge can never accept a head
    reviewed under a lease another session has since taken."""

    def apply(fresh: State) -> State | None:
        held = st.live_lease(fresh, cfg.clock(), lane) or {}
        if held.get("task") != tid or held.get("session") != cfg.session or st.status(fresh, tid) != st.IN_PROGRESS:
            return None
        review = {"reviewed_sha": res.head_sha, "review_lease": copy.deepcopy(held), "branch": res.branch}
        return st.with_task(fresh, tid, **review)

    return apply


def _release_onto(base: State, final: State, holder: tuple[str, str, str]) -> Callable[[State], State | None]:
    """The transform for finish's last write, the lease release (update_state, D-515).

    On an unchanged state it is `final` itself. When another writer moved the state meanwhile (an
    inbox record, a graph sync), it applies only what finish changed (the keys and task entries that
    differ from `base`) onto the fresh state, and only while this session still holds the task's
    lane lease and the task's status is still the one finish started from; otherwise it gives up.
    What it applies replaces whole values (see _ours_onto): this task's own entry and the top-level
    keys finish touched (`usage`, `pacing`), nothing else."""
    tid, lane, session = holder

    def apply(fresh: State) -> State | None:
        if fresh == base:
            return final
        held = st.lease_in(fresh, lane) or {}
        if held.get("task") != tid or held.get("session") != session or st.status(fresh, tid) != st.status(base, tid):
            return None
        return st.with_lease(_ours_onto(base, final, fresh), lane, None)

    return apply


def _ours_onto(base: State, ours: State, fresh: State) -> State:
    """`fresh` with every top-level key and task entry that `ours` changed from `base` taken from `ours`.

    Replacement is whole, not field by field: a changed task entry replaces the fresh entry entirely,
    and a changed top-level key (in finish: `usage`, and `pacing` if it moved) replaces the fresh value.
    finish changes only its own task's entry and those keys, so another writer's tasks and keys stay;
    the lease keys are never taken from `ours` (the caller sets the lane itself)."""
    out = copy.deepcopy(fresh)
    for key in (set(base) | set(ours)) - {"tasks", "lease", "leases"}:
        if key in ours and ours[key] != base.get(key):
            out[key] = copy.deepcopy(ours[key])
        elif key not in ours and key in base:
            out.pop(key, None)
    before = {t["id"]: t for t in base["tasks"]}
    for task in ours["tasks"]:
        if before.get(task["id"]) != task:
            out = _put_task(out, task)
    return out


def _put_task(state: State, task: dict[str, Any]) -> State:
    """`state` with the task's entry replaced whole (appended when the state lacks it)."""
    tasks = [copy.deepcopy(task) if t["id"] == task["id"] else t for t in state["tasks"]]
    if not any(t["id"] == task["id"] for t in state["tasks"]):
        tasks.append(copy.deepcopy(task))
    return dict(state, tasks=tasks)


def _push_failed(env: Env, state: State, tid: str, lane: str = st.LANE_ONE, head: str | None = None) -> str:
    """A rejected branch push is a credential or permission problem, not the worker's: retrying would
    spend a whole attempt to fail the same way. The task is BLOCKED with the reason (only its dependents
    wait), the lease is released at once and the build moves on; a person fixes the cause and re-queues
    the task (D-513). With no head at all the push was refused before it ran, because git could not
    read the branch or main; that gets its own reason (D-551)."""
    reason = "branch push rejected; fix the push credential, then set the task back to QUEUED"
    if head == "":
        reason = "no head to push: git could not read the task branch or main; fix that, then set the task back to QUEUED"
    blocked = st.with_lease(st.with_task(state, tid, status=st.BLOCKED, reason=reason), lane, None)
    return "push_failed" if env.push_state(blocked, state) else "lost_race"


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

"""One runner cycle (BUILD_BRIEF.md:205-292). The outside world comes in through Env."""

from __future__ import annotations

import copy
import re
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Protocol

from runner import lanes, outbox, state as st, verify
from runner.state import State

MAX_ATTEMPTS = 3  # BUILD_BRIEF.md:540; one more only through a plan-fix re-queue (D-677)
HOURLY = timedelta(hours=1)
WAIT_RESULTS = ("merge_pending", "not_done", "check_failed", "no_changes", "unreviewed_head",
                "branch_updated", "update_conflict", "update_push_failed")
# Stage C (D-620): a reviewed head that fell behind main is brought up to date by the runner's own update
# merge. A real conflict is never resolved here: the attempt fails and the retry starts from the branch.
UPDATE_CONFLICT = ("merging main into the task branch conflicts in {paths}: the runner resolves only docs/decisions.md "
                   "and a plan lock both sides kept true, so this attempt fails and the next one starts from the "
                   "branch with main merged in, or fresh from main when that still conflicts (D-583, D-620)")
UPDATE_PUSH_LIMIT = 3  # rejected update-merge pushes (not races) before the attempt fails (D-620)
UPDATE_WAIT = timedelta(hours=2)  # how long an update merge git cannot make waits before the attempt fails (D-620)
UPDATE_STUCK = "the branch could not be brought up to date with main for {wait}: {why} (D-620)"
UPDATE_CLOSE = ("Closed by the runner: this branch could not be brought up to date with main, and auto-merge was on "
                "({why}). The next attempt continues this branch with main merged in, or starts fresh when that "
                "still conflicts (D-620).")
NO_FREE_LANE = "no_free_lane"  # a run whose lane is busy or above the lanes in use, with nothing else to do (D-620)
# An empty hand-in (no commits on top of main): GitHub opens no pull request for it, so it can never
# merge. Only a cleanup task may legitimately find nothing to change (D-551).
NO_CHANGES = "no changes: the hand-in has no commits on top of main (only a cleanup task may hand in nothing, D-551)"
# A branch push rejected as not a fast-forward is the attempt's fault, never the credential's (D-583).
PUSH_NOT_FAST_FORWARD = ("branch push rejected as not a fast-forward: the remote task branch has commits this attempt "
                         "did not build on (the next attempt starts from it, D-583)")
REQUEUE_HINT = 'run `python3 -m runner --requeue <task> --reason "..."` (D-583)'
UNREVIEWED_MERGE = ("merged head {head} is not the reviewed head {reviewed}: work nobody reviewed was merged, so main "
                    "is red and FIX-MAIN reverts it (D-583, D-584)")
UNREVIEWED_HEAD = ("pull request head {head} is not the reviewed head {reviewed}: commits were pushed after the review, "
                   "so auto-merge is refused for it; the next attempt continues the branch and is reviewed again (D-584)")
UPDATED_HEAD = ("pull request head {head} is the reviewed head {reviewed} updated from main, but auto-merge is off and the "
                "runner turns it on only for a reviewed head; the next attempt continues the branch (D-584)")
UNREVIEWED_CLOSE = ("Closed by the runner: head `{head}` is not the head its review approved, and auto-merge was on, "
                    "so it could have merged unreviewed work. The next attempt continues this branch and is reviewed again (D-584).")
HEAD_UNPLACED = ("unchecked merge: merged head {head} (merge {merge}) could not be checked against the reviewed head "
                 "for {wait}; a person must look at that merge, then re-queue the task with --requeue (D-584)")
HEAD_CHECK_WAIT = timedelta(hours=2)  # how long a merged head git cannot place waits before it is BLOCKED (D-584)
# The marker the runner's GitHubEnv puts in an auto-merge closing comment (runner/github_env.py DROP_MARKER).
DROP_MARKER = "<!-- askhava-runner: closed to drop auto-merge (D-584) -->"
# The BLOCKED reasons that are not the task's own fault, the only ones `--requeue` lifts (D-583).
REQUEUABLE = ("branch push rejected", "no head to push", "permission blocked", st.PROOF_MISSING, "unchecked merge")
# `--requeue T### --plan-fix D-###`: one more attempt for a capped task after a logged plan fix (D-677).
PLAN_FIX_ID = re.compile(r"^D-\d{3,4}$")


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

    def push_rejection(self) -> str:
        """The git output of the last rejected push_branch, '' when none or unknown (D-583)."""
        ...

    def reviewed_plus_main(self, reviewed: str, head: str, number: int) -> bool | None:
        """Whether a PR head holds exactly the reviewed work combined with main (GitHub's "Update branch",
        by merge or by rebase; never a merge that adds content of its own): True or False, None when git
        cannot say (D-584)."""
        ...

    def close_pr(self, number: int, body: str) -> bool:
        """Close a pull request (which also clears its auto-merge) and then comment on it; False when the
        close failed, and then no comment is posted (D-584)."""
        ...

    def prepare_branch(self, task: dict[str, Any], attempt: int, session: str) -> dict[str, Any]:
        """Make the local task branch the attempt works on before the worker starts: on top of origin's
        task branch (main merged in) when an earlier attempt pushed one, else from main; a new name when
        that branch's pull request was closed or merged or main does not merge cleanly. An open pull
        request with auto-merge on is closed first, while `session` holds the lease. Returns what it made
        ({"branch", "branch_ready", "base", "renamed_from"?, "dropped_pr"?, ...}), {} when nothing (D-583)."""
        ...

    def enable_auto_merge(self, branch: str, head_sha: str) -> tuple[bool, str]:
        """Find (or open, REST) the branch's PR and turn on squash auto-merge as the runner's own identity,
        so the merge starts workflows (pull_request.closed); returns (done, how or why not)."""
        ...

    def merge_main(self) -> str:
        """Merge main into claude/kit-state with a normal merge commit (BUILD_BRIEF.md Appendix B, K0 step 4)."""
        ...

    def update_branch(self, branch: str, reviewed: str) -> dict[str, Any]:
        """Merge main into a task branch whose head is the reviewed head and push it (stage C, D-620):
        {"status": "current" | "pushed" (with "sha", "main") | "conflict" (with "paths") | "push_failed"
        (with "why") | "moved" | "error"}; runner/github_env.py documents each."""
        ...

    def update_merge_of(self, reviewed: str, head: str, number: int) -> bool | None:
        """Whether `head` is an update merge of the reviewed head: parents exactly the reviewed commit and
        a commit on main, tree the update merge's own (D-620); None when git cannot say."""
        ...

    def checks_on(self, sha: str) -> dict[str, str] | None:
        """{required check name: conclusion, or "running"} on that commit (REST, read only); None when
        the checks cannot be read (D-630)."""
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
    lanes: int = 1  # kit/runner.json "lanes": how many tasks may be in flight at once (D-515, D-620)
    # Reads the D-numbers on main's docs/decisions.md (None when unreadable). A claim reserves the
    # task's decision numbers only when this is set and returns a set (D-530).
    decisions: Callable[[], set[int] | None] | None = None
    # The stages whose named proof the runner records itself (runner/proof.py, D-551), and the check that
    # returns (the proof record, or None, and what the proof still waits for) for one of them.
    proof_stages: frozenset[str] = frozenset()
    stage_proof: Callable[[State, str], tuple[dict[str, Any] | None, list[str]]] | None = None
    # The lane a backup-runner run serves (RUNNER_LANE, from the workflow's gate); a run with one claims
    # only that lane. None (a manual or routine run) claims the first free lane (D-620).
    lane_hint: str | None = None
    # Whether a path is in the locked blueprint (scripts/check_plan_lock.py): a task owning one also owns
    # kit/plan.lock, so two such tasks never run side by side (D-620).
    locked: Callable[[str], bool] | None = None


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


def _resolve_merges(state: State, env: Env, cfg: Config, now: datetime) -> tuple[State, list[tuple[str, str, str]]]:
    """Resolve every MERGE_PENDING task, each on its own lane (D-620); [(task, lane, result)] in state
    order. With one lane there is at most one, exactly as before."""
    outcomes: list[tuple[str, str, str]] = []
    for tid in [t["id"] for t in state["tasks"] if t.get("status") == st.MERGE_PENDING]:
        lane = st.task_lane(st.entry(state, tid))
        state, result = _resolve_one(state, env, cfg, now, st.entry(state, tid))
        outcomes.append((tid, lane, result))
        state = _lane_outcome(state, tid, lane, result, now, env.checks_on)
    return state, outcomes


def _resolve_one(state: State, env: Env, cfg: Config, now: datetime, task: dict[str, Any]) -> tuple[State, str]:
    """One MERGE_PENDING task: its pull request merged (DONE or a red main), still open, or absent."""
    tid = task["id"]
    pr = env.pr_for(tid)
    if pr is not None and _earlier_pr(task, pr, env):
        pr = None  # an earlier attempt's PR beside an empty hand-in already on main (D-584)
    if pr is None:
        return _without_pr(state, env, task, now)
    reviewed = _head_reviewed(state, env, tid, pr)
    if reviewed is not None and task.get("head_check_since"):
        state = st.with_task(state, tid, head_check_since=None)  # the head was placed: the wait is over (D-584)
    if not pr.get("merged"):
        return _open_pr_step(state, env, tid, pr, reviewed, now)
    if verify.checks_state(pr) == "pending":
        return state, "merge_pending"
    if reviewed is None:
        return _unplaced_merge(state, cfg, tid, pr, now)
    return _merged(state, tid, pr, cfg, now, reviewed)


def _lane_outcome(state: State, tid: str, lane: str, result: str, now: datetime,
                  reread: Callable[[str], dict[str, str] | None] = lambda sha: None) -> State:
    """Strikes for the lane fall back (D-620), only for an attempt that ended: a task on the lane DONE
    clears them; a required check that failed on an update merge after the same check passed on the
    reviewed head before it (main's change broke it, `_broken_by_main`, which reads a check that was
    still running at the update again through `reread`), or an update merge that conflicted or could
    not be pushed for good, is a strike; any other failed check is the task's own and clears them. A
    merge still waiting never strikes. Lane "1" and the "fix" lane never strike."""
    if lane in (st.LANE_ONE, st.FIX_LANE):
        return state
    if result == "done" or (result == "check_failed" and not _broken_by_main(st.entry(state, tid), reread)):
        return lanes.clear_strikes(state, lane)
    if result in ("check_failed", "update_conflict", "update_push_failed"):
        why = str(st.entry(state, tid).get("last_failure") or st.entry(state, tid).get("reason") or result)
        return lanes.strike(state, lane, f"{tid} {result}: {why[:200]}", now)
    return state


FAILED = ("failure", "cancelled", "timed_out")


def _broken_by_main(task: dict[str, Any], reread: Callable[[str], dict[str, str] | None]) -> bool:
    """Whether a check that failed on the attempt's last head had passed on the reviewed head the last
    update merge started from: the failure came with main's change, not the task's own work (D-620).
    A check that had not finished when the update was recorded is read again now (`reread`, the Env's
    `checks_on` of that head), so it is judged on its final conclusion (D-630)."""
    updates, history = task.get("update_merges") or [], task.get("history") or []
    if not updates or not history:
        return False
    failed = {str(c.get("name")) for c in history[-1].get("checks") or [] if c.get("conclusion") in FAILED}
    before = dict(updates[-1].get("checks") or {})
    if any(before.get(name) in (None, "None") for name in failed) and updates[-1].get("from"):
        before.update({k: v for k, v in (reread(str(updates[-1]["from"])) or {}).items() if before.get(k) in (None, "None")})
    return any(before.get(name) == "success" for name in failed)


def _earlier_pr(task: dict[str, Any], pr: dict[str, Any], env: Env) -> bool:
    """Whether the branch's latest PR is not this attempt's: the task's reviewed head is already on main
    (an empty hand-in, for which GitHub opens no PR) and the PR carries another head, so the hand-in is
    resolved by _without_pr and never judged by an old merge (D-584)."""
    reviewed = str(task.get("reviewed_sha") or "")
    return bool(reviewed) and pr.get("head_sha") != reviewed and env.in_main(reviewed) is True


def _head_reviewed(state: State, env: Env, tid: str, pr: dict[str, Any]) -> bool | None:
    """Whether the PR's head is the task's `reviewed_sha`, or holds exactly that work combined with main
    (GitHub's "Update branch", by merge or rebase); True when the task has no `reviewed_sha` to compare
    with; None when git cannot say (D-583, D-584)."""
    reviewed, head = str(st.entry(state, tid).get("reviewed_sha") or ""), str(pr.get("head_sha") or "")
    if not reviewed or head == reviewed or head in _earlier_reviewed(st.entry(state, tid)):
        return True
    return env.reviewed_plus_main(reviewed, head, int(pr.get("number") or 0))


def _earlier_reviewed(task: dict[str, Any]) -> set[str]:
    """The task's reviewed heads before its update merges (D-620): reviewed work on an older main. A PR
    still showing one just after an update merge was pushed is GitHub catching up, not new work."""
    return {str(u.get("from")) for u in task.get("update_merges") or [] if u.get("from")}


def _open_pr_step(state: State, env: Env, tid: str, pr: dict[str, Any], reviewed: bool | None, now: datetime) -> tuple[State, str]:
    """An unmerged PR (D-584). A head nobody reviewed (or one git cannot place) is a failed attempt,
    never a wait for an auto-merge the runner refuses; if auto-merge is on, the PR is closed first so
    GitHub cannot merge that head, and if the close fails the task stays MERGE_PENDING so `_merged`
    still judges whatever merges. Red checks fail the attempt. An update merge of the reviewed head
    (the runner's own whose record was lost, or GitHub's "Update branch" by merge) becomes the reviewed
    head, and a reviewed head that fell behind main is brought up to date (`_update_step`, D-620).
    Another reviewed head updated from main merges only if auto-merge is already on; with it off the
    runner cannot turn it on for that head, so that too is a failed attempt. Otherwise auto-merge is
    turned on."""
    head, task = str(pr.get("head_sha") or ""), st.entry(state, tid)
    if not reviewed:
        return _unreviewed_step(state, env, tid, pr, head)
    if verify.checks_state(pr) == "failed":
        reason = pr.get("failure_log") or "required check failed on head"
        return _close_history(record_failure(state, tid, reason), tid, pr, "check_failed"), "check_failed"
    if state.get("main_red") and tid != st.FIX_MAIN:
        return state, "merge_pending"  # while main is red only FIX-MAIN moves: no update merge, no auto-merge (D-620)
    sha = str(task.get("reviewed_sha") or "")
    if sha and head != sha and head in _earlier_reviewed(task):
        return _update_wait(state, env, tid, pr, f"the pull request still shows {head[:12]}, the head before the update merge {sha[:12]}", now)
    if sha and head != sha and env.update_merge_of(sha, head, int(pr.get("number") or 0)) is True:
        state, sha = _adopt_update(state, tid, {"from": sha, "to": head, "how": "adopted"}, now), head
    if sha and head == sha:
        state, stepped = _update_step(state, env, tid, pr, now)
        if stepped:
            return state, stepped
    if pr.get("auto_merge"):
        return state, "merge_pending"
    if sha and head != sha:
        why = UPDATED_HEAD.format(head=head[:12], reviewed=sha[:12])
        return _close_history(record_failure(state, tid, why), tid, pr, "unreviewed_head"), "unreviewed_head"
    return _auto_merge(state, env, tid, head), "merge_pending"


def _unreviewed_step(state: State, env: Env, tid: str, pr: dict[str, Any], head: str) -> tuple[State, str]:
    """A PR head nobody reviewed: close it first when auto-merge is on, then fail the attempt (D-584)."""
    task = st.entry(state, tid)
    state, closed = _drop_auto_merge(state, env, tid, pr, UNREVIEWED_CLOSE.format(head=head[:12]))
    if not closed:
        why = f"PR head {head[:12]} is not the reviewed head and its auto-merge could not be turned off in time; waiting so any merge is judged"
        return st.with_task(state, tid, auto_merge={"enabled": True, "why": why, "head": head}), "merge_pending"
    why = UNREVIEWED_HEAD.format(head=head[:12], reviewed=str(task.get("reviewed_sha"))[:12])
    return _close_history(record_failure(state, tid, why), tid, pr, "unreviewed_head"), "unreviewed_head"


def _drop_auto_merge(state: State, env: Env, tid: str, pr: dict[str, Any], body: str) -> tuple[State, bool]:
    """Close a PR whose auto-merge is on (closing clears it), with the DROP_MARKER comment, and record it
    in `runner_closed_prs` so the retry continues the branch (D-584). (state, False) when the close
    failed or GitHub merged it just before (a close of a merged PR can still answer 200): the task then
    stays MERGE_PENDING so `_merged` judges whatever merged. A PR without auto-merge is left open."""
    if not pr.get("auto_merge"):
        return state, True
    closed = env.close_pr(int(pr.get("number") or 0), body + "\n\n" + DROP_MARKER)
    if not closed or (env.pr_for(tid) or {}).get("merged"):
        return state, False
    known = st.entry(state, tid).get("runner_closed_prs") or []
    return st.with_task(state, tid, runner_closed_prs=[*known, pr.get("number")]), True


def _adopt_update(state: State, tid: str, note: dict[str, Any], now: datetime) -> State:
    """The update merge `note["to"]` of the reviewed head `note["from"]` becomes the task's `reviewed_sha`:
    it holds exactly the reviewed work plus main (checked by the caller against `UpdateMerge.tree`), so
    `_merged`, the unreviewed-head path and auto-merge treat it as reviewed (D-584, D-620)."""
    task = st.entry(state, tid)
    record = {**note, "at": st.iso(now)}
    return st.with_task(state, tid, reviewed_sha=note["to"], update_merges=[*(task.get("update_merges") or []), record][-10:],
                        update_waiting_since=None, update_push_failures=None)


def _update_step(state: State, env: Env, tid: str, pr: dict[str, Any], now: datetime) -> tuple[State, str]:
    """Keep the reviewed head current with main (stage C, D-620); the result is '' when it already is.

    A clean merge is pushed by `env.update_branch` and becomes the reviewed head, and auto-merge stays
    on (or is turned on) for it. A real conflict fails the attempt (after closing a PR whose auto-merge
    is on), and the retry starts from the branch (prepare_branch, D-583). A rejected push that is not a
    race counts toward UPDATE_PUSH_LIMIT; a merge git cannot make waits at most UPDATE_WAIT."""
    task = st.entry(state, tid)
    done = env.update_branch(str(task.get("branch") or ""), str(task.get("reviewed_sha") or ""))
    status, why = done.get("status"), str(done.get("why") or "")
    if status == "current":
        return (st.with_task(state, tid, update_waiting_since=None) if task.get("update_waiting_since") else state), ""
    if status == "pushed":
        note = {"from": task.get("reviewed_sha"), "to": done["sha"], "main": done.get("main"), "how": "runner", "checks": _head_checks(pr)}
        state = _adopt_update(state, tid, note, now)
        return (state if pr.get("auto_merge") else _auto_merge(state, env, tid, str(done["sha"]))), "branch_updated"
    if status == "conflict":
        return _update_failed(state, env, tid, pr, UPDATE_CONFLICT.format(paths=", ".join(done.get("paths") or ["?"])), "update_conflict")
    if status == "push_failed" and not ("non-fast-forward" in why or "fetch first" in why):
        count = int(task.get("update_push_failures") or 0) + 1
        if count >= UPDATE_PUSH_LIMIT:
            return _update_failed(state, env, tid, pr, f"the update merge could not be pushed {count} times: {why[:300]} (D-620)", "update_push_failed")
        return st.with_task(state, tid, update_push_failures=count), "merge_pending"  # not a strike until the attempt fails
    return _update_wait(state, env, tid, pr, why or str(status), now)


def _head_checks(pr: dict[str, Any]) -> dict[str, str]:
    """{check name: conclusion} of the required checks on the PR's head, kept with an update merge so a
    later failure can be told apart from one main brought in (`_broken_by_main`, D-620)."""
    head = pr.get("head_sha")
    # Only finished checks: one still running is left out, and _broken_by_main reads it again later (D-630).
    return {str(c.get("name")): str(c.get("conclusion")) for c in pr.get("checks", [])
            if c.get("sha") == head and c.get("name") in verify.REQUIRED_CHECKS and c.get("conclusion")}


def _update_failed(state: State, env: Env, tid: str, pr: dict[str, Any], reason: str, result: str) -> tuple[State, str]:
    """The update merge failed for good: close the PR first when its auto-merge is on (so nothing merges
    while the task is back in the queue), then fail the attempt; a failed close keeps it MERGE_PENDING."""
    state, closed = _drop_auto_merge(state, env, tid, pr, UPDATE_CLOSE.format(why=reason[:300]))
    if not closed:
        head = str(pr.get("head_sha") or "")
        why = f"{reason[:300]}; its auto-merge could not be turned off in time, so it waits and any merge is judged"
        return st.with_task(state, tid, auto_merge={"enabled": True, "why": why, "head": head}), "merge_pending"
    failed = st.with_task(record_failure(state, tid, reason), tid, update_waiting_since=None, update_push_failures=None)
    return _close_history(failed, tid, pr, result), result


def _update_wait(state: State, env: Env, tid: str, pr: dict[str, Any], why: str, now: datetime) -> tuple[State, str]:
    """An update merge git could not make or push for a passing reason (origin moved, a push race, a git
    error): wait, saying why, for at most UPDATE_WAIT; a behind branch can never merge, so after that the
    attempt fails instead of stalling the build."""
    since = str(st.entry(state, tid).get("update_waiting_since") or st.iso(now))
    if now - st.parse(since) >= UPDATE_WAIT:
        return _update_failed(state, env, tid, pr, UPDATE_STUCK.format(wait=UPDATE_WAIT, why=why[:300]), "update_push_failed")
    return st.with_task(state, tid, update_waiting_since=since), "merge_pending"


def _unplaced_merge(state: State, cfg: Config, tid: str, pr: dict[str, Any], now: datetime) -> tuple[State, str]:
    """A merged head git cannot place against the reviewed head (D-584): never a red main on a guess,
    and never a wait without end. It waits up to HEAD_CHECK_WAIT, saying why; then the task is BLOCKED
    with the reason and an outbox item asks a person to look at that merge."""
    head = str(pr.get("head_sha") or "")
    since = str(st.entry(state, tid).get("head_check_since") or st.iso(now))
    if now - st.parse(since) < HEAD_CHECK_WAIT:
        return st.with_task(state, tid, head_check_since=since), "merge_pending"
    reason = HEAD_UNPLACED.format(head=head[:12], merge=str(pr.get("merged_sha", ""))[:12], wait=HEAD_CHECK_WAIT)
    body = f"{reason}\n\nLook at that merge on main: revert it if it holds work nobody reviewed, then re-queue the task."
    outbox.write_outbox(cfg.outbox_dir, f"{tid}-merged-head", f"{tid}: a merged head the runner could not check", "blocked", body)
    return _close_history(st.with_task(state, tid, status=st.BLOCKED, reason=reason), tid, pr, "not_done"), "not_done"


def _merged(state: State, tid: str, pr: dict[str, Any], cfg: Config, now: datetime, reviewed: bool = True) -> tuple[State, str]:
    """A merged PR: DONE only from check_done and a reviewed head; otherwise main is red (governance
    section 5). A merged head nobody reviewed (work pushed after the review that merged on its own) is
    treated like a tree mismatch: the task is BLOCKED and main is red with check "review", so FIX-MAIN
    reverts the merge and the task then returns to the queue with its attempt counted (D-583, D-584)."""
    problems = verify.check_done(pr, pr.get("head_tree", ""), pr.get("merge_tree", ""))
    if not reviewed:
        entry_sha = str(st.entry(state, tid).get("reviewed_sha") or "")
        problems = [UNREVIEWED_MERGE.format(head=str(pr.get("head_sha"))[:12], reviewed=entry_sha[:12]), *problems]
    if problems and tid == st.FIX_MAIN:
        return _close_history(record_failure(state, tid, "; ".join(problems)), tid, pr, "not_done"), "not_done"
    if problems:
        out = st.with_task(state, tid, status=st.BLOCKED, reason="; ".join(problems))
        merge = str(pr.get("merged_sha", ""))
        out = set_main_red(out, cfg, "tree" if reviewed else "review", merge, (tid, merge, list(pr.get("files", []))), now)
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
    """A worker that died after its reviewed head was pushed but before MERGE_PENDING: look at the PR,
    don't redo the work.

    Every lane's lease is looked at (D-515). Only a PR this very attempt finished is adopted (D-584): it
    is open (or already merged) and not one the runner closed itself (`runner_closed_prs`), its head is the task's
    `reviewed_sha`, and that review was made under the lease that died (`review_lease`). Any other PR
    (a closed one with red checks, a head nobody reviewed, an earlier attempt's) leaves the task QUEUED
    for its retry, which continues the branch and is reviewed again."""
    for lease in st.leases_of(before).values():
        if not lease or st.status(before, lease["task"]) != st.IN_PROGRESS:
            continue
        tid = lease["task"]
        if st.status(after, tid) != st.QUEUED:
            continue
        pr = env.pr_for(tid)
        if pr is not None and _finished_by(st.entry(after, tid), pr, lease):
            after = st.with_task(after, tid, status=st.MERGE_PENDING)
    return after


def _finished_by(task: dict[str, Any], pr: dict[str, Any], lease: dict[str, Any]) -> bool:
    """Whether the PR carries the head this lease's attempt reviewed, and is open or already merged (D-584)."""
    closed = pr.get("state", "open") != "open" or pr.get("number") in (task.get("runner_closed_prs") or [])
    if closed and not pr.get("merged"):
        return False
    reviewed = str(task.get("reviewed_sha") or "")
    return bool(reviewed) and pr.get("head_sha") == reviewed and (task.get("review_lease") or {}).get("session") == lease.get("session")


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
    state, outcomes = _resolve_merges(state, env, cfg, now)
    if outcomes:
        _note_fallback(cfg, base, state)
        if state != base and not env.push_state(state, base):
            _drop_fallback_note(cfg)
            return "lost_race"
        base = state
    lane = lanes.claim_lane(cfg.lanes, state, now, cfg.lane_hint) or lanes.fix_lane(state, now)
    if lane is None and state.get("main_red"):
        # No lane can claim, yet a red main must still be cleared once FIX-MAIN is DONE, or a task the
        # red-main guard holds in a full lane would stall the build for good (D-630).
        state, red = _red_main_step(cfg, env, state, now)
        if red:
            return red if state == base or env.push_state(state, base) else "lost_race"
    stop = _stop_result(outcomes, lane or cfg.lane_hint, lane is not None, (state, now))
    if stop:
        return stop
    state, red = _red_main_step(cfg, env, state, now)
    if red:
        return red if state == base or env.push_state(state, base) else "lost_race"
    done = next((r for _, _, r in outcomes if r == "done"), "")
    return _next_step(cfg, env, state, base, now, done, str(lane))


def _stop_result(outcomes: list[tuple[str, str, str]], lane: str | None, claims: bool, at: tuple[State, datetime]) -> str:
    """Why the cycle ends after resolving merges, '' when it goes on to claim (D-620).

    `lane` is the lane this cycle claims on, or (when `claims` is false) the lane its run serves, if
    any. A waiting or failed result on that lane ends the cycle, as with one lane (where every task is
    on lane "1"); one on another lane does not, so a run serving a free lane still claims. With nothing
    to claim, the cycle ends with its own lane's result, else the first waiting result, else
    "lease_live" (a live lease holds the lane), else NO_FREE_LANE."""
    waits = [(task_lane, r) for _, task_lane, r in outcomes if r in WAIT_RESULTS]
    own = [r for task_lane, r in waits if lane is None or task_lane == lane]
    if own:
        return own[0]
    if claims:
        return ""
    if waits:
        return waits[0][1]
    state, now = at
    return "lease_live" if st.live_leases(state, now) else NO_FREE_LANE


def _note_fallback(cfg: Config, before: State, after: State) -> None:
    """The write that sets `lanes_fallback` tells Casey why, through the outbox (D-620)."""
    if lanes.fell_back(before, after):
        outbox.write_outbox(cfg.outbox_dir, "lanes-fallback", "Runner: back to one lane", "info", lanes.fallback_note(after))


def refill_lanes(cfg: Config, state: State, now: datetime) -> list[str]:
    """The free lanes a new run should be started for, at most one per task that could be claimed beside
    the tasks in flight (pacing aside); the backup-runner chain step dispatches one run per lane (D-620)."""
    free = lanes.free_lanes(cfg.lanes, state, now)
    if not free:
        return []
    ids = _by_day(cfg, st.eligible(state, now, _passed(cfg, state), cfg.proof_stages), now)
    ready = lanes.compatible_ids(ids, state, now, lambda tid: _task_def(cfg, state, tid), cfg.locked)
    return free[: len(ready)]


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


def _next_step(cfg: Config, env: Env, state: State, base: State, now: datetime, result: str, lane: str = st.LANE_ONE) -> str:
    """Open a gate, or claim the first eligible task that may run beside the tasks in flight on `lane`."""
    opened = _open_gate_p(cfg, state) or _next_gate(cfg, env, state)
    if opened is not None:
        return "gate_open" if env.push_state(opened, base) else "lost_race"
    ids = _by_day(cfg, st.eligible(state, now, _passed(cfg, state), cfg.proof_stages), now)
    ids = lanes.compatible_ids(ids, state, now, lambda tid: _task_def(cfg, state, tid), cfg.locked)
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
    claimed = st.claim(state, task["id"], cfg.session, now, lane, taken=cfg.decisions() if cfg.decisions else None)
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
        if current["lost"] or not st.renewal_due(current["state"], moment, lane):
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

    answers = {d: a for d in task.get("deps", []) if (a := st.entry(claimed, d).get("answer"))}
    if answers:
        task = dict(task, gate_answers=answers)  # a conditional gate's yes or no reaches its dependent task (T170)
    prepared = _prepare_branch(cfg, env, current, task, lane)
    if prepared is None:
        return "lost_race"  # the lease is not this session's any more: write nothing else (D-515)
    res = env.run_worker(prepared, model, str(entry.get("last_failure", "")), heartbeat)
    if res.outcome == "handed_off":
        return "handed_off"  # the session works the task and calls `python3 -m runner --after`
    if current["lost"]:
        return "lost_race"  # the lease may belong to another session now: write nothing (D-515)
    return finish(cfg, env, current["state"], task, res, model, now)


def _prepare_branch(cfg: Config, env: Env, current: dict[str, Any], task: dict[str, Any], lane: str) -> dict[str, Any] | None:
    """The task as the worker gets it, after env.prepare_branch made its local branch (D-583).

    A retry continues the branch an earlier attempt pushed, so its push fast-forwards. A new branch
    name (the old pull request was closed or merged, or main did not merge cleanly) is recorded as the
    task's `branch`, and a pull request the runner closed to drop auto-merge is added to the task's
    `runner_closed_prs` (so the next attempt still continues the branch), while this session holds the
    lease, so `--after` and auto-merge use them; None when that write finds the lease gone."""
    tid = task["id"]
    prep = env.prepare_branch(task, int(st.entry(current["state"], tid).get("attempts", 0)) + 1, cfg.session)
    if prep.get("renamed_from") or prep.get("dropped_pr"):
        recorded = env.update_state(_branch_record(tid, lane, cfg, prep))
        if recorded is None:
            return None
        current["state"] = recorded
    return dict(task, branch=prep["branch"], branch_prep=prep) if prep else task


def _branch_record(tid: str, lane: str, cfg: Config, prep: dict[str, Any]) -> Callable[[State], State | None]:
    """The transform that records the prepared branch and a pull request closed to drop auto-merge,
    only while this session holds the lane's live lease."""

    def apply(fresh: State) -> State | None:
        held = st.live_lease(fresh, cfg.clock(), lane) or {}
        if held.get("task") != tid or held.get("session") != cfg.session:
            return None
        closed = list(st.entry(fresh, tid).get("runner_closed_prs") or [])
        if prep.get("dropped_pr") and prep["dropped_pr"] not in closed:
            closed.append(prep["dropped_pr"])
        extra = {"runner_closed_prs": closed} if closed else {}
        return st.with_task(fresh, tid, branch=str(prep["branch"]), **extra)

    return apply


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
            return _lost_writes(cfg, env, tid, lane, "its review record")  # no review record, no branch push (D-515)
        if not env.push_branch(res.branch, res.head_sha):
            return _push_failed(cfg, env, reviewed, _ours_onto(state, final, reviewed), tid, (lane, res.head_sha))
        merged = _auto_merge(reviewed, env, tid, res.head_sha)
        review = {k: st.entry(reviewed, tid)[k] for k in ("reviewed_sha", "review_lease", "branch")}
        final = st.with_task(_ours_onto(state, final, reviewed), tid, **review, auto_merge=st.entry(merged, tid)["auto_merge"])
        final = _without(final, tid, *UPDATE_KEYS)  # as the review record dropped them (D-620)
        state = reviewed
    elif st.status(final, tid) in (st.QUEUED, st.BLOCKED) and lane != st.LANE_ONE:
        final = lanes.clear_strikes(final, lane)  # a failure of the task's own work breaks a run of strikes (D-620)
    final = st.with_lease(final, lane, None)
    if env.update_state(_release_onto(state, final, (tid, lane, cfg.session))) is None:
        return _lost_writes(cfg, env, tid, lane, "its lease release")
    return str(st.entry(final, tid)["status"])


def _lost_writes(cfg: Config, env: Env, tid: str, lane: str, what: str) -> str:
    """finish could not write `what` (a lost race, or the lease is not this session's): "lost_race".
    On a lane above 1 that is a harness-type strike (D-620)."""
    record_strike(cfg, env, lane, f"{tid}: the attempt could not write {what} (lost race)")
    return "lost_race"


def record_strike(cfg: Config, env: Env, lane: str | None, why: str) -> None:
    """A harness-type failure on a lane above 1, recorded on the fresh state through update_state when it
    can be; the strike that sets `lanes_fallback` also writes the outbox item (D-620). Lane "1" never strikes."""
    if not lane or lane == st.LANE_ONE:
        return
    now = cfg.clock()

    def apply(fresh: State) -> State:
        out = lanes.strike(fresh, lane, why, now)
        _note_fallback(cfg, fresh, out)
        return out

    if env.update_state(apply) is None:
        _drop_fallback_note(cfg)  # the strike was not recorded, so neither is the fall back it would have set


def _drop_fallback_note(cfg: Config) -> None:
    """Remove a fall-back outbox item whose state write was lost (D-620): it must not reach Casey later
    with a state that never fell back."""
    (cfg.outbox_dir / "lanes-fallback.md").unlink(missing_ok=True)


def _review_record(tid: str, lane: str, cfg: Config, res: WorkerResult) -> Callable[[State], State | None]:
    """The transform for the review record (update_state, D-515): `reviewed_sha`, the lease held and the
    branch go on the task only while, on the fresh state, this session holds the task's lane lease (live)
    and the task is IN_PROGRESS; otherwise nothing is written, so auto-merge can never accept a head
    reviewed under a lease another session has since taken."""

    def apply(fresh: State) -> State | None:
        held = st.live_lease(fresh, cfg.clock(), lane) or {}
        if held.get("task") != tid or held.get("session") != cfg.session or st.status(fresh, tid) != st.IN_PROGRESS:
            return None
        review = {"reviewed_sha": res.head_sha, "review_lease": copy.deepcopy(held), "branch": res.branch, "head_check_since": None}
        # A new review starts a new chain of update merges: an earlier attempt's never counts as reviewed (D-620).
        return _without(st.with_task(fresh, tid, **review), tid, *UPDATE_KEYS)

    return apply


UPDATE_KEYS = ("update_merges", "update_waiting_since", "update_push_failures")  # an attempt's update-merge record (D-620)


def _without(state: State, tid: str, *keys: str) -> State:
    """The state with those keys removed from the task's entry (absent keys are fine)."""
    out = copy.deepcopy(state)
    for task in out["tasks"]:
        if task["id"] == tid:
            for key in keys:
                task.pop(key, None)
    return out


def _release_onto(base: State, final: State, holder: tuple[str, str, str]) -> Callable[[State], State | None]:
    """The transform for finish's last write, the lease release (update_state, D-515).

    On an unchanged state it is `final` itself. When another writer moved the state meanwhile (an
    inbox record, a graph sync), it applies only what finish changed (the keys and task entries that
    differ from `base`) onto the fresh state, and only while this session still holds the task's
    lane lease and the task's status is still the one finish started from; otherwise it gives up.
    What it applies (see _ours_onto): this task's own entry whole, and only this attempt's part of the
    shared keys (its `usage` delta, its lane's strikes), so another lane's write is never undone."""
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
    """`fresh` with what `ours` changed from `base` laid onto it.

    A changed task entry replaces the fresh entry whole, and a changed top-level key replaces the fresh
    value, except the two keys both lanes write (D-620): `usage` takes only this attempt's delta
    (`_usage_onto`) and `lane_strikes` only the lanes `ours` changed, so a concurrent write by the
    other lane's attempt is kept. The lease keys are never taken from `ours` (the caller sets the lane)."""
    out = copy.deepcopy(fresh)
    merged = {"usage": _usage_onto, "lane_strikes": _per_key_onto}
    for key in (set(base) | set(ours)) - {"tasks", "lease", "leases"}:
        if key in ours and ours[key] != base.get(key):
            join = merged.get(key)
            out[key] = join(base.get(key) or {}, ours[key] or {}, fresh.get(key) or {}) if join else copy.deepcopy(ours[key])
        elif key not in ours and key in base:
            out.pop(key, None)
    before = {t["id"]: t for t in base["tasks"]}
    for task in ours["tasks"]:
        if before.get(task["id"]) != task:
            out = _put_task(out, task)
    return out


def _per_key_onto(base: dict[str, Any], ours: dict[str, Any], fresh: dict[str, Any]) -> dict[str, Any]:
    """`fresh` with each entry `ours` changed from `base` taken from `ours`, and the rest left as fresh has them."""
    out = copy.deepcopy(fresh)
    for key, value in ours.items():
        if value != base.get(key):
            out[key] = copy.deepcopy(value)
    return out


def _usage_onto(base: dict[str, Any], ours: dict[str, Any], fresh: dict[str, Any]) -> dict[str, Any]:
    """This attempt's usage laid onto the fresh `usage` (D-620): the `by_task` entries it changed, the
    other keys it changed (`last_reading`, `limit_hit`, `source`), and its tokens added to each pacing
    window (`_windows_onto`) instead of replacing what another lane's attempt added meanwhile."""
    out = _per_key_onto({k: v for k, v in base.items() if k not in ("by_task", "windows")},
                        {k: v for k, v in ours.items() if k not in ("by_task", "windows")}, fresh)
    out["by_task"] = _per_key_onto(base.get("by_task") or {}, ours.get("by_task") or {}, fresh.get("by_task") or {})
    if "windows" in ours and ours["windows"] != base.get("windows"):
        out["windows"] = _windows_onto(base.get("windows") or {}, ours["windows"], fresh.get("windows") or {})
    if not out["by_task"] and "by_task" not in fresh and "by_task" not in ours:
        out.pop("by_task")
    return out


def _windows_onto(base: dict[str, Any], ours: dict[str, Any], fresh: dict[str, Any]) -> dict[str, Any]:
    """Pacing windows ({name: {start, tokens}}) with this attempt's tokens added to the fresh ones. A
    window nobody else moved is taken from `ours`. Otherwise the result is the newer of the two windows
    (by start; a window not started is the oldest), holding the fresh tokens only when the fresh window
    is that window, plus this attempt's delta (all its tokens when its window rolled over) only when
    its window is that window: tokens of a window that has rolled over no longer count (D-630)."""
    out = copy.deepcopy(fresh)
    for name, mine in ours.items():
        before, now = base.get(name) or {}, fresh.get(name) or {}
        if mine == before:
            continue
        if now == before:
            out[name] = copy.deepcopy(mine)
            continue
        start = max(str(mine.get("start") or ""), str(now.get("start") or "")) or None
        same = mine.get("start") == before.get("start")
        delta = int(mine.get("tokens") or 0) - (int(before.get("tokens") or 0) if same else 0)
        tokens = (int(now.get("tokens") or 0) if now.get("start") == start else 0) + (delta if mine.get("start") == start else 0)
        out[name] = {"start": start, "tokens": tokens}
    return out


def _put_task(state: State, task: dict[str, Any]) -> State:
    """`state` with the task's entry replaced whole (appended when the state lacks it)."""
    tasks = [copy.deepcopy(task) if t["id"] == task["id"] else t for t in state["tasks"]]
    if not any(t["id"] == task["id"] for t in state["tasks"]):
        tasks.append(copy.deepcopy(task))
    return dict(state, tasks=tasks)


def _push_failed(cfg: Config, env: Env, base: State, ours: State, tid: str, where: tuple[str, str]) -> str:
    """A rejected branch push, with the lease released at once either way.

    `base` is the state with the review record (what is pushed onto), `ours` the same with this
    attempt's history entry and usage (finish's result laid onto it), so the attempt is on record
    whatever happens; `where` is (lane, head). Rejected as not a fast-forward (origin's task branch
    has commits the attempt did not build on): a failed attempt on the normal retry ladder, since
    the next attempt starts from origin's branch (prepare_branch, D-583). Any other rejection is a
    credential or permission problem, not the worker's: retrying would spend a whole attempt to fail
    the same way, so the task is BLOCKED (only its dependents wait) until a person fixes the cause and
    re-queues it with `--requeue` (D-513). With no head at all the push was refused before it ran,
    because git could not read the branch or main; that gets its own reason (D-551). On a lane above 1
    either is a harness-type strike toward the fall back to one lane (D-620)."""
    lane, head = where
    why = env.push_rejection()
    ours = lanes.strike(ours, lane, f"{tid}: branch push rejected ({why[:200] or 'no reason given'})", cfg.clock())
    _note_fallback(cfg, base, ours)
    if head and ("non-fast-forward" in why or "fetch first" in why):
        out = record_failure(ours, tid, PUSH_NOT_FAST_FORWARD)
    else:
        reason = f"branch push rejected ({why or 'no reason given'}); fix the push credential or permission, then {REQUEUE_HINT}"
        if not head:
            reason = f"no head to push: git could not read the task branch or main; fix that, then {REQUEUE_HINT}"
        out = st.with_task(ours, tid, status=st.BLOCKED, reason=reason[:600])
    history = [dict(h) for h in st.entry(out, tid).get("history", [])]
    if history:
        history[-1]["result"] = st.status(out, tid)  # the attempt ended here, not MERGE_PENDING
    out = st.with_lease(st.with_task(out, tid, history=history), lane, None)
    if env.push_state(out, base):
        return "push_failed"
    _drop_fallback_note(cfg)
    return "lost_race"


def requeue_refusal(state: State, tid: str, now: datetime, cfg: Config | None = None) -> str:
    """Why `python3 -m runner --requeue` must not move this task, '' when it may (D-583).

    With one lane (or no `cfg`) any live lease refuses it, as before. With two or more lanes only a
    live lease that matters refuses it (D-620): one on the task itself, or one whose task could not run
    beside it (`lanes.refusal` minus the dependency rule: overlapping owned paths, another stage, a
    cleanup task on either side), since re-queuing then could let the two meet."""
    if tid == st.FIX_MAIN:
        return "FIX-MAIN is never re-queued by hand; it clears only with main (docs/governance.md section 5)"
    if not any(t["id"] == tid for t in state.get("tasks", [])):
        return f"no task {tid} in the state"
    if st.status(state, tid) != st.BLOCKED:
        return f"{tid} is {st.status(state, tid)}, not BLOCKED"
    entry = st.entry(state, tid)
    if int(entry.get("attempts", 0)) >= MAX_ATTEMPTS:
        return (f"{tid} has used all {MAX_ATTEMPTS} attempts; it stays BLOCKED for the end-of-build list (BUILD_BRIEF.md:540) "
                "unless a logged plan fix changed its task entry or prompt: then --plan-fix D-### gives one more (D-677)")
    if not str(entry.get("reason") or "").startswith(REQUEUABLE):
        return f"{tid} is BLOCKED for its own work ({str(entry.get('reason'))[:120]}), not for a push, permission or proof"
    held = _requeue_blockers(state, tid, now, cfg)
    if held:
        return f"a live lease is held ({', '.join(held)}); try again once it ends"
    return ""


def _requeue_blockers(state: State, tid: str, now: datetime, cfg: Config | None) -> list[str]:
    """The tasks whose live lease refuses a re-queue of `tid` (see requeue_refusal)."""
    leased = [str(lease.get("task")) for lease in st.live_leases(state, now).values()]
    if cfg is None or cfg.lanes < 2:
        return leased
    target = dict(_task_def(cfg, state, tid), deps=[])
    return [other for other in leased if other == tid or lanes.refusal(target, [_task_def(cfg, state, other)], set(), cfg.locked)]


def requeue(state: State, tid: str, reason: str, now: datetime, by: str, cfg: Config | None = None) -> State | None:
    """BLOCKED to QUEUED with `reason` cleared (D-583); None when refused (requeue_refusal).

    Only a task blocked for a cause outside its own work (a rejected push, no head, a permission
    block, a missing stage proof) with attempts left, and only while no lease that matters is live
    (any lease with one lane; with two, see requeue_refusal, D-620). `attempts` is
    left as it is, so the 3-attempt cap holds (BUILD_BRIEF.md:271, :540); the given reason is appended
    to `last_failure` as "re-queued: ...", so the retry sees both the earlier failure and why it was
    re-queued (D-584). The re-queue is kept in the task's `requeues` list (when, why, by whom, the old
    reason); `history` holds attempts only."""
    if requeue_refusal(state, tid, now, cfg):
        return None
    entry = st.entry(state, tid)
    note = {"at": st.iso(now), "reason": reason, "by": by, "was": entry.get("reason"), "attempts": int(entry.get("attempts", 0))}
    earlier = str(entry.get("last_failure") or "")
    last = f"{earlier}\nre-queued: {reason}" if earlier else f"re-queued: {reason}"
    return st.with_task(state, tid, status=st.QUEUED, reason=None, last_failure=last,
                        requeues=[*entry.get("requeues", []), note])


def plan_since(entry: dict[str, Any]) -> str | None:
    """The moment a capped task's plan is measured from (D-677): the start of its last attempt (the
    `started_at` of its last `history` entry), or that attempt's `ended_at` when no start was recorded.
    The start, not the end, so a plan fix merged while the final attempt ran still counts. None when the
    state has no such record."""
    history = entry.get("history") or []
    if not history:
        return None
    last = history[-1]
    return str(last.get("started_at") or last.get("ended_at") or "") or None


def plan_fix_precheck(state: State, tid: str, decision: str, now: datetime, cfg: Config | None = None) -> str:
    """The state-only half of plan_fix_refusal (D-677): '' when main is worth reading."""
    if tid == st.FIX_MAIN:
        return "FIX-MAIN is never re-queued by hand; it clears only with main (docs/governance.md section 5)"
    if not any(t["id"] == tid for t in state.get("tasks", [])):
        return f"no task {tid} in the state"
    if st.status(state, tid) != st.BLOCKED:
        return f"{tid} is {st.status(state, tid)}, not BLOCKED"
    if not PLAN_FIX_ID.match(decision):
        return f"--plan-fix needs a decision number like D-677, not {decision[:40]!r}"
    entry = st.entry(state, tid)
    used = int(entry.get("attempts", 0))
    if used < MAX_ATTEMPTS:
        return f"{tid} has attempts left ({used} of {MAX_ATTEMPTS} used); --plan-fix is only for a task that used them all"
    if any(r.get("plan_fix") == decision for r in entry.get("requeues") or []):
        return f"{tid} was already re-queued once for {decision}; a plan fix gives exactly one more attempt"
    if not plan_since(entry):
        return f"no record of when {tid}'s last attempt started or ended"
    held = _requeue_blockers(state, tid, now, cfg)
    return f"a live lease is held ({', '.join(held)}); try again once it ends" if held else ""


def plan_fix_refusal(
    state: State, tid: str, decision: str, evidence: dict[str, Any] | None, now: datetime, cfg: Config | None = None
) -> str:
    """Why `python3 -m runner --requeue T### --plan-fix D-###` must not move this task, '' when it may (D-677).

    BUILD_BRIEF.md:271 and :540 keep a task that used all its attempts BLOCKED for the end-of-build
    list, with one exception: a logged plan fix gives it exactly one more attempt. That needs the task
    BLOCKED with every attempt used, no earlier plan-fix re-queue for this decision, no lease that
    matters live (as for `--requeue`), and, on main (`evidence`, GitHubEnv.plan_evidence read for
    `plan_since`, the start of the task's last attempt), the checks of _plan_fix_main_refusal."""
    first = plan_fix_precheck(state, tid, decision, now, cfg)
    if first:
        return first
    if evidence is None:
        return f"git could not read main at the start of {tid}'s last attempt; nothing was re-queued"
    since = plan_since(st.entry(state, tid))
    if evidence.get("since") != since:
        return f"main was read for {evidence.get('since')}, but {tid}'s last attempt started at {since}; try again"
    return _plan_fix_main_refusal(tid, decision, evidence)


def _plan_fix_main_refusal(tid: str, decision: str, evidence: dict[str, Any]) -> str:
    """D-677 on main, then (main's commit at `plan_since`) against now: the decision is in docs/decisions.md
    and in the task's own `decisions` list in kit/tasks.json now and in neither then, and the task's plan
    (its kit/tasks.json entry and prompt file, verify.plan_fingerprint) changed. The list check keeps an
    unrelated decision, after a regeneration of every prompt for example, from freeing a capped task."""
    since, base, number = evidence.get("since"), str(evidence.get("base"))[:12], int(decision[2:])
    if number not in evidence.get("decisions_now", []):
        return f"{decision} is not in docs/decisions.md on main"
    if number in evidence.get("decisions_then", []):
        return f"{decision} was already on main when {tid}'s last attempt started (main {base}); name the plan fix's own decision"
    if decision not in evidence.get("task_decisions_now", []):
        return f"{decision} is not in {tid}'s decisions list in kit/tasks.json on main; the plan fix must list it there"
    if decision in evidence.get("task_decisions_then", []):
        return f"{decision} was already in {tid}'s decisions list when its last attempt started (main {base})"
    then, later = evidence.get("plan_then"), evidence.get("plan_now")
    if not then or not later:
        return f"kit/tasks.json could not be read on main (then {base} or now); nothing was re-queued"
    if then == later:
        return (f"{tid}'s kit/tasks.json entry and prompt file are unchanged on main since its last attempt started at "
                f"{since} (main {base}); a plan fix must change one of them")
    return ""


def requeue_plan_fix(
    state: State, tid: str, reason: str, decision: str, evidence: dict[str, Any] | None, now: datetime, by: str,
    cfg: Config | None = None,
) -> State | None:
    """BLOCKED to QUEUED for exactly one more attempt after a plan fix (D-677); None when refused
    (plan_fix_refusal). `attempts` stays as it is, so attempt_model keeps the attempt on Opus and its
    failure blocks the task again (record_failure); the re-queue is kept in `requeues` with the decision,
    the moment measured from (`plan_since`), main's commit then and now, and both plan fingerprints, so
    anyone can check it."""
    if evidence is None or plan_fix_refusal(state, tid, decision, evidence, now, cfg):
        return None
    entry = st.entry(state, tid)
    note = {"at": st.iso(now), "reason": reason, "by": by, "was": entry.get("reason"), "attempts": int(entry.get("attempts", 0)),
            "plan_fix": decision, "plan_since": evidence["since"], "plan_base": evidence["base"], "plan_head": evidence["head"],
            "plan_then": evidence["plan_then"], "plan_now": evidence["plan_now"]}
    earlier = str(entry.get("last_failure") or "")
    line = f"re-queued after plan fix {decision}: {reason}"
    return st.with_task(state, tid, status=st.QUEUED, reason=None, last_failure=f"{earlier}\n{line}" if earlier else line,
                        requeues=[*entry.get("requeues", []), note])


def session_lease(state: State, session: str, now: datetime) -> dict[str, Any] | None:
    """The live lease this session holds, on any numbered lane or "fix", or None (D-620)."""
    return next((lease for lease in st.live_leases(state, now).values() if lease.get("session") == session), None)


def after_handoff(cfg: Config, env: Env, res_for: Callable[[dict[str, Any]], WorkerResult]) -> str:
    """`python3 -m runner --after`: finish the task this session holds the lease on, on whichever lane."""
    state = env.read_state()
    lease = session_lease(state, cfg.session, cfg.clock())
    if lease is None:
        return "not_lease_holder"
    tid = str(lease["task"])
    graph_def = next((t for t in cfg.graph if t["id"] == tid), {})
    task = {**st.entry(state, tid), **graph_def}
    if st.status(state, tid) != st.IN_PROGRESS:
        return f"{tid} is {st.status(state, tid)}"
    model = attempt_model(task, int(st.entry(state, tid).get("attempts", 0)), cfg.models)
    return finish(cfg, env, state, task, res_for(task), model, cfg.clock())

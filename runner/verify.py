"""Checks the runner makes itself, never on a subagent's word (BUILD_BRIEF.md:247-248, 261-263)."""

from __future__ import annotations

import json
import re
from collections.abc import Iterable
from typing import Any

REQUIRED_CHECKS = ("Ci", "Twin", "Ally")
NEVER_TOUCH = ("kit/state.json", "kit/nightly.json", "STATUS.md", "PROGRESS.md", "outbox/", "inbox/")


def _matches(path: str, pattern: str) -> bool:
    if pattern.endswith("/"):
        return path.startswith(pattern)
    return path == pattern


def _under(path: str, patterns: Iterable[str]) -> bool:
    return any(_matches(path, p) for p in patterns)


def check_paths(
    changed: Iterable[str],
    task: dict[str, Any],
    protected: Iterable[str],
    shared: Iterable[str],
) -> list[str]:
    """Violations in a diff: not owned, or protected without a harness task."""
    protected = tuple(protected)
    owns = tuple(task.get("owns", []))
    problems: list[str] = []
    for path in sorted(set(changed)):
        if path.startswith("/") or ".." in path.split("/"):
            problems.append(f"{path}: not a repository-relative path")
        elif _under(path, NEVER_TOUCH):
            problems.append(f"{path}: runner state is written only by the runner")
        elif _under(path, protected) and task.get("type") != "harness":
            problems.append(f"{path}: protected path, harness tasks only")
        elif not _under(path, owns) and not _under(path, shared):
            problems.append(f"{path}: not owned by {task.get('id')}")
    return problems


def models_in_transcript(lines: Iterable[str]) -> list[str]:
    """Model IDs the session records itself wrote (one JSON object per line).

    The model is read from `message.model` of assistant records, the field the
    session transcript carries. Synthetic entries (no model call) are ignored.
    """
    found: list[str] = []
    for line in lines:
        try:
            record = json.loads(line)
        except ValueError:
            continue
        message = record.get("message") if isinstance(record, dict) else None
        model = message.get("model") if isinstance(message, dict) else None
        if isinstance(model, str) and not model.startswith("<"):
            found.append(model)
    return found


def check_model(evidence: Iterable[str], expected: str, known: Iterable[str]) -> list[str]:
    """Every model seen in the evidence must be the full ID the task names."""
    seen = sorted(set(evidence))
    if expected not in set(known):
        return [f"{expected}: not a full model ID from kit/tasks.json models"]
    if not seen:
        return ["no model evidence found; a run without evidence fails"]
    return [f"model {m} ran, task names {expected}" for m in seen if m != expected]


def check_done(pr: dict[str, Any], head_tree: str, merge_tree: str) -> list[str]:
    """Why a task cannot yet be marked DONE; empty means it can.

    pr: {"merged": bool, "head_sha": str, "checks": [{"name", "sha", "conclusion"}]}
    """
    problems: list[str] = []
    if not pr.get("merged"):
        problems.append("PR is not merged")
    head = pr.get("head_sha", "")
    for name in REQUIRED_CHECKS:
        runs = [c for c in pr.get("checks", []) if c.get("name") == name and c.get("sha") == head]
        if not runs:
            problems.append(f"{name}: no run on head {head[:7] or 'unknown'}")
        elif not all(r.get("conclusion") == "success" for r in runs):
            problems.append(f"{name}: not green on head {head[:7]}")
    if not head_tree or head_tree != merge_tree:
        problems.append("squash commit tree differs from the tested head tree")
    return problems


def checks_state(pr: dict[str, Any]) -> str:
    """'green', 'failed' or 'pending' for the three required checks on the head SHA."""
    head = pr.get("head_sha", "")
    states = []
    for name in REQUIRED_CHECKS:
        runs = [c for c in pr.get("checks", []) if c.get("name") == name and c.get("sha") == head]
        if not runs:
            states.append("pending")
        elif any(r.get("conclusion") in ("failure", "cancelled", "timed_out") for r in runs):
            states.append("failed")
        elif all(r.get("conclusion") == "success" for r in runs):
            states.append("green")
        else:
            states.append("pending")
    for wanted in ("failed", "pending"):
        if wanted in states:
            return wanted
    return "green"


USAGE_FIELDS = ("input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens")


def usage_in_transcript(lines: Iterable[str]) -> dict[str, int]:
    """Token totals from `message.usage` of a session transcript, for usage.by_task (BUILD_BRIEF.md:510)."""
    totals = dict.fromkeys(USAGE_FIELDS, 0)
    for line in lines:
        try:
            record = json.loads(line)
        except ValueError:
            continue
        message = record.get("message") if isinstance(record, dict) else None
        usage = message.get("usage") if isinstance(message, dict) else None
        if isinstance(usage, dict):
            for key in USAGE_FIELDS:
                value = usage.get(key)
                totals[key] += value if isinstance(value, int) else 0
    return totals


TASK_BRANCH = re.compile(r"^claude/task/(FIX-MAIN|[A-Za-z]+\d+[A-Za-z]?)(?:-[A-Za-z0-9._-]*)?$")
LIVE_STATUSES = ("IN_PROGRESS", "MERGE_PENDING")


def task_id_from_branch(branch: str) -> str:
    """`claude/task/T012-quote-page` -> `T012`; '' for a branch that is not a task branch."""
    found = TASK_BRANCH.match(branch)
    return found.group(1) if found else ""


LEASE_KEYS = ("task", "session", "expires")


def lane_lease(lease: dict[str, Any] | None, lane: str) -> dict[str, Any] | None:
    """The lease that decides for a lane, from either format auto-pr may pass (D-514).

    `lease` is a single lease (the old `lease` of lease.json, which is lane "1") or the `leases`
    map {lane: lease | None}. A single lease is taken as it is, whatever the lane, so a caller that
    still passes the old shape decides exactly as before D-514.
    """
    if not lease:
        return None
    if any(key in lease for key in LEASE_KEYS):
        return lease
    found = lease.get(lane)
    return found if isinstance(found, dict) else None


def auto_merge_reason(state: dict[str, Any], branch: str, head_sha: str, lease: dict[str, Any] | None = None) -> str:
    """Why auto-pr must not enable auto-merge on this push; '' when it may (D-486, D-479).

    The runner writes `reviewed_sha` and `review_lease` (the lease it held) on the task in
    kit/state.json before it pushes a task branch. Auto-merge needs both to name this branch's
    task and this exact head, the task still in flight, and no other task holding the task's lane
    lease: the lane named in `review_lease` (lane "1" when it names none), read from `lease`, which
    is either a single lease or the lane map (lane_lease, D-514).
    """
    tid = task_id_from_branch(branch)
    if not tid:
        return f"{branch} is not a runner task branch"
    task = next((t for t in state.get("tasks", []) if t.get("id") == tid), None)
    if task is None:
        return f"{tid} is not in kit/state.json"
    if task.get("reviewed_sha") != head_sha:
        return f"no runner review record for {head_sha[:12] or 'this head'} (reviewed_sha {str(task.get('reviewed_sha'))[:12]})"
    review_lease = task.get("review_lease") or {}
    if review_lease.get("task") != tid:
        return f"the review record for {tid} was not written under its lease"
    if task.get("status") not in LIVE_STATUSES:
        return f"{tid} is {task.get('status')}, not in flight"
    held = lane_lease(lease, str(review_lease.get("lane") or "1"))
    if held and held.get("task") not in (None, tid):
        return f"the lease is held by {held.get('task')}"
    return ""


def auto_merge_allowed(state: dict[str, Any], branch: str, head_sha: str, lease: dict[str, Any] | None = None) -> bool:
    """True only when auto_merge_reason finds nothing (the auto-pr workflow's decision)."""
    return auto_merge_reason(state, branch, head_sha, lease) == ""

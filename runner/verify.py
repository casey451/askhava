"""Checks the runner makes itself, never on a subagent's word (BUILD_BRIEF.md:247-248, 261-263)."""

from __future__ import annotations

import json
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

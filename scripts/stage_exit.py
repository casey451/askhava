"""Stage exit test: may the build leave stage N (docs/governance.md section 6, docs/build-readiness.md section 3)?

A stage passes only when all of these hold (D-486, D-479):
  1. every task whose `stage` value in kit/tasks.json equals N is DONE in kit/state.json (a gate may
     also be SKIPPED); a BLOCKED task anywhere in the stage keeps it closed;
  2. kit/nightly.json (written by nightly-main on claude/kit-state) is green on a commit that contains
     every cleanup merge of the stage: each cleanup task's `merged_sha` is an ancestor of the nightly
     SHA (`git merge-base --is-ancestor`, D-484), or every merge when the stage has no cleanup task;
  3. the stage's named proof (its row in docs/build-readiness.md section 3) is recorded in state as
     `stage_proofs[N]` with a non-empty `evidence` string (the runner's `--record-proof`);
  4. the coverage and mutant flags Ci recorded (a results file, and `ci_flags` in state) are green
     where present.

The runner refuses to claim a task of the next stage until this passes for the previous stage
(runner.state.eligible with `passed`), so stage gating does not depend on every stage task through
the dependency chain.

Usage: python3 scripts/stage_exit.py --stage N [--state PATH] [--nightly PATH] [--results PATH] [--root DIR]
Exit status 0 when the stage passes, 1 otherwise; the reasons are printed one per line.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

Ancestor = Callable[[str, str], "bool | None"]
GREEN = ("green", "success", "pass", "passed", True)
FLAGS = ("coverage", "mutants")
ROW = re.compile(r"^\| \*\*(?P<stage>\d+)\. (?P<name>[^*]+)\*\*[^|]*\| (?P<test>.+) \|\s*$")


def git_is_ancestor(root: Path) -> Ancestor:
    """`git merge-base --is-ancestor older newer` in root; None when git cannot answer."""

    def check(older: str, newer: str) -> bool | None:
        try:
            run = subprocess.run(
                ["git", "-C", str(root), "merge-base", "--is-ancestor", older, newer],
                capture_output=True, text=True, timeout=20, check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        return {0: True, 1: False}.get(run.returncode)

    return check


def named_proofs(root: Path) -> dict[str, str]:
    """Stage number -> its exit-test text from docs/build-readiness.md section 3."""
    path = root / "docs" / "build-readiness.md"
    if not path.exists():
        return {}
    out: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        found = ROW.match(line)
        if found:
            out[found.group("stage")] = found.group("test").strip()
    return out


def stage_tasks(graph: list[dict[str, Any]], stage: str) -> list[dict[str, Any]]:
    return [t for t in graph if str(t.get("stage")) == str(stage)]


def _status_problems(tasks: list[dict[str, Any]], states: dict[str, dict[str, Any]]) -> list[str]:
    problems = []
    for task in tasks:
        status = states.get(task["id"], {}).get("status", "QUEUED")
        allowed = ("DONE", "SKIPPED") if task.get("type") == "gate" else ("DONE",)
        if status not in allowed:
            problems.append(f"{task['id']} is {status}, not DONE")
    return problems


def _merges_to_check(tasks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    cleanup = [t for t in tasks if t.get("type") == "cleanup"]
    return cleanup or [t for t in tasks if t.get("type") != "gate"]


def _nightly_problems(
    tasks: list[dict[str, Any]], states: dict[str, dict[str, Any]], nightly: dict[str, Any] | None, is_ancestor: Ancestor
) -> list[str]:
    if not nightly or nightly.get("conclusion") != "success" or not nightly.get("sha"):
        return ["kit/nightly.json is not a green run on main"]
    tested = str(nightly["sha"])
    problems = []
    for task in _merges_to_check(tasks):
        merge = str(states.get(task["id"], {}).get("merged_sha") or "")
        if not merge:
            problems.append(f"{task['id']}: no merged_sha recorded")
        elif is_ancestor(merge, tested) is not True:
            problems.append(f"{task['id']}: merge {merge[:12]} is not in the nightly run's commit {tested[:12]}")
    return problems


def _flag_problems(results: dict[str, Any], state: dict[str, Any]) -> list[str]:
    flags = {**(state.get("ci_flags") or {}), **results}
    return [f"Ci flag {name} is {flags[name]!r}, not green" for name in FLAGS if name in flags and flags[name] not in GREEN]


def evaluate(
    stage: str,
    graph: list[dict[str, Any]],
    state: dict[str, Any],
    nightly: dict[str, Any] | None,
    results: dict[str, Any],
    is_ancestor: Ancestor,
) -> list[str]:
    """Why stage `stage` cannot be left yet; an empty list means it passes."""
    tasks = stage_tasks(graph, stage)
    if not tasks:
        return [f"no task has stage {stage}"]
    states = {t["id"]: t for t in state.get("tasks", [])}
    problems = _status_problems(tasks, states)
    if problems:
        return problems  # git is asked only once every task is done
    problems += _nightly_problems(tasks, states, nightly, is_ancestor)
    proof = (state.get("stage_proofs") or {}).get(str(stage)) or {}
    if not str(proof.get("evidence", "")).strip():
        problems.append(f"stage {stage}'s named proof is not recorded in state (stage_proofs[{stage}].evidence)")
    return problems + _flag_problems(results, state)


def passed_stages(
    graph: list[dict[str, Any]],
    state: dict[str, Any],
    nightly: dict[str, Any] | None,
    results: dict[str, Any],
    is_ancestor: Ancestor,
) -> set[str]:
    """Every numeric stage whose exit test passes now (what runner.state.eligible takes as `passed`)."""
    stages = {str(t["stage"]) for t in graph if str(t.get("stage", "")).isdigit()}
    return {s for s in stages if not evaluate(s, graph, state, nightly, results, is_ancestor)}


def _load(path: Path | None) -> Any:
    if path is None or not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--stage", required=True)
    ap.add_argument("--state", type=Path, default=Path("kit/state.json"))
    ap.add_argument("--nightly", type=Path)
    ap.add_argument("--results", type=Path)
    ap.add_argument("--root", type=Path, default=Path("."))
    args = ap.parse_args(argv)
    graph = json.loads((args.root / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]
    state = _load(args.state) or {"tasks": []}
    nightly = _load(args.nightly or args.state.parent / "nightly.json")
    results = _load(args.results) or {}
    problems = evaluate(str(args.stage), graph, state, nightly, results, git_is_ancestor(args.root))
    proof = named_proofs(args.root).get(str(args.stage), "")
    print(f"stage {args.stage}: {'PASS' if not problems else 'FAIL'}" + (f" (exit test: {proof})" if proof else ""))
    for line in problems:
        print(f"  - {line}")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())

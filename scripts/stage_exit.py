"""Stage exit test: may the build leave stage N (docs/governance.md section 6, docs/build-readiness.md section 3)?

A stage passes only when all of these hold (D-486, D-479):
  1. every task whose `stage` value in kit/tasks.json equals N is DONE in kit/state.json (a gate may
     also be SKIPPED); a BLOCKED task anywhere in the stage keeps it closed;
  2. kit/nightly.json (written by nightly-main on claude/kit-state) is green on a commit that contains
     every cleanup merge of the stage: each cleanup task's `merged_sha` is an ancestor of the nightly
     SHA (`git merge-base --is-ancestor`, D-484), or every merge when the stage has no cleanup task;
     a cleanup task that handed in nothing records as `merged_sha` the main commit it certified, so
     for a stage the runner proves itself (runner/proof.py MACHINE_PROOFS: 1, 2 and 4) "the nightly
     contains all the stage's merges" rests on runner/proof.py, which records that stage's proof only
     from a nightly commit containing every merge of the stage (D-551, D-1473);
  3. the stage's named proof (its row in docs/build-readiness.md section 3) is recorded in state as
     `stage_proofs[N]` with a non-empty `evidence` string (the runner's `--record-proof`);
  4. the coverage and mutant flags Ci recorded (a results file, and `ci_flags` in state) are green
     where present;
  5. the integration-closure register (docs/integration-closure.md, D-1760, TD-OPS-31, SEC-022) leaves
     no `security`, `money` or `privacy` handoff without a live owner. A row of those areas that is not
     `closed` is judged at stage N only when every owner it names has a numeric `stage` at or below N
     in kit/tasks.json (otherwise it is printed: the runner re-runs every earlier stage's exit on each
     claim, runner/state.py `_stage_open`, so a later owner's merge must not close an earlier stage);
     a judged row fails when every owner is DONE or SKIPPED. A row with no owner, or naming a task id
     missing from kit/tasks.json, fails at every stage; a `closed` row of those areas fails unless its
     evidence names a merged commit SHA or PR number and a test. Rows of other areas are printed and
     never fail. A missing register fails. `evaluate` judges the register only when given its path
     (`register=`); the command line always passes `<root>/docs/integration-closure.md`.

The runner refuses to claim a task of the next stage until this passes for the previous stage
(runner.state.eligible with `passed`), so stage gating does not depend on every stage task through
the dependency chain.

Usage: python3 scripts/stage_exit.py --stage N [--state PATH] [--nightly PATH] [--results PATH] [--root DIR]
Exit status 0 when the stage passes, 1 otherwise; the reasons are printed one per line, then the
register rows that were printed and not judged, one per line as `note:`.
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
GATING_AREAS = ("security", "money", "privacy")
FINISHED = ("DONE", "SKIPPED")
COMMIT_OR_PR = re.compile(r"(?<![0-9A-Za-z])(?:[0-9a-f]{7,40}|#\d+)(?![0-9A-Za-z])")
TEST_NAME = re.compile(r"\btest_\w+|\bTD-[A-Z0-9]+-\d+\b")
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


def read_register(path: Path) -> list[dict[str, str]] | None:
    """The rows of the register's table as column name -> cell text; None when the file is missing."""
    if not path.exists():
        return None
    header: list[str] = []
    rows: list[dict[str, str]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith("|"):
            if rows:
                break  # the register's one table has ended
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if not header:
            header = cells
        elif not set("".join(cells)) <= set("-: "):
            rows.append(dict(zip(header, cells)))
    return rows


def _owners(row: dict[str, str]) -> list[str]:
    cells = [o.strip() for o in row.get("Owner", "").split(",")]
    return [o for o in cells if o and o.lower() not in ("none", "-")]


def _numeric(stage: Any) -> float | None:
    try:
        return float(str(stage))
    except ValueError:
        return None


def _closed_problem(row: dict[str, str]) -> str | None:
    evidence = row.get("Evidence", "")
    if COMMIT_OR_PR.search(evidence) and TEST_NAME.search(evidence):
        return None
    return f"{row.get('ID', '?')} ({row.get('Area')}): closed without evidence (a merged commit SHA or PR number and a test)"


def _open_row_verdict(
    stage: str, row: dict[str, str], stages: dict[str, Any], states: dict[str, dict[str, Any]]
) -> tuple[str | None, str | None]:
    """(problem, note) for a gating row that is not closed (D-1760, TD-OPS-31)."""
    label = f"{row.get('ID', '?')} ({row.get('Area')})"
    owners = _owners(row)
    if not owners:
        return f"{label}: no owner task", None
    unknown = [o for o in owners if o not in stages]
    if unknown:
        return f"{label}: owner {', '.join(unknown)} is not a task in kit/tasks.json", None
    here = _numeric(stage)
    later = [o for o in owners if here is None or (n := _numeric(stages[o])) is None or n > here]
    if later:
        return None, f"{label} is {row.get('Status')}; judged at the stage of {', '.join(later)}"
    if all(states.get(o, {}).get("status") in FINISHED for o in owners):
        return f"{label}: every owner ({', '.join(owners)}) is DONE or SKIPPED; name a live owner or close it", None
    return None, None


def closure_review(
    stage: str, graph: list[dict[str, Any]], state: dict[str, Any], rows: list[dict[str, str]]
) -> tuple[list[str], list[str]]:
    """(problems, notes) for the integration-closure register at stage `stage` (D-1760, SEC-022)."""
    stages = {t["id"]: t.get("stage") for t in graph}
    states = {t["id"]: t for t in state.get("tasks", [])}
    problems: list[str] = []
    notes: list[str] = []
    for row in rows:
        closed = row.get("Status", "").lower() == "closed"
        if row.get("Area", "").lower() not in GATING_AREAS:
            if not closed:
                notes.append(f"{row.get('ID', '?')} ({row.get('Area')}) is {row.get('Status')}; not gating")
            continue
        problem, note = (_closed_problem(row), None) if closed else _open_row_verdict(stage, row, stages, states)
        problems += [problem] if problem else []
        notes += [note] if note else []
    return problems, notes


def _register_problems(stage: str, graph: list[dict[str, Any]], state: dict[str, Any], register: Path) -> list[str]:
    rows = read_register(register)
    if rows is None:
        return [f"the integration-closure register {register} is missing"]
    return closure_review(stage, graph, state, rows)[0]


def evaluate(
    stage: str,
    graph: list[dict[str, Any]],
    state: dict[str, Any],
    nightly: dict[str, Any] | None,
    results: dict[str, Any],
    is_ancestor: Ancestor,
    register: Path | None = None,
) -> list[str]:
    """Why stage `stage` cannot be left yet; an empty list means it passes. The register (condition 5)
    is judged only when its path is given."""
    tasks = stage_tasks(graph, stage)
    if not tasks:
        return [f"no task has stage {stage}"]
    closure = _register_problems(stage, graph, state, register) if register is not None else []
    states = {t["id"]: t for t in state.get("tasks", [])}
    problems = _status_problems(tasks, states)
    if problems:
        return problems + closure  # git is asked only once every task is done
    problems += _nightly_problems(tasks, states, nightly, is_ancestor)
    proof = (state.get("stage_proofs") or {}).get(str(stage)) or {}
    if not str(proof.get("evidence", "")).strip():
        problems.append(f"stage {stage}'s named proof is not recorded in state (stage_proofs[{stage}].evidence)")
    return problems + _flag_problems(results, state) + closure


def passed_stages(
    graph: list[dict[str, Any]],
    state: dict[str, Any],
    nightly: dict[str, Any] | None,
    results: dict[str, Any],
    is_ancestor: Ancestor,
    register: Path | None = None,
) -> set[str]:
    """Every numeric stage whose exit test passes now (what runner.state.eligible takes as `passed`)."""
    stages = {str(t["stage"]) for t in graph if str(t.get("stage", "")).isdigit()}
    return {s for s in stages if not evaluate(s, graph, state, nightly, results, is_ancestor, register)}


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
    register = args.root / "docs" / "integration-closure.md"
    problems = evaluate(str(args.stage), graph, state, nightly, results, git_is_ancestor(args.root), register)
    proof = named_proofs(args.root).get(str(args.stage), "")
    print(f"stage {args.stage}: {'PASS' if not problems else 'FAIL'}" + (f" (exit test: {proof})" if proof else ""))
    for line in problems:
        print(f"  - {line}")
    for line in closure_review(str(args.stage), graph, state, read_register(register) or [])[1]:
        print(f"  note: {line}")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())

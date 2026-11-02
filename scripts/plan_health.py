"""Plan-health check: is a task's plan sound enough to claim it? (A+ plan item 3, D-1350)

A worker that starts a task whose plan has a hole spends its attempt finding the hole: a file
its tests need that no task writes, a dependency whose files never reached `main`, a context
budget that leaves no room to work. This script finds those holes before the claim, from the
plan alone, so the runner (runner/cycle.py) can hold the task as a visible `plan_gap` with no
attempt used and claim the next task instead. Three checks, each reusing the plan's own rules:

(a) **Context.** The prompt on disk plus the files the task loads, measured exactly as
    scripts/gen_prompts.py measures them (`context_bytes`, `percent`), against the task's limit
    (scripts/validate_tasks.py `context_limit`, D-1036).
(b) **Cited paths.** Every repository path in backticks in the requirement sentences and the
    test-design rows the task names (what its prompt pastes, read with gen_prompts'
    `load_sources`) and every path it lists under `reads` is owned by the task, by one of its
    ancestors in the dependency graph, or by the shared list, or already exists on disk. The
    ownership test is scripts/check_pr_change.py `task_paths_check` (runner/verify.py
    `check_paths`), the same rule a hand-in is held to (D-1058). A path that a decision the
    prompt pastes (superseded ones included, since the prompt shows them as history) names is
    held to a weaker rule: some task in the graph owns it, or it exists. A decision often names
    a later task's file (the deploy notes, the README) as background; that is no gap for this
    task, while a file no task will ever write is.
(c) **Dependencies.** Every dependency is DONE or SKIPPED in kit/state.json, and every regular
    file a DONE dependency owns is on disk (the runner's checkout of `main`).

Each problem is one plain string. Exit 0 when clean, 1 when there are gaps, 2 when the inputs
cannot be read.

Usage:
  python3 scripts/plan_health.py T148 [--json] [--state PATH]
  python3 scripts/plan_health.py --all-eligible [--json] [--state PATH]   tasks claimable now
  python3 scripts/plan_health.py --all [--json] [--state PATH]            every task not DONE or SKIPPED (a
                                         dependency not yet DONE is no gap here), and every DONE task's owned files

Without --state the state is read from `origin/claude/kit-state` (`git show`), read only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Callable, Iterable
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import check_pr_change  # sibling scripts, imported after their folder is on the path
import gen_prompts
import validate_tasks

from runner import state as st

ROOT = Path(__file__).resolve().parent.parent
STATE_REF = "origin/claude/kit-state"
CITED_PATH = re.compile(
    r"`((?:app|tests|migrations|scripts|docs|kit|runner|twin|stand_ins|\.github)/[A-Za-z0-9_./-]+\.[A-Za-z0-9]+)`"
)
# Written only by the runner on claude/kit-state (runner/verify.py NEVER_TOUCH), never by a task.
RUNNER_WRITTEN = ("kit/state.json", "kit/nightly.json", "STATUS.md", "PROGRESS.md")
DONE_STATES = (st.DONE, st.SKIPPED)
Checker = Callable[[st.State, str], list[str]]


def under(path: str, prefixes: Iterable[str]) -> bool:
    """A path equals one of the prefixes, or sits under one that is a folder (ends with "/")."""
    return any(path == p or (p.endswith("/") and path.startswith(p)) for p in prefixes)


def ancestors(graph: dict[str, dict[str, Any]], task_id: str) -> set[str]:
    """Every task the given one depends on, directly or through other dependencies."""
    seen: set[str] = set()
    todo = list(graph.get(task_id, {}).get("deps", []))
    while todo:
        dep = todo.pop()
        if dep not in seen:
            seen.add(dep)
            todo.extend(graph.get(dep, {}).get("deps", []))
    return seen


def cited(texts: Iterable[str]) -> list[str]:
    """Repository paths quoted in backticks in the given texts, sorted, without the runner's own files."""
    return sorted({p for text in texts for p in CITED_PATH.findall(text) if p not in RUNNER_WRITTEN})


def context_problems(root: Path, task: dict[str, Any]) -> list[str]:
    """(a) The measured context against the task's limit (D-1036, D-1350)."""
    prompt_file = root / str(task.get("prompt") or "")
    if not task.get("prompt") or not prompt_file.is_file():
        return [f"{task['id']}: prompt {task.get('prompt')} is missing; run scripts/gen_prompts.py"]
    prompt = prompt_file.read_text(encoding="utf-8")
    pct = gen_prompts.percent(gen_prompts.context_bytes(root, task, prompt))
    limit = validate_tasks.context_limit(task)
    if pct > limit:
        return [f"{task['id']}: measures {pct}% of the context window, over its {limit}% limit"]
    return []


def unowned(root: Path, task_id: str, paths: list[str]) -> list[str]:
    """The paths check_pr_change.task_paths_check would refuse on this task's branch, as bare paths."""
    found = check_pr_change.task_paths_check(root, f"claude/task/{task_id}-plan-health", paths)
    return [p.removeprefix("task paths: ").split(": ", 1)[0] for p in found]


def path_problems(root: Path, doc: dict[str, Any], src: gen_prompts.Sources, task: dict[str, Any]) -> list[str]:
    """(b) Paths the requirements, the test rows and the reads name, then those the decisions name."""
    graph = {t["id"]: t for t in doc["tasks"]}
    tid = task["id"]
    texts = [src.requirements.get(r, ("", ""))[0] for r in task.get("requirements", [])]
    texts += [src.tests.get(t, "") for t in task.get("tests", [])]
    named = sorted(set(cited(texts)) | {r for r in task.get("reads", []) if not r.endswith("/")})
    upstream = [o for a in ancestors(graph, tid) for o in graph.get(a, {}).get("owns", [])]
    problems = [
        f"{tid}: {p} is named by its requirements, tests or reads, but neither it nor a dependency owns it "
        "and it does not exist yet"
        for p in unowned(root, tid, named)
        if not under(p, upstream) and not (root / p).exists()
    ]
    return problems + decision_problems(root, doc, src, task)


def decision_problems(root: Path, doc: dict[str, Any], src: gen_prompts.Sources, task: dict[str, Any]) -> list[str]:
    """(b) A path a pasted decision names that no task in the graph owns and that does not exist."""
    everyone = [o for t in doc["tasks"] for o in t.get("owns", [])] + list(doc.get("shared_paths", []))
    out: list[str] = []
    for did in task.get("decisions", []):
        by = src.superseded.get(did)
        note = f"{did}, superseded by {by} (pasted as history)" if by else did
        for path in cited([src.decisions.get(did, "")]):
            if not under(path, everyone) and not (root / path).exists():
                out.append(f"{task['id']}: {path} is named by {note}, but no task owns it and it does not exist")
    return out


def dep_problems(root: Path, doc: dict[str, Any], state: st.State, task: dict[str, Any], pending_ok: bool) -> list[str]:
    """(c) Each dependency DONE or SKIPPED (unless `pending_ok`: a survey of tasks not yet eligible), and
    each regular file a DONE dependency owns on disk."""
    graph = {t["id"]: t for t in doc["tasks"]}
    out: list[str] = []
    for dep in task.get("deps", []):
        status = st.status(state, dep)
        if status not in DONE_STATES:
            out += [] if pending_ok else [f"{task['id']}: dependency {dep} is {status}, not DONE"]
            continue
        if status == st.DONE:
            missing = [o for o in graph.get(dep, {}).get("owns", []) if not o.endswith("/") and not (root / o).exists()]
            out += [f"{task['id']}: dependency {dep} is DONE but its owned file {m} is not on main" for m in missing]
    return out


def check_task(
    root: Path, doc: dict[str, Any], src: gen_prompts.Sources, state: st.State, task_id: str, pending_ok: bool = False
) -> list[str]:
    """Every plan-health problem of one task, in the order (a), (b), (c); [] when it may be claimed."""
    task = next((t for t in doc["tasks"] if t["id"] == task_id), None)
    if task is None:
        return [f"{task_id}: not in kit/tasks.json"]
    if task.get("type") == "gate":
        return []
    deps = dep_problems(root, doc, state, task, pending_ok)
    return context_problems(root, task) + path_problems(root, doc, src, task) + deps


def checker(root: Path, doc: dict[str, Any], pending_ok: bool = False) -> Checker:
    """The runner's hook (runner/cycle.py Config.plan_health): sources read once, the state passed per call."""
    src = gen_prompts.load_sources(root)
    return lambda state, task_id: check_task(root, doc, src, state, task_id, pending_ok)


def read_state(root: Path, path: Path | None) -> st.State:
    """kit/state.json from a file, or from claude/kit-state read only with `git show`."""
    if path is not None:
        return json.loads(path.read_text(encoding="utf-8"))
    out = subprocess.run(["git", "-C", str(root), "show", f"{STATE_REF}:kit/state.json"],
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def chosen(args: argparse.Namespace, doc: dict[str, Any], state: st.State) -> list[str]:
    """The task IDs the command line asks about, in graph order for --all and state order for --all-eligible."""
    if args.all_eligible:
        return [t for t in st.eligible(state, datetime.now(timezone.utc)) if t != st.FIX_MAIN]
    if args.all:
        return [t["id"] for t in doc["tasks"] if t.get("prompt") and st.status(state, t["id"]) not in DONE_STATES]
    return list(args.task)


def done_file_problems(root: Path, doc: dict[str, Any], state: st.State) -> list[str]:
    """--all also lists every DONE task's owned regular file that is not on disk, whether or not a task
    still waiting depends on it (a DONE task with no waiting dependent is no claim's gap, but is the plan's)."""
    return [
        f"{t['id']}: DONE but its owned file {o} is not on main"
        for t in doc["tasks"]
        if st.status(state, t["id"]) == st.DONE
        for o in t.get("owns", [])
        if not o.endswith("/") and not (root / o).exists()
    ]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("task", nargs="*", help="task IDs such as T148")
    parser.add_argument("--all-eligible", action="store_true", help="every task claimable now")
    parser.add_argument("--all", action="store_true", help="every task not DONE or SKIPPED")
    parser.add_argument("--json", action="store_true", help="print {task: [problems]}")
    parser.add_argument("--state", type=Path, help="a kit/state.json file instead of claude/kit-state")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    try:
        doc = json.loads((args.root / gen_prompts.TASKS_FILE).read_text(encoding="utf-8"))
        state = read_state(args.root, args.state)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"ERROR inputs unreadable: {str(error)[:300]}")
        return 2
    check = checker(args.root, doc, pending_ok=args.all)
    found = {tid: check(state, tid) for tid in chosen(args, doc, state)}
    if args.all:
        found["DONE tasks"] = done_file_problems(args.root, doc, state)
    report(found, args.json)
    return 1 if any(found.values()) else 0


def report(found: dict[str, list[str]], as_json: bool) -> None:
    """Print the problems, one per line, or as JSON; a clean task prints `ok`."""
    if as_json:
        print(json.dumps(found, indent=1))
        return
    for tid, problems in found.items():
        print(f"ok  {tid}" if not problems else "\n".join(f"GAP {p}" for p in problems))
    print(f"checked {len(found)} tasks, {sum(1 for p in found.values() if p)} with gaps")


if __name__ == "__main__":
    sys.exit(main())

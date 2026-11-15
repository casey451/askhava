"""Plan-health check (f), report-only: does a task serve a page `Ally` has no states for? (D-3837)

`tests/a11y/axe_all_pages.spec.ts` (A11Y-002) fails `Ally` for every GET route in the route table (each
`.route(` in `app/*/src`, `tests/a11y/fixtures.ts` `ROUTES`) with no entry in `tests/a11y/page_states.ts`
`PAGE_STATES` and no reasoned `NOT_PAGES` line in `fixtures.ts`, the moment the route is in the code. T261's
attempt 4 (D-3836) wired the Confirm route and flipped its `docs/architecture.md` section 13 rows to `yes`
while the plan gave the entry to T495, which ran later: the whole attempt failed on that one test.

This module finds that hole from the plan alone. A task serves a GET route when a section 13 row
`| GET | <path> | <module> | T### |` names it (the queued task that wires the row, D-1868), or when its goal
or acceptance quotes `` `GET <path>` `` for a path section 13 does not list (a task that serves a route not
listed adds its row as `yes`; a sentence that names a stand-in, Stripe or Twilio, whose routes are not the
app's, and a path with a query string or under `/v1/`, a call the app makes, are left out). Such a route is warned unless:

- `page_states.ts` already quotes the path, `fixtures.ts`' `NOT_PAGES` lists it, or it is a test-control
  `/__test/` route (left out of the route table);
- the task's own goal or acceptance names the registry (`PAGE_STATES`, `PAGE_ENTRIES`, `page_states.ts` or
  `NOT_PAGES`) in a sentence that does not hand the entry away ("adds none", "is T261's", as T495's does):
  it is planned to add the entry in the same change, as D-3836 now has T261 do;
- a task it depends on, directly or not (so it is merged first), has a goal or acceptance sentence that
  names the registry and the path.

Report-only, as checks (d) and (e) are (D-2550, D-3496): `warnings` never holds a task or sets an exit code;
scripts/plan_health.py prints them as `WARN` lines under `--all` or `--pages` and imports this module late,
inside a guard, so a fault in it cannot stop the runner.

Usage: python3 scripts/plan_pages.py [--root REPO] [--state PATH] [T### ...]
  (no task IDs: every task not DONE or SKIPPED)
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from runner import state as st

ROOT = Path(__file__).resolve().parent.parent
ROUTES_DOC = "docs/architecture.md"
STATES_FILE = "tests/a11y/page_states.ts"
FIXTURES_FILE = "tests/a11y/fixtures.ts"
SECTION = re.compile(r"^## 13\. Routes\n(.*?)(?=^## |\Z)", re.MULTILINE | re.DOTALL)
GET_ROW = re.compile(r"^\|\s*GET\s*\|\s*(/[^\s|]*)\s*\|[^|]*\|\s*([^|]*?)\s*\|", re.MULTILINE)
GOAL_GET = re.compile(r"`GET (/[^`\s]*)`")
QUOTED_PATH = re.compile(r"\"(/[^\"\s]*)\"")
NOT_PAGES = re.compile(r"\[\s*\"(/[^\"]*)\",")
OUTSIDE = re.compile(r"stand[-_ ]in|\bstripe\b|\btwilio\b", re.IGNORECASE)
REGISTRY = re.compile(r"\b(PAGE_STATES|PAGE_ENTRIES|NOT_PAGES)\b|page_states\.ts")
DECLINES = re.compile(r"\badds (none|no)\b|\bis T\d{3}'s\b")
SENTENCE = re.compile(r"(?<=[.!?])\s+(?=[A-Z(`])")
TASK_ID = re.compile(r"^T\d{3}$")


@dataclass
class Plan:
    """The plan and the three files the check reads, read once for every task."""

    tasks: dict[str, dict[str, Any]]
    rows: dict[str, str]
    covered: set[str]


def text(root: Path, rel: str) -> str:
    """A file's text; empty when it does not exist yet."""
    path = root / rel
    return path.read_text(encoding="utf-8") if path.is_file() else ""


def read_plan(root: Path) -> Plan:
    """kit/tasks.json, section 13's GET rows (path to its `Wired` cell) and every path Ally already covers."""
    doc = json.loads(text(root, "kit/tasks.json"))
    section = SECTION.search(text(root, ROUTES_DOC))
    rows = {m.group(1): m.group(2) for m in GET_ROW.finditer(section.group(1) if section else "")}
    fixtures = text(root, FIXTURES_FILE)
    not_pages = fixtures.split("NOT_PAGES", 1)[1].split("];", 1)[0] if "NOT_PAGES" in fixtures else ""
    covered = set(QUOTED_PATH.findall(text(root, STATES_FILE))) | set(NOT_PAGES.findall(not_pages))
    return Plan({t["id"]: t for t in doc["tasks"]}, rows, covered)


def sentences(task: dict[str, Any]) -> list[str]:
    """The goal's and the acceptance's sentences."""
    return [s for s in SENTENCE.split(f"{task.get('goal', '')} {task.get('acceptance', '')}") if s.strip()]


def served(plan: Plan, task: dict[str, Any]) -> list[str]:
    """The GET routes the task serves: its section 13 rows, then the unlisted `GET <path>` its text quotes."""
    rows = [path for path, wired in plan.rows.items() if wired == task["id"]]
    quoted = [p for s in sentences(task) if not OUTSIDE.search(s) for p in GOAL_GET.findall(s)
              if "?" not in p and not p.startswith("/v1/")]
    return list(dict.fromkeys(rows + [p for p in quoted if p not in plan.rows]))


def ancestors(plan: Plan, task_id: str) -> set[str]:
    """Every task the given one depends on, directly or through other dependencies."""
    seen: set[str] = set()
    todo = list(plan.tasks.get(task_id, {}).get("deps", []))
    while todo:
        dep = todo.pop()
        if dep not in seen:
            seen.add(dep)
            todo.extend(plan.tasks.get(dep, {}).get("deps", []))
    return seen


def planned(plan: Plan, task: dict[str, Any], path: str) -> bool:
    """The task names the registry, or a task merged before it names the registry and the path."""
    if any(REGISTRY.search(s) and not DECLINES.search(s) for s in sentences(task)):
        return True
    earlier = (plan.tasks[t] for t in sorted(ancestors(plan, task["id"])) if t in plan.tasks)
    return any(REGISTRY.search(s) and path in s for dep in earlier for s in sentences(dep))


def task_warnings(plan: Plan, task_id: str) -> list[str]:
    """The task's warnings: one per GET route it serves that nothing gives page states."""
    task = plan.tasks.get(task_id)
    if task is None or not TASK_ID.match(task_id):
        return []
    found = []
    for path in served(plan, task):
        if path in plan.covered or path.startswith("/__test/") or planned(plan, task, path):
            continue
        where = "its section 13 row" if plan.rows.get(path) == task_id else "its goal"
        found.append(
            f"{task_id}: serves `GET {path}` ({where}) but no `PAGE_STATES` entry in {STATES_FILE} or `NOT_PAGES` "
            f"line covers it, and neither its goal nor a task it depends on adds one: A11Y-002 fails Ally once "
            f"the route is in the code (D-3836, D-3837)"
        )
    return found


def warnings(root: Path, task_ids: Iterable[str]) -> list[str]:
    """Every warning for the tasks, in their order; the files are read once."""
    plan = read_plan(root)
    return [w for tid in task_ids for w in task_warnings(plan, tid)]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("task", nargs="*", help="task IDs; none means every task not DONE or SKIPPED")
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--state", type=Path, help="a kit/state.json file; without it every task is checked")
    args = parser.parse_args(argv)
    state = json.loads(args.state.read_text(encoding="utf-8")) if args.state else None
    doc = json.loads((args.root / "kit/tasks.json").read_text(encoding="utf-8"))
    ids = args.task or [t["id"] for t in doc["tasks"] if st.status(state or st.new_state(), t["id"]) not in (st.DONE, st.SKIPPED)]
    found = warnings(args.root, ids)
    print("\n".join(f"WARN {line}" for line in found) + ("\n" if found else "") + f"{len(found)} warning(s) (D-3837)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

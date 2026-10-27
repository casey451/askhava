"""Generate kit/prompts/T###.md from kit/tasks.json and kit/prompts/TEMPLATE.md (kit step K7).

Every prompt comes from the one template, so they stay consistent. Gate tasks
have no prompt. With --check, nothing is written: the script exits 1 if a
prompt is missing, differs from what the template would produce (stale), or
does not name its model, owned files, tests, requirements and branch.

Usage: python3 scripts/gen_prompts.py [--root REPO_ROOT] [--check]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from string import Template
from typing import Any

TASKS_FILE = Path("kit/tasks.json")
TEMPLATE_FILE = Path("kit/prompts/TEMPLATE.md")
PROMPTS_DIR = Path("kit/prompts")
NONE_NOTE = "none for this task"
REQUIRED_KEYS = (
    "id",
    "slug",
    "title",
    "goal",
    "type",
    "phase",
    "stage",
    "model",
    "effort",
    "reviewer_model",
    "review_areas",
    "owns",
    "reads",
    "requirements",
    "tests",
    "decisions",
    "cites",
    "budget",
    "checks",
    "branch",
    "prompt",
)
BUDGET_KEYS = ("files", "lines", "context_pct")
TESTS_DONE = "The tests named above pass, then the full test suite and the twin pass."
NO_TESTS_DONE = "This task names no test entries: done means the full test suite and the smoke test pass."


def bullets(items: list[str], empty: str = NONE_NOTE) -> str:
    """Render a list as markdown bullets, or a plain note when empty."""
    if not items:
        return empty
    return "\n".join(f"- `{item}`" for item in items)


def require_keys(task: dict[str, Any]) -> None:
    """Raise a clear error naming the task if a required key is missing."""
    name = task.get("id", "<no id>")
    missing = [k for k in REQUIRED_KEYS if k not in task]
    budget = task.get("budget")
    if isinstance(budget, dict):
        missing += [f"budget.{k}" for k in BUDGET_KEYS if k not in budget]
    if missing:
        raise ValueError(f"task {name}: missing required key(s): {', '.join(missing)}")


def render(template: Template, task: dict[str, Any]) -> str:
    """Fill the template for one task."""
    require_keys(task)
    budget = task["budget"]
    values = {
        "id": task["id"],
        "title": task["title"],
        "type": task["type"],
        "phase": task["phase"],
        "stage": task["stage"],
        "slug": task["slug"],
        "branch": task["branch"],
        "model": task["model"],
        "effort": task["effort"],
        "reviewer_model": task["reviewer_model"],
        "review_areas": ", ".join(task["review_areas"]) or "none",
        "goal_section": ""
        if task["goal"] == task["title"]
        else f"## Goal\n\n{task['goal']}\n\n",
        "tests_done": TESTS_DONE if task["tests"] else NO_TESTS_DONE,
        "owns": bullets(task["owns"], "none (no files are owned; report only)"),
        "reads": bullets(task["reads"]),
        "requirements": bullets(task["requirements"]),
        "tests": bullets(task["tests"]),
        "decisions": bullets(task["decisions"]),
        "cites": bullets(task["cites"]),
        "budget_files": budget["files"],
        "budget_lines": budget["lines"],
        "budget_context": budget["context_pct"],
        "checks": ", ".join(f"`{c}`" for c in task["checks"]),
    }
    return template.substitute(values)


def prompt_tasks(data: dict[str, Any]) -> list[dict[str, Any]]:
    """Tasks that have a prompt file (gates do not)."""
    return [t for t in data["tasks"] if t.get("prompt")]


def check_task(root: Path, template: Template, task: dict[str, Any]) -> list[str]:
    """Return the problems with one task's prompt file."""
    path = root / task["prompt"]
    if not path.is_file():
        return [f"{task['id']}: missing {task['prompt']}"]
    text = path.read_text(encoding="utf-8")
    if text != render(template, task):
        return [f"{task['id']}: stale, differs from the template output"]
    return []


def check(root: Path, data: dict[str, Any], template: Template) -> list[str]:
    """Return every problem across all prompts."""
    problems: list[str] = []
    tasks = prompt_tasks(data)
    for task in tasks:
        problems.extend(check_task(root, template, task))
    expected = {Path(task["prompt"]).name for task in tasks}
    for path in sorted((root / PROMPTS_DIR).glob("T[0-9][0-9][0-9].md")):
        if path.name not in expected:
            problems.append(f"orphan prompt with no task: {path.name}")
    return problems


def generate(root: Path, data: dict[str, Any], template: Template) -> int:
    """Write every prompt; return how many were written."""
    tasks = prompt_tasks(data)
    for task in tasks:
        path = root / task["prompt"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(render(template, task), encoding="utf-8")
    return len(tasks)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    parser.add_argument("--check", action="store_true", help="verify, write nothing")
    args = parser.parse_args(argv)
    root: Path = args.root
    data = json.loads((root / TASKS_FILE).read_text(encoding="utf-8"))
    template = Template((root / TEMPLATE_FILE).read_text(encoding="utf-8"))
    try:
        return run(root, data, template, args.check)
    except ValueError as error:
        print(f"ERROR {error}")
        return 1


def run(root: Path, data: dict[str, Any], template: Template, check_only: bool) -> int:
    """Check or generate; ValueError means a malformed task."""
    if check_only:
        problems = check(root, data, template)
        for problem in problems:
            print(f"ERROR {problem}")
        count = len(prompt_tasks(data))
        print(f"checked {count} prompts, {len(problems)} problems")
        return 1 if problems else 0
    print(f"wrote {generate(root, data, template)} prompts")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Build progress measured from verified requirements, not from task counts or estimates.

A requirement counts as **verified** only when every build task that names it is DONE
(merged with Ci, Twin and Ally green on the tested tree, as the runner records it in
kit/state.json), every test-design entry that covers it is owned by a DONE task, and the
last nightly run of the full suite on main (recorded by the nightly-main workflow in
kit/nightly.json on claude/kit-state) was green at or after the latest of those merges. Anything less is
**partial** (some but not all of its tasks are DONE) or **unstarted**. Withdrawn
requirements are excluded from every count.

Tasks DONE is still reported, but only as throughput; the headline number is
requirements verified. This is the figure STATUS.md shows (BUILD_BRIEF.md section 7,
"STATUS.md (Casey's view)") and the one the readiness assessment reads.

Usage: python3 scripts/progress.py [--state PATH] [--json] [--markdown]
  --state   kit/state.json to read (default: kit/state.json if present, else all tasks pending)
  --nightly kit/nightly.json to read (default: beside the state file)
  --json    machine-readable output
  --markdown  the block the runner pastes into STATUS.md
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trace_spec  # noqa: E402  (sibling script)

TASKS_FILE = Path("kit/tasks.json")
TEST_DESIGN = Path("docs/test-design.md")
TD_ROW = re.compile(r"^\| (?P<id>TD-[A-Z0-9]+-\d{2}) \|(?P<rest>.*)$")
REQ_ID = re.compile(r"\b[A-Z0-9]+-\d{3}\b")
DONE = "DONE"


@dataclass
class ReqStatus:
    req_id: str
    area: str
    tasks: list[str]
    done_tasks: list[str]
    td: list[str]
    td_done: list[str]
    verified: bool
    nightly_ok: bool

    @property
    def state(self) -> str:
        if self.verified:
            return "verified"
        if self.done_tasks:
            return "partial"
        return "unstarted"


def _td_requirements(root: Path) -> dict[str, set[str]]:
    """Map each test-design entry to the requirement IDs its row names."""
    out: dict[str, set[str]] = {}
    for line in (root / TEST_DESIGN).read_text(encoding="utf-8").splitlines():
        m = TD_ROW.match(line)
        if m:
            out[m.group("id")] = set(REQ_ID.findall(m.group("rest")))
    return out


def _task_states(state: dict[str, Any] | None) -> dict[str, dict[str, Any]]:
    if not state:
        return {}
    return {t["id"]: t for t in state.get("tasks", [])}


def _is_ancestor(root: Path, older: str, newer: str) -> bool | None:
    """git merge-base --is-ancestor; None when git or either commit is unavailable."""
    try:
        r = subprocess.run(
            ["git", "-C", str(root), "merge-base", "--is-ancestor", older, newer],
            capture_output=True, text=True, timeout=20,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return {0: True, 1: False}.get(r.returncode)


def _nightly_green_after(
    nightly: dict[str, Any] | None, merges: list[tuple[str, str]], root: Path | None = None
) -> bool:
    """True when the recorded nightly-main run was green and tested every merge named.

    A merge counts as tested when its squash commit is an ancestor of the nightly run's commit
    (D-484); only when git cannot answer does it fall back to the run's start time (`started_at`,
    else `ran_at`) being after the merge.
    """
    if not nightly or nightly.get("conclusion") != "success":
        return False
    tested = nightly.get("sha", "")
    started = nightly.get("started_at") or nightly.get("ran_at", "")
    for sha, at in merges:
        answer = _is_ancestor(root, sha, tested) if (root and sha and tested) else None
        if answer is None:
            if at and started < at:
                return False
        elif not answer:
            return False
    return True


def compute(root: Path, state: dict[str, Any] | None, nightly: dict[str, Any] | None = None) -> list[ReqStatus]:
    report = trace_spec.run(root)
    graph = json.loads((root / TASKS_FILE).read_text(encoding="utf-8"))["tasks"]
    states = _task_states(state)
    td_map = _td_requirements(root)
    tasks_for: dict[str, list[str]] = defaultdict(list)
    td_owner: dict[str, list[str]] = defaultdict(list)
    for task in graph:
        for rid in task.get("requirements", []):
            tasks_for[rid].append(task["id"])
        for td in task.get("tests", []):
            td_owner[td].append(task["id"])
    td_for: dict[str, list[str]] = defaultdict(list)
    for td, rids in td_map.items():
        for rid in rids:
            td_for[rid].append(td)

    def is_done(tid: str) -> bool:
        return states.get(tid, {}).get("status") == DONE

    def merged_at(tid: str) -> str:
        return str(states.get(tid, {}).get("merged_at") or states.get(tid, {}).get("updated_at") or "")

    out: list[ReqStatus] = []
    for req in report.requirements:
        if req.text.startswith("Withdrawn"):
            continue
        tasks = sorted(tasks_for.get(req.req_id, []))
        done = [t for t in tasks if is_done(t)]
        tds = sorted(td_for.get(req.req_id, []))
        tds_done = [td for td in tds if td_owner.get(td) and all(is_done(t) for t in td_owner[td])]
        all_tasks_done = bool(tasks) and len(done) == len(tasks)
        all_td_done = len(tds_done) == len(tds)
        merges = [(str(states.get(t, {}).get("merged_sha") or ""), merged_at(t)) for t in done]
        nightly_ok = _nightly_green_after(nightly, merges, root)
        verified = all_tasks_done and all_td_done and nightly_ok
        out.append(
            ReqStatus(
                req.req_id,
                req.spec_file,
                tasks,
                done,
                tds,
                tds_done,
                verified,
                nightly_ok,
            )
        )
    return out


def summarize(rows: list[ReqStatus], graph: list[dict[str, Any]], states: dict[str, dict[str, Any]]) -> dict[str, Any]:
    by_area: dict[str, dict[str, int]] = defaultdict(lambda: {"verified": 0, "partial": 0, "unstarted": 0, "total": 0})
    for r in rows:
        by_area[r.area][r.state] += 1
        by_area[r.area]["total"] += 1
    total = len(rows)
    verified = sum(1 for r in rows if r.verified)
    partial = sum(1 for r in rows if r.state == "partial")
    build_tasks = [t for t in graph if t.get("type") != "gate"]
    tasks_done = sum(1 for t in build_tasks if states.get(t["id"], {}).get("status") == DONE)
    return {
        "requirements_total": total,
        "requirements_verified": verified,
        "requirements_partial": partial,
        "requirements_unstarted": total - verified - partial,
        "percent_verified": round(100.0 * verified / total, 1) if total else 0.0,
        "tasks_done": tasks_done,
        "tasks_total": len(build_tasks),
        "by_area": dict(sorted(by_area.items())),
    }


def markdown(summary: dict[str, Any]) -> str:
    lines = [
        f"**Requirements verified:** {summary['requirements_verified']} of {summary['requirements_total']} "
        f"({summary['percent_verified']}%) · partial {summary['requirements_partial']} · "
        f"unstarted {summary['requirements_unstarted']}",
        f"**Tasks merged (throughput only):** {summary['tasks_done']} of {summary['tasks_total']}",
        "",
        "| Area | Verified | Partial | Unstarted | Total |",
        "|---|---|---|---|---|",
    ]
    for area, c in summary["by_area"].items():
        name = Path(area).stem
        lines.append(f"| {name} | {c['verified']} | {c['partial']} | {c['unstarted']} | {c['total']} |")
    lines.append("")
    lines.append(
        "A requirement is verified only when every task naming it merged green, every test entry "
        "covering it is owned by a merged task, and the nightly full run on main was green afterwards."
    )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument("--state", default=None, help="path to kit/state.json")
    parser.add_argument("--nightly", default=None, help="path to kit/nightly.json")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--markdown", action="store_true")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    state_path = Path(args.state) if args.state else root / "kit" / "state.json"
    state = json.loads(state_path.read_text(encoding="utf-8")) if state_path.exists() else None
    nightly_path = Path(args.nightly) if args.nightly else state_path.with_name("nightly.json")
    nightly_text = nightly_path.read_text(encoding="utf-8") if nightly_path.is_file() else ""
    nightly = json.loads(nightly_text) if nightly_text.strip() else None
    rows = compute(root, state, nightly)
    graph = json.loads((root / TASKS_FILE).read_text(encoding="utf-8"))["tasks"]
    summary = summarize(rows, graph, _task_states(state))
    if args.json:
        summary["requirements"] = [
            {"id": r.req_id, "state": r.state, "tasks": r.tasks, "done": r.done_tasks, "td": r.td}
            for r in rows
        ]
        print(json.dumps(summary, indent=1))
    elif args.markdown:
        print(markdown(summary))
    else:
        print(
            f"Requirements: {summary['requirements_verified']} verified, {summary['requirements_partial']} partial, "
            f"{summary['requirements_unstarted']} unstarted of {summary['requirements_total']} "
            f"({summary['percent_verified']}%)"
        )
        print(f"Tasks merged: {summary['tasks_done']} of {summary['tasks_total']} (throughput only)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

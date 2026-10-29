"""Checks on one change set (a task branch or a plan change) against its base commit.

Run by `Ci` on every push and pull request, comparing the head against the merge base
with `main`. It makes four governance rules machine-enforced (docs/governance.md):

1. **Task branches stay inside their task.** A branch named `claude/task/T###-<slug>`
   may change only the paths task T### owns in `kit/tasks.json` plus the shared paths,
   and protected paths only when the task is a `harness` task. This is the same check the
   runner makes (runner/verify.py); here it runs where a worker cannot skip it.
2. **The decisions log is append-only.** Every `D-###` entry line in the base copy of
   `docs/decisions.md` is still there, byte for byte and in the same order, and new entries
   come only after the last base entry.
3. **A change to the locked blueprint needs a decision.** When any locked file or locked
   architecture section (scripts/check_plan_lock.py) changed, `kit/plan.lock` changed too
   and `docs/decisions.md` gained at least one new `D-###` entry. When a requirement line
   in `docs/spec/` changed, a new decision entry names that requirement's ID. On a task
   branch the lock may be rewritten only by a `harness` task or by a task that owns the
   locked file it changed.
4. **Withdrawals keep their ID.** A requirement ID present in the base spec is never
   removed; it is withdrawn in place ("Withdrawn by D-###.").
5. **A plan change says so.** A branch that is not a task branch and touches a protected
   or locked path must carry a `Plan-Change: D-###` trailer on one of its commits naming one
   of the new decision entries. (Until Claude runs under its own GitHub identity, this proves
   intent, not authorship; docs/governance.md section 2 records that gap.)

Diffs are listed with `--no-renames`, so a protected file moved into an owned path shows as
a deletion.

Usage: python3 scripts/check_pr_change.py --base <git ref> [--branch <name>] [--root .]
Exit 1 on any violation.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import check_plan_lock  # noqa: E402
from runner import verify  # noqa: E402

TASK_BRANCH = re.compile(r"^claude/task/(?P<id>T\d{3})-")
DECISION = re.compile(r"^- \*\*(D-\d{3})\*\*")
REQ_LINE = re.compile(r"^- \*\*(?P<id>[A-Z0-9]+-\d{3})\*\* ")
DECISIONS = "docs/decisions.md"
LOCK = check_plan_lock.LOCK_FILE.as_posix()


def git(root: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True, text=True).stdout


def show(root: Path, ref: str, path: str) -> str | None:
    try:
        return git(root, "show", f"{ref}:{path}")
    except subprocess.CalledProcessError:
        return None


def changed_files(root: Path, base: str) -> list[str]:
    out = git(root, "diff", "--no-renames", "--name-only", f"{base}...HEAD")
    return sorted(line for line in out.splitlines() if line.strip())


def task_paths_check(root: Path, branch: str, changed: list[str]) -> list[str]:
    m = TASK_BRANCH.match(branch or "")
    if not m:
        return []
    graph = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    task = next((t for t in graph["tasks"] if t["id"] == m.group("id")), None)
    if task is None:
        return [f"branch {branch}: task {m.group('id')} is not in kit/tasks.json"]
    return [f"task paths: {p}" for p in verify.check_paths(changed, task, graph["protected_paths"], graph["shared_paths"])]


def decisions_append_only(base_text: str | None, head_text: str | None) -> list[str]:
    """Base entries are a byte-identical prefix of the head's entries, in order."""
    if base_text is None or head_text is None:
        return []
    base_entries = [line for line in base_text.splitlines() if DECISION.match(line)]
    head_entries = [line for line in head_text.splitlines() if DECISION.match(line)]
    if head_entries[: len(base_entries)] == base_entries:
        return []
    for i, line in enumerate(base_entries):
        if i >= len(head_entries) or head_entries[i] != line:
            return [f"docs/decisions.md is append-only; entry {i + 1} was changed, moved or removed: {line[:80]}"]
    return ["docs/decisions.md is append-only; base entries are not an unchanged prefix of the head"]


def new_decisions(base_text: str | None, head_text: str | None) -> list[str]:
    old = set(DECISION.match(line).group(1) for line in (base_text or "").splitlines() if DECISION.match(line))
    out = []
    for line in (head_text or "").splitlines():
        m = DECISION.match(line)
        if m and m.group(1) not in old:
            out.append(line)
    return out


def requirement_lines(text: str | None) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in (text or "").splitlines():
        m = REQ_LINE.match(line)
        if m:
            out[m.group("id")] = line
    return out


def spec_changes(root: Path, base: str, changed: list[str]) -> tuple[set[str], list[str]]:
    """Requirement IDs whose line changed, and errors for IDs that vanished."""
    ids: set[str] = set()
    errors: list[str] = []
    for path in changed:
        if not path.startswith("docs/spec/") or not path.endswith(".md"):
            continue
        before = requirement_lines(show(root, base, path))
        after = requirement_lines(show(root, "HEAD", path))
        for rid, line in before.items():
            if rid not in after:
                errors.append(f"{path}: {rid} was removed; retire it in place with 'Withdrawn by D-###.'")
            elif after[rid] != line:
                ids.add(rid)
        ids.update(rid for rid in after if rid not in before)
    return ids, errors


def task_for_branch(root: Path, branch: str) -> dict | None:
    m = TASK_BRANCH.match(branch or "")
    if not m:
        return None
    graph = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    return next((t for t in graph["tasks"] if t["id"] == m.group("id")), None)


def lock_rewrite_check(root: Path, branch: str, changed: list[str]) -> list[str]:
    """On a task branch, only a harness task or the owner of a changed locked file may rewrite the lock."""
    task = task_for_branch(root, branch)
    if task is None or LOCK not in changed or task.get("type") == "harness":
        return []
    locked = set(check_plan_lock.locked_paths(root))
    owned_locked = [p for p in changed if p in locked and p in task.get("owns", [])]
    if owned_locked:
        return []
    return [f"{LOCK} rewritten by {task['id']}, which is not a harness task and owns no locked file in this change"]


def plan_change_trailer_check(
    root: Path, branch: str, changed: list[str], decisions_added: list[str], base: str
) -> list[str]:
    """A non-task branch touching protected or locked paths must say which decision it carries out."""
    if TASK_BRANCH.match(branch or ""):
        return []
    graph = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    protected = tuple(graph["protected_paths"])
    locked = set(check_plan_lock.locked_paths(root))
    touched = [p for p in changed if p in locked or any(p.startswith(q) if q.endswith("/") else p == q for q in protected)]
    if not touched:
        return []
    # The trailer may sit on any commit of the change (a pull-request run checks out a merge commit).
    message = git(root, "log", "--format=%B", f"{base}..HEAD")
    found = re.findall(r"^Plan-Change: (D-\d{3})\s*$", message, re.M)
    if not found:
        return [f"plan change touching {touched[0]} needs a 'Plan-Change: D-###' trailer on one of its commits"]
    m = re.match(r"(D-\d{3})", found[0])  # the newest commit's trailer governs
    if not any(m.group(1) in line for line in decisions_added):
        return [f"Plan-Change trailer names {m.group(1)}, which is not a new entry in docs/decisions.md"]
    return []


def locked_change_check(root: Path, base: str, changed: list[str], decisions_added: list[str]) -> list[str]:
    errors: list[str] = []
    locked = set(check_plan_lock.locked_paths(root))
    touched = [p for p in changed if p in locked]
    base_arch = show(root, base, check_plan_lock.ARCHITECTURE) or ""
    head_arch = show(root, "HEAD", check_plan_lock.ARCHITECTURE) or ""
    for heading in check_plan_lock.LOCKED_SECTIONS:
        if check_plan_lock.section_text(base_arch, heading) != check_plan_lock.section_text(head_arch, heading):
            touched.append(f"{check_plan_lock.ARCHITECTURE}#{heading}")
    if not touched:
        return errors
    if LOCK not in changed:
        errors.append(f"locked blueprint changed ({', '.join(touched[:6])}) but {LOCK} was not rewritten")
    if not decisions_added:
        errors.append(f"locked blueprint changed ({', '.join(touched[:6])}) with no new docs/decisions.md entry")
    return errors


RANGE = re.compile(r"\b([A-Z0-9]+)-(\d{3}) to (?:\1-)?(\d{3})\b")


def ids_named(text: str) -> set[str]:
    """Requirement IDs a decision names, with 'SEC-035 to SEC-048' expanded."""
    named = set(re.findall(r"\b[A-Z0-9]+-\d{3}\b", text))
    for prefix, start, end in RANGE.findall(text):
        named.update(f"{prefix}-{n:03d}" for n in range(int(start), int(end) + 1))
    return named


def spec_decision_check(spec_ids: set[str], decisions_added: list[str]) -> list[str]:
    named = ids_named("\n".join(decisions_added))
    return [f"{rid} changed in docs/spec but no new decision entry names it" for rid in sorted(spec_ids) if rid not in named]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    parser.add_argument("--base", required=True, help="base commit or ref (merge base with main)")
    parser.add_argument("--branch", default="", help="head branch name (claude/task/T###-slug enables the path check)")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    base = git(root, "merge-base", args.base, "HEAD").strip()
    changed = changed_files(root, base)
    errors: list[str] = []
    errors += task_paths_check(root, args.branch, changed)
    base_dec = show(root, base, DECISIONS)
    head_dec = show(root, "HEAD", DECISIONS)
    errors += decisions_append_only(base_dec, head_dec)
    added = new_decisions(base_dec, head_dec)
    errors += locked_change_check(root, base, changed, added)
    errors += lock_rewrite_check(root, args.branch, changed)
    errors += plan_change_trailer_check(root, args.branch, changed, added, base)
    spec_ids, spec_errors = spec_changes(root, base, changed)
    errors += spec_errors
    errors += spec_decision_check(spec_ids, added)
    errors += [f"plan lock: {d}" for d in check_plan_lock.differences(root)]
    print(f"change check: {len(changed)} files changed against {base[:7]}; {len(added)} new decision(s); branch '{args.branch or '-'}'")
    for e in errors:
        print(f"ERROR {e}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())

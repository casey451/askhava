"""Checks on one change set (a task branch or a plan change) against its base commit.

Run by `Ci` on every push and pull request, comparing the head against the merge base
with `main`. It makes six governance rules machine-enforced (docs/governance.md):

1. **Task branches stay inside their task.** A branch named `claude/task/T###-<slug>`
   may change only the paths task T### owns in `kit/tasks.json` plus the shared paths,
   and protected paths only when the task is a `harness` task. This is the same check the
   runner makes (runner/verify.py); here it runs where a worker cannot skip it.
2. **The decisions log is append-only.** Every `D-###` entry line in the base copy of
   `docs/decisions.md` is still there, byte for byte and in the same order, and new entries
   come only after the last base entry. No D-number appears on two entries of the head copy,
   and no new entry uses a number the `--base` ref's current copy already has (a stale branch);
   gaps in the numbering are allowed (numbers are reserved per task, D-530).
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
6. **An over-limit crate guide is trimmed when the task is alone in its crate (D-5080).**
   On `claude/task/T###` branches only: the claim base is the parent of the oldest commit in
   `git rev-list --first-parent HEAD --not <base>`. For each crate the task writes (its owned
   `app/<crate>/` paths plus its `module`), if the claim base's `app/<crate>/CLAUDE.md` is over
   7,392 bytes, the head's must be at most 5,500. Skipped when the claim base's own copy of this
   script lacks the rule (tasks claimed earlier are not held), when the claim base cannot be
   found, and on other branches. It compares with the claim base, so an update merge of `main`
   never trips it.

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
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import check_plan_lock  # noqa: E402
from runner import state as runner_state, verify  # noqa: E402

TASK_BRANCH = re.compile(r"^claude/task/(?P<id>T\d{3})-")
DECISION = re.compile(r"^- \*\*(D-\d{3,})\*\*")
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


def base_graph(root: Path) -> dict:
    """The task graph as it stands on the base: a branch never grants itself paths or a type (D-484)."""
    text = show(root, BASE[0], "kit/tasks.json") if BASE else None
    if text is None:
        text = (root / "kit/tasks.json").read_text(encoding="utf-8")
    return json.loads(text)


BASE: list[str] = []
GUIDE_OVER = 7392
GUIDE_TRIMMED = 5500
RULE6_MARK = "def crate_guide_trim_check"
APP_PATH = re.compile(r"^app/(?P<crate>[^/]+)/")


def task_paths_check(root: Path, branch: str, changed: list[str]) -> list[str]:
    m = TASK_BRANCH.match(branch or "")
    if not m:
        return []
    graph = base_graph(root)
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


def decisions_unique(head_text: str | None) -> list[str]:
    """Each D-number names one entry of the head's docs/decisions.md; gaps are fine (D-530)."""
    counts = Counter(runner_state.decision_numbers(head_text or ""))
    return [
        f"docs/decisions.md has D-{n:03d} on {count} entries; renumber the newer one to a free number "
        "(the handoff's decision_ids, or `python3 -m runner --reserve-decision` for a plan change, D-1058)"
        for n, count in sorted(counts.items())
        if count > 1
    ]


def decisions_not_taken_on_main(base_text: str | None, main_text: str | None, head_text: str | None) -> list[str]:
    """No new head entry (a number absent from the merge base's copy) uses a number main's current copy
    already has: a stale branch whose numbers main took meanwhile fails before it merges (D-530)."""
    old = set(runner_state.decision_numbers(base_text or ""))
    on_main = set(runner_state.decision_numbers(main_text or ""))
    added = set(runner_state.decision_numbers(head_text or "")) - old
    return [
        f"docs/decisions.md: D-{n:03d} is new on this branch but main already has a D-{n:03d} entry; "
        "renumber this branch's entry (the handoff's decision_ids, or `python3 -m runner --reserve-decision`, D-1058)"
        for n in sorted(added & on_main)
    ]


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
    graph = base_graph(root)
    return next((t for t in graph["tasks"] if t["id"] == m.group("id")), None)


def claim_base(root: Path, base_ref: str) -> str | None:
    """The parent of the task's oldest first-parent commit not on the base ref, or None."""
    try:
        commits = git(root, "rev-list", "--first-parent", "HEAD", "--not", base_ref).split()
        return git(root, "rev-parse", f"{commits[-1]}^").strip() if commits else None
    except subprocess.CalledProcessError:
        return None


def crate_guide_trim_check(root: Path, branch: str, base_ref: str) -> list[str]:
    """Rule 6 (D-5080): a crate guide over 7,392 bytes at the claim base is at most 5,500 at the head."""
    task = task_for_branch(root, branch)
    if task is None:
        return []
    claim = claim_base(root, base_ref)
    if claim is None or RULE6_MARK not in (show(root, claim, "scripts/check_pr_change.py") or ""):
        return []
    crates = {m.group("crate") for p in task.get("owns", []) if (m := APP_PATH.match(p))}
    if task.get("module"):
        crates.add(task["module"])
    errors = []
    for crate in sorted(crates):
        path = f"app/{crate}/CLAUDE.md"
        before, after = show(root, claim, path), show(root, "HEAD", path)
        if before is None or after is None:
            continue
        old, new = len(before.encode("utf-8")), len(after.encode("utf-8"))
        if old > GUIDE_OVER and new > GUIDE_TRIMMED:
            errors.append(
                f"crate guide not trimmed: {path} was {old} bytes at the claim base (over {GUIDE_OVER}) and is {new} now "
                f"(limit {GUIDE_TRIMMED}); D-5080: move the oldest `## Never break` bullets' detail word for word to NOTES.md "
                f"as `(never N)` lines, keep each invariant and its guarding test, end the bullet \"More: `(never N)` in NOTES.md.\", "
                f"run `python3 scripts/split_crate_doc.py --verify origin/main app/{crate}`"
            )
    return errors


def lock_rewrite_check(root: Path, branch: str, changed: list[str]) -> list[str]:
    """On a task branch, only a harness task or the owner of a changed locked file may rewrite the lock."""
    task = task_for_branch(root, branch)
    if task is None or LOCK not in changed or task.get("type") == "harness":
        return []
    locked = locked_set(root, BASE[0]) if BASE else set(check_plan_lock.locked_paths(root))
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
    graph = base_graph(root)
    protected = tuple(graph["protected_paths"])
    locked = locked_set(root, base)
    touched = [p for p in changed if p in locked or any(p.startswith(q) if q.endswith("/") else p == q for q in protected)]
    if not touched:
        return []
    # The trailer may sit on any commit of the change (a pull-request run checks out a merge commit).
    message = git(root, "log", "--format=%B", f"{base}..HEAD")
    found = re.findall(r"^Plan-Change: (D-\d{3,})\s*$", message, re.M)
    if not found:
        return [f"plan change touching {touched[0]} needs a 'Plan-Change: D-###' trailer on one of its commits"]
    m = re.match(r"(D-\d{3,})", found[0])  # the newest commit's trailer governs
    if not any(DECISION.match(line).group(1) == m.group(1) for line in decisions_added):
        return [f"Plan-Change trailer names {m.group(1)}, which is not a new entry in docs/decisions.md"]
    return []


def locked_set(root: Path, base: str) -> set[str]:
    """Locked paths on the head plus every file the base lock names, so a deleted locked file still counts."""
    locked = set(check_plan_lock.locked_paths(root))
    base_lock = show(root, base, check_plan_lock.LOCK_FILE.as_posix())
    if base_lock:
        locked |= {k for k in json.loads(base_lock).get("items", {}) if "#" not in k}
    return locked


def locked_change_check(root: Path, base: str, changed: list[str], decisions_added: list[str]) -> list[str]:
    errors: list[str] = []
    locked = locked_set(root, base)
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
    try:
        base = git(root, "merge-base", args.base, "HEAD").strip()
    except subprocess.CalledProcessError:
        print(f"ERROR no merge base between {args.base} and HEAD: fetch more history (git fetch --deepen) and rerun", file=sys.stderr)
        return 1
    BASE[:] = [base]
    changed = changed_files(root, base)
    errors: list[str] = []
    errors += task_paths_check(root, args.branch, changed)
    base_dec = show(root, base, DECISIONS)
    head_dec = show(root, "HEAD", DECISIONS)
    errors += decisions_append_only(base_dec, head_dec)
    errors += decisions_unique(head_dec)
    errors += decisions_not_taken_on_main(base_dec, show(root, args.base, DECISIONS), head_dec)
    added = new_decisions(base_dec, head_dec)
    errors += locked_change_check(root, base, changed, added)
    errors += crate_guide_trim_check(root, args.branch, args.base)
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

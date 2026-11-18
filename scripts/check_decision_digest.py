"""Every decision-log entry up to a bound is counted exactly once in the digest (D-3856, D-4447).

Reads `docs/decisions.md` (an entry is a line starting `- **D-n**` or `**D-n**`, n of three or more
digits; D-1552 to D-1561 have no leading `- `), `docs/decisions-current.md`,
`kit/superseded_decisions.json` and `kit/tasks.json`, and only reads. Only three digest line forms
count a number: a rule line's leading numbers under an area (`- D-a[, D-b] (area) text`, text at
most 300 characters); the left side of a `D-a -> D-b` pair under `## Superseded` (the pair must be
in the pair file); the numbers on a `- T###: D-a, D-b` line under `## Task-scoped` (T### in
`kit/tasks.json`). Numbers compare as integers whatever the padding, and the bound is numeric, not a
file position (the log is not in numeric order). Fails on: an unknown or doubled number, a log entry
at or below the bound the digest does not count (a late merge, named for the next piece), a pair
from the pair file with both numbers at or below the bound that is not listed under
`## Superseded`, a malformed line, a missing `Through D-####` header, and a header lower than the
base branch's. Entries above the bound, and pairs whose newer number is above it, are pending. With
no digest every entry is pending and the check passes, except under `--all`.

Usage: python3 scripts/check_decision_digest.py [--through D-#### | --all] [--root .]
       [--base-ref origin/main | --base-file PATH]
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

LOG = "docs/decisions.md"
DIGEST = "docs/decisions-current.md"
PAIRS = "kit/superseded_decisions.json"
TASKS = "kit/tasks.json"
ENTRY = re.compile(r"^(?:- )?\*\*D-(\d{3,})\*\*")
HEADER = re.compile(r"Through D-(\d+)")
RULE = re.compile(r"^- (D-\d+(?:, D-\d+)*) \(([^()]+)\) (.+)$")
PAIR = re.compile(r"^(?:- )?D-(\d+) -> D-(\d+)\s*$")
TASK_LINE = re.compile(r"^- (T\d{3,}): (D-\d+(?:, D-\d+)*)\s*$")
NUMBER = re.compile(r"D-(\d+)")
SUPERSEDED = "Superseded"
TASK_SCOPED = "Task-scoped"
MAX_RULE_TEXT = 300


def log_entries(root: Path) -> set[int]:
    """Every entry number in the log, as an integer."""
    text = (root / LOG).read_text(encoding="utf-8")
    return {int(m.group(1)) for m in map(ENTRY.match, text.splitlines()) if m}


def load_pairs(root: Path) -> dict[int, int]:
    """The superseded pairs (older -> newer) of the pair file, as integers."""
    data = json.loads((root / PAIRS).read_text(encoding="utf-8"))
    return {int(k[2:]): int(v[2:]) for k, v in data.get("superseded", {}).items()}


def load_task_ids(root: Path) -> set[str]:
    """Every task ID in the task graph."""
    data = json.loads((root / TASKS).read_text(encoding="utf-8"))
    return {task["id"] for task in data.get("tasks", [])}


def header_number(text: str) -> int | None:
    """The digest's `Through D-####` number, from the lines before its first `## ` heading."""
    for line in text.splitlines():
        if line.startswith("## "):
            break
        found = HEADER.search(line)
        if found:
            return int(found.group(1))
    return None


def base_digest(root: Path, base_ref: str | None, base_file: str | None) -> str | None:
    """The base branch's digest text, or None when the base has none (or git cannot say)."""
    if base_file is not None:
        path = Path(base_file)
        return path.read_text(encoding="utf-8") if path.is_file() else None
    if base_ref is None:
        return None
    shown = subprocess.run(
        ["git", "show", f"{base_ref}:{DIGEST}"], cwd=root, capture_output=True, text=True
    )
    return shown.stdout if shown.returncode == 0 else None


class Digest:
    """What a digest counts: number -> where, the pairs it lists and its line problems."""

    def __init__(self, task_ids: set[str]) -> None:
        self.task_ids = task_ids
        self.counted: dict[int, list[str]] = {}
        self.listed_pairs: dict[int, int] = {}
        self.problems: list[str] = []

    def count(self, numbers: list[int], where: str) -> None:
        for number in numbers:
            self.counted.setdefault(number, []).append(where)

    def read_line(self, section: str, line: str, where: str) -> None:
        """Count one digest line under its `## ` section."""
        if section == SUPERSEDED:
            self.read_pair(line, where)
        elif section == TASK_SCOPED:
            self.read_task(line, where)
        elif line.startswith("- "):
            self.read_rule(line, where)

    def read_pair(self, line: str, where: str) -> None:
        found = PAIR.match(line)
        if found:
            older, newer = int(found.group(1)), int(found.group(2))
            self.listed_pairs[older] = newer
            self.count([older], where + " (superseded)")
        elif line.strip():
            self.problems.append(f"{where}: not a `D-a -> D-b` pair: {line[:80]}")

    def read_task(self, line: str, where: str) -> None:
        found = TASK_LINE.match(line)
        if not found:
            if line.strip():
                self.problems.append(f"{where}: not a `- T###: D-a, D-b` line: {line[:80]}")
            return
        if found.group(1) not in self.task_ids:
            self.problems.append(f"{where}: task {found.group(1)} is not in {TASKS}")
        numbers = [int(n) for n in NUMBER.findall(found.group(2))]
        self.count(numbers, where + " (task-scoped)")

    def read_rule(self, line: str, where: str) -> None:
        found = RULE.match(line)
        if not found:
            shape = "`- D-a[, D-b] (area) text`"
            self.problems.append(f"{where}: not a {shape} rule line: {line[:80]}")
            return
        if len(found.group(3)) > MAX_RULE_TEXT:
            self.problems.append(f"{where}: rule text is over {MAX_RULE_TEXT} characters")
        self.count([int(n) for n in NUMBER.findall(found.group(1))], where + " (rule)")


def parse_digest(text: str, task_ids: set[str]) -> Digest:
    """Read every counted number of the digest, section by section."""
    digest = Digest(task_ids)
    section = None
    for index, line in enumerate(text.splitlines(), start=1):
        if line.startswith("## "):
            section = line[3:].strip()
        elif section is not None:
            digest.read_line(section, line, f"{DIGEST}:{index}")
    return digest


def count_problems(digest: Digest, entries: set[int], bound: float) -> list[str]:
    """Unknown, doubled and missing numbers against the log, up to the bound."""
    problems = []
    for number, places in sorted(digest.counted.items()):
        if number not in entries:
            problems.append(f"D-{number:03d} is not an entry of {LOG} ({places[0]})")
        elif len(places) > 1:
            problems.append(f"D-{number:03d} is counted {len(places)} times: {', '.join(places)}")
    for number in sorted(n for n in entries if n <= bound and n not in digest.counted):
        problems.append(f"D-{number:03d} is at or below the bound but not in the digest")
    return problems


def pair_problems(
    digest: Digest, pairs: dict[int, int], entries: set[int], bound: float
) -> list[str]:
    """Listed pairs must be in the pair file; due pairs must be listed, not kept as rule lines."""
    problems = []
    for older, newer in sorted(digest.listed_pairs.items()):
        if pairs.get(older) != newer:
            problems.append(f"D-{older:03d} -> D-{newer:03d} is not a pair in {PAIRS}")
    for older, newer in sorted(pairs.items()):
        due = older in entries and older <= bound and newer <= bound
        if due and digest.listed_pairs.get(older) != newer:
            problems.append(f"D-{older:03d} -> D-{newer:03d} is due but not under ## {SUPERSEDED}")
    return problems


def header_problems(text: str, base_text: str | None) -> list[str]:
    """The digest needs a header, never lower than the base branch's."""
    through = header_number(text)
    if through is None:
        return [f"{DIGEST} has no `Through D-####` header"]
    base = header_number(base_text) if base_text is not None else None
    if base is not None and through < base:
        return [f"header Through D-{through:03d} is lower than the base's D-{base:03d}"]
    return []


def check(
    root: Path, through: int | None, every: bool, base_text: str | None
) -> tuple[list[str], int]:
    """All problems, and how many log entries are pending (above the bound)."""
    entries = log_entries(root)
    digest_path = root / DIGEST
    if not digest_path.is_file():
        missing = [f"{DIGEST} does not exist (--all needs every entry counted)"] if every else []
        return missing, len(entries)
    text = digest_path.read_text(encoding="utf-8")
    problems = header_problems(text, base_text)
    if problems:
        return problems, len(entries)
    header = header_number(text)
    bound: float = float("inf") if every else (through if through is not None else header)
    digest = parse_digest(text, load_task_ids(root))
    problems = digest.problems + count_problems(digest, entries, bound)
    problems += pair_problems(digest, load_pairs(root), entries, bound)
    return problems, sum(1 for n in entries if n > bound)


def through_number(value: str) -> int:
    found = re.fullmatch(r"D-(\d+)", value)
    if not found:
        raise argparse.ArgumentTypeError(f"expected D-####, got {value}")
    return int(found.group(1))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    bound = parser.add_mutually_exclusive_group()
    bound.add_argument("--through", type=through_number, help="check entries up to D-####")
    bound.add_argument("--all", action="store_true", help="check every entry of the log")
    parser.add_argument("--root", default=".", help="repository root")
    base = parser.add_mutually_exclusive_group()
    base.add_argument("--base-ref", default="origin/main", help="git ref of the base digest")
    base.add_argument("--base-file", help="base digest file instead of the git ref")
    args = parser.parse_args(argv)
    root = Path(args.root)
    base_text = base_digest(root, args.base_ref, args.base_file)
    problems, pending = check(root, args.through, args.all, base_text)
    for problem in problems:
        print(f"check_decision_digest: {problem}")
    status = "FAILED" if problems else "ok"
    print(f"check_decision_digest: {status}, {len(problems)} problem(s), {pending} entries pending")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

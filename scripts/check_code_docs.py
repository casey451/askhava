"""Check the code documentation standard (docs/code-docs.md, D-473).

Checks, for the code that exists so far (nothing fails before the workspace exists):
- every crate under app/ (a folder with Cargo.toml) has a CLAUDE.md of at most
  120 lines with the six headings, in order, and is named in docs/architecture.md;
- every .rs file under app/ and tests/ starts with a //! module comment;
- every SQL file in migrations/ starts with a -- comment;
- every requirement ID named in app/ code or a crate CLAUDE.md exists in docs/spec/;
- once every file a build task owns exists, each of the task's requirements is
  named in one of those files.

Usage: python3 scripts/check_code_docs.py [--root REPO_ROOT]
Exit status 1 lists each problem.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

HEADINGS = (
    "## Purpose",
    "## What it owns",
    "## Rules it enforces",
    "## Never break",
    "## How to change",
    "## Tests that guard it",
)
MAX_CRATE_DOC_LINES = 120
REQ_DEF = re.compile(r"^- \*\*([A-Z0-9]+-\d{3})\*\*", re.MULTILINE)
REQ_REF = re.compile(r"\b(?!TD-)(?!D-)([A-Z][A-Z0-9]{1,5}-\d{3})\b")
SKIP_DIRS = {"target", "node_modules", ".git"}


def spec_ids(root: Path) -> set[str]:
    """Every requirement ID defined in docs/spec/*.md."""
    ids: set[str] = set()
    for path in sorted((root / "docs/spec").glob("*.md")):
        ids.update(REQ_DEF.findall(path.read_text(encoding="utf-8")))
    return ids


def files_under(root: Path, folder: str, suffix: str) -> list[Path]:
    base = root / folder
    if not base.is_dir():
        return []
    return [p for p in sorted(base.rglob(f"*{suffix}")) if not SKIP_DIRS & set(p.relative_to(root).parts)]


def first_text_line(path: Path) -> str:
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            return line.strip()
    return ""


def check_crate_doc(root: Path, crate: Path, architecture: str) -> list[str]:
    rel = crate.relative_to(root).as_posix()
    problems = [] if rel in architecture else [f"{rel}: crate not named in docs/architecture.md"]
    doc = crate / "CLAUDE.md"
    if not doc.is_file():
        return problems + [f"{rel}/CLAUDE.md: missing (docs/code-docs.md section 2)"]
    lines = doc.read_text(encoding="utf-8").splitlines()
    if len(lines) > MAX_CRATE_DOC_LINES:
        problems.append(f"{rel}/CLAUDE.md: {len(lines)} lines, limit {MAX_CRATE_DOC_LINES}")
    found = [h for h in (line.strip() for line in lines) if h in HEADINGS]
    if found != list(HEADINGS):
        problems.append(f"{rel}/CLAUDE.md: headings must be, in order: {', '.join(HEADINGS)}")
    return problems


def check_crates(root: Path) -> list[str]:
    arch_path = root / "docs/architecture.md"
    architecture = arch_path.read_text(encoding="utf-8") if arch_path.is_file() else ""
    problems: list[str] = []
    for manifest in sorted((root / "app").glob("*/Cargo.toml")) if (root / "app").is_dir() else []:
        problems += check_crate_doc(root, manifest.parent, architecture)
    return problems


def check_headers(root: Path) -> list[str]:
    problems: list[str] = []
    for folder in ("app", "tests"):
        for path in files_under(root, folder, ".rs"):
            if not first_text_line(path).startswith("//!"):
                problems.append(f"{path.relative_to(root)}: must start with a //! module comment")
    for path in files_under(root, "migrations", ".sql"):
        if not first_text_line(path).startswith("--"):
            problems.append(f"{path.relative_to(root)}: must start with a -- comment")
    return problems


def check_references(root: Path, known: set[str]) -> list[str]:
    problems: list[str] = []
    sources = files_under(root, "app", ".rs") + files_under(root, "app", "CLAUDE.md")
    for path in sources:
        for ref in sorted(set(REQ_REF.findall(path.read_text(encoding="utf-8")))):
            if ref not in known:
                problems.append(f"{path.relative_to(root)}: names {ref}, which is not in docs/spec/")
    return problems


def check_trace(root: Path) -> list[str]:
    tasks_path = root / "kit/tasks.json"
    if not tasks_path.is_file():
        return []
    problems: list[str] = []
    for task in json.loads(tasks_path.read_text(encoding="utf-8"))["tasks"]:
        files = [root / p for p in task.get("owns", []) if not p.endswith("/")]
        if task.get("type") != "build" or not files or not all(p.is_file() for p in files):
            continue
        text = "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in files)
        for req in task.get("requirements", []):
            if not re.search(rf"\b{re.escape(req)}\b", text):
                problems.append(f"{task['id']}: {req} is named in none of its owned files")
    return problems


def run(root: Path) -> list[str]:
    return check_crates(root) + check_headers(root) + check_references(root, spec_ids(root)) + check_trace(root)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args(argv)
    problems = run(args.root)
    for problem in problems:
        print(problem)
    print(f"check_code_docs: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

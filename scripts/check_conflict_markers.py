"""No git conflict markers are left in any tracked text file (D-3307).

Runs `git ls-files`, skips binary files (any file with a NUL byte), and fails with file:line for
every line that starts with seven `<` and a space, starts with seven `>` and a space, or is exactly
seven `=`. A commit that kept such lines in docs/for-casey-review.md once passed every check (Nov 12).
No tracked file legitimately has these lines, so there is no allowlist; write a setext heading
underline with a different length. Usage: python3 scripts/check_conflict_markers.py [--root .]
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

OPEN = "<" * 7 + " "
CLOSE = ">" * 7 + " "
MIDDLE = "=" * 7


def tracked(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True
    ).stdout
    return [p for p in out.decode("utf-8", "replace").split("\0") if p]


def is_marker(line: str) -> bool:
    return line.startswith(OPEN) or line.startswith(CLOSE) or line.rstrip("\r\n") == MIDDLE


def scan(root: Path) -> list[str]:
    problems = []
    for name in tracked(root):
        path = root / name
        if not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        for number, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1):
            if is_marker(line):
                problems.append(f"{name}:{number}: git conflict marker")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".")
    problems = scan(Path(parser.parse_args().root))
    for problem in problems:
        print(problem)
    if problems:
        print(f"FAILED: {len(problems)} conflict marker line(s)")
        return 1
    print("ok: no conflict markers in tracked files")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""The plan lock: a fingerprint of the approved blueprint, checked by Ci on every push.

`kit/plan.lock` records a SHA-256 for each locked file and for the locked sections of
`docs/architecture.md` (module ownership, dependency rules, the shared kernel and the job
state machine). `Ci` fails when the working tree disagrees with the lock, so nothing in
the blueprint changes without the lock being rewritten in the same pull request, which
`scripts/check_pr_change.py` only allows together with a new entry in `docs/decisions.md`.
The runner's path check decides *who* may change a file; this check makes sure a change
cannot be silent.

Usage:
  python3 scripts/check_plan_lock.py            # verify; exit 1 on any mismatch
  python3 scripts/check_plan_lock.py --write    # rewrite kit/plan.lock from the tree
  python3 scripts/check_plan_lock.py --changed  # print the locked items that differ
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

LOCK_FILE = Path("kit/plan.lock")

# Whole files that define what is built, how, and how it is checked.
LOCKED_FILES = (
    "BUILD_BRIEF.md",
    "CLAUDE.md",
    "docs/constitution.md",
    "docs/sources.md",
    "docs/test-design.md",
    "docs/threat-model.md",
    "docs/code-docs.md",
    "docs/governance.md",
    "kit/tasks.json",
    "kit/prompts/TEMPLATE.md",
    "kit/sources.sha256",
    "kit/superseded_decisions.json",  # rewrites what every prompt tells a worker about a decision (D-491)
    "kit/layers.json",
    "kit/allowed_crates.txt",
)
LOCKED_GLOBS = ("docs/spec/*.md", "scripts/*.py", "scripts/*.sh", "docs/catalog/*.yaml")

# Sections of docs/architecture.md whose change is an architecture change (needs a decision).
LOCKED_SECTIONS = (
    "## 3. Modules, what each owns, and its tables",
    "## 4. Dependency rules",
    "## 5. Shared kernel (`core`)",
    "## 6. Job lifecycle state machine",
)
ARCHITECTURE = "docs/architecture.md"
HEADING = re.compile(r"^## \d+\. ")


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def locked_paths(root: Path) -> list[str]:
    paths = set(LOCKED_FILES)
    for pattern in LOCKED_GLOBS:
        paths.update(p.relative_to(root).as_posix() for p in root.glob(pattern))
    return sorted(p for p in paths if (root / p).is_file())


def section_text(markdown: str, heading: str) -> str | None:
    """The text of one `## N.` section, from its heading to the next `## N.` heading."""
    lines = markdown.splitlines()
    try:
        start = lines.index(heading)
    except ValueError:
        return None
    end = next((i for i in range(start + 1, len(lines)) if HEADING.match(lines[i])), len(lines))
    return "\n".join(lines[start:end]).rstrip() + "\n"


def fingerprint(root: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for rel in locked_paths(root):
        out[rel] = _sha((root / rel).read_bytes())
    arch = root / ARCHITECTURE
    if arch.is_file():
        text = arch.read_text(encoding="utf-8")
        for heading in LOCKED_SECTIONS:
            body = section_text(text, heading)
            key = f"{ARCHITECTURE}#{heading.split('.')[0].strip('# ')}"
            out[key] = _sha(body.encode("utf-8")) if body is not None else "MISSING"
    return out


def read_lock(root: Path) -> dict[str, str]:
    path = root / LOCK_FILE
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8")).get("items", {})


def write_lock(root: Path) -> None:
    items = fingerprint(root)
    payload = {
        "schema": 1,
        "note": "Fingerprints of the approved blueprint; see scripts/check_plan_lock.py and docs/governance.md.",
        "items": items,
    }
    (root / LOCK_FILE).write_text(json.dumps(payload, indent=1, sort_keys=True) + "\n", encoding="utf-8")


def differences(root: Path) -> list[str]:
    """Locked items whose fingerprint differs from the lock, plus items added or removed."""
    current = fingerprint(root)
    locked = read_lock(root)
    out = []
    for key in sorted(set(current) | set(locked)):
        if key not in locked:
            out.append(f"{key}: not in the lock (new locked file)")
        elif key not in current:
            out.append(f"{key}: in the lock but missing from the tree")
        elif current[key] != locked[key]:
            out.append(f"{key}: changed")
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    parser.add_argument("--write", action="store_true", help="rewrite the lock from the working tree")
    parser.add_argument("--changed", action="store_true", help="list differing items and exit 0")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    if args.write:
        write_lock(root)
        print(f"wrote {LOCK_FILE} with {len(read_lock(root))} items")
        return 0
    diffs = differences(root)
    if args.changed:
        print("\n".join(diffs) if diffs else "plan lock: no differences")
        return 0
    if diffs:
        print("plan lock: the blueprint changed without the lock being updated:", file=sys.stderr)
        for d in diffs:
            print(f"  {d}", file=sys.stderr)
        print(
            "Rewrite it with `python3 scripts/check_plan_lock.py --write` in the same pull request, "
            "with a new docs/decisions.md entry naming the change (docs/governance.md).",
            file=sys.stderr,
        )
        return 1
    print(f"plan lock: {len(read_lock(root))} items match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

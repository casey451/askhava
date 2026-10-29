"""Every direct Rust dependency is on the approved list (kit/allowed_crates.txt, D-479).

Reads the workspace `Cargo.toml` and every `app/*/Cargo.toml` and `tests/*/Cargo.toml` with
Python's tomllib, collects the names under `[workspace.dependencies]`, `[dependencies]`,
`[dev-dependencies]` and `[build-dependencies]`, and fails on any name not in the list.
No `Cargo.toml` yet exits 0. Usage: python3 scripts/check_crates.py [--root .]
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

ALLOWED_FILE = Path("kit/allowed_crates.txt")
SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")


def allowed(root: Path) -> set[str]:
    out: set[str] = set()
    for line in (root / ALLOWED_FILE).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def manifests(root: Path) -> list[Path]:
    found = []
    if (root / "Cargo.toml").is_file():
        found.append(root / "Cargo.toml")
    for folder in ("app", "tests"):
        found += sorted((root / folder).glob("*/Cargo.toml"))
    return found


def names_in(manifest: Path) -> set[str]:
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    names: set[str] = set()
    for section in SECTIONS:
        names.update(data.get(section, {}).keys())
        for target in data.get("target", {}).values():
            names.update(target.get(section, {}).keys())
    names.update(data.get("workspace", {}).get("dependencies", {}).keys())
    return names


def run(root: Path) -> list[str]:
    ok = allowed(root)
    problems = []
    for manifest in manifests(root):
        rel = manifest.relative_to(root).as_posix()
        for name in sorted(names_in(manifest) - ok):
            problems.append(f"{rel}: crate '{name}' is not in {ALLOWED_FILE.as_posix()} (needs a decision entry)")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    problems = run(Path(args.root).resolve())
    for p in problems:
        print(f"ERROR {p}", file=sys.stderr)
    print(f"crates: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())

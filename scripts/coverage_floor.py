"""Coverage floors (tests/coverage_floor.json, D-694): `Ci`'s comparison, and which crates a change must measure.

`Ci`'s Coverage job and the pre-hand-in gate (scripts/prehandin.sh, D-1300) both call this file, so the two
cannot judge a measurement differently (D-1440; tests/test_prehandin.py keeps the callers equal).

  python3 scripts/coverage_floor.py compare REPORT [--only PREFIX ...] [--base REF]
      REPORT is `cargo llvm-cov --branch --json --summary-only` output. Every floor in
      tests/coverage_floor.json is compared with it (branch coverage; a part with no branches yet by its
      lines), and a floor lowered or removed against REF (default origin/main) without a docs/decisions.md
      entry is refused. With --only (the gate's per-crate run) only those floors are judged and the workspace
      total is not, because a run over some crates has no workspace total and measures other floored parts
      only in part. A missing measurement is red, never a pass (D-1759, T264): a report with no files or zero
      total lines; a floor whose path exists on disk but which the report does not measure, or measures with
      zero lines (a crate left out of the run, or a report under another path prefix); a --only prefix the
      report does not measure. A floor whose path is not on disk passes only when the floor file's `future`
      list names it with a reason, printed on a `future:` line; `future` may not gain a path that exists.
  python3 scripts/coverage_floor.py select [--root .] < changed-paths
      For the paths a change touches (one per line, as `git diff --name-only` prints them), the floored
      crates it touches, each with every workspace crate that depends on it (their tests reach its code,
      as they do in `Ci`'s workspace run), minus `Ci`'s excluded crates. Prints `package NAME` and
      `judge PREFIX` lines; nothing when no floored crate is touched. A change to the floor file selects
      every floored crate. A floor whose path has no file yet (a `future` entry) is never selected: there is
      nothing to measure, and a run for it would be workspace-sized (D-1440 review).
      Dependencies are read from `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]`; a
      platform-specific `[target.*.dependencies]` table is not read (no workspace crate has one).
Exit status: 0 green, 1 red (each failure on a `FAIL:` line).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

FLOOR_FILE = "tests/coverage_floor.json"
# `Ci`'s --exclude list: the two trybuild drivers, whose .stderr files are tied to the stable rustc (D-488).
EXCLUDED = ("askhava-security", "askhava-recovery-tests")
SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
SLACK = 0.005  # rounding slack: a part 0.005 points under its floor still passes, as in Ci since D-694


def percent(part: dict[str, Any]) -> float | None:
    """Branch coverage; a part with no branches yet is measured by its lines."""
    unit = part["branches"] if part["branches"]["count"] else part["lines"]
    return (100.0 * unit["covered"] / unit["count"]) if unit["count"] else None


def add(parts: dict[str, Any], key: str, summary: dict[str, Any]) -> None:
    """Add a file's branch and line counts to `parts[key]`."""
    slot = parts.setdefault(key, {k: {"count": 0, "covered": 0} for k in ("branches", "lines")})
    for kind in slot:
        slot[kind]["count"] += summary[kind]["count"]
        slot[kind]["covered"] += summary[kind]["covered"]


def measured(data: dict[str, Any], floors: dict[str, Any], root: str) -> tuple[dict[str, Any], dict[str, Any]]:
    """The report's files summed per crate folder (`app/<crate>`) and per floored prefix."""
    crates: dict[str, Any] = {}
    floored: dict[str, Any] = {}
    for f in data["files"]:
        rel = f["filename"].removeprefix(root)
        add(crates, "/".join(rel.split("/")[:2]), f["summary"])
        for prefix in floors["floors"]:
            if rel.startswith(prefix):
                add(floored, prefix, f["summary"])
    return crates, floored


def judge(floors: dict[str, Any], data: dict[str, Any], root: str, only: list[str] | None = None) -> list[str]:
    """Print each measurement and return the failures: Ci's rule (D-694); `only` limits it to those prefixes.

    A missing measurement fails (D-1759): an empty report, and a judged floor whose path is on disk (or that
    `only` selects) but which the report leaves unmeasured or measures with zero lines. A floor with no file
    on disk is left to `future` (FEE-034, PRIV-022, LIC-033).
    """
    crates, floored = measured(data, floors, root)
    failures = []
    for name, part in sorted(crates.items()):
        print(f"{name}: {percent(part)}")
    if not data["files"] or not data["totals"]["lines"]["count"]:
        failures.append("the report measured nothing (no files or zero total lines)")
    if not only:
        total = percent(data["totals"])
        print(f"total: {total} (floor {floors['total']})")
        if total is not None and total + SLACK < floors["total"]:
            failures.append(f"total {total:.2f} is below the floor {floors['total']}")
    failures += [f"--only {p} is not a floor in {FLOOR_FILE}" for p in only or [] if p not in floors["floors"]]
    for prefix, floor in floors["floors"].items():
        if only and prefix not in only:
            continue
        part = floored.get(prefix)
        got = percent(part) if part and part["lines"]["count"] else None
        print(f"{prefix}: {got} (floor {floor})")
        if got is None and (only or (Path(root) / prefix).exists()):
            failures.append(f"{prefix} is not measured by the report (no lines under {root}{prefix})")
        elif got is not None and got + SLACK < floor:
            failures.append(f"{prefix} {got:.2f} is below the floor {floor}")
    return failures


def future(floors: dict[str, Any], root: str) -> list[str]:
    """Floors with no file on disk: each passes only when `future` lists it with a reason, printed (D-1759)."""
    reasons = {entry["path"]: entry.get("reason", "").strip() for entry in floors.get("future", [])}
    failures = []
    for prefix in floors["floors"]:
        if (Path(root) / prefix).exists():
            continue
        if reasons.get(prefix):
            print(f"future: {prefix} has no file yet ({reasons[prefix]})")
        else:
            failures.append(f"{prefix} has no file on disk and no reason in the \"future\" list of {FLOOR_FILE}")
    return failures


def flat(d: dict[str, Any]) -> dict[str, float]:
    """The floor file's total and floors as one {key: floor} map."""
    return {"total": d["total"], **d["floors"]}


def built_future(old: dict[str, Any], new: dict[str, Any], root: Path) -> list[str]:
    """`future` entries added against `old` for a path that exists: a built path must be measured (D-1759)."""
    before = {entry["path"] for entry in old.get("future", [])}
    added = [entry["path"] for entry in new.get("future", []) if entry["path"] not in before]
    return [f"a \"future\" entry was added for {p}, which exists on disk" for p in added if (root / p).exists()]


def lowered(floors: dict[str, Any], base: str, root: Path = Path(".")) -> list[str]:
    """A floor lowered or removed against `base`, unless the change's docs/decisions.md says "coverage floor"
    (D-694); and, with no way round it, a `future` entry added for a path that exists (D-1759)."""
    old_text = subprocess.run(["git", "show", f"{base}:{FLOOR_FILE}"], capture_output=True, text=True, check=False)
    if old_text.returncode != 0:
        return []
    old = json.loads(old_text.stdout)
    down = [k for k, v in flat(old).items() if flat(floors).get(k, -1) < v]
    diff = subprocess.run(["git", "diff", base, "HEAD", "--", "docs/decisions.md"], capture_output=True, text=True,
                          check=False).stdout
    failures = built_future(old, floors, root)
    if down and "coverage floor" not in diff.lower():
        failures.append(f"floor lowered without a docs/decisions.md entry: {down}")
    return failures


def compare(report: str, only: list[str] | None, base: str) -> int:
    """The `compare` command: judge the report and the `future` list, refuse a lowered floor; 1 when red."""
    floors = json.loads(Path(FLOOR_FILE).read_text(encoding="utf-8"))
    data = json.loads(Path(report).read_text(encoding="utf-8"))["data"][0]
    root = os.getcwd() + "/"
    failures = judge(floors, data, root, only) + future(floors, root) + lowered(floors, base)
    for line in failures:
        print("FAIL:", line)
    return 1 if failures else 0


def members(root: Path) -> dict[str, tuple[str, set[str]]]:
    """Every workspace crate as {name: (folder, the workspace crates it depends on)}, read with tomllib."""
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8")).get("workspace", {})
    found: dict[str, tuple[str, set[str]]] = {}
    for pattern in workspace.get("members", []):
        for manifest in sorted(root.glob(f"{pattern}/Cargo.toml")):
            data = tomllib.loads(manifest.read_text(encoding="utf-8"))
            deps: set[str] = set()
            for section in SECTIONS:
                table = data.get(section, {})
                deps |= {spec.get("package", key) if isinstance(spec, dict) else key for key, spec in table.items()}
            found[data["package"]["name"]] = (manifest.parent.relative_to(root).as_posix(), deps)
    return {name: (folder, deps & set(found)) for name, (folder, deps) in found.items()}


def dependents(crates: dict[str, tuple[str, set[str]]], name: str) -> set[str]:
    """`name` and every crate that depends on it, directly or through another crate."""
    reach, grew = {name}, True
    while grew:
        more = {other for other, (_, deps) in crates.items() if deps & reach} - reach
        reach, grew = reach | more, bool(more)
    return reach


def select(root: Path, changed: list[str]) -> tuple[list[str], list[str]]:
    """(packages to measure, floored prefixes to judge) for a change; both empty when nothing floored is touched."""
    floors = json.loads((root / FLOOR_FILE).read_text(encoding="utf-8"))["floors"]
    crates = members(root)
    every = FLOOR_FILE in changed
    packages: set[str] = set()
    judged = []
    for prefix in floors:
        if not (root / prefix).exists():
            continue  # a floor with no file yet has nothing to measure (D-1440 review)
        owner = next((n for n, (folder, _) in crates.items() if prefix.startswith(folder + "/")), None)
        if owner and (every or any(p.startswith(crates[owner][0] + "/") for p in changed)):
            judged.append(prefix)
            packages |= dependents(crates, owner)
    return sorted(packages - set(EXCLUDED)), judged


def main(argv: list[str] | None = None) -> int:
    """Parse the command line and run `compare` or `select`."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    cmp = sub.add_parser("compare")
    cmp.add_argument("report")
    cmp.add_argument("--only", action="append")
    cmp.add_argument("--base", default="origin/main")
    sel = sub.add_parser("select")
    sel.add_argument("--root", default=".")
    args = parser.parse_args(argv)
    if args.command == "compare":
        return compare(args.report, args.only, args.base)
    packages, judged = select(Path(args.root), [line.strip() for line in sys.stdin if line.strip()])
    for name in packages:
        print(f"package {name}")
    for prefix in judged:
        print(f"judge {prefix}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

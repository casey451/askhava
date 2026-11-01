"""Apply each named mutant in tests/mutation/mutants.toml and fail when its tests stay green.

docs/test-design.md section 4 (D-481, T056): each entry names a file, an exact text replacement
(`find` must occur exactly once in the file) and the tests that must fail. Each mutant is written
into the working tree in place, the named tests run with one shared CARGO_TARGET_DIR
(`cargo test -p <package> <target> -- <tests>`), and the file is restored whatever happens. A
mutant whose crate no longer compiles is reported `broken`: it proves nothing and fails the run
like a survivor, so an entry is fixed rather than left to pass by accident. Each distinct test
command first runs once with no mutant applied and must pass, so a kill is the mutant's doing.

Usage:
  python3 scripts/run_named_mutants.py --all            # nightly-main: the full list
  python3 scripts/run_named_mutants.py --base origin/main  # Ci: mutants of files this branch changed
  python3 scripts/run_named_mutants.py --only "<name>"  # one entry, by name
  python3 scripts/run_named_mutants.py --list [--all | --base REV]
With --base, a change to the list or to this script runs every mutant. Exit status 1 names each
mutant that survived or broke, or each entry whose `find` text is missing or repeated.
"""

from __future__ import annotations

import argparse
import os
import signal
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path

MANIFEST = Path("tests/mutation/mutants.toml")
SELF = Path("scripts/run_named_mutants.py")
DEFAULT_TARGET = "--lib"


@dataclass(frozen=True)
class Mutant:
    """One entry: where, what replaces what, and which tests must turn red."""

    name: str
    rule: str
    file: str
    find: str
    replace: str
    package: str
    target: str
    tests: tuple[str, ...]

    def command(self, no_run: bool) -> list[str]:
        """The cargo command for this mutant's tests; `no_run` only builds them."""
        cmd = ["cargo", "test", "--locked", "-p", self.package, *self.target.split()]
        if no_run:
            return [*cmd, "--no-run"]
        return [*cmd, "--", *self.tests]


def load(path: Path) -> list[Mutant]:
    """Every mutant in the manifest; a mutant's own keys override its group's."""
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    mutants: list[Mutant] = []
    for group in data.get("group", []):
        for entry in group["mutants"]:
            merged = {**{k: v for k, v in group.items() if k != "mutants"}, **entry}
            mutants.append(
                Mutant(
                    name=merged["name"],
                    rule=merged["rule"],
                    file=merged["file"],
                    find=merged["find"],
                    replace=merged["replace"],
                    package=merged["package"],
                    target=merged.get("target", DEFAULT_TARGET),
                    tests=tuple(merged["tests"]),
                )
            )
    names = [m.name for m in mutants]
    repeated = sorted({n for n in names if names.count(n) > 1})
    if repeated:
        raise SystemExit(f"repeated mutant names in {path}: {repeated}")
    return mutants


def changed_files(base: str) -> set[str]:
    """Files this branch changed since its merge base with `base`."""
    merge_base = subprocess.run(
        ["git", "merge-base", base, "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    diff = subprocess.run(
        ["git", "diff", "--no-renames", "--name-only", merge_base, "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return {line for line in diff.splitlines() if line}


def select(mutants: list[Mutant], args: argparse.Namespace) -> list[Mutant]:
    """The mutants this run applies: all, one by name, or those of the changed files."""
    if args.only:
        chosen = [m for m in mutants if m.name == args.only]
        if not chosen:
            raise SystemExit(f"no mutant named {args.only!r}")
        return chosen
    if args.all:
        return mutants
    changed = changed_files(args.base)
    if {str(MANIFEST), str(SELF)} & changed:
        return mutants
    return [m for m in mutants if m.file in changed]


def problems_in_texts(mutants: list[Mutant]) -> list[str]:
    """Entries whose `find` text is not in its file exactly once, or equals its replacement."""
    problems = []
    for m in mutants:
        path = Path(m.file)
        if not path.is_file():
            problems.append(f"{m.name}: {m.file} does not exist")
            continue
        count = path.read_text(encoding="utf-8").count(m.find)
        if count != 1:
            problems.append(f"{m.name}: `find` occurs {count} times in {m.file}, not once")
        if m.find == m.replace:
            problems.append(f"{m.name}: `replace` equals `find`")
    return problems


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    """Runs a cargo command, output captured (stdout and stderr together)."""
    return subprocess.run(cmd, check=False, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def apply_and_test(m: Mutant) -> tuple[str, str]:
    """`killed`, `survived` or `broken` for one mutant, with the tail of the cargo output; the
    file is written back byte for byte before this returns or raises."""
    path = Path(m.file)
    original = path.read_bytes()
    try:
        path.write_bytes(original.decode("utf-8").replace(m.find, m.replace, 1).encode("utf-8"))
        build = run(m.command(no_run=True))
        if build.returncode != 0:
            return "broken", build.stdout[-3000:]
        tests = run(m.command(no_run=False))
        return ("survived" if tests.returncode == 0 else "killed"), tests.stdout[-3000:]
    finally:
        path.write_bytes(original)


def red_baselines(mutants: list[Mutant]) -> list[str]:
    """The test commands already failing with no mutant applied: a kill there would prove nothing."""
    red = []
    for cmd in dict.fromkeys(tuple(m.command(no_run=False)) for m in mutants):
        result = run(list(cmd))
        if result.returncode != 0:
            print(result.stdout[-3000:])
            red.append(" ".join(cmd))
    return red


def stop_on_term(_signum: int, _frame: object) -> None:
    """Turns SIGTERM (a job timeout) into an exit, so the `finally` restoring the file runs."""
    raise SystemExit(143)


def main(argv: list[str] | None = None) -> int:
    """Runs the selected mutants and prints one line each; 1 on any survivor or broken entry."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    pick = parser.add_mutually_exclusive_group(required=True)
    pick.add_argument("--all", action="store_true", help="every mutant (nightly-main)")
    pick.add_argument("--base", help="only mutants of files changed since the merge base with BASE")
    pick.add_argument("--only", help="the one mutant with this name")
    parser.add_argument("--list", action="store_true", help="print the selection and stop")
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    args = parser.parse_args(argv)
    signal.signal(signal.SIGTERM, stop_on_term)
    chosen = select(load(args.manifest), args)
    problems = problems_in_texts(chosen)
    if args.list:
        for m in chosen:
            print(f"{m.rule} | {m.name} | {m.file}")
    for line in problems:
        print(f"FAIL: {line}")
    if args.list or problems:
        return 1 if problems else 0
    os.environ.setdefault("CARGO_TARGET_DIR", str(Path.cwd() / "target"))
    # A property test failing under a mutant must not leave a proptest-regressions file behind.
    os.environ["PROPTEST_DISABLE_FAILURE_PERSISTENCE"] = "1"
    print(f"{len(chosen)} named mutants")
    red = red_baselines(chosen)
    for cmd in red:
        print(f"FAIL: red before any mutant: {cmd}")
    if red:
        return 1
    failed = []
    for m in chosen:
        started = time.monotonic()
        verdict, tail = apply_and_test(m)
        print(f"{verdict:8} {time.monotonic() - started:6.1f}s  {m.rule} | {m.name}", flush=True)
        if verdict != "killed":
            failed.append(m.name)
            print(tail)
    for name in failed:
        print(f"FAIL: mutant not killed: {name}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

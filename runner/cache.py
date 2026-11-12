"""The backup job's two build caches, kept small and one entry each (D-3143).

GitHub keeps at most 10 GB of Actions cache per repository and evicts entries once a save goes past it. Every
backup job used to save the worker's whole `target/debug` with the registry (about 3.4 GB) once per commit of
`main`, and a job that compiled the smoke saved `target/smoke` (about 3.9 GB), so each merge added about 7.3 GB
and pushed `Ci`'s `main`-only entries (D-2782) out within hours: on November 12, 0 of 36 Rust restores and 0 of
12 sqlx-cli restores in 12 `ci` runs hit. `backup-runner.yml` calls this module at four points:

- `seed`: after the smoke warm-up and before the session, give the worker's empty `target/debug` the smoke build's
  dependency artifacts (`github_env.seed_files`, the set the smoke seed copies the other way, D-1890). A fresh
  checkout gives every workspace file a new mtime, so Cargo rebuilds every member of the worker's build whatever
  it held; only dependencies ever carried over, and the smoke's entry already holds them for `main`'s head.
- `clear`: before the worker entry is saved, empty `target/debug`, so that entry holds the registry alone.
- `drop`: delete a family's older entries (key prefix, created before the job started) just before the job saves
  its newer one, so the family holds one entry; it answers `save=no` when another job has saved a newer entry of
  the family since this one started, so two lanes never keep two smoke entries.

Standard library only. Only `key=value` lines go to standard output (the workflow appends them to
`$GITHUB_OUTPUT`, which refuses any other line); messages go to standard error. Nothing here ever fails the
job: a failure only leaves the cache or the worker's build as it was.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

from runner.github_env import SEED_MARGIN, _member_names, _tree_size, copy_seed, seed_files

Run = Callable[[list[str]], tuple[int, str]]
PAGE = 100  # GitHub's largest page of the cache list
STAMP = 19  # the characters of an ISO timestamp up to the second ("2026-11-12T04:05:06")


def run_command(args: list[str]) -> tuple[int, str]:
    """Run a command; (exit code, standard output). A missing program counts as a failure, never raises."""
    try:
        done = subprocess.run(args, capture_output=True, text=True, check=False, timeout=120)
    except (OSError, subprocess.SubprocessError) as exc:
        return 1, str(exc)
    return done.returncode, done.stdout


def note(text: str) -> None:
    """A message for the job log, on standard error so `$GITHUB_OUTPUT` never sees it."""
    print(text, file=sys.stderr)


def members_of(root: Path, run: Run = run_command) -> set[str]:
    """The workspace's package and target names (`cargo metadata --no-deps --offline` in `root`); empty when
    cargo cannot say, which makes `seed` copy nothing."""
    code, out = run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline", "--manifest-path",
                     str(root / "Cargo.toml")])
    try:
        return _member_names(json.loads(out)) if code == 0 else set()
    except (ValueError, KeyError, TypeError):
        return set()


def seed(root: Path, smoke: Path, debug: Path, run: Run = run_command) -> str:
    """Copy the dependency artifacts of `smoke` (the smoke build's profile folder) into `debug` (the worker's)
    when `debug` holds no dependency build yet and the disk has room; what happened, in one line."""
    if any((debug / "deps").glob("*")):
        return "worker build not seeded: it already holds a build"
    if not (smoke / "deps").is_dir():
        return "worker build not seeded: the smoke build has no deps folder"
    members = members_of(root, run)
    if not members:
        return "worker build not seeded: the workspace members are unknown"
    entries = seed_files(smoke, members)
    need, free = _tree_size(entries), shutil.disk_usage(debug.parent if debug.parent.exists() else root).free
    if free <= need + SEED_MARGIN:
        return f"worker build not seeded: {need >> 20} MB needed, {free >> 20} MB free"
    copy_seed(smoke, debug, entries)
    return f"worker build seeded from the smoke build: {len(entries)} entries, {need >> 20} MB"


def clear(debug: Path) -> str:
    """Empty `debug` before the worker entry is saved, so it holds the registry alone: `pruned=yes`, or
    `pruned=no` when it could not be emptied (the workflow then saves nothing)."""
    try:
        if debug.is_symlink():
            debug.unlink()
        elif debug.exists():
            shutil.rmtree(debug)
    except OSError as exc:
        note(f"worker build not emptied: {str(exc)[:200]}")
        return "pruned=no"
    note("worker build emptied: the worker entry holds the registry alone")
    return "pruned=yes"


def superseded(caches: list[dict[str, Any]], prefix: str, before: str) -> list[int]:
    """The ids of `caches` (GitHub's `actions_caches` rows) whose key starts with `prefix` and that were created
    before `before` (an ISO timestamp compared to the second, both UTC): the entries a job that started at
    `before` replaces. An entry created since (another lane's newer save) is kept. Rows without an id or a
    creation time are kept."""
    out = []
    for row in caches:
        created, key = str(row.get("created_at") or ""), str(row.get("key") or "")
        if key.startswith(prefix) and len(created) >= STAMP and created[:STAMP] < before[:STAMP] and isinstance(row.get("id"), int):
            out.append(int(row["id"]))
    return out


def newer(caches: list[dict[str, Any]], prefix: str, before: str) -> bool:
    """Whether an entry of the family was created at or after `before`: another job saved since this one began."""
    return any(str(r.get("key") or "").startswith(prefix) and str(r.get("created_at") or "")[:STAMP] >= before[:STAMP]
               for r in caches)


def list_caches(repo: str, prefix: str, run: Run = run_command) -> list[dict[str, Any]] | None:
    """Every cache entry whose key starts with `prefix` (GitHub matches `key` as a prefix), page by page; None
    when a page cannot be read."""
    rows: list[dict[str, Any]] = []
    for page in range(1, 51):
        code, out = run(["gh", "api", f"repos/{repo}/actions/caches?key={prefix}&per_page={PAGE}&page={page}"])
        try:
            found = json.loads(out).get("actions_caches", []) if code == 0 else None
        except (ValueError, AttributeError):
            found = None
        if not isinstance(found, list):
            return None
        rows += found
        if len(found) < PAGE:
            break
    return rows


def drop(repo: str, prefix: str, before: str, run: Run = run_command) -> str:
    """Delete the entries `superseded` names, unless a newer entry exists; the `save=` line for the save step
    (`save=no` only when another job saved a newer entry since `before`; an unreadable list says `save=yes`,
    the behaviour before D-3143)."""
    rows = list_caches(repo, prefix, run)
    if rows is None:
        note(f"cache entries under {prefix} not listed; none dropped")
        return "save=yes"
    if newer(rows, prefix, before):
        note(f"a newer entry under {prefix} was saved since this job started: nothing dropped, nothing saved")
        return "save=no"
    ids = superseded(rows, prefix, before)
    gone = [i for i in ids if run(["gh", "api", "-X", "DELETE", f"repos/{repo}/actions/caches/{i}"])[0] == 0]
    note(f"dropped {len(gone)} of {len(ids)} superseded entries under {prefix} ({len(rows)} listed)")
    return "save=yes"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python3 -m runner.cache")
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("seed")
    s.add_argument("--root", type=Path, default=Path("."))
    s.add_argument("--smoke", type=Path, default=Path("target/smoke/debug"))
    s.add_argument("--target", type=Path, default=Path("target/debug"))
    c = sub.add_parser("clear")
    c.add_argument("--target", type=Path, default=Path("target/debug"))
    d = sub.add_parser("drop")
    d.add_argument("--prefix", required=True)
    d.add_argument("--before", required=True)
    d.add_argument("--repo", default="")
    args = ap.parse_args(argv)
    if args.cmd == "seed":
        try:
            note(seed(args.root, args.smoke, args.target))
        except OSError as exc:
            note(f"worker build not seeded: {str(exc)[:200]}")
    elif args.cmd == "clear":
        print(clear(args.target))
    elif not args.repo or len(args.before) < STAMP:
        note("cache entries not dropped: no repository or start time")
        print("save=yes")
    else:
        print(drop(args.repo, args.prefix, args.before))
    return 0


if __name__ == "__main__":
    sys.exit(main())

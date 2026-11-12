"""The backup job's two build caches, kept small and one entry each (D-3143).

GitHub keeps at most 10 GB of Actions cache per repository and evicts entries once a save goes past it. Every
backup job used to save the worker's whole `target/debug` with the registry (about 3.4 GB) once per commit of
`main`, and a job that compiled the smoke saved `target/smoke` (about 3.9 GB), so each merge added about 7.3 GB
and pushed `Ci`'s `main`-only entries (D-2782) out within hours: on November 12, 0 of 36 Rust restores and 0 of
12 sqlx-cli restores in 12 `ci` runs hit. Two steps of `backup-runner.yml` call this module before those saves:

- `prune`: cut `target/debug` down to its dependency artifacts (`github_env.seed_files`, the set the smoke seed
  already copies, D-1890). A fresh checkout gives every workspace file a new mtime, so Cargo rebuilds every
  member whatever the cache holds; only the dependencies ever carry over, and they are what the entry keeps.
- `drop`: delete this family's older entries (key prefix, created before the job started) just before the job
  saves its newer one, so the family holds one entry instead of one per commit of `main`.

Standard library only; `python3 -m runner.cache prune --target DIR` prints `pruned=yes|no` for `$GITHUB_OUTPUT`;
`python3 -m runner.cache drop --prefix KEY_PREFIX --before ISO` deletes through `gh api` with the job token.
Neither ever fails the job: a failure only leaves the cache as it was.
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

from runner.github_env import SEED_DIRS, _member_names, seed_files

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


def prune_target(debug: Path, members: set[str]) -> tuple[int, int]:
    """Remove from `debug` (a profile folder such as `target/debug`) everything but the dependency artifacts in
    SEED_DIRS: member artifacts, test binaries, `incremental`, examples and top-level outputs. (kept, removed)."""
    keep = set(seed_files(debug, members))
    removed = 0
    for entry in sorted(debug.iterdir()) if debug.is_dir() else []:
        inner = sorted(entry.iterdir()) if entry.name in SEED_DIRS and entry.is_dir() and not entry.is_symlink() else [entry]
        for item in inner:
            if item not in keep:
                _remove(item)
                removed += 1
    return len(keep), removed


def _remove(path: Path) -> None:
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    else:
        path.unlink(missing_ok=True)


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
    """Delete the entries `superseded` names; one line saying what happened."""
    rows = list_caches(repo, prefix, run)
    if rows is None:
        return f"cache entries under {prefix} not listed; none dropped"
    ids = superseded(rows, prefix, before)
    gone = [i for i in ids if run(["gh", "api", "-X", "DELETE", f"repos/{repo}/actions/caches/{i}"])[0] == 0]
    return f"dropped {len(gone)} of {len(ids)} superseded entries under {prefix} ({len(rows)} listed)"


def members_of(root: Path, run: Run = run_command) -> set[str]:
    """The workspace's package and target names (`cargo metadata --no-deps --offline` in `root`); empty when
    cargo cannot say, which makes `prune` keep everything and say so."""
    code, out = run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline", "--manifest-path",
                     str(root / "Cargo.toml")])
    try:
        return _member_names(json.loads(out)) if code == 0 else set()
    except (ValueError, KeyError, TypeError):
        return set()


def prune(root: Path, target: Path, run: Run = run_command) -> list[str]:
    """`prune`: the lines to print; the last is `pruned=yes` only when the target was cut to its dependencies."""
    members = members_of(root, run)
    if not members or not (target / "deps").is_dir():
        why = "the workspace members are unknown" if not members else f"{target} has no deps folder"
        return [f"worker build not pruned: {why}", "pruned=no"]
    try:
        kept, removed = prune_target(target, members)
    except OSError as exc:
        return [f"worker build not pruned: {str(exc)[:200]}", "pruned=no"]
    return [f"worker build pruned to its dependencies: {kept} entries kept, {removed} removed", "pruned=yes"]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python3 -m runner.cache")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prune")
    p.add_argument("--target", type=Path, default=Path("target/debug"))
    p.add_argument("--root", type=Path, default=Path("."))
    d = sub.add_parser("drop")
    d.add_argument("--prefix", required=True)
    d.add_argument("--before", required=True)
    d.add_argument("--repo", default="")
    args = ap.parse_args(argv)
    if args.cmd == "prune":
        print("\n".join(prune(args.root, args.target)))
        return 0
    if not args.repo or len(args.before) < STAMP:
        print("cache entries not dropped: no repository or start time")
        return 0
    print(drop(args.repo, args.prefix, args.before))
    return 0


if __name__ == "__main__":
    sys.exit(main())

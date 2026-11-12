"""The backup job's build caches stay small and single, and only `main` saves shared caches (D-3143).

GitHub keeps 10 GB of Actions cache per repository. Each merge used to add about 7.3 GB of backup-job entries
(the worker's whole `target/debug` per commit, the smoke's `target/smoke` per compiling job), which pushed `Ci`'s
`main`-only entries (D-2782) out within hours. These tests pin the fix: `runner/cache.py` cuts the worker build to
its dependency artifacts and deletes a family's superseded entries just before a newer save; the workflows save
only from `main`'s ref; and what the restores read, and so what is tested, is unchanged.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest
import yaml

from runner import cache

ROOT = Path(__file__).resolve().parent.parent
FLOWS = ROOT / ".github" / "workflows"
MAIN = "github.ref == 'refs/heads/main'"


def _flow(name: str) -> dict[str, Any]:
    return yaml.safe_load((FLOWS / name).read_text(encoding="utf-8"))


def _steps(name: str, job: str) -> list[dict[str, Any]]:
    return list(_flow(name)["jobs"][job]["steps"])


def _named(steps: list[dict[str, Any]], name: str) -> dict[str, Any]:
    return next(s for s in steps if s.get("name") == name)


# ---- prune: the worker build cut to its dependency artifacts


def _target(tmp_path: Path) -> Path:
    debug = tmp_path / "target" / "debug"
    for rel in ("deps/libserde-0123456789abcdef.rlib", "deps/libaskhava_core-0123456789abcdef.rlib",
                "deps/payments-0123456789abcdef", ".fingerprint/serde-0123456789abcdef/lib",
                ".fingerprint/askhava-core-0123456789abcdef/lib", "build/ring-0123456789abcdef/out/x.o",
                "build/askhava-server-0123456789abcdef/out/y", "incremental/askhava_core-1abc/s-x/q.bin",
                "askhava-server", "examples/demo-0123456789abcdef", ".cargo-lock", "deps/README"):
        (debug / rel).parent.mkdir(parents=True, exist_ok=True)
        (debug / rel).write_text("x")
    return debug


MEMBERS = {"askhava-core", "askhava-server", "payments"}


def test_prune_keeps_only_dependency_artifacts(tmp_path: Path) -> None:
    debug = _target(tmp_path)
    kept, removed = cache.prune_target(debug, MEMBERS)
    left = sorted(str(p.relative_to(debug)) for p in debug.rglob("*") if p.is_file())
    assert left == [".fingerprint/serde-0123456789abcdef/lib", "build/ring-0123456789abcdef/out/x.o",
                    "deps/libserde-0123456789abcdef.rlib"]
    assert kept == 3 and removed == 9
    assert sorted(p.name for p in debug.iterdir()) == [".fingerprint", "build", "deps"]


def test_prune_leaves_a_link_and_never_follows_it(tmp_path: Path) -> None:
    debug = _target(tmp_path)
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "keep.txt").write_text("x")
    (debug / "link").symlink_to(outside, target_is_directory=True)
    cache.prune_target(debug, MEMBERS)
    assert not (debug / "link").exists() and (outside / "keep.txt").exists()


def _metadata(names: set[str]) -> str:
    return json.dumps({"packages": [{"name": n, "targets": [{"name": n.replace("-", "_")}]} for n in sorted(names)]})


def test_prune_says_yes_only_when_it_cut_the_target(tmp_path: Path) -> None:
    debug = _target(tmp_path)
    assert cache.prune(tmp_path, debug, lambda args: (1, ""))[-1] == "pruned=no"  # members unknown: nothing cut
    assert (debug / "askhava-server").exists()
    assert cache.prune(tmp_path, tmp_path / "none", lambda args: (0, _metadata(MEMBERS)))[-1] == "pruned=no"
    lines = cache.prune(tmp_path, debug, lambda args: (0, _metadata(MEMBERS)))
    assert lines[-1] == "pruned=yes" and "3 entries kept" in lines[0] and not (debug / "askhava-server").exists()
    assert cache.prune(tmp_path, debug, lambda args: (0, "not json"))[-1] == "pruned=no"


# ---- drop: a family's superseded entries


ROWS = [
    {"id": 1, "key": "rust-smoke-Linux-h-aaa-1-1", "created_at": "2026-11-12T04:00:00.000Z"},
    {"id": 2, "key": "rust-smoke-Linux-h-bbb-2-1", "created_at": "2026-11-12T04:59:59Z"},
    {"id": 3, "key": "rust-smoke-Linux-h-ccc-3-1", "created_at": "2026-11-12T05:00:00Z"},  # saved since the job began
    {"id": 4, "key": "rust-Linux-h-aaa", "created_at": "2026-11-12T03:00:00Z"},
    {"id": 5, "key": "rust-smoke-Linux-h-ddd-4-1", "created_at": ""},
    {"key": "rust-smoke-Linux-h-eee-5-1", "created_at": "2026-11-12T01:00:00Z"},
    {"id": 7, "key": "v0-rust-checks-Linux-x64-1-2", "created_at": "2026-11-11T17:25:00Z"},
]


def test_superseded_names_only_older_entries_of_the_family() -> None:
    assert cache.superseded(ROWS, "rust-smoke-Linux-", "2026-11-12T05:00:00") == [1, 2]
    assert cache.superseded(ROWS, "rust-Linux-", "2026-11-12T05:00:00") == [4]  # never a smoke entry
    assert cache.superseded(ROWS, "rust-smoke-Linux-", "2026-11-12T04:00:00") == []
    assert cache.superseded(ROWS, "v0-", "2026-11-12T05:00:00") == [7]


class Gh:
    """`gh api` over a list of cache rows: list pages and deletes, with failures on request."""

    def __init__(self, rows: list[dict[str, Any]], fail_list: bool = False, fail_delete: set[int] | None = None) -> None:
        self.rows, self.fail_list, self.fail_delete = rows, fail_list, fail_delete or set()
        self.calls: list[list[str]] = []

    def __call__(self, args: list[str]) -> tuple[int, str]:
        self.calls.append(args)
        if args[:4] == ["gh", "api", "-X", "DELETE"]:
            found = int(args[4].rsplit("/", 1)[1])
            return (1, "") if found in self.fail_delete else (0, "")
        if self.fail_list:
            return 1, ""
        query = dict(part.split("=", 1) for part in args[2].split("?", 1)[1].split("&"))
        page, size = int(query["page"]), int(query["per_page"])
        rows = [r for r in self.rows if str(r.get("key", "")).startswith(query["key"])]
        return 0, json.dumps({"total_count": len(rows), "actions_caches": rows[(page - 1) * size: page * size]})


def test_drop_deletes_the_superseded_entries_and_counts_failures() -> None:
    gh = Gh(ROWS, fail_delete={2})
    line = cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", gh)
    assert line == "dropped 1 of 2 superseded entries under rust-smoke-Linux- (5 listed)"
    assert [c[4] for c in gh.calls if c[:4] == ["gh", "api", "-X", "DELETE"]] == ["repos/o/r/actions/caches/1",
                                                                                 "repos/o/r/actions/caches/2"]


def test_drop_lists_every_page_and_deletes_nothing_when_a_page_is_unreadable() -> None:
    many = [{"id": i, "key": f"rust-smoke-Linux-{i}", "created_at": "2026-11-12T00:00:00Z"} for i in range(1, 251)]
    gh = Gh(many)
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", gh).startswith("dropped 250 of 250")
    assert sum(1 for c in gh.calls if c[:3] == ["gh", "api", "repos/o/r/actions/caches?key=rust-smoke-Linux-&per_page=100&page=3"]) == 1
    broken = Gh(ROWS, fail_list=True)
    assert cache.drop("o/r", "rust-smoke-Linux-", "2026-11-12T05:00:00", broken).endswith("not listed; none dropped")
    assert not [c for c in broken.calls if "DELETE" in c]
    assert cache.list_caches("o/r", "x", lambda args: (0, "[]")) is None  # a page that is not the expected object


def test_main_never_fails_the_step(capsys: pytest.CaptureFixture[str]) -> None:
    assert cache.main(["drop", "--prefix", "rust-smoke-Linux-", "--before", ""]) == 0
    assert "not dropped" in capsys.readouterr().out
    assert cache.run_command(["/nonexistent/program"])[0] != 0


# ---- the workflows


def test_the_worker_cache_is_restored_then_saved_cut_and_alone_from_main_only() -> None:
    steps = _steps("backup-runner.yml", "backup")
    order = [s.get("name") or s.get("uses") for s in steps]
    restore = _named(steps, "Rust build cache (D-514)")
    prune = _named(steps, "Cut the worker build to its dependencies before the save (D-3143)")
    drop = _named(steps, "Drop the superseded worker build entries (D-3143)")
    save = _named(steps, "Rust build cache, save (D-514, D-3143)")
    assert restore["uses"] == "actions/cache/restore@v4" and save["uses"] == "actions/cache/save@v4"
    assert restore["with"]["path"] == save["with"]["path"] and save["with"]["key"] == "${{ steps.rust-cache.outputs.cache-primary-key }}"
    assert restore["id"] == "rust-cache" and prune["id"] == "rust-prune"
    assert MAIN in prune["if"] and "steps.rust-cache.outputs.cache-hit != 'true'" in prune["if"] and prune["if"].startswith("always()")
    assert drop["if"] == save["if"] == "always() && steps.rust-prune.outputs.pruned == 'yes'"
    assert order.index("anthropics/claude-code-action@v1") < order.index(prune["name"]) < order.index(drop["name"]) < order.index(save["name"])
    assert order.index("Smoke build cache, save (D-1911)") < order.index(prune["name"])
    assert 'cd "$RUNNER_TEMP/keeper" && python3 -m runner.cache prune' in prune["run"] and 'tee -a "$GITHUB_OUTPUT"' in prune["run"]
    assert drop["env"]["PREFIX"] == "rust-${{ runner.os }}-" and restore["with"]["key"].startswith(drop["env"]["PREFIX"])
    assert not any(str(s.get("uses", "")).startswith("actions/cache@") for s in steps)  # no save left to a post step


def test_the_smoke_entry_is_saved_only_from_main_after_its_older_entries_are_dropped() -> None:
    steps = _steps("backup-runner.yml", "backup")
    order = [s.get("name") or s.get("uses") for s in steps]
    drop = _named(steps, "Drop the superseded smoke build entries (D-3143)")
    save = _named(steps, "Smoke build cache, save (D-1911)")
    restore = _named(steps, "Smoke build cache, restore (D-1890, D-1911)")
    assert drop["if"] == save["if"] and MAIN in save["if"] and "hashFiles('target/smoke-ran')" in save["if"]
    assert order.index(drop["name"]) + 1 == order.index(save["name"]) and drop["continue-on-error"] is True
    assert drop["env"]["PREFIX"] == "rust-smoke-${{ runner.os }}-" and restore["with"]["key"].startswith(drop["env"]["PREFIX"])
    assert '--before "${JOB_START:-}"' in drop["run"] and 'cd "$RUNNER_TEMP/keeper"' in drop["run"]
    start = _named(steps, "Note when this job started (for the chaining step)")["run"]
    assert "JOB_START=$(date -u +%Y-%m-%dT%H:%M:%S)" in start  # the format `superseded` compares to the second


@pytest.mark.parametrize(("flow", "job"), [("ci.yml", "e2e"), ("a11y.yml", "a11y")])
def test_the_playwright_browsers_are_saved_from_main_only(flow: str, job: str) -> None:
    steps = _steps(flow, job)
    restore = next(s for s in steps if s.get("id") == "browsers")
    save = _named(steps, "Save the Playwright browsers to the cache (main only, D-3143)")
    assert restore["uses"].startswith("actions/cache/restore@") and save["uses"].startswith("actions/cache/save@")
    assert restore["uses"].split("@")[1] == save["uses"].split("@")[1]
    assert save["if"].startswith(MAIN + " && steps.browsers.outputs.cache-hit != 'true'")
    assert save["with"] == {"path": restore["with"]["path"], "key": "${{ steps.browsers.outputs.cache-primary-key }}"}
    order = [s.get("name") for s in steps]
    assert order.index("Chromium and its system libraries") < order.index(save["name"])


def test_no_branch_run_saves_a_shared_cache() -> None:
    """Every save in the workflows a task branch runs (ci, twin, a11y) is guarded by main's ref: a rust-cache
    `save-if`, or an `if` on an actions/cache/save step; no combined actions/cache step saves at a job's end."""
    for flow in ("ci.yml", "twin.yml", "a11y.yml"):
        for job in _flow(flow)["jobs"].values():
            for step in job.get("steps") or []:
                uses = str(step.get("uses", ""))
                assert not uses.startswith("actions/cache@"), f"{flow}: {step.get('name')}"
                if uses.startswith("Swatinem/rust-cache@"):
                    assert step["with"]["save-if"] == "${{ " + MAIN + " }}", f"{flow}: {step.get('name')}"
                if uses.startswith("actions/cache/save@"):
                    assert str(step.get("if", "")).startswith(MAIN), f"{flow}: {step.get('name')}"
